// SPDX-License-Identifier: MIT OR Apache-2.0

//! SVG path data (SVG 2 §9.3) and the number lists of other attributes,
//! turned into curves in user space.
//!
//! Elliptical arcs become circles or ellipses (SVG 2 appendix B.2.4), not
//! Béziers, so they reach the toolpath as arcs (D05 §4).

use crate::conic::{arc, line};
use fluut_geom::{BSplineCurve3, Curve3, Interval, Point3, Vec3};
use fluut_topo::WireCurve;
use std::f64::consts::TAU;
use std::sync::Arc;

/// A cursor over the numbers, flags and command letters of an attribute.
#[derive(Debug, Clone)]
pub(crate) struct Scanner<'a> {
    s: &'a [u8],
    pub(crate) i: usize,
}

impl<'a> Scanner<'a> {
    pub(crate) fn new(s: &'a str) -> Scanner<'a> {
        Scanner {
            s: s.as_bytes(),
            i: 0,
        }
    }

    /// Skips white space and at most one comma.
    pub(crate) fn skip_separators(&mut self) {
        self.skip_ws();
        if self.peek() == Some(b',') {
            self.i += 1;
            self.skip_ws();
        }
    }

    pub(crate) fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')) {
            self.i += 1;
        }
    }

    pub(crate) fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    pub(crate) fn at_end(&self) -> bool {
        self.i >= self.s.len()
    }

    /// True if a number starts here.
    pub(crate) fn number_next(&self) -> bool {
        matches!(self.peek(), Some(b'0'..=b'9' | b'.' | b'-' | b'+'))
    }

    /// A number: `[+-]? (digits [. digits?] | . digits) ([eE] [+-]? digits)?`,
    /// then separators. `None` (and the cursor unmoved) if there is none or
    /// it is not finite.
    pub(crate) fn number(&mut self) -> Option<f64> {
        let start = self.i;
        let mut j = self.i;
        let digits = |j: &mut usize| {
            let k = *j;
            while self.s.get(*j).is_some_and(u8::is_ascii_digit) {
                *j += 1;
            }
            *j > k
        };
        if matches!(self.s.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let int = digits(&mut j);
        let mut frac = false;
        if self.s.get(j) == Some(&b'.') {
            j += 1;
            frac = digits(&mut j);
        }
        if !(int || frac) {
            return None;
        }
        if matches!(self.s.get(j), Some(b'e' | b'E')) {
            let mut k = j + 1;
            if matches!(self.s.get(k), Some(b'+' | b'-')) {
                k += 1;
            }
            if digits(&mut k) {
                j = k;
            }
        }
        let text = std::str::from_utf8(&self.s[start..j]).ok()?;
        let v: f64 = text.parse().ok()?;
        if !v.is_finite() {
            return None;
        }
        self.i = j;
        self.skip_separators();
        Some(v)
    }

    /// An arc flag: a single `0` or `1`, which may run into what follows.
    fn flag(&mut self) -> Option<bool> {
        let f = match self.peek()? {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.i += 1;
        self.skip_separators();
        Some(f)
    }
}

/// All numbers of a list such as `points` or `viewBox`; `None` if anything
/// else is in it.
pub(crate) fn numbers(s: &str) -> Option<Vec<f64>> {
    let mut sc = Scanner::new(s);
    sc.skip_ws();
    let mut out = Vec::new();
    while !sc.at_end() {
        out.push(sc.number()?);
    }
    Some(out)
}

/// Curves of one path or shape in user space (z = 0), built from absolute
/// commands.
#[derive(Debug)]
pub(crate) struct Builder {
    pub(crate) curves: Vec<WireCurve>,
    cur: [f64; 2],
    start: [f64; 2],
    /// Malformed pieces skipped (degenerate Béziers count as nothing).
    pub(crate) dropped: u32,
    max: usize,
    pub(crate) overflow: bool,
}

fn p3(p: [f64; 2]) -> Point3 {
    Point3::new(p[0], p[1], 0.0)
}

impl Builder {
    /// A builder that stops adding curves after `max`.
    pub(crate) fn new(max: usize) -> Builder {
        Builder {
            curves: Vec::new(),
            cur: [0.0; 2],
            start: [0.0; 2],
            dropped: 0,
            max,
            overflow: false,
        }
    }

    fn add(&mut self, c: Option<WireCurve>) {
        match c {
            Some(_) if self.curves.len() >= self.max => self.overflow = true,
            Some(c) => self.curves.push(c),
            None => self.dropped += 1,
        }
    }

    pub(crate) fn move_to(&mut self, p: [f64; 2]) {
        self.cur = p;
        self.start = p;
    }

    pub(crate) fn line_to(&mut self, p: [f64; 2]) {
        if p != self.cur {
            self.add(line(p3(self.cur), p3(p)));
        }
        self.cur = p;
    }

    pub(crate) fn close(&mut self) {
        let s = self.start;
        self.line_to(s);
    }

    /// A Bézier of degree `poles.len() − 1` from the current point.
    fn bezier(&mut self, ctrl: &[[f64; 2]]) {
        let mut poles = vec![p3(self.cur)];
        poles.extend(ctrl.iter().map(|&p| p3(p)));
        let end = ctrl[ctrl.len() - 1];
        if poles.iter().all(|p| *p == poles[0]) {
            self.cur = end;
            return;
        }
        let d = poles.len() - 1;
        let mut knots = vec![0.0; d + 1];
        knots.extend(std::iter::repeat_n(1.0, d + 1));
        let c = BSplineCurve3::new(d as u8, knots, poles, None)
            .ok()
            .map(|b| WireCurve {
                curve: Curve3::BSpline(Arc::new(b)),
                range: Interval { lo: 0.0, hi: 1.0 },
            });
        self.add(c);
        self.cur = end;
    }

    pub(crate) fn cubic_to(&mut self, c1: [f64; 2], c2: [f64; 2], p: [f64; 2]) {
        self.bezier(&[c1, c2, p]);
    }

    pub(crate) fn quad_to(&mut self, c: [f64; 2], p: [f64; 2]) {
        self.bezier(&[c, p]);
    }

    /// An elliptical arc to `p` (endpoint form), converted to centre form.
    pub(crate) fn arc_to(
        &mut self,
        rx: f64,
        ry: f64,
        rot_deg: f64,
        large: bool,
        sweep: bool,
        p: [f64; 2],
    ) {
        let p1 = self.cur;
        if p1 == p {
            return;
        }
        let (mut rx, mut ry) = (rx.abs(), ry.abs());
        if rx == 0.0 || ry == 0.0 {
            self.line_to(p);
            return;
        }
        let (s, c) = libm::sincos(rot_deg.to_radians());
        let (hx, hy) = (0.5 * (p1[0] - p[0]), 0.5 * (p1[1] - p[1]));
        let (x1, y1) = (c * hx + s * hy, -s * hx + c * hy);
        let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
        if lambda > 1.0 {
            let k = lambda.sqrt();
            rx *= k;
            ry *= k;
        }
        let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
        let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
        let mut coef = if den > 0.0 {
            (num / den).max(0.0).sqrt()
        } else {
            0.0
        };
        if large == sweep {
            coef = -coef;
        }
        let (cx1, cy1) = (coef * rx * y1 / ry, -coef * ry * x1 / rx);
        let centre = [
            c * cx1 - s * cy1 + 0.5 * (p1[0] + p[0]),
            s * cx1 + c * cy1 + 0.5 * (p1[1] + p[1]),
        ];
        let angle =
            |ux: f64, uy: f64, vx: f64, vy: f64| libm::atan2(ux * vy - uy * vx, ux * vx + uy * vy);
        let (ux, uy) = ((x1 - cx1) / rx, (y1 - cy1) / ry);
        let (vx, vy) = ((-x1 - cx1) / rx, (-y1 - cy1) / ry);
        let theta1 = angle(1.0, 0.0, ux, uy);
        let mut delta = angle(ux, uy, vx, vy);
        if !sweep && delta > 0.0 {
            delta -= TAU;
        } else if sweep && delta < 0.0 {
            delta += TAU;
        }
        // A rounding-level half turn may come out with the wrong sign.
        if delta.abs() > TAU {
            delta = delta.signum() * TAU;
        }
        let a = Vec3::new(rx * c, rx * s, 0.0);
        let mut b = Vec3::new(-ry * s, ry * c, 0.0);
        let mut lo = theta1;
        if delta < 0.0 {
            // Reverse the parameter: o + a·cos(−t) + (−b)·sin(−t) is the same point.
            b = -b;
            lo = -theta1;
        }
        let range = Interval {
            lo,
            hi: lo + delta.abs(),
        };
        self.add(arc(p3(centre), a, b, range));
        self.cur = p;
    }
}

/// Why path data stopped early.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PathError {
    pub(crate) at: usize,
    pub(crate) msg: String,
}

/// Parses path data into `b`. On an error, the curves before it are kept,
/// as SVG renderers do, and the error says where it stopped.
pub(crate) fn parse_path(d: &str, b: &mut Builder) -> Result<(), PathError> {
    let mut sc = Scanner::new(d);
    sc.skip_ws();
    let err = |sc: &Scanner<'_>, msg: &str| PathError {
        at: sc.i,
        msg: msg.to_owned(),
    };
    let mut cmd: Option<u8> = None;
    let mut first = true;
    // Last control point and whether it came from a cubic (`c`) or quadratic.
    let mut last_ctrl: Option<([f64; 2], bool)> = None;
    while !sc.at_end() {
        if b.overflow {
            return Ok(());
        }
        let c = match sc.peek() {
            Some(ch) if ch.is_ascii_alphabetic() => {
                sc.i += 1;
                sc.skip_ws();
                ch
            }
            Some(_) if sc.number_next() => match cmd {
                // Repeated arguments; after a moveto they are linetos.
                Some(b'M') => b'L',
                Some(b'm') => b'l',
                Some(c) if c != b'Z' && c != b'z' => c,
                _ => return Err(err(&sc, "numbers without a command")),
            },
            _ => return Err(err(&sc, "unexpected character")),
        };
        if first && !matches!(c, b'M' | b'm') {
            return Err(err(&sc, "path data must start with M"));
        }
        first = false;
        let rel = c.is_ascii_lowercase();
        let o = if rel { b.cur } else { [0.0, 0.0] };
        let pt = |sc: &mut Scanner<'_>| -> Result<[f64; 2], PathError> {
            let x = sc.number().ok_or_else(|| err(sc, "number expected"))?;
            let y = sc.number().ok_or_else(|| err(sc, "number expected"))?;
            Ok([o[0] + x, o[1] + y])
        };
        let mut ctrl = None;
        match c.to_ascii_uppercase() {
            b'M' => {
                let p = pt(&mut sc)?;
                b.move_to(p);
            }
            b'L' => {
                let p = pt(&mut sc)?;
                b.line_to(p);
            }
            b'H' => {
                let x = sc.number().ok_or_else(|| err(&sc, "number expected"))?;
                let x = if rel { b.cur[0] + x } else { x };
                b.line_to([x, b.cur[1]]);
            }
            b'V' => {
                let y = sc.number().ok_or_else(|| err(&sc, "number expected"))?;
                let y = if rel { b.cur[1] + y } else { y };
                b.line_to([b.cur[0], y]);
            }
            b'C' => {
                let (c1, c2, p) = (pt(&mut sc)?, pt(&mut sc)?, pt(&mut sc)?);
                b.cubic_to(c1, c2, p);
                ctrl = Some((c2, true));
            }
            b'S' => {
                let c1 = reflect(b.cur, last_ctrl, true);
                let (c2, p) = (pt(&mut sc)?, pt(&mut sc)?);
                b.cubic_to(c1, c2, p);
                ctrl = Some((c2, true));
            }
            b'Q' => {
                let (c1, p) = (pt(&mut sc)?, pt(&mut sc)?);
                b.quad_to(c1, p);
                ctrl = Some((c1, false));
            }
            b'T' => {
                let c1 = reflect(b.cur, last_ctrl, false);
                let p = pt(&mut sc)?;
                b.quad_to(c1, p);
                ctrl = Some((c1, false));
            }
            b'A' => {
                let rx = sc.number().ok_or_else(|| err(&sc, "number expected"))?;
                let ry = sc.number().ok_or_else(|| err(&sc, "number expected"))?;
                let rot = sc.number().ok_or_else(|| err(&sc, "number expected"))?;
                let large = sc.flag().ok_or_else(|| err(&sc, "arc flag expected"))?;
                let sweep = sc.flag().ok_or_else(|| err(&sc, "arc flag expected"))?;
                let p = pt(&mut sc)?;
                b.arc_to(rx, ry, rot, large, sweep, p);
            }
            b'Z' => b.close(),
            _ => return Err(err(&sc, "unknown command")),
        }
        last_ctrl = ctrl;
        cmd = Some(c);
    }
    Ok(())
}

/// The first control point of a smooth curve: the previous control point
/// reflected in the current point if the previous segment was of the same
/// kind, else the current point.
fn reflect(cur: [f64; 2], last: Option<([f64; 2], bool)>, cubic: bool) -> [f64; 2] {
    match last {
        Some((c, k)) if k == cubic => [2.0 * cur[0] - c[0], 2.0 * cur[1] - c[1]],
        _ => cur,
    }
}

/// A full ellipse with semi-axes along x and y, as a circle when equal.
pub(crate) fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> Option<WireCurve> {
    arc(
        Point3::new(cx, cy, 0.0),
        Vec3::new(rx, 0.0, 0.0),
        Vec3::new(0.0, ry, 0.0),
        Interval { lo: 0.0, hi: TAU },
    )
}

/// The outline of a rectangle with corner radii, as the SVG 2 `rect`
/// equivalent path (§10.2): clockwise on screen from the top edge.
pub(crate) fn rect(b: &mut Builder, x: f64, y: f64, w: f64, h: f64, rx: f64, ry: f64) {
    b.move_to([x + rx, y]);
    b.line_to([x + w - rx, y]);
    b.arc_to(rx, ry, 0.0, false, true, [x + w, y + ry]);
    b.line_to([x + w, y + h - ry]);
    b.arc_to(rx, ry, 0.0, false, true, [x + w - rx, y + h]);
    b.line_to([x + rx, y + h]);
    b.arc_to(rx, ry, 0.0, false, true, [x, y + h - ry]);
    b.line_to([x, y + ry]);
    b.arc_to(rx, ry, 0.0, false, true, [x + rx, y]);
}
