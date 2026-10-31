// SPDX-License-Identifier: MIT OR Apache-2.0

//! Line and arc fitting (D02 §7).
//!
//! - [`biarc_fit`]: a planar curve becomes a G1 chain of lines and arcs within a
//!   tolerance (used for 2-D offsetting of B-spline boundaries, spec 04 §4.8).
//! - [`fit_points`]: a sampled path becomes the fewest lines and arcs found by a
//!   greedy longest-run search (segment fitting, D08 §5).
//!
//! Consecutive output segments share endpoints bitwise.

use crate::bounds::Interval;
use crate::curve::Curve3;
use crate::types::{Point3, UnitVec3, Vec3};
use std::f64::consts::FRAC_PI_2;

/// A fitted segment. Arcs use the toolpath convention (D08 §2): `sweep` is
/// signed, positive counter-clockwise seen from the tip of `normal`.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum FitSeg {
    Line {
        a: Point3,
        b: Point3,
    },
    Arc {
        a: Point3,
        b: Point3,
        centre: Point3,
        normal: UnitVec3,
        sweep: f64,
    },
}

impl FitSeg {
    pub fn start(&self) -> Point3 {
        match self {
            FitSeg::Line { a, .. } | FitSeg::Arc { a, .. } => *a,
        }
    }

    pub fn end(&self) -> Point3 {
        match self {
            FitSeg::Line { b, .. } | FitSeg::Arc { b, .. } => *b,
        }
    }

    /// Distance from `p` to the segment.
    pub fn distance(&self, p: Point3) -> f64 {
        match self {
            FitSeg::Line { a, b } => distance_to_segment(p, *a, *b),
            FitSeg::Arc {
                a,
                b,
                centre,
                normal,
                sweep,
            } => {
                let n = normal.get();
                let r0 = *a - *centre;
                let r = r0.length();
                let d = p - *centre;
                let off_plane = d.dot(n);
                let in_plane = d - n * off_plane;
                // Angle of p from a, measured in the sweep direction.
                let y = n.cross(r0);
                let ang = libm::atan2(in_plane.dot(y), in_plane.dot(r0));
                let ang = if *sweep >= 0.0 {
                    ang.rem_euclid(std::f64::consts::TAU)
                } else {
                    -(-ang).rem_euclid(std::f64::consts::TAU)
                };
                if ang.abs() <= sweep.abs() && ang.signum() == sweep.signum() || ang == 0.0 {
                    libm::hypot(in_plane.length() - r, off_plane)
                } else {
                    p.distance(*a).min(p.distance(*b))
                }
            }
        }
    }

    /// Point at fraction `f` along the segment.
    pub fn at(&self, f: f64) -> Point3 {
        match self {
            FitSeg::Line { a, b } => a.lerp(*b, f),
            FitSeg::Arc {
                a,
                centre,
                normal,
                sweep,
                ..
            } => {
                let r0 = *a - *centre;
                let (s, c) = libm::sincos(sweep * f);
                *centre + r0 * c + normal.get().cross(r0) * s
            }
        }
    }

    /// Unit tangent at fraction `f`.
    pub fn tangent(&self, f: f64) -> Option<Vec3> {
        match self {
            FitSeg::Line { a, b } => (*b - *a).normalized().map(UnitVec3::get),
            FitSeg::Arc {
                centre,
                normal,
                sweep,
                ..
            } => {
                let r = self.at(f) - *centre;
                (normal.get().cross(r) * sweep.signum())
                    .normalized()
                    .map(UnitVec3::get)
            }
        }
    }
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

/// The arc (or line) from `p` with unit tangent `t` to `q`, in the plane with
/// normal `n`.
fn arc_from_tangent(p: Point3, t: Vec3, q: Point3, n: Vec3) -> FitSeg {
    let chord = q - p;
    let side = n.cross(t); // left normal of the tangent
    let h = chord.dot(side);
    let len2 = chord.length_squared();
    if h.abs() <= 1e-12 * len2.sqrt().max(1e-300) {
        return FitSeg::Line { a: p, b: q };
    }
    // Centre on the left normal at signed distance r: |q − c| = |r|.
    let r = len2 / (2.0 * h);
    let centre = p + side * r;
    let (r0, r1) = (p - centre, q - centre);
    let normal = if r > 0.0 { n } else { -n };
    // Sweep from r0 to r1 about `normal`, in (0, 2π).
    let y = normal.cross(r0);
    let mut sweep = libm::atan2(r1.dot(y), r1.dot(r0));
    if sweep <= 0.0 {
        sweep += std::f64::consts::TAU;
    }
    FitSeg::Arc {
        a: p,
        b: q,
        centre,
        normal: UnitVec3::new(normal).unwrap_or(UnitVec3::Z),
        sweep,
    }
}

/// Biarc between two points with unit tangents, equal-chord rule (d₁ = d₂).
/// `None` when the construction degenerates (tangents opposed to the chord).
fn biarc(p0: Point3, t0: Vec3, p1: Point3, t1: Vec3, n: Vec3) -> Option<[FitSeg; 2]> {
    let v = p1 - p0;
    let t = t0 + t1;
    let denom = 2.0 * (1.0 - t0.dot(t1));
    let vt = v.dot(t);
    let d = if denom.abs() < 1e-12 {
        let k = 4.0 * v.dot(t1);
        if k <= 0.0 {
            return None;
        }
        v.length_squared() / k
    } else {
        (-vt + (vt * vt + denom * v.length_squared()).sqrt()) / denom
    };
    if !(d.is_finite() && d > 0.0) {
        return None;
    }
    let join = (p0 + t0 * d).lerp(p1 - t1 * d, 0.5);
    let first = arc_from_tangent(p0, t0, join, n);
    // Second arc: from p1 backwards along −t1 to the join, then reversed.
    let second = reverse(arc_from_tangent(p1, -t1, join, n));
    Some([align_normal(first, n), align_normal(second, n)])
}

/// Expresses an arc with its normal on the same side as `n` (flipping the
/// sweep sign instead), so arcs in one plane share one normal.
fn align_normal(s: FitSeg, n: Vec3) -> FitSeg {
    match s {
        FitSeg::Arc {
            a,
            b,
            centre,
            normal,
            sweep,
        } if normal.dot(n) < 0.0 => FitSeg::Arc {
            a,
            b,
            centre,
            normal: -normal,
            sweep: -sweep,
        },
        other => other,
    }
}

fn reverse(s: FitSeg) -> FitSeg {
    match s {
        FitSeg::Line { a, b } => FitSeg::Line { a: b, b: a },
        FitSeg::Arc {
            a,
            b,
            centre,
            normal,
            sweep,
        } => FitSeg::Arc {
            a: b,
            b: a,
            centre,
            normal,
            sweep: -sweep,
        },
    }
}

/// Samples per piece when measuring biarc deviation.
const BIARC_SAMPLES: usize = 16;
/// Maximum recursion depth of biarc splitting.
const BIARC_MAX_DEPTH: u32 = 30;

/// G1 chain of lines and arcs within `tol` of the planar curve `c` over
/// `range`, in the plane with normal `n` (D02 §7). Pieces turning more than 90°
/// are split first; a piece whose biarc deviates more than `tol` (sampled, then
/// refined at the worst sample) is split at its worst point.
pub fn biarc_fit(c: &Curve3, range: Interval, n: UnitVec3, tol: f64) -> Vec<FitSeg> {
    let mut out = Vec::new();
    let breaks = c.breaks(range);
    for w in breaks.windows(2) {
        fit_piece(c, w[0], w[1], n.get(), tol, 0, &mut out);
    }
    // Share endpoints bitwise.
    for i in 1..out.len() {
        let p = out[i - 1].end();
        set_start(&mut out[i], p);
    }
    out
}

fn set_start(s: &mut FitSeg, p: Point3) {
    match s {
        FitSeg::Line { a, .. } | FitSeg::Arc { a, .. } => *a = p,
    }
}

fn unit_tangent(c: &Curve3, t: f64) -> Option<Vec3> {
    c.derivs(t, 1)[1].normalized().map(UnitVec3::get)
}

fn fit_piece(c: &Curve3, t0: f64, t1: f64, n: Vec3, tol: f64, depth: u32, out: &mut Vec<FitSeg>) {
    let (p0, p1) = (c.eval(t0), c.eval(t1));
    let split = |out: &mut Vec<FitSeg>, at: f64| {
        fit_piece(c, t0, at, n, tol, depth + 1, out);
        fit_piece(c, at, t1, n, tol, depth + 1, out);
    };
    if depth >= BIARC_MAX_DEPTH || p0.distance(p1) <= tol * 1e-3 {
        out.push(FitSeg::Line { a: p0, b: p1 });
        return;
    }
    let (Some(d0), Some(d1)) = (unit_tangent(c, t0), unit_tangent(c, t1)) else {
        split(out, 0.5 * (t0 + t1));
        return;
    };
    if libm::acos(d0.dot(d1).clamp(-1.0, 1.0)) > FRAC_PI_2 {
        split(out, 0.5 * (t0 + t1));
        return;
    }
    let Some(arcs) = biarc(p0, d0, p1, d1, n) else {
        split(out, 0.5 * (t0 + t1));
        return;
    };
    let mut worst = (0.0, 0.5 * (t0 + t1));
    for i in 1..BIARC_SAMPLES {
        let t = t0 + (t1 - t0) * i as f64 / BIARC_SAMPLES as f64;
        let q = c.eval(t);
        let d = arcs[0].distance(q).min(arcs[1].distance(q));
        if d > worst.0 {
            worst = (d, t);
        }
    }
    if worst.0 <= tol * 0.9 {
        out.extend(arcs);
    } else {
        split(out, worst.1);
    }
}

/// Fits the fewest lines and arcs through `points` (a sampled path) such that
/// every point lies within `tol` of its segment. Greedy: from each start, the
/// longest run that fits a line or an arc wins (ties go to lines). Arcs pass
/// exactly through their end points and the run's middle point. Segment ends
/// are input points, so the chain is continuous.
pub fn fit_points(points: &[Point3], tol: f64) -> Vec<FitSeg> {
    let mut out = Vec::new();
    if points.len() < 2 {
        return out;
    }
    let mut i = 0;
    while i + 1 < points.len() {
        let line_end = longest_run(points, i, |run| line_fits(run, tol));
        let arc_end = longest_run(points, i, |run| arc_through(run, tol).is_some());
        if arc_end > line_end
            && let Some(arc) = arc_through(&points[i..=arc_end], tol)
        {
            out.push(arc);
            i = arc_end;
            continue;
        }
        out.push(FitSeg::Line {
            a: points[i],
            b: points[line_end],
        });
        i = line_end;
    }
    out
}

/// Largest `j > i` such that `fits(points[i..=j])`, searching by doubling then
/// bisection. Runs of two points always fit.
fn longest_run(points: &[Point3], i: usize, fits: impl Fn(&[Point3]) -> bool) -> usize {
    let last = points.len() - 1;
    let mut good = i + 1;
    let mut step = 1;
    let mut bad = None;
    while good < last {
        let j = (good + step).min(last);
        if fits(&points[i..=j]) {
            good = j;
            step *= 2;
        } else {
            bad = Some(j);
            break;
        }
    }
    if let Some(mut bad) = bad {
        while bad - good > 1 {
            let mid = (good + bad) / 2;
            if fits(&points[i..=mid]) {
                good = mid;
            } else {
                bad = mid;
            }
        }
    }
    good
}

fn line_fits(run: &[Point3], tol: f64) -> bool {
    let (a, b) = (run[0], run[run.len() - 1]);
    run.iter().all(|&p| distance_to_segment(p, a, b) <= tol)
}

/// Arc through the first, middle and last points of `run` if every point lies
/// within `tol` of it and the points progress monotonically along it.
fn arc_through(run: &[Point3], tol: f64) -> Option<FitSeg> {
    if run.len() < 3 {
        return None;
    }
    let (a, m, b) = (run[0], run[run.len() / 2], run[run.len() - 1]);
    let (u, v) = (m - a, b - a);
    let n = u.cross(v);
    let n_len2 = n.length_squared();
    if n_len2 <= 1e-24 * u.length_squared() * v.length_squared() {
        return None; // collinear
    }
    // Circumcentre of a, m, b.
    let centre =
        a + (n.cross(u) * v.length_squared() + v.cross(n) * u.length_squared()) / (2.0 * n_len2);
    let normal = UnitVec3::new(n)?;
    let r0 = a - centre;
    let y = normal.get().cross(r0);
    let angle = |p: Point3| {
        let d = p - centre;
        libm::atan2(d.dot(y), d.dot(r0)).rem_euclid(std::f64::consts::TAU)
    };
    let sweep = angle(b);
    let arc = FitSeg::Arc {
        a,
        b,
        centre,
        normal,
        sweep,
    };
    let mut prev = 0.0;
    for &p in run {
        let ang = angle(p);
        if ang + 1e-12 < prev || ang > sweep + 1e-12 || arc.distance(p) > tol {
            return None;
        }
        prev = ang;
    }
    Some(arc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bspline::BSplineCurve3;
    use crate::frame::Frame;
    use std::f64::consts::{PI, TAU};
    use std::sync::Arc;

    fn check_chain(segs: &[FitSeg]) {
        for w in segs.windows(2) {
            assert_eq!(w[0].end(), w[1].start(), "chain not continuous");
        }
    }

    fn max_dev(c: &Curve3, range: Interval, segs: &[FitSeg]) -> f64 {
        (0..=400)
            .map(|i| {
                let q = c.eval(range.at(i as f64 / 400.0));
                segs.iter()
                    .map(|s| s.distance(q))
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn biarc_of_circle_is_exact_arcs() {
        let c = Curve3::circle(Frame::world_xy(), 5.0).unwrap();
        let range = Interval { lo: 0.0, hi: PI };
        let segs = biarc_fit(&c, range, UnitVec3::Z, 0.001);
        check_chain(&segs);
        assert!(max_dev(&c, range, &segs) < 1e-9);
        assert!(segs.len() <= 4, "{}", segs.len());
        for s in &segs {
            let FitSeg::Arc { centre, sweep, .. } = s else {
                panic!("expected arcs")
            };
            assert!(centre.distance(Point3::ORIGIN) < 1e-9);
            assert!(*sweep > 0.0, "counter-clockwise");
        }
    }

    #[test]
    fn biarc_of_ellipse_within_tolerance_and_g1() {
        let c = Curve3::ellipse(Frame::world_xy(), 8.0, 3.0).unwrap();
        let range = Interval { lo: 0.0, hi: TAU };
        for tol in [0.1, 0.01, 0.001] {
            let segs = biarc_fit(&c, range, UnitVec3::Z, tol);
            check_chain(&segs);
            assert!(max_dev(&c, range, &segs) <= tol, "tol {tol}");
            for w in segs.windows(2) {
                let (t0, t1) = (w[0].tangent(1.0).unwrap(), w[1].tangent(0.0).unwrap());
                assert!(t0.dot(t1) > 1.0 - 1e-9, "G1 at joint (tol {tol})");
            }
        }
    }

    #[test]
    fn biarc_of_s_curve() {
        let p = Point3::new;
        let b = BSplineCurve3::new(
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            vec![p(0., 0., 0.), p(3., 4., 0.), p(5., -4., 0.), p(8., 0., 0.)],
            None,
        )
        .unwrap();
        let c = Curve3::BSpline(Arc::new(b));
        let range = Interval { lo: 0.0, hi: 1.0 };
        let segs = biarc_fit(&c, range, UnitVec3::Z, 0.005);
        check_chain(&segs);
        assert!(max_dev(&c, range, &segs) <= 0.005);
        assert_eq!(segs.first().unwrap().start(), c.eval(0.0));
        assert_eq!(segs.last().unwrap().end(), c.eval(1.0));
    }

    #[test]
    fn fit_points_line_and_circle() {
        let line: Vec<Point3> = (0..=50)
            .map(|i| Point3::new(i as f64 * 0.2, 1.0, 0.0))
            .collect();
        assert_eq!(fit_points(&line, 0.001).len(), 1);

        let arc: Vec<Point3> = (0..=60)
            .map(|i| {
                let t = i as f64 / 60.0 * 2.0;
                Point3::new(libm::cos(t) * 4.0, libm::sin(t) * 4.0, 2.0)
            })
            .collect();
        let segs = fit_points(&arc, 0.001);
        assert_eq!(segs.len(), 1);
        let FitSeg::Arc { centre, sweep, .. } = segs[0] else {
            panic!("expected an arc")
        };
        assert!(centre.distance(Point3::new(0.0, 0.0, 2.0)) < 1e-9);
        assert!((sweep - 2.0).abs() < 1e-9);
    }

    #[test]
    fn fit_points_line_then_arc() {
        let mut pts: Vec<Point3> = (0..=20)
            .map(|i| Point3::new(-4.0 + i as f64 * 0.2, -2.0, 0.0))
            .collect();
        // Quarter circle of radius 2 about (0, 0), tangent to the line at (0, −2).
        for i in 1..=30 {
            let t = -FRAC_PI_2 + i as f64 / 30.0 * FRAC_PI_2;
            pts.push(Point3::new(libm::cos(t) * 2.0, libm::sin(t) * 2.0, 0.0));
        }
        let segs = fit_points(&pts, 0.001);
        assert_eq!(segs.len(), 2, "{segs:?}");
        check_chain(&segs);
        for p in &pts {
            assert!(
                segs.iter()
                    .map(|s| s.distance(*p))
                    .fold(f64::INFINITY, f64::min)
                    <= 0.001
            );
        }
    }

    #[test]
    fn fit_points_noise_below_tolerance() {
        // Deterministic wiggle of ±0.0004 on a line: still one segment at 0.001.
        let pts: Vec<Point3> = (0..=100)
            .map(|i| {
                Point3::new(
                    i as f64 * 0.1,
                    if i % 2 == 0 { 0.0004 } else { -0.0004 },
                    0.0,
                )
            })
            .collect();
        assert_eq!(fit_points(&pts, 0.001).len(), 1);
        assert!(fit_points(&pts, 0.0001).len() > 10);
    }

    #[test]
    fn fit_points_degenerate_inputs() {
        assert!(fit_points(&[], 0.1).is_empty());
        assert!(fit_points(&[Point3::ORIGIN], 0.1).is_empty());
        assert_eq!(
            fit_points(&[Point3::ORIGIN, Point3::new(1.0, 0.0, 0.0)], 0.1).len(),
            1
        );
    }
}
