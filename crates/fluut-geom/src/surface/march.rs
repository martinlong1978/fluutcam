// SPDX-License-Identifier: MIT OR Apache-2.0

//! Plane–surface marching (D02 §5.1) for surfaces without a closed-form section.
//!
//! 1. Seeds: each patch of the box (see `Surface::breaks`) is split into a
//!    `SEED_CELLS²` grid; sign changes of `f = n·S − h` along cell edges are
//!    solved to points on the section.
//! 2. March from each seed: predictor along the section tangent (in uv,
//!    perpendicular to ∇f), corrector by Newton back onto the plane. Step length
//!    is halved until the cubic Hermite chord between steps deviates from the
//!    true section by at most `machining / 2`.
//! 3. Stop at the box boundary (found by bisection), back at the start (closed
//!    loop), or where ∇f vanishes (tangential contact, CH-03).
//! 4. Fit: the points and tangents become a G1 cubic B-spline (Hermite spans).
//!
//! A loop lying entirely inside one seed cell is missed; the cell grid is fine
//! enough for machining features, and CH-03 tracks the general robustness work.

use super::section::{SectionCurve, SectionPlane, SurfaceSection};
use super::{Surface, UvBox};
use crate::bounds::Interval;
use crate::bspline::BSplineCurve3;
use crate::curve::Curve3;
use crate::roots::newton_bracketed;
use crate::tol::Tol;
use crate::types::{Point3, Vec3};
use std::sync::Arc;

/// Seed cells per patch side.
const SEED_CELLS: usize = 8;
/// Upper bound on march steps per curve.
const MAX_STEPS: usize = 100_000;
/// Newton iterations for the corrector.
const CORRECTOR_ITER: usize = 12;

/// One traced point: parameters, position and unit tangent (march direction).
#[derive(Copy, Clone, Debug)]
struct Node {
    u: f64,
    v: f64,
    p: Point3,
    t: Vec3,
}

struct Ctx<'a> {
    s: &'a Surface,
    uv: UvBox,
    plane: &'a SectionPlane,
    tol: &'a Tol,
}

impl Ctx<'_> {
    fn f(&self, u: f64, v: f64) -> f64 {
        self.plane.signed_distance(self.s.eval(u, v))
    }

    /// `f` and its uv gradient.
    fn f_grad(&self, u: f64, v: f64) -> (f64, f64, f64, Point3, Vec3, Vec3) {
        let d = self.s.derivs(u, v);
        let n = self.plane.normal().get();
        (
            self.plane.signed_distance(d.p),
            n.dot(d.su),
            n.dot(d.sv),
            d.p,
            d.su,
            d.sv,
        )
    }

    /// Newton back onto the plane along the uv gradient. `None` if it does not
    /// converge or leaves the box by more than a step.
    fn correct(&self, mut u: f64, mut v: f64) -> Option<(f64, f64)> {
        for _ in 0..CORRECTOR_ITER {
            let (f, gu, gv, ..) = self.f_grad(u, v);
            if f.abs() <= self.tol.solver {
                return Some((u, v));
            }
            let g2 = gu * gu + gv * gv;
            if g2 == 0.0 || !g2.is_finite() {
                return None;
            }
            u -= f * gu / g2;
            v -= f * gv / g2;
        }
        (self.f(u, v).abs() <= self.tol.solver * 10.0).then_some((u, v))
    }

    /// Node at a point on the section; tangent oriented along `dir_hint`.
    fn node(&self, u: f64, v: f64, dir_hint: Option<Vec3>) -> Option<Node> {
        let (_, gu, gv, p, su, sv) = self.f_grad(u, v);
        let t = (su * -gv + sv * gu).normalized()?.get();
        let t = match dir_hint {
            Some(h) if t.dot(h) < 0.0 => -t,
            _ => t,
        };
        Some(Node { u, v, p, t })
    }

    /// uv step that moves about `h` in 3-D along the node's tangent.
    fn uv_step(&self, n: &Node, h: f64) -> (f64, f64) {
        let (_, gu, gv, _, su, sv) = self.f_grad(n.u, n.v);
        let (mut du, mut dv) = (-gv, gu);
        let speed = (su * du + sv * dv).length();
        if speed == 0.0 {
            return (0.0, 0.0);
        }
        if (su * du + sv * dv).dot(n.t) < 0.0 {
            du = -du;
            dv = -dv;
        }
        (du * h / speed, dv * h / speed)
    }

    fn inside(&self, u: f64, v: f64) -> bool {
        self.uv.contains(u, v)
    }

    /// Periods of the directions in which the box spans a whole period; the
    /// march wraps across those seams instead of stopping.
    fn wrap_periods(&self) -> (Option<f64>, Option<f64>) {
        let (pu, pv) = self.s.periodicity();
        let full = |p: Option<f64>, iv: Interval| p.filter(|&p| iv.length() >= p * (1.0 - 1e-12));
        (full(pu, self.uv.u), full(pv, self.uv.v))
    }

    /// Wraps parameters across full-period seams into the box.
    fn wrap(&self, u: f64, v: f64) -> (f64, f64) {
        let (pu, pv) = self.wrap_periods();
        let w = |x: f64, p: Option<f64>, iv: Interval| match p {
            Some(p) if x < iv.lo => x + p,
            Some(p) if x > iv.hi => x - p,
            _ => x,
        };
        (w(u, pu, self.uv.u), w(v, pv, self.uv.v))
    }

    /// Midpoint of two parameter pairs, taking the short way across seams.
    fn mid_uv(&self, a: &Node, b: &Node) -> (f64, f64) {
        let (pu, pv) = self.wrap_periods();
        let unwrap = |x: f64, ref_x: f64, p: Option<f64>| match p {
            Some(p) if x - ref_x > 0.5 * p => x - p,
            Some(p) if ref_x - x > 0.5 * p => x + p,
            _ => x,
        };
        let (bu, bv) = (unwrap(b.u, a.u, pu), unwrap(b.v, a.v, pv));
        self.wrap(0.5 * (a.u + bu), 0.5 * (a.v + bv))
    }
}

pub(super) fn march_section(
    s: &Surface,
    uv: UvBox,
    plane: &SectionPlane,
    tol: &Tol,
) -> SurfaceSection {
    let cx = Ctx { s, uv, plane, tol };
    let (seeds, coincident) = find_seeds(&cx);
    let mut out = SurfaceSection {
        coincident,
        ..SurfaceSection::default()
    };
    if coincident {
        return out;
    }
    let mut traced: Vec<Vec<Node>> = Vec::new();
    for (u, v) in seeds {
        let Some(seed) = cx.node(u, v, None) else {
            // ∇f = 0 on the section: tangential contact.
            out.touch_points.push(s.eval(u, v));
            continue;
        };
        if traced.iter().any(|c| near_traced(seed.p, c, tol.machining)) {
            continue;
        }
        let (fwd, closed) = trace(&cx, seed);
        let nodes = if closed {
            fwd
        } else {
            let back_seed = Node { t: -seed.t, ..seed };
            let (mut back, _) = trace(&cx, back_seed);
            back.reverse();
            for n in back.iter_mut() {
                n.t = -n.t;
            }
            back.pop(); // the seed itself, present in both
            back.extend(fwd);
            back
        };
        if nodes.len() < 2 {
            continue;
        }
        if let Some(curve) = hermite_bspline(&nodes) {
            let d = curve.domain();
            out.curves.push(SectionCurve {
                curve: Curve3::BSpline(Arc::new(curve)),
                range: Interval { lo: d.lo, hi: d.hi },
                closed,
                tangent: false,
            });
        }
        traced.push(nodes);
    }
    out
}

/// Seeds on cell edges where `f` changes sign; `coincident` if `f` is within
/// `tol.model` of zero at every grid point.
fn find_seeds(cx: &Ctx) -> (Vec<(f64, f64)>, bool) {
    let (bu, bv) = cx.s.breaks(cx.uv);
    let us = refine_breaks(&bu);
    let vs = refine_breaks(&bv);
    let grid: Vec<Vec<f64>> = us
        .iter()
        .map(|&u| vs.iter().map(|&v| cx.f(u, v)).collect())
        .collect();
    let coincident = grid.iter().flatten().all(|f| f.abs() <= cx.tol.model);
    if coincident {
        return (Vec::new(), true);
    }
    let mut seeds = Vec::new();
    let solve_u = |v: f64, a: f64, b: f64| {
        newton_bracketed(
            |u| {
                let (f, gu, ..) = cx.f_grad(u, v);
                (f, gu)
            },
            a,
            b,
            cx.tol.solver,
        )
        .ok()
    };
    let solve_v = |u: f64, a: f64, b: f64| {
        newton_bracketed(
            |v| {
                let (f, _, gv, ..) = cx.f_grad(u, v);
                (f, gv)
            },
            a,
            b,
            cx.tol.solver,
        )
        .ok()
    };
    for (i, &u) in us.iter().enumerate() {
        for (j, &v) in vs.iter().enumerate() {
            let f0 = grid[i][j];
            if j + 1 < vs.len()
                && f0.signum() != grid[i][j + 1].signum()
                && let Some(vr) = solve_v(u, v, vs[j + 1])
            {
                seeds.push((u, vr));
            }
            if i + 1 < us.len()
                && f0.signum() != grid[i + 1][j].signum()
                && let Some(ur) = solve_u(v, u, us[i + 1])
            {
                seeds.push((ur, v));
            }
        }
    }
    (seeds, false)
}

/// Splits each break interval into `SEED_CELLS` equal cells.
fn refine_breaks(breaks: &[f64]) -> Vec<f64> {
    let mut out = vec![breaks[0]];
    for w in breaks.windows(2) {
        for k in 1..=SEED_CELLS {
            out.push(w[0] + (w[1] - w[0]) * k as f64 / SEED_CELLS as f64);
        }
    }
    out
}

/// Marches from `seed` along its tangent. Returns the nodes and whether the
/// curve closed on itself.
fn trace(cx: &Ctx, seed: Node) -> (Vec<Node>, bool) {
    let tol = cx.tol.machining;
    let diag = {
        let a = cx.s.eval(cx.uv.u.lo, cx.uv.v.lo);
        let b = cx.s.eval(cx.uv.u.hi, cx.uv.v.hi);
        let c = cx.s.eval(cx.uv.u.lo, cx.uv.v.hi);
        a.distance(b).max(a.distance(c)).max(tol * 10.0)
    };
    let h_max = diag / 8.0;
    let mut h = (h_max / 4.0).max(tol);
    let mut nodes = vec![seed];
    let mut travelled = 0.0;
    for _ in 0..MAX_STEPS {
        let cur = *nodes.last().unwrap_or(&seed);
        let mut accepted: Option<(Node, bool)> = None;
        let mut h_try = h;
        for _ in 0..40 {
            let (du, dv) = cx.uv_step(&cur, h_try);
            if du == 0.0 && dv == 0.0 {
                break;
            }
            let (pu, pv) = cx.wrap(cur.u + du, cur.v + dv);
            let leaving = !cx.inside(pu, pv);
            let candidate = if leaving {
                boundary_node(cx, &cur, du, dv)
            } else {
                cx.correct(pu, pv)
                    .map(|(u, v)| cx.wrap(u, v))
                    .and_then(|(u, v)| cx.node(u, v, Some(cur.t)))
            };
            let Some(next) = candidate else {
                h_try *= 0.5;
                continue;
            };
            if next.p.distance(cur.p) == 0.0 {
                break;
            }
            if hermite_error(cx, &cur, &next) <= tol * 0.5 {
                accepted = Some((next, leaving));
                break;
            }
            h_try *= 0.5;
            if h_try < cx.tol.solver {
                break;
            }
        }
        let Some((next, at_boundary)) = accepted else {
            break;
        };
        let step = next.p.distance(cur.p);
        travelled += step;
        // Closed: back near the seed after going round, heading the same way.
        if travelled > 2.0 * step && next.p.distance(seed.p) < step && next.t.dot(seed.t) > 0.0 {
            // Replace the last node with the seed so the loop closes exactly,
            // provided the closing chord is accurate.
            if hermite_error(cx, &cur, &seed) <= tol * 0.5 {
                nodes.push(seed);
                return (nodes, true);
            }
        }
        nodes.push(next);
        if at_boundary {
            break;
        }
        h = (h_try * 1.5).min(h_max);
    }
    (nodes, false)
}

/// The point where the section leaves the box between `cur` and the predicted
/// step `(du, dv)`, found by bisection on the step fraction.
fn boundary_node(cx: &Ctx, cur: &Node, du: f64, dv: f64) -> Option<Node> {
    let (mut lo, mut hi) = (0.0, 1.0);
    let mut best: Option<(f64, f64)> = None;
    for _ in 0..50 {
        let m = 0.5 * (lo + hi);
        let (u, v) = cx.uv.clamp(cur.u + du * m, cur.v + dv * m);
        match cx.correct(u, v) {
            Some((cu, cv)) if cx.inside(cu, cv) => {
                lo = m;
                best = Some((cu, cv));
            }
            _ => hi = m,
        }
        if (hi - lo) * (du.abs() + dv.abs()) < cx.tol.solver {
            break;
        }
    }
    // Snap to the box edge along the section by one more corrected clamp.
    let (u, v) = best?;
    let (u, v) = cx.uv.clamp(u, v);
    let (u, v) = cx.correct(u, v).map(|(a, b)| cx.uv.clamp(a, b))?;
    cx.node(u, v, Some(cur.t))
}

/// Deviation of the cubic Hermite span between two nodes from the true
/// section, measured at the span midpoint.
fn hermite_error(cx: &Ctx, a: &Node, b: &Node) -> f64 {
    let chord = a.p.distance(b.p);
    let mid = hermite_mid(a.p, a.t * chord, b.p, b.t * chord);
    // Project the midpoint onto the section: start at the uv midpoint.
    let (u, v) = cx.mid_uv(a, b);
    match cx.correct(u, v) {
        Some((cu, cv)) => {
            let on = cx.s.eval(cu, cv);
            mid.distance(on)
        }
        None => f64::INFINITY,
    }
}

fn hermite_mid(p0: Point3, m0: Vec3, p1: Point3, m1: Vec3) -> Point3 {
    // H(½) = ½(p0 + p1) + (m0 − m1)/8
    p0.lerp(p1, 0.5) + (m0 - m1) * 0.125
}

/// Sub-samples per Hermite span when testing whether a seed is on a traced curve.
const NEAR_SAMPLES: usize = 16;

/// Whether `p` lies within `d` of the Hermite spans through `nodes`.
fn near_traced(p: Point3, nodes: &[Node], d: f64) -> bool {
    nodes.windows(2).any(|w| {
        let (a, b) = (&w[0], &w[1]);
        let chord = a.p.distance(b.p);
        let (m0, m1) = (a.t * chord, b.t * chord);
        let mut prev = a.p;
        (1..=NEAR_SAMPLES).any(|i| {
            let q = hermite_at(a.p, m0, b.p, m1, i as f64 / NEAR_SAMPLES as f64);
            let hit = distance_to_segment(p, prev, q) <= d;
            prev = q;
            hit
        })
    }) || nodes.iter().any(|n| n.p.distance(p) <= d)
}

fn hermite_at(p0: Point3, m0: Vec3, p1: Point3, m1: Vec3, t: f64) -> Point3 {
    let (t2, t3) = (t * t, t * t * t);
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    Point3((p0.to_vec() * h00 + m0 * h10 + p1.to_vec() * h01 + m1 * h11).0)
}

fn distance_to_segment(q: Point3, a: Point3, b: Point3) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 0.0 {
        ((q - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    q.distance(a + ab * t)
}

/// G1 cubic B-spline through the nodes: one Bézier span per step, with control
/// points from the Hermite tangents scaled by chord length; parameter is
/// cumulative chord length. Interior knots have multiplicity 3.
fn hermite_bspline(nodes: &[Node]) -> Option<BSplineCurve3> {
    let mut poles = vec![nodes[0].p];
    let mut knots = vec![0.0; 4];
    let mut s = 0.0;
    for w in nodes.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let chord = a.p.distance(b.p);
        poles.push(a.p + a.t * (chord / 3.0));
        poles.push(b.p - b.t * (chord / 3.0));
        poles.push(b.p);
        s += chord;
        knots.extend([s; 3]);
    }
    // Last knot needs multiplicity 4.
    knots.push(s);
    BSplineCurve3::new(3, knots, poles, None).ok()
}
