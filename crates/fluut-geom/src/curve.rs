// SPDX-License-Identifier: MIT OR Apache-2.0

//! 3-D curves (D02 §3). A closed set of kinds, so an enum with static dispatch.

use crate::bounds::Interval;
use crate::bspline::BSplineCurve3;
use crate::frame::Frame;
use crate::roots::newton_bracketed;
use crate::tol::Tol;
use crate::transform::Affine;
use crate::types::{Point3, UnitVec3, Vec3};
use smallvec::{SmallVec, smallvec};
use std::f64::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;

#[derive(Clone, PartialEq, Debug)]
pub enum Curve3 {
    /// `C(t) = origin + t·dir`. Unbounded.
    Line {
        origin: Point3,
        dir: UnitVec3,
    },
    /// `C(t) = o + r·(cos t·x + sin t·y)` in `frame` (STEP parametrisation), `t ∈ [0, 2π]`.
    Circle {
        frame: Frame,
        radius: f64,
    },
    /// `C(t) = o + a·cos t·x + b·sin t·y` (STEP parametrisation), `t ∈ [0, 2π]`.
    Ellipse {
        frame: Frame,
        a: f64,
        b: f64,
    },
    BSpline(Arc<BSplineCurve3>),
    /// STEP `OFFSET_CURVE_3D`: `C(t) = B(t) + dist·n(t)`, with
    /// `n = (B′ × dir) / |B′ × dir|`.
    Offset {
        basis: Box<Curve3>,
        dist: f64,
        dir: UnitVec3,
    },
}

/// Relative scale tolerance below which an affine map counts as a similarity.
const SIMILARITY_TOL: f64 = 1e-12;

impl Curve3 {
    /// A circle; `None` if `radius` is not finite and positive.
    pub fn circle(frame: Frame, radius: f64) -> Option<Curve3> {
        positive(radius).then_some(Curve3::Circle { frame, radius })
    }

    /// An ellipse; `None` unless both semi-axes are finite and positive.
    pub fn ellipse(frame: Frame, a: f64, b: f64) -> Option<Curve3> {
        (positive(a) && positive(b)).then_some(Curve3::Ellipse { frame, a, b })
    }

    pub fn line(origin: Point3, dir: UnitVec3) -> Curve3 {
        Curve3::Line { origin, dir }
    }

    /// An offset curve; `None` if `dist` is not finite.
    pub fn offset(basis: Curve3, dist: f64, dir: UnitVec3) -> Option<Curve3> {
        dist.is_finite().then(|| Curve3::Offset {
            basis: Box::new(basis),
            dist,
            dir,
        })
    }

    pub fn eval(&self, t: f64) -> Point3 {
        match self {
            Curve3::Line { origin, dir } => *origin + dir.get() * t,
            Curve3::Circle { frame, radius } => {
                let (s, c) = libm::sincos(t);
                frame.origin + (frame.x.get() * c + frame.y().get() * s) * *radius
            }
            Curve3::Ellipse { frame, a, b } => {
                let (s, c) = libm::sincos(t);
                frame.origin + frame.x.get() * (a * c) + frame.y().get() * (b * s)
            }
            Curve3::BSpline(b) => b.eval(t),
            Curve3::Offset { .. } => Point3(self.derivs(t, 0)[0].0),
        }
    }

    /// `[C, C', C'', C''']` up to order `n ≤ 3`. Index 0 is the point as a vector
    /// from the origin.
    ///
    /// For offset curves orders 0 and 1 are exact; orders 2 and 3 are central
    /// differences of the exact first derivative (relative step 1e-5).
    pub fn derivs(&self, t: f64, n: usize) -> SmallVec<[Vec3; 4]> {
        let n = n.min(3);
        let mut out: SmallVec<[Vec3; 4]> = match self {
            Curve3::Line { origin, dir } => smallvec![
                (*origin + dir.get() * t).to_vec(),
                dir.get(),
                Vec3::ZERO,
                Vec3::ZERO,
            ],
            Curve3::Circle { frame, radius } => {
                let (s, c) = libm::sincos(t);
                let (x, y, r) = (frame.x.get(), frame.y().get(), *radius);
                smallvec![
                    frame.origin.to_vec() + (x * c + y * s) * r,
                    (y * c - x * s) * r,
                    -(x * c + y * s) * r,
                    (x * s - y * c) * r,
                ]
            }
            Curve3::Ellipse { frame, a, b } => {
                let (s, c) = libm::sincos(t);
                let (x, y) = (frame.x.get() * *a, frame.y().get() * *b);
                smallvec![
                    frame.origin.to_vec() + x * c + y * s,
                    y * c - x * s,
                    -(x * c + y * s),
                    x * s - y * c,
                ]
            }
            Curve3::BSpline(b) => return b.derivs(t, n),
            Curve3::Offset { basis, dist, dir } => {
                let first = |t: f64| offset_first(basis, *dist, *dir, t);
                let (p, d1) = first(t);
                let mut v: SmallVec<[Vec3; 4]> = smallvec![p, d1];
                if n >= 2 {
                    let h = 1e-5 * (1.0 + t.abs());
                    let (_, dp) = first(t + h);
                    let (_, dm) = first(t - h);
                    v.push((dp - dm) / (2.0 * h));
                    v.push((dp - d1 * 2.0 + dm) / (h * h));
                }
                v
            }
        };
        out.truncate(n + 1);
        out
    }

    /// `None` for an unbounded line.
    pub fn natural_range(&self) -> Option<Interval> {
        match self {
            Curve3::Line { .. } => None,
            Curve3::Circle { .. } | Curve3::Ellipse { .. } => Some(Interval { lo: 0.0, hi: TAU }),
            Curve3::BSpline(b) => Some(b.domain()),
            Curve3::Offset { basis, .. } => basis.natural_range(),
        }
    }

    pub fn as_circle(&self) -> Option<(&Frame, f64)> {
        match self {
            Curve3::Circle { frame, radius } => Some((frame, *radius)),
            _ => None,
        }
    }

    /// Parameters inside `range` where the curve's smoothness or shape changes:
    /// B-spline knots, quarter turns of circles and ellipses. Includes both range
    /// ends. Numeric algorithms work piece by piece between these.
    pub fn breaks(&self, range: Interval) -> SmallVec<[f64; 16]> {
        let mut inner: SmallVec<[f64; 16]> = SmallVec::new();
        match self {
            Curve3::BSpline(b) => inner.extend(b.span_breaks()),
            Curve3::Circle { .. } | Curve3::Ellipse { .. } => {
                let first = (range.lo / FRAC_PI_2).floor() as i64;
                let last = (range.hi / FRAC_PI_2).ceil() as i64;
                inner.extend((first..=last).map(|k| k as f64 * FRAC_PI_2));
            }
            Curve3::Offset { basis, .. } => return basis.breaks(range),
            Curve3::Line { .. } => {}
        }
        let mut out: SmallVec<[f64; 16]> = smallvec![range.lo];
        out.extend(inner.into_iter().filter(|&k| k > range.lo && k < range.hi));
        if range.hi > range.lo {
            out.push(range.hi);
        }
        out
    }

    /// Closest point to `p` with parameter in `range`: returns `(t, distance)`.
    pub fn closest(&self, p: Point3, range: Interval, tol: &Tol) -> (f64, f64) {
        match self {
            Curve3::Line { origin, dir } => {
                let t = range.clamp((p - *origin).dot(dir.get()));
                (t, self.eval(t).distance(p))
            }
            Curve3::Circle { frame, .. } => {
                let local = frame.to_local(p);
                let radial = libm::hypot(local.x(), local.y());
                let in_range = if radial > 0.0 {
                    let theta = libm::atan2(local.y(), local.x());
                    // The first angle equivalent to theta at or after range.lo.
                    let t = range.lo + (theta - range.lo).rem_euclid(TAU);
                    (t <= range.hi).then_some(t)
                } else {
                    // On the axis every point is equally close.
                    Some(range.lo)
                };
                let t = in_range.unwrap_or_else(|| {
                    let (dlo, dhi) = (
                        self.eval(range.lo).distance(p),
                        self.eval(range.hi).distance(p),
                    );
                    if dlo <= dhi { range.lo } else { range.hi }
                });
                (t, self.eval(t).distance(p))
            }
            _ => closest_by_roots(self, p, range, tol),
        }
    }

    /// Arc length over `range`.
    pub fn length(&self, range: Interval, tol: &Tol) -> f64 {
        match self {
            Curve3::Line { .. } => range.length(),
            Curve3::Circle { radius, .. } => range.length() * radius,
            _ => self
                .breaks(range)
                .windows(2)
                .map(|w| adaptive_length(self, w[0], w[1], tol.solver, 0))
                .sum(),
        }
    }

    /// Points `(t, C(t))` such that the chord height between neighbours is at most
    /// `chord`. Includes both range ends.
    pub fn tessellate(&self, range: Interval, chord: f64) -> Vec<(f64, Point3)> {
        let ts: Vec<f64> = match self {
            Curve3::Line { .. } => vec![range.lo, range.hi],
            Curve3::Circle { radius, .. } => {
                let step = if chord < *radius {
                    (2.0 * libm::acos(1.0 - chord / radius)).min(FRAC_PI_2)
                } else {
                    FRAC_PI_2
                };
                let n = ((range.length() / step).ceil() as usize).max(1);
                (0..=n).map(|i| range.at(i as f64 / n as f64)).collect()
            }
            _ => {
                let breaks = self.breaks(range);
                let mut ts = vec![breaks[0]];
                for w in breaks.windows(2) {
                    subdivide(self, w[0], w[1], chord, 0, &mut ts);
                }
                ts
            }
        };
        ts.into_iter().map(|t| (t, self.eval(t))).collect()
    }

    /// The curve mapped by `a`: `C′(transformed_param(a, t)) = a(C(t))`
    /// ([`Curve3::transformed_param`]). Lines and B-splines accept any affine
    /// map; circles, ellipses and offsets only similarities (otherwise
    /// `None`). A mirror keeps a circle's or ellipse's point set and
    /// parametrisation and flips its axis.
    pub fn transformed(&self, a: &Affine) -> Option<Curve3> {
        let similar_frame = |frame: &Frame| -> Option<(Frame, f64)> {
            let s = a.uniform_scale(SIMILARITY_TOL)?;
            let x = a.apply_vec(frame.x.get()) / s;
            let y = a.apply_vec(frame.y().get()) / s;
            Some((Frame::new(a.apply_point(frame.origin), x.cross(y), x)?, s))
        };
        match self {
            Curve3::Line { origin, dir } => Some(Curve3::Line {
                origin: a.apply_point(*origin),
                dir: UnitVec3::new(a.apply_vec(dir.get()))?,
            }),
            Curve3::Circle { frame, radius } => {
                let (frame, s) = similar_frame(frame)?;
                Curve3::circle(frame, radius * s)
            }
            Curve3::Ellipse {
                frame,
                a: semi_a,
                b: semi_b,
            } => {
                let (frame, s) = similar_frame(frame)?;
                Curve3::ellipse(frame, semi_a * s, semi_b * s)
            }
            Curve3::BSpline(b) => Some(Curve3::BSpline(Arc::new(b.transformed(a)))),
            Curve3::Offset { basis, dist, dir } => {
                let s = a.uniform_scale(SIMILARITY_TOL)?;
                // n ∝ B′ × dir; a mirror maps it to −(aB′ × a·dir), so the
                // distance changes sign to keep the same point set.
                let sign = if a.reverses_orientation() { -1.0 } else { 1.0 };
                Curve3::offset(
                    basis.transformed(a)?,
                    dist * s * sign,
                    UnitVec3::new(a.apply_vec(dir.get()))?,
                )
            }
        }
    }

    /// Maps a parameter of `self` to the parameter of `self.transformed(a)`
    /// at the mapped point. Lines are parametrised by arc length, so `t`
    /// scales by `|a·dir|`; an offset maps as its basis; every other kind
    /// keeps `t`. The map is increasing, so ranges keep their order.
    pub fn transformed_param(&self, a: &Affine, t: f64) -> f64 {
        match self {
            Curve3::Line { dir, .. } => t * a.apply_vec(dir.get()).length(),
            Curve3::Offset { basis, .. } => basis.transformed_param(a, t),
            _ => t,
        }
    }

    /// [`Curve3::transformed_param`] applied to both ends of `range`.
    pub fn transformed_range(&self, a: &Affine, range: Interval) -> Interval {
        Interval {
            lo: self.transformed_param(a, range.lo),
            hi: self.transformed_param(a, range.hi),
        }
    }
}

fn positive(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

/// Point and exact first derivative of an offset curve.
fn offset_first(basis: &Curve3, dist: f64, dir: UnitVec3, t: f64) -> (Vec3, Vec3) {
    let d = basis.derivs(t, 2);
    let w = d[1].cross(dir.get());
    let len = w.length();
    if len == 0.0 {
        // Tangent parallel to dir: offset direction undefined; stay on the basis.
        return (d[0], d[1]);
    }
    let n = w / len;
    let w1 = d[2].cross(dir.get());
    let n1 = (w1 - n * n.dot(w1)) / len;
    (d[0] + n * dist, d[1] + n1 * dist)
}

/// 5-point Gauss–Legendre nodes and weights on [-1, 1].
const GL5: [(f64, f64); 5] = [
    (0.0, 0.568_888_888_888_888_9),
    (-0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
    (0.906_179_845_938_664, 0.236_926_885_056_189_1),
];

fn gauss_length(c: &Curve3, a: f64, b: f64) -> f64 {
    let (m, h) = (0.5 * (a + b), 0.5 * (b - a));
    GL5.iter()
        .map(|&(x, w)| w * c.derivs(m + h * x, 1)[1].length())
        .sum::<f64>()
        * h
}

fn adaptive_length(c: &Curve3, a: f64, b: f64, tol: f64, depth: u32) -> f64 {
    let whole = gauss_length(c, a, b);
    let m = 0.5 * (a + b);
    let halves = gauss_length(c, a, m) + gauss_length(c, m, b);
    if (whole - halves).abs() <= tol || depth >= 30 {
        halves
    } else {
        adaptive_length(c, a, m, 0.5 * tol, depth + 1)
            + adaptive_length(c, m, b, 0.5 * tol, depth + 1)
    }
}

/// Appends parameters in `(a, b]` so that each piece's midpoint deviation from its
/// chord is at most `chord`. Samples at quarter points too, so an S-shaped piece
/// with a straight-looking midpoint is still split.
fn subdivide(c: &Curve3, a: f64, b: f64, chord: f64, depth: u32, out: &mut Vec<f64>) {
    let (pa, pb) = (c.eval(a), c.eval(b));
    let deviation = [0.25, 0.5, 0.75]
        .iter()
        .map(|&f| {
            let q = c.eval(a + (b - a) * f);
            distance_to_segment(q, pa, pb)
        })
        .fold(0.0f64, f64::max);
    if deviation <= chord || depth >= 24 {
        out.push(b);
    } else {
        let m = 0.5 * (a + b);
        subdivide(c, a, m, chord, depth + 1, out);
        subdivide(c, m, b, chord, depth + 1, out);
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

/// Closest point by root finding: sign changes of `g(t) = (C(t) − p)·C'(t)` on
/// dense seeds per piece (see [`Curve3::breaks`]), each refined by bracketed
/// Newton; range ends are candidates too. D02 §3.2 control-polygon seeding for
/// B-splines replaces the dense seeds if the S2 spike shows it is needed.
fn closest_by_roots(curve: &Curve3, p: Point3, range: Interval, tol: &Tol) -> (f64, f64) {
    const SEEDS_PER_PIECE: usize = 16;
    let g = |t: f64| {
        let d = curve.derivs(t, 2);
        let diff = d[0] - p.to_vec();
        (diff.dot(d[1]), d[1].dot(d[1]) + diff.dot(d[2]))
    };
    let mut best = (range.lo, curve.eval(range.lo).distance(p));
    let mut consider = |t: f64| {
        let dist = curve.eval(t).distance(p);
        if dist < best.1 {
            best = (t, dist);
        }
    };
    consider(range.hi);

    for w in curve.breaks(range).windows(2) {
        let mut prev_t = w[0];
        let mut prev_g = g(prev_t).0;
        for i in 1..=SEEDS_PER_PIECE {
            let t = w[0] + (w[1] - w[0]) * i as f64 / SEEDS_PER_PIECE as f64;
            let gt = g(t).0;
            if prev_g.signum() != gt.signum() || gt == 0.0 {
                match newton_bracketed(g, prev_t, t, tol.solver) {
                    Ok(root) => consider(root),
                    Err(_) => consider(t),
                }
            }
            prev_t = t;
            prev_g = gt;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn tol() -> Tol {
        Tol::for_machining(1e-6, 1e-3)
    }

    fn unit_circle() -> Curve3 {
        Curve3::circle(Frame::world_xy(), 2.0).unwrap()
    }

    fn full() -> Interval {
        Interval { lo: 0.0, hi: TAU }
    }

    #[test]
    fn transformed_param_maps_points() {
        let line = Curve3::line(
            Point3::new(1.0, 2.0, 3.0),
            UnitVec3::new(Vec3::new(1.0, 1.0, 0.0)).unwrap(),
        );
        let offset = Curve3::offset(line.clone(), 0.5, UnitVec3::Z).unwrap();
        let shear = Affine::scale(2.0, 3.0, 1.0);
        let similar = Affine::scale(2.0, 2.0, 2.0);
        for (c, a) in [
            (&line, &shear),
            (&line, &similar),
            (&offset, &similar),
            (&unit_circle(), &similar),
        ] {
            let t2 = c.transformed(a).unwrap();
            for t in [-1.5, 0.0, 0.7, 4.0] {
                let p = t2.eval(c.transformed_param(a, t));
                assert!(p.distance(a.apply_point(c.eval(t))) < 1e-12, "{c:?} t={t}");
            }
        }
    }

    #[test]
    fn circle_rejects_bad_radius() {
        assert!(Curve3::circle(Frame::world_xy(), 0.0).is_none());
        assert!(Curve3::circle(Frame::world_xy(), -1.0).is_none());
        assert!(Curve3::circle(Frame::world_xy(), f64::NAN).is_none());
    }

    #[test]
    fn circle_eval_quadrants() {
        let c = unit_circle();
        let close = |a: Point3, b: Point3| a.distance(b) < 1e-15;
        assert!(close(c.eval(0.0), Point3::new(2.0, 0.0, 0.0)));
        assert!(close(c.eval(PI / 2.0), Point3::new(0.0, 2.0, 0.0)));
        assert!(close(c.eval(PI), Point3::new(-2.0, 0.0, 0.0)));
        assert!(close(c.eval(3.0 * PI / 2.0), Point3::new(0.0, -2.0, 0.0)));
    }

    #[test]
    fn circle_derivs_match_finite_difference() {
        let c = Curve3::circle(
            Frame::new(
                Point3::new(1.0, 2.0, 3.0),
                Vec3::new(1.0, 1.0, 1.0),
                Vec3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            3.5,
        )
        .unwrap();
        let h = 1e-6;
        for t in [0.0, 0.4, 2.0, 5.5] {
            let d = c.derivs(t, 3);
            for k in 1..=3 {
                let fd = (c.derivs(t + h, 2)[k - 1] - c.derivs(t - h, 2)[k - 1]) / (2.0 * h);
                assert!(
                    (d[k] - fd).length() < 1e-6 * (1.0 + d[k].length()),
                    "k={k} t={t}"
                );
            }
        }
    }

    #[test]
    fn circle_closest_wraps() {
        let c = unit_circle();
        let p = Point3::new(libm::cos(-0.1) * 5.0, libm::sin(-0.1) * 5.0, 1.0);
        let (t, d) = c.closest(p, full(), &tol());
        assert!((t - (TAU - 0.1)).abs() < 1e-14);
        assert!((d - libm::hypot(3.0, 1.0)).abs() < 1e-14);
    }

    #[test]
    fn circle_closest_range_crossing_two_pi() {
        let c = unit_circle();
        let range = Interval { lo: 5.0, hi: 7.0 };
        let p = Point3::new(libm::cos(0.5), libm::sin(0.5), 0.0);
        let (t, _) = c.closest(p, range, &tol());
        assert!((t - (TAU + 0.5)).abs() < 1e-14);
    }

    #[test]
    fn circle_closest_clamps_to_range() {
        let c = unit_circle();
        let range = Interval {
            lo: 0.0,
            hi: PI / 2.0,
        };
        // Angle 0.6π is outside the range and nearer the end π/2 than 0.
        let p = Point3::new(libm::cos(0.6 * PI) * 3.0, libm::sin(0.6 * PI) * 3.0, 0.0);
        assert_eq!(c.closest(p, range, &tol()).0, PI / 2.0);
        let p = Point3::new(libm::cos(-0.2) * 3.0, libm::sin(-0.2) * 3.0, 0.0);
        assert_eq!(c.closest(p, range, &tol()).0, 0.0);
    }

    #[test]
    fn circle_closest_centre() {
        let (t, d) = unit_circle().closest(Point3::ORIGIN, Interval { lo: 1.0, hi: 2.0 }, &tol());
        assert_eq!(t, 1.0);
        assert_eq!(d, 2.0);
    }

    #[test]
    fn line_closest() {
        let l = Curve3::line(Point3::new(1.0, 1.0, 0.0), UnitVec3::X);
        let (t, d) = l.closest(
            Point3::new(4.0, 3.0, 0.0),
            Interval {
                lo: -10.0,
                hi: 10.0,
            },
            &tol(),
        );
        assert_eq!((t, d), (3.0, 2.0));
        let (t, _) = l.closest(
            Point3::new(40.0, 3.0, 0.0),
            Interval {
                lo: -10.0,
                hi: 10.0,
            },
            &tol(),
        );
        assert_eq!(t, 10.0);
    }

    #[test]
    fn circle_length() {
        assert!(
            (unit_circle().length(Interval { lo: 0.0, hi: PI }, &tol()) - 2.0 * PI).abs() < 1e-15
        );
    }

    #[test]
    fn tessellate_chord_error() {
        let c = unit_circle();
        for chord in [0.001, 0.1, 1.0, 5.0] {
            let pts = c.tessellate(full(), chord);
            assert!(pts.len() >= 5, "chord {chord}: at least 4 segments");
            for w in pts.windows(2) {
                let mid = c.eval(0.5 * (w[0].0 + w[1].0));
                let sagitta = distance_to_segment(mid, w[0].1, w[1].1);
                assert!(sagitta <= chord * (1.0 + 1e-12), "chord {chord}: {sagitta}");
            }
        }
    }

    #[test]
    fn mirror_circle_flips_z() {
        let m = unit_circle()
            .transformed(&Affine::scale(-1.0, 1.0, 1.0))
            .unwrap();
        let (frame, r) = m.as_circle().unwrap();
        assert_eq!(r, 2.0);
        assert!((frame.z.get() - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-15);
        // Same point set: every mirrored point lies on the new circle.
        for i in 0..8 {
            let q = Affine::scale(-1.0, 1.0, 1.0).apply_point(unit_circle().eval(i as f64));
            assert!(m.closest(q, full(), &tol()).1 < 1e-14);
        }
    }

    #[test]
    fn non_uniform_scale_circle_is_none() {
        assert!(
            unit_circle()
                .transformed(&Affine::scale(1.0, 2.0, 1.0))
                .is_none()
        );
    }

    #[test]
    fn full_rational_circle_matches_circle() {
        let nurbs = Curve3::BSpline(Arc::new(crate::bspline::tests_support::full_circle()));
        let exact = Curve3::circle(Frame::world_xy(), 1.0).unwrap();
        for i in 0..=200 {
            let q = nurbs.eval(i as f64 / 200.0);
            assert!(exact.closest(q, full(), &tol()).1 < 1e-12);
        }
    }

    #[test]
    fn bspline_length_of_nurbs_circle() {
        let nurbs = Curve3::BSpline(Arc::new(crate::bspline::tests_support::full_circle()));
        let len = nurbs.length(Interval { lo: 0.0, hi: 1.0 }, &tol());
        assert!((len - TAU).abs() < 1e-8, "{len}");
    }

    #[test]
    fn bspline_closest_on_nurbs_circle() {
        let nurbs = Curve3::BSpline(Arc::new(crate::bspline::tests_support::full_circle()));
        let p = Point3::new(libm::cos(2.0) * 3.0, libm::sin(2.0) * 3.0, 0.0);
        let (t, d) = nurbs.closest(p, Interval { lo: 0.0, hi: 1.0 }, &tol());
        assert!((d - 2.0).abs() < 1e-9, "{d}");
        let q = nurbs.eval(t);
        assert!((libm::atan2(q.y(), q.x()) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn bspline_tessellate_respects_chord() {
        let nurbs = Curve3::BSpline(Arc::new(crate::bspline::tests_support::full_circle()));
        let pts = nurbs.tessellate(Interval { lo: 0.0, hi: 1.0 }, 0.01);
        assert_eq!(pts.first().unwrap().0, 0.0);
        assert_eq!(pts.last().unwrap().0, 1.0);
        for w in pts.windows(2) {
            let mid = nurbs.eval(0.5 * (w[0].0 + w[1].0));
            assert!(distance_to_segment(mid, w[0].1, w[1].1) <= 0.01);
        }
    }

    fn ellipse() -> Curve3 {
        let f = Frame::new(
            Point3::new(1.0, 2.0, -1.0),
            Vec3::new(0.2, 0.1, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
        )
        .unwrap();
        Curve3::ellipse(f, 5.0, 2.0).unwrap()
    }

    #[test]
    fn ellipse_rejects_bad_axes() {
        assert!(Curve3::ellipse(Frame::world_xy(), 1.0, 0.0).is_none());
        assert!(Curve3::ellipse(Frame::world_xy(), f64::INFINITY, 1.0).is_none());
    }

    #[test]
    fn ellipse_derivs_match_finite_difference() {
        let c = ellipse();
        let h = 1e-6;
        for t in [0.0, 0.4, 2.0, 5.5] {
            let d = c.derivs(t, 3);
            for k in 1..=3 {
                let fd = (c.derivs(t + h, 2)[k - 1] - c.derivs(t - h, 2)[k - 1]) / (2.0 * h);
                assert!(
                    (d[k] - fd).length() < 1e-6 * (1.0 + d[k].length()),
                    "k={k} t={t}"
                );
            }
        }
    }

    #[test]
    fn ellipse_perimeter() {
        // Ramanujan's second approximation, then the exact value from the
        // Gauss–Kummer series (a = 5, b = 2): 23.0131005...
        let (a, b): (f64, f64) = (5.0, 2.0);
        let h = ((a - b) / (a + b)).powi(2);
        let ramanujan = PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()));
        let len = ellipse().length(full(), &tol());
        assert!(
            ((len - ramanujan) / ramanujan).abs() < 1e-5,
            "{len} vs {ramanujan}"
        );
        let mut series = 1.0;
        let mut coef = 1.0;
        for n in 1..60 {
            // ((2n−3)!! / (2n)!!)² style coefficients: binom(1/2, n)².
            coef *= (0.5 - (n - 1) as f64) / n as f64;
            series += coef * coef * h.powi(n);
        }
        let exact = PI * (a + b) * series;
        assert!((len - exact).abs() < 1e-9, "{len} vs {exact}");
    }

    #[test]
    fn ellipse_closest() {
        let c = ellipse();
        let Curve3::Ellipse { frame, .. } = &c else {
            panic!("not an ellipse")
        };
        for t in [0.3, 1.7, 3.0, 4.5, 6.0] {
            // Point displaced outward along the in-plane normal.
            let d = c.derivs(t, 1);
            let n = d[1].cross(frame.z.get()).normalized().unwrap().get();
            let p = c.eval(t) + n * 0.5;
            let (tt, dist) = c.closest(p, full(), &tol());
            assert!((dist - 0.5).abs() < 1e-9, "t={t}: {dist}");
            assert!((tt - t).abs() < 1e-7, "t={t}: {tt}");
        }
    }

    #[test]
    fn offset_of_circle_is_circle() {
        // CCW circle about +Z: T × Z points outward, so radius becomes r + d.
        let base = Curve3::circle(Frame::world_xy(), 3.0).unwrap();
        let off = Curve3::offset(base, 0.5, UnitVec3::Z).unwrap();
        for t in [0.0, 1.0, 2.5, 4.0] {
            assert!((off.eval(t).to_vec().length() - 3.5).abs() < 1e-14);
        }
        let len = off.length(full(), &tol());
        assert!((len - 3.5 * TAU).abs() < 1e-9, "{len}");
        let d = off.derivs(1.0, 2);
        let exact = Curve3::circle(Frame::world_xy(), 3.5)
            .unwrap()
            .derivs(1.0, 2);
        assert!((d[1] - exact[1]).length() < 1e-12);
        assert!((d[2] - exact[2]).length() < 1e-5);
    }

    #[test]
    fn offset_mirror_keeps_points() {
        let off = Curve3::offset(ellipse(), 0.7, UnitVec3::Z).unwrap();
        let m = Affine::scale(1.0, -1.0, 1.0);
        let t_off = off.transformed(&m).unwrap();
        for t in [0.2, 1.4, 3.3] {
            assert!(t_off.eval(t).distance(m.apply_point(off.eval(t))) < 1e-12);
        }
    }

    #[test]
    fn ellipse_tessellate_respects_chord() {
        let c = ellipse();
        let pts = c.tessellate(full(), 0.01);
        for w in pts.windows(2) {
            let mid = c.eval(0.5 * (w[0].0 + w[1].0));
            assert!(distance_to_segment(mid, w[0].1, w[1].1) <= 0.01);
        }
    }
}
