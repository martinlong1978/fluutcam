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
use crate::surface::{Probe, Side, SurfaceQuery};
use fluut_geom::{Point2, Vec2};
use rayon::prelude::*;
use std::f64::consts::FRAC_PI_2;

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
/// Classifying drops are exact within `WINDOW · max_step` of the level
/// and stop early beyond it.
const WINDOW: f64 = 4.0;

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
        // Lines are independent: in parallel, collected in order (D01 §6).
        let lines: Vec<SeedLine> = ys
            .par_iter()
            .map(|&y| {
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
                SeedLine { y, prof }
            })
            .collect();
        w.lines = lines;
        w
    }

    /// The contours of each level of `zs`, in parallel, in the order of
    /// `zs` (D01 §6: levels are independent).
    pub fn levels(&self, zs: &[f64]) -> Vec<Vec<Contour>> {
        zs.par_iter().map(|&z| self.level(z)).collect()
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

    /// The side of level `z` at `p` (classified at z + δ), with τ − (z + δ)
    /// clamped to the window.
    fn side(&self, p: Point2, z: f64) -> Side {
        self.q
            .above(p.x(), p.y(), z + DELTA, WINDOW * self.max_step)
    }

    /// The boundary on the segment from `inn` (inside) to `out` (outside),
    /// to [`CUTOFF`]: the outside end and the full probe at the inside
    /// end.
    ///
    /// Regula falsi on τ − z (Illinois), with bisection when it stalls. At
    /// a cliff (the tip far above the level, so τ jumps) the inside end's
    /// contact `P` holds the tip up until it leaves the footprint, so the
    /// boundary is no nearer `out` than where |axis − P| = R. The trial is
    /// just beyond that point: outside, it closes the bracket at once;
    /// inside, its contact gives the next such point.
    fn bisect(
        &self,
        mut inn: Point2,
        mut out: Point2,
        z: f64,
        si: Side,
        so: Side,
    ) -> Option<(Point2, Probe)> {
        let (mut gin, mut gout) = (si.g, so.g);
        let mut cin = si.contact;
        let w = WINDOW * self.max_step;
        let mut side = 0i32;
        let mut width = (out - inn).length();
        let mut stall = 0;
        // Cliff steps, while those that land inside move the inside end by
        // less than half the move before (a steep wall, not a cliff: its
        // contact slides as the axis moves, and the steps barely shrink).
        let mut cliffs = true;
        let mut moved = f64::INFINITY;
        while (out - inn).length() > CUTOFF {
            let d = out - inn;
            let len = d.length();
            let cliff = cin.filter(|_| gin >= w && cliffs).and_then(|p| {
                let s = exit(inn, d * (1.0 / len), p, self.q.radius())?;
                (s > 0.5 * CUTOFF && s < len - 0.5 * CUTOFF).then_some(s / len)
            });
            let frac = if let Some(f) = cliff {
                f + 0.45 * CUTOFF / len
            } else if stall >= 2 || !(gin > 0.0 && gout <= 0.0) {
                0.5
            } else {
                (gin / (gin - gout)).clamp(0.05, 0.95)
            };
            let m = inn + d * frac;
            let sm = self.side(m, z);
            if sm.inside {
                if cliff.is_some() {
                    let mv = (m - inn).length();
                    cliffs = mv < 0.5 * moved;
                    moved = mv;
                }
                inn = m;
                gin = sm.g;
                cin = sm.contact;
                if side == 1 {
                    gout *= 0.5;
                }
                side = 1;
            } else {
                out = m;
                gout = sm.g.min(0.0);
                if side == -1 {
                    gin *= 0.5;
                }
                side = -1;
                if let (Some(f), Some(p)) = (cliff, cin) {
                    // Just inside the exit point P still holds the tip up.
                    let q = inn + d * (f - 0.45 * CUTOFF / len).max(0.0);
                    if self
                        .q
                        .tip_touching(q.x(), q.y(), p)
                        .is_some_and(|t| t > z + DELTA)
                    {
                        inn = q;
                    }
                }
            }
            let w = (out - inn).length();
            if w > 0.5 * width {
                stall += 1;
            } else {
                stall = 0;
                width = w;
            }
        }
        Some((out, self.q.probe(inn.x(), inn.y())?))
    }

    /// The boundary on the line through `c` along `dir` (unit, towards the
    /// material), within `reach` of `c`. A Newton step on τ − z first: τ
    /// rises at tan β along the contact direction, so where the probe at
    /// `c` is exact and not a cliff the root is about −(τ − z) / tan β
    /// away. A bracket of ±5 % around it, or else one grown from `guess`
    /// by doubling, then [`Waterline::bisect`].
    fn locate(
        &self,
        c: Point2,
        dir: Vec2,
        z: f64,
        guess: f64,
        reach: f64,
    ) -> Option<(Point2, Probe)> {
        let sc = self.side(c, z);
        // Towards the boundary.
        let sign = if sc.inside { -1.0 } else { 1.0 };
        let at = |s: f64| c + dir * s;
        let newton = sc.probe.and_then(|pr| {
            let along = pr.toward.dot(dir);
            if along < 0.5 || pr.slope < 1e-3 || pr.slope > FRAC_PI_2 - 1e-6 {
                return None;
            }
            let s = -sc.g / (libm::tan(pr.slope) * along);
            (s * sign >= 0.0 && s.abs() <= reach).then_some(s)
        });
        // The last point on c's side and the first beyond.
        let mut near = (0.0, sc);
        let mut far: Option<(f64, Side)> = None;
        let mut step = guess.max(4.0 * CUTOFF);
        if let Some(sn) = newton {
            let e = (0.05 * sn.abs()).max(4.0 * CUTOFF);
            let s1 = sn + sign * e;
            let a1 = self.side(at(s1), z);
            if a1.inside != sc.inside {
                far = Some((s1, a1));
                let s0 = sn - sign * e;
                if s0 * sign > 0.0 {
                    let a0 = self.side(at(s0), z);
                    if a0.inside == sc.inside {
                        near = (s0, a0);
                    } else {
                        far = Some((s0, a0));
                    }
                }
            } else {
                near = (s1, a1);
                step = step.max(e);
            }
        }
        while far.is_none() {
            let s = near.0 + sign * step;
            if s.abs() > reach * (1.0 + 1e-12) {
                return None;
            }
            let a = self.side(at(s), z);
            if a.inside != sc.inside {
                far = Some((s, a));
            } else {
                near = (s, a);
                step *= 2.0;
            }
        }
        let far = far?;
        let (inn, out) = if sc.inside { (near, far) } else { (far, near) };
        self.bisect(at(inn.0), at(out.0), z, inn.1, out.1)
    }

    /// The boundary on the circle of radius `r` about `p`, searched from
    /// the open side (−`n`) through the travel direction `t` to the
    /// material side (`n`): the fallback at inside corners, where the line
    /// along `n` runs parallel to the next wall.
    fn around(&self, p: Point2, t: Vec2, n: Vec2, r: f64, z: f64) -> Option<(Point2, Probe)> {
        const K: usize = 16;
        let at = |th: f64| {
            let (s, c) = libm::sincos(th);
            p + (t * c + n * s) * r
        };
        let half = std::f64::consts::FRAC_PI_2;
        let mut prev = (-half, self.side(at(-half), z).inside);
        for i in 1..=K {
            let th = -half + std::f64::consts::PI * i as f64 / K as f64;
            let cur = (th, self.side(at(th), z).inside);
            if !prev.1 && cur.1 {
                let (mut lo, mut hi) = (prev.0, cur.0);
                while (at(hi) - at(lo)).length() > CUTOFF {
                    let m = 0.5 * (lo + hi);
                    if self.side(at(m), z).inside {
                        hi = m;
                    } else {
                        lo = m;
                    }
                }
                let q = at(hi);
                return Some((at(lo), self.q.probe(q.x(), q.y())?));
            }
            prev = cur;
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
        // Crossings already covered: per line, the traced segments that
        // cross it.
        let mut covered: Vec<Vec<(Point2, Point2)>> = vec![Vec::new(); self.lines.len()];
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
                let crossing = |&(a, b): &(Point2, Point2)| {
                    let x = if (b.y() - a.y()).abs() < 1e-300 {
                        a.x()
                    } else {
                        a.x() + (b.x() - a.x()) * (line.y - a.y()) / (b.y() - a.y())
                    };
                    x >= lo && x <= hi
                };
                if covered[li].iter().any(crossing) {
                    continue;
                }
                let (pa, pb) = (Point2::new(xa, line.y), Point2::new(xb, line.y));
                let (inn, outp) = if ta > th { (pa, pb) } else { (pb, pa) };
                // The profile may be off by its chord error: check.
                let (si, so) = (self.side(inn, z), self.side(outp, z));
                if !si.inside || so.inside {
                    continue;
                }
                let Some((p, pin)) = self.bisect(inn, outp, z, si, so) else {
                    continue;
                };
                let n0 = if inn.x() > outp.x() {
                    Vec2::new(1.0, 0.0)
                } else {
                    Vec2::new(-1.0, 0.0)
                };
                // A contour nearly along the line crosses it far from the
                // profile's crossing: check the distance too. The chord
                // rule keeps traced segments within tol/4 of the contour.
                if covered[li]
                    .iter()
                    .any(|&(a, b)| seg_dist(p, a, b) <= 0.5 * self.tol)
                {
                    continue;
                }
                let seed = self.point(p, &pin, n0, z);
                let c = self.trace(seed, z);
                self.cover(&c, &mut covered);
                if !self.thin(&c, z) {
                    out.push(c);
                }
            }
        }
        out
    }

    /// A lost contour around material less than `tol/4` above the level
    /// for `max_step` inwards: noise at a tangent level (CH-03), such as a
    /// wall's top edge a micron above the flat it meets (a model gap).
    /// Leaving it uncut leaves less than the tolerance.
    fn thin(&self, c: &Contour, z: f64) -> bool {
        if c.closed || (c.ends.0 != End::Lost && c.ends.1 != End::Lost) {
            return false;
        }
        c.pts.iter().all(|w| {
            let q = w.p + w.n * self.max_step;
            self.q
                .drop(q.x(), q.y())
                .is_none_or(|h| h.tip <= z + 0.25 * self.tol)
        })
    }

    /// Marks the seed-line crossings of `c`.
    fn cover(&self, c: &Contour, covered: &mut [Vec<(Point2, Point2)>]) {
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
                cov.push((a, b));
            }
        }
    }

    /// The contour through `seed`: forward (climb) until it closes or ends,
    /// then backward from the seed if it did not close.
    fn trace(&self, seed: WPoint, z: f64) -> Contour {
        let (fwd, f_end) = self.march(seed, z, 1.0);
        let Some(f_end) = f_end else {
            return closed(fwd);
        };
        let (back, b_end) = self.march(seed, z, -1.0);
        join(fwd, f_end, back, b_end)
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
        // Turn of the contact direction per mm over the last step (signed),
        // and how far the last point was from its prediction.
        let mut kappa = 0.0;
        let mut miss = self.tol;
        loop {
            if pts.len() >= MAX_POINTS || travelled > limit || tiny > MAX_TINY {
                return (pts, Some(End::Lost));
            }
            let cur = pts[pts.len() - 1];
            let t = Vec2::new(-cur.n.y(), cur.n.x()) * sign;
            // Predict along the arc of the last step's curvature: the chord
            // turns by half the expected turn, the normal by all of it.
            let th = (kappa * step).clamp(-0.5, 0.5);
            let c = cur.p + rotate(t, 0.5 * th) * step;
            let found = self
                .locate(c, rotate(cur.n, th), z, 2.0 * miss, 2.0 * step)
                .or_else(|| self.around(cur.p, t, cur.n, step, z));
            let ok = found.and_then(|(p, pin)| {
                let nxt = self.point(p, &pin, cur.n, z);
                let d = nxt.p - cur.p;
                let len = d.length();
                if len <= 0.0 || d.dot(t) <= 0.25 * len || len > 3.0 * step {
                    return None;
                }
                let turn = libm::atan2(cur.n.perp_dot(nxt.n), cur.n.dot(nxt.n));
                let sag = 0.125 * len * libm::fabs(turn);
                (sag <= 0.25 * self.tol || step <= min_step).then_some((nxt, len, sag, turn))
            });
            let Some((mut nxt, len, sag, turn)) = ok else {
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
            // Back at the seed, heading the same way: close. A lap that
            // misses the seed by a little (drop noise on free-form faces)
            // still closes; without that it ran on to the length limit.
            if pts.len() >= 3
                && travelled > 4.0 * self.tol
                && seg_dist(seed.p, cur.p, nxt.p) <= (0.5 * self.tol).max(0.25 * len)
                && seed.n.dot(nxt.n) > 0.5
            {
                pts.push(seed);
                return (pts, None);
            }
            travelled += len;
            miss = (nxt.p - c).length();
            // A kink (an inside corner) is not a curvature to predict with.
            kappa = if libm::fabs(turn) < 0.5 {
                turn / len
            } else {
                0.0
            };
            pts.push(nxt);
            tiny = if step <= min_step { tiny + 1 } else { 0 };
            if sag < 0.1 * self.tol {
                step = (1.5 * step).min(self.max_step);
            }
            // The chord rule ahead: sagitta κ L² / 8 within 0.2 tol.
            if kappa != 0.0 {
                let fit = libm::sqrt(1.6 * self.tol / libm::fabs(kappa));
                step = step.min(fit).max(min_step);
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
            let m = match self.locate(mid, n, z, 0.125 * len, len) {
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

fn closed(pts: Vec<WPoint>) -> Contour {
    Contour {
        pts,
        closed: true,
        ends: (End::Region, End::Region),
    }
}

/// The contour from a forward trace that ended at `f_end` and the
/// backward trace from the same seed. A backward trace that closed went
/// round the whole loop (the forward one stopped at a spot it could not
/// pass): it is the contour, in climb order.
fn join(fwd: Vec<WPoint>, f_end: End, mut back: Vec<WPoint>, b_end: Option<End>) -> Contour {
    back.reverse();
    let Some(b_end) = b_end else {
        return closed(back);
    };
    back.pop();
    back.extend(fwd);
    Contour {
        pts: back,
        closed: false,
        ends: (b_end, f_end),
    }
}

/// How far from `a` along `u` (unit) the point `p` leaves the disc of
/// radius `r` about the axis, in XY; `None` if `a` is not in it.
fn exit(a: Point2, u: Vec2, p: fluut_geom::Point3, r: f64) -> Option<f64> {
    let w = Vec2::new(a.x() - p.x(), a.y() - p.y());
    let b = w.dot(u);
    let disc = b * b - w.dot(w) + r * r;
    (w.dot(w) <= r * r && disc >= 0.0).then(|| -b + libm::sqrt(disc))
}

/// `v` turned by `th` radians (counter-clockwise).
fn rotate(v: Vec2, th: f64) -> Vec2 {
    let (s, c) = libm::sincos(th);
    Vec2::new(c * v.x() - s * v.y(), s * v.x() + c * v.y())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn wp(x: f64, y: f64) -> WPoint {
        WPoint {
            p: Point2::new(x, y),
            slope: 0.0,
            drive: true,
            n: Vec2::new(1.0, 0.0),
        }
    }

    #[test]
    fn a_backward_trace_that_closes_is_the_contour() {
        // nist_ctc_02 at Z −185 (5 mm): the forward trace stopped at an
        // inside corner, the backward one went round and closed.
        let s = wp(0.0, 0.0);
        let fwd = vec![s, wp(0.0, -1.0)];
        let back = vec![s, wp(0.0, 1.0), wp(1.0, 1.0), wp(1.0, -1.0), s];
        let c = join(fwd.clone(), End::Lost, back.clone(), None);
        assert!(c.closed);
        let mut want = back.clone();
        want.reverse();
        assert_eq!(c.pts, want);
        // Neither closed: back reversed, then forward, sharing the seed.
        let c = join(fwd, End::Lost, back[..3].to_vec(), Some(End::Region));
        assert!(!c.closed);
        assert_eq!(c.ends, (End::Region, End::Lost));
        assert_eq!(c.pts, vec![wp(1.0, 1.0), wp(0.0, 1.0), s, wp(0.0, -1.0)]);
    }
}
