// SPDX-License-Identifier: MIT OR Apache-2.0

//! Constant-Z CL contours (waterlines) from the drop function (D07 §7,
//! D09 §4.4).
//!
//! The contour at level z is the boundary of {τ > z}, where τ(x, y) is the
//! tip height of the drop with the axis through (x, y). It is found
//! without a plane section, so it is exact for any tool and wall shape:
//!
//! - **Seeds.** Lines along X across the region, at most `R/2` apart. Each
//!   line's τ profile is sampled once (chords within `tol/4`); every level
//!   crossing on a line is a seed. One profile serves every level.
//! - **Tracing.** Step along the contour, then correct along the contact
//!   direction (the unit XY vector from the axis to the contact, along
//!   which τ rises) by a bracketed root search to [`CUTOFF`]. No gradient is
//!   needed, so a flat tool against a wall, where τ jumps, is traced too.
//!   Each point is on the τ ≤ z side.
//! - **Tangent levels (CH-03).** A level at the Z of a horizontal face
//!   makes τ = z on an area. Every level is classified against z + δ
//!   ([`DELTA`]): the section just above.

use crate::follow::CUTOFF;
use crate::layout::layout_even;
use crate::region::{Inside, Region2};
use crate::surface::{Probe, SurfaceQuery};
use fluut_geom::{Point2, Vec2};

/// A probe this far above the level just inside the contour is on top of a
/// cliff (τ jumps), mm.
const CLIFF: f64 = 1e-3;
/// Levels are classified against z + δ (CH-03), mm.
pub const DELTA: f64 = 1e-6;
/// Most points one contour may have (a guard against a runaway trace).
const MAX_POINTS: usize = 2_000_000;
/// Most consecutive points at the smallest step: a kink takes a few, a
/// spot the trace cannot follow (a missed drop) takes many.
const MAX_TINY: usize = 64;

/// One contour point: on the τ ≤ z side within [`CUTOFF`] of the boundary,
/// with the slope and drive flag of the contact just inside.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct WPoint {
    pub p: Point2,
    pub slope: f64,
    pub drive: bool,
    /// Unit XY normal towards the material ({τ > z}).
    pub n: Vec2,
}

/// How an open contour ends.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum End {
    /// On the region boundary.
    Region,
    /// Tracing gave up inside the region (`cam.contour.loop_open`).
    Lost,
}

/// A traced contour, in climb order: the material on the right.
#[derive(Clone, PartialEq, Debug)]
pub struct Contour {
    pub pts: Vec<WPoint>,
    /// Closed loops repeat the first point at the end.
    pub closed: bool,
    /// For open contours: how the start and the end were reached.
    pub ends: (End, End),
}

/// The τ profile of one seed line.
#[derive(Clone, Debug)]
struct SeedLine {
    y: f64,
    /// `(x, τ)` in order, τ = −∞ where nothing is under the tool.
    prof: Vec<(f64, f64)>,
}

/// Waterlines of one surface query over a region.
#[derive(Debug)]
pub struct Waterline<'a> {
    pub q: &'a SurfaceQuery<'a>,
    pub region: &'a Region2,
    pub tol: f64,
    /// Longest step along a contour and between profile samples.
    pub max_step: f64,
    lines: Vec<SeedLine>,
    y0: f64,
    dy: f64,
}

impl<'a> Waterline<'a> {
    /// Samples the seed lines. `spacing` is the largest gap between them.
    pub fn new(
        q: &'a SurfaceQuery<'a>,
        region: &'a Region2,
        tol: f64,
        max_step: f64,
        spacing: f64,
    ) -> Waterline<'a> {
        let mut w = Waterline {
            q,
            region,
            tol,
            max_step,
            lines: Vec::new(),
            y0: 0.0,
            dy: 1.0,
        };
        if region.is_empty() {
            return w;
        }
        let (lo, hi) = region.bbox();
        let ys = layout_even(lo.y(), hi.y(), spacing);
        if ys.len() > 1 {
            w.dy = ys[1] - ys[0];
        }
        w.y0 = ys[0];
        for y in ys {
            let mut prof = Vec::new();
            for (t0, t1) in region.clip_line(Point2::new(0.0, y), Vec2::new(1.0, 0.0)) {
                if t1 - t0 <= CUTOFF {
                    continue;
                }
                // A span boundary is not a crossing: separate spans by NaN.
                if !prof.is_empty() {
                    prof.push((f64::NAN, f64::NAN));
                }
                w.profile(y, t0, t1, &mut prof);
            }
            w.lines.push(SeedLine { y, prof });
        }
        w
    }

    fn tau(&self, p: Point2) -> f64 {
        self.q
            .drop(p.x(), p.y())
            .map_or(f64::NEG_INFINITY, |h| h.tip)
    }

    /// Adaptive τ profile of `y` over `[x0, x1]`.
    fn profile(&self, y: f64, x0: f64, x1: f64, out: &mut Vec<(f64, f64)>) {
        let a = (x0, self.tau(Point2::new(x0, y)));
        let b = (x1, self.tau(Point2::new(x1, y)));
        out.push(a);
        let mut stack = vec![(a, b)];
        while let Some((a, b)) = stack.pop() {
            let len = b.0 - a.0;
            let mx = 0.5 * (a.0 + b.0);
            let split = if len > self.max_step {
                true
            } else if len <= 0.25 * self.tol {
                false
            } else {
                let m = self.tau(Point2::new(mx, y));
                let dev = if a.1.is_finite() && b.1.is_finite() && m.is_finite() {
                    (m - 0.5 * (a.1 + b.1)).abs()
                } else if a.1.is_finite() == b.1.is_finite() && a.1.is_finite() == m.is_finite() {
                    0.0
                } else {
                    f64::INFINITY
                };
                dev > 0.25 * self.tol
            };
            if split {
                let m = (mx, self.tau(Point2::new(mx, y)));
                stack.push((m, b));
                stack.push((a, m));
            } else {
                out.push(b);
            }
        }
    }

    /// Whether `p` is inside {τ > z} at level `z` (classified at z + δ).
    fn inside(&self, p: Point2, z: f64) -> (bool, Option<Probe>) {
        let pr = self.q.probe(p.x(), p.y());
        (pr.is_some_and(|h| h.tip > z + DELTA), pr)
    }

    /// The boundary on the segment from `inn` (inside) to `out` (outside),
    /// to [`CUTOFF`]: the outside end and the probe at the inside end.
    fn bisect(&self, mut inn: Point2, mut out: Point2, z: f64, mut pin: Probe) -> (Point2, Probe) {
        let full = (out - inn).length();
        // Regula falsi on τ − z (Illinois), with bisection when it stalls.
        let g = |pr: Option<&Probe>| pr.map_or(-full.max(1.0), |h| h.tip - (z + DELTA));
        let mut gin = g(Some(&pin));
        let mut gout = g(self.q.probe(out.x(), out.y()).as_ref());
        let mut side = 0i32;
        let mut width = full;
        let mut stall = 0;
        while (out - inn).length() > CUTOFF {
            let frac = if stall >= 2 || !(gin > 0.0 && gout <= 0.0) {
                0.5
            } else {
                (gin / (gin - gout)).clamp(0.05, 0.95)
            };
            let m = inn + (out - inn) * frac;
            let (is_in, pr) = self.inside(m, z);
            let gm = g(pr.as_ref());
            if is_in {
                inn = m;
                gin = gm;
                pin = pr.unwrap_or(pin);
                if side == 1 {
                    gout *= 0.5;
                }
                side = 1;
            } else {
                out = m;
                gout = gm.min(0.0);
                if side == -1 {
                    gin *= 0.5;
                }
                side = -1;
            }
            let w = (out - inn).length();
            if w > 0.5 * width {
                stall += 1;
            } else {
                stall = 0;
                width = w;
            }
        }
        (out, pin)
    }

    /// The boundary on the line through `c` along `dir`, within `reach`.
    fn project(&self, c: Point2, dir: Vec2, reach: f64, z: f64) -> Option<(Point2, Probe)> {
        let (c_in, c_pr) = self.inside(c, z);
        let sign = if c_in { -1.0 } else { 1.0 };
        let mut prev = (c, c_pr);
        let mut d = 0.125 * reach;
        while d <= reach * (1.0 + 1e-12) {
            let p = c + dir * (sign * d);
            let (p_in, pr) = self.inside(p, z);
            if p_in != c_in {
                let (inn, out, pin) = if c_in {
                    (prev.0, p, prev.1?)
                } else {
                    (p, prev.0, pr?)
                };
                return Some(self.bisect(inn, out, z, pin));
            }
            prev = (p, pr);
            d *= 2.0;
        }
        None
    }

    /// A contour point at `p` from the probe just inside. At a cliff (τ
    /// jumps: a vertical wall), the probe touches the top of the wall, so
    /// the slope and drive flag come from the wall face at the level.
    fn point(&self, p: Point2, pin: &Probe, n_old: Vec2, z: f64) -> WPoint {
        let n = if pin.toward.length() > 0.5 {
            pin.toward
        } else {
            n_old
        };
        let (mut slope, mut drive) = (pin.slope, pin.drive);
        if pin.tip > z + CLIFF
            && let Some((s, d)) = self.q.wall_at(p, n, z, 0.1 * self.tol.max(1e-3))
        {
            (slope, drive) = (s, d);
        }
        WPoint { p, slope, drive, n }
    }

    /// Every contour of level `z` in the region, in seed order.
    pub fn level(&self, z: f64) -> Vec<Contour> {
        let mut out: Vec<Contour> = Vec::new();
        // Crossings already covered: (line, x) of traced contours.
        let mut covered: Vec<Vec<f64>> = vec![Vec::new(); self.lines.len()];
        for li in 0..self.lines.len() {
            let line = &self.lines[li];
            let th = z + DELTA;
            for w in line.prof.windows(2) {
                let ((xa, ta), (xb, tb)) = (w[0], w[1]);
                if xa.is_nan() || xb.is_nan() || (ta > th) == (tb > th) {
                    continue;
                }
                let lo = xa.min(xb) - self.tol;
                let hi = xa.max(xb) + self.tol;
                if covered[li].iter().any(|&x| x >= lo && x <= hi) {
                    continue;
                }
                let (pa, pb) = (Point2::new(xa, line.y), Point2::new(xb, line.y));
                let (inn, outp) = if ta > th { (pa, pb) } else { (pb, pa) };
                // The profile may be off by its chord error: check.
                let (in_ok, pin) = self.inside(inn, z);
                let (out_in, _) = self.inside(outp, z);
                let Some(pin) = pin.filter(|_| in_ok && !out_in) else {
                    continue;
                };
                let (p, pin) = self.bisect(inn, outp, z, pin);
                let n0 = if inn.x() > outp.x() {
                    Vec2::new(1.0, 0.0)
                } else {
                    Vec2::new(-1.0, 0.0)
                };
                let seed = self.point(p, &pin, n0, z);
                let c = self.trace(seed, z);
                self.cover(&c, &mut covered);
                out.push(c);
            }
        }
        out
    }

    /// Marks the seed-line crossings of `c`.
    fn cover(&self, c: &Contour, covered: &mut [Vec<f64>]) {
        if self.lines.is_empty() {
            return;
        }
        for w in c.pts.windows(2) {
            let (a, b) = (w[0].p, w[1].p);
            let (ylo, yhi) = (a.y().min(b.y()), a.y().max(b.y()));
            let j0 = ((ylo - self.y0) / self.dy - 1.0).floor().max(0.0) as usize;
            let j1 = (((yhi - self.y0) / self.dy + 1.0).ceil().max(0.0) as usize)
                .min(self.lines.len() - 1);
            for (j, cov) in covered.iter_mut().enumerate().take(j1 + 1).skip(j0) {
                let y = self.lines[j].y;
                if y < ylo || y > yhi {
                    continue;
                }
                let x = if (b.y() - a.y()).abs() < 1e-300 {
                    a.x()
                } else {
                    a.x() + (b.x() - a.x()) * (y - a.y()) / (b.y() - a.y())
                };
                cov.push(x);
            }
        }
    }

    /// The contour through `seed`: forward (climb) until it closes or ends,
    /// then backward from the seed if it did not close.
    fn trace(&self, seed: WPoint, z: f64) -> Contour {
        let (fwd, f_end) = self.march(seed, z, 1.0);
        if f_end.is_none() {
            return Contour {
                pts: fwd,
                closed: true,
                ends: (End::Region, End::Region),
            };
        }
        let (mut back, b_end) = self.march(seed, z, -1.0);
        back.reverse();
        back.pop();
        back.extend(fwd);
        Contour {
            pts: back,
            closed: false,
            ends: (b_end.unwrap_or(End::Lost), f_end.unwrap_or(End::Lost)),
        }
    }

    /// Steps along the contour from `seed`: `sign` 1 keeps the material on
    /// the right. `None` as the end: closed back at the seed.
    fn march(&self, seed: WPoint, z: f64, sign: f64) -> (Vec<WPoint>, Option<End>) {
        let mut pts = vec![seed];
        let min_step = (self.tol / 16.0).max(4.0 * CUTOFF);
        let mut step = (0.25 * self.max_step).max(min_step);
        let mut travelled = 0.0;
        let limit = 8.0 * self.region.perimeter() + 1e3 * self.max_step;
        // Consecutive points at the smallest step: thrashing, not tracing.
        let mut tiny = 0;
        loop {
            if pts.len() >= MAX_POINTS || travelled > limit || tiny > MAX_TINY {
                return (pts, Some(End::Lost));
            }
            let cur = pts[pts.len() - 1];
            let t = Vec2::new(-cur.n.y(), cur.n.x()) * sign;
            let c = cur.p + t * step;
            let found = self.project(c, cur.n, 2.0 * step, z);
            let ok = found.and_then(|(p, pin)| {
                let nxt = self.point(p, &pin, cur.n, z);
                let d = nxt.p - cur.p;
                let len = d.length();
                if len <= 0.0 || d.dot(t) <= 0.25 * len || len > 3.0 * step {
                    return None;
                }
                let turn = libm::fabs(libm::atan2(cur.n.perp_dot(nxt.n), cur.n.dot(nxt.n)));
                let sag = 0.125 * len * turn;
                (sag <= 0.25 * self.tol || step <= min_step).then_some((nxt, len, sag))
            });
            let Some((mut nxt, len, sag)) = ok else {
                if step <= min_step {
                    return (pts, Some(End::Lost));
                }
                step = (0.5 * step).max(min_step);
                continue;
            };
            // Out of the region: end on its boundary.
            if self.region.contains(nxt.p) == Inside::Out {
                let d = nxt.p - cur.p;
                let t1 = self
                    .region
                    .clip_line(cur.p, d)
                    .into_iter()
                    .find(|&(a, b)| a <= 1e-9 && b >= -1e-9)
                    .map_or(0.0, |iv| iv.1.clamp(0.0, 1.0));
                if t1 * d.length() > CUTOFF {
                    nxt.p = cur.p + d * t1;
                    pts.push(nxt);
                }
                return (pts, Some(End::Region));
            }
            // Back at the seed: close.
            if pts.len() >= 3
                && travelled > 4.0 * self.tol
                && seg_dist(seed.p, cur.p, nxt.p) <= 0.5 * self.tol
            {
                pts.push(seed);
                return (pts, None);
            }
            travelled += len;
            pts.push(nxt);
            tiny = if step <= min_step { tiny + 1 } else { 0 };
            if sag < 0.1 * self.tol {
                step = (1.5 * step).min(self.max_step);
            }
        }
    }

    /// The point where the classification `cut` changes between contour
    /// points `a` (whose class it is) and `b`, to [`CUTOFF`]: chord
    /// midpoints projected back onto the contour.
    pub fn cutoff(
        &self,
        mut a: WPoint,
        mut b: WPoint,
        z: f64,
        cut: impl Fn(&WPoint) -> bool,
    ) -> WPoint {
        let ca = cut(&a);
        while (b.p - a.p).length() > CUTOFF {
            let d = b.p - a.p;
            let len = d.length();
            let n = Vec2::new(d.y(), -d.x()) * (1.0 / len);
            // Project along the side the material is on.
            let n = if n.dot(a.n) < 0.0 { n * -1.0 } else { n };
            let mid = a.p + d * 0.5;
            let m = match self.project(mid, n, len, z) {
                Some((p, pin)) => self.point(p, &pin, a.n, z),
                None => break,
            };
            if (m.p - a.p).length() >= len || (m.p - b.p).length() >= len {
                break;
            }
            if cut(&m) == ca {
                a = m;
            } else {
                b = m;
            }
        }
        a
    }
}

fn seg_dist(p: Point2, a: Point2, b: Point2) -> f64 {
    let d = b - a;
    let l2 = d.dot(d);
    let t = if l2 > 0.0 {
        ((p - a).dot(d) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + d * t - p).length()
}
