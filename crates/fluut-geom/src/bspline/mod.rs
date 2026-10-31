// SPDX-License-Identifier: MIT OR Apache-2.0

//! B-spline and NURBS curves (D02 §3).

pub mod basis;

use crate::bounds::Interval;
use crate::transform::Affine;
use crate::types::{Point3, Vec3};
use basis::{MAX_DEGREE, SMALL, ders_basis_funs_n, find_span};
use glam::DVec4;
use smallvec::SmallVec;

#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum BSplineError {
    #[error("degree must be between 1 and {MAX_DEGREE}")]
    DegreeZeroOrTooHigh,
    #[error("need at least degree + 1 poles")]
    TooFewPoles,
    #[error("knot count must equal poles + degree + 1")]
    BadKnotCount,
    #[error("knots must be finite and non-decreasing")]
    KnotsDecreasing,
    #[error("parameter domain is empty")]
    EmptyDomain,
    #[error("weights must be finite, positive and one per pole")]
    BadWeights,
    #[error("poles must be finite")]
    NonFinitePole,
}

/// A B-spline curve, rational when `weights` is present. The knot vector is the
/// full (expanded) vector, as STEP stores it.
#[derive(Clone, PartialEq, Debug)]
pub struct BSplineCurve3 {
    degree: u8,
    knots: Vec<f64>,
    poles: Vec<Point3>,
    weights: Option<Vec<f64>>,
    periodic: bool,
}

impl BSplineCurve3 {
    pub fn new(
        degree: u8,
        knots: Vec<f64>,
        poles: Vec<Point3>,
        weights: Option<Vec<f64>>,
    ) -> Result<BSplineCurve3, BSplineError> {
        let p = degree as usize;
        if p == 0 || p > MAX_DEGREE {
            return Err(BSplineError::DegreeZeroOrTooHigh);
        }
        if poles.len() < p + 1 {
            return Err(BSplineError::TooFewPoles);
        }
        if knots.len() != poles.len() + p + 1 {
            return Err(BSplineError::BadKnotCount);
        }
        if knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| w[1] < w[0]) {
            return Err(BSplineError::KnotsDecreasing);
        }
        if knots[p] >= knots[poles.len()] {
            return Err(BSplineError::EmptyDomain);
        }
        if poles.iter().any(|q| !q.is_finite()) {
            return Err(BSplineError::NonFinitePole);
        }
        if let Some(w) = &weights
            && (w.len() != poles.len() || w.iter().any(|&x| !(x.is_finite() && x > 0.0)))
        {
            return Err(BSplineError::BadWeights);
        }
        Ok(BSplineCurve3 {
            degree,
            knots,
            poles,
            weights,
            periodic: false,
        })
    }

    /// Marks the curve as periodic (closed with matching derivatives). The knot
    /// vector stays the full expanded one.
    pub fn with_periodic(mut self, periodic: bool) -> BSplineCurve3 {
        self.periodic = periodic;
        self
    }

    pub fn degree(&self) -> u8 {
        self.degree
    }
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }
    pub fn poles(&self) -> &[Point3] {
        &self.poles
    }
    pub fn weights(&self) -> Option<&[f64]> {
        self.weights.as_deref()
    }
    pub fn is_rational(&self) -> bool {
        self.weights.is_some()
    }
    pub fn is_periodic(&self) -> bool {
        self.periodic
    }

    /// Parameter domain `[knots[p], knots[n+1]]`.
    pub fn domain(&self) -> Interval {
        Interval {
            lo: self.knots[self.degree as usize],
            hi: self.knots[self.poles.len()],
        }
    }

    /// Distinct knot values inside the domain, including both ends. Each pair of
    /// neighbours bounds one polynomial span.
    pub fn span_breaks(&self) -> SmallVec<[f64; 16]> {
        let d = self.domain();
        let mut out: SmallVec<[f64; 16]> = SmallVec::new();
        for &k in &self.knots {
            if k >= d.lo && k <= d.hi && out.last() != Some(&k) {
                out.push(k);
            }
        }
        out
    }

    /// Point at `t`. `t` outside the domain is clamped to it.
    pub fn eval(&self, t: f64) -> Point3 {
        Point3(self.derivs(t, 0)[0].0)
    }

    /// `[C(t), C'(t), …]` up to order `n ≤ 3` (index 0 is the point as a vector
    /// from the origin). Rational curves use homogeneous coordinates and the
    /// quotient rule (A4.2). `t` outside the domain is clamped to it.
    pub fn derivs(&self, t: f64, n: usize) -> SmallVec<[Vec3; 4]> {
        if (self.degree as usize) < SMALL {
            self.derivs_n::<SMALL>(t, n)
        } else {
            self.derivs_n::<{ MAX_DEGREE + 1 }>(t, n)
        }
    }

    fn derivs_n<const N: usize>(&self, t: f64, n: usize) -> SmallVec<[Vec3; 4]> {
        let n = n.min(3);
        let p = self.degree as usize;
        let last = self.poles.len() - 1;
        let t = self.domain().clamp(t);
        let span = find_span(last, p, t, &self.knots);
        let nders = ders_basis_funs_n::<N>(span, t, p, n, &self.knots);

        // Homogeneous derivatives: (w·P, w).
        let mut aw = [DVec4::ZERO; 4];
        for (k, row) in nders.iter().enumerate().take(n + 1) {
            for (j, &b) in row.iter().enumerate().take(p + 1) {
                let i = span - p + j;
                let w = self.weights.as_ref().map_or(1.0, |w| w[i]);
                aw[k] += b * (self.poles[i].0 * w).extend(w);
            }
        }

        let mut out: SmallVec<[Vec3; 4]> = SmallVec::new();
        if self.weights.is_none() {
            for a in aw.iter().take(n + 1) {
                out.push(Vec3(a.truncate()));
            }
            return out;
        }
        // CK[k] = (A[k] - Σ_{i=1..k} C(k,i)·w[i]·CK[k-i]) / w[0]
        const BINOM: [[f64; 4]; 4] = [
            [1.0, 0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0, 0.0],
            [1.0, 2.0, 1.0, 0.0],
            [1.0, 3.0, 3.0, 1.0],
        ];
        let w0 = aw[0].w;
        for k in 0..=n {
            let mut v = aw[k].truncate();
            for i in 1..=k {
                v -= BINOM[k][i] * aw[i].w * out[k - i].0;
            }
            out.push(Vec3(v / w0));
        }
        out
    }

    /// The curve with `a` applied to its poles. Weights are unchanged (exact for
    /// affine maps).
    pub fn transformed(&self, a: &Affine) -> BSplineCurve3 {
        BSplineCurve3 {
            poles: self.poles.iter().map(|&q| a.apply_point(q)).collect(),
            ..self.clone()
        }
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use std::f64::consts::FRAC_1_SQRT_2;

    /// Nine-pole NURBS unit circle in the XY plane, domain [0, 1].
    pub(crate) fn full_circle() -> BSplineCurve3 {
        let p = Point3::new;
        let w = FRAC_1_SQRT_2;
        BSplineCurve3::new(
            2,
            vec![
                0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
            ],
            vec![
                p(1., 0., 0.),
                p(1., 1., 0.),
                p(0., 1., 0.),
                p(-1., 1., 0.),
                p(-1., 0., 0.),
                p(-1., -1., 0.),
                p(0., -1., 0.),
                p(1., -1., 0.),
                p(1., 0., 0.),
            ],
            Some(vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0]),
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::full_circle;
    use super::*;
    use std::f64::consts::FRAC_1_SQRT_2;

    fn p(x: f64, y: f64, z: f64) -> Point3 {
        Point3::new(x, y, z)
    }

    #[test]
    fn rejects_bad_knot_count() {
        let r = BSplineCurve3::new(
            1,
            vec![0.0, 0.0, 1.0],
            vec![p(0., 0., 0.), p(1., 0., 0.)],
            None,
        );
        assert_eq!(r, Err(BSplineError::BadKnotCount));
    }

    #[test]
    fn rejects_decreasing_knots() {
        let r = BSplineCurve3::new(
            1,
            vec![0.0, 0.0, 2.0, 1.0, 1.0],
            vec![p(0., 0., 0.), p(1., 0., 0.), p(2., 0., 0.)],
            None,
        );
        assert_eq!(r, Err(BSplineError::KnotsDecreasing));
    }

    #[test]
    fn rejects_bad_weights_and_degree() {
        let poles = vec![p(0., 0., 0.), p(1., 0., 0.)];
        let knots = vec![0.0, 0.0, 1.0, 1.0];
        assert_eq!(
            BSplineCurve3::new(1, knots.clone(), poles.clone(), Some(vec![1.0, 0.0])),
            Err(BSplineError::BadWeights)
        );
        assert_eq!(
            BSplineCurve3::new(1, knots.clone(), poles.clone(), Some(vec![1.0])),
            Err(BSplineError::BadWeights)
        );
        assert_eq!(
            BSplineCurve3::new(0, knots, poles, None),
            Err(BSplineError::DegreeZeroOrTooHigh)
        );
    }

    fn polyline() -> BSplineCurve3 {
        BSplineCurve3::new(
            1,
            vec![0.0, 0.0, 1.0, 2.0, 2.0],
            vec![p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.)],
            None,
        )
        .unwrap()
    }

    #[test]
    fn linear_bspline_is_polyline() {
        let c = polyline();
        assert_eq!(c.eval(1.5), p(1.0, 0.5, 0.0));
        assert_eq!(c.eval(0.5), p(0.5, 0.0, 0.0));
    }

    #[test]
    fn eval_at_end_knot_is_last_pole() {
        assert_eq!(polyline().eval(2.0), p(1.0, 1.0, 0.0));
        assert_eq!(polyline().eval(0.0), p(0.0, 0.0, 0.0));
        // Outside the domain clamps.
        assert_eq!(polyline().eval(7.0), p(1.0, 1.0, 0.0));
    }

    fn cubic_bezier() -> BSplineCurve3 {
        BSplineCurve3::new(
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            vec![p(0., 0., 0.), p(1., 2., 0.), p(3., 2., 1.), p(4., 0., -1.)],
            None,
        )
        .unwrap()
    }

    #[test]
    fn bezier_cubic_matches_bernstein() {
        let c = cubic_bezier();
        let q = c.poles().to_vec();
        for i in 0..=10 {
            let t = i as f64 / 10.0;
            let s = 1.0 - t;
            let b = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
            let want = q
                .iter()
                .zip(b)
                .fold(Vec3::ZERO, |acc, (pt, bi)| acc + pt.to_vec() * bi);
            assert!((c.eval(t).to_vec() - want).length() < 1e-15, "t = {t}");
        }
    }

    fn check_derivs_by_finite_difference(c: &BSplineCurve3, ts: &[f64]) {
        let h = 1e-6;
        for &t in ts {
            let d = c.derivs(t, 2);
            let d0p = c.derivs(t + h, 1);
            let d0m = c.derivs(t - h, 1);
            let fd1 = (d0p[0] - d0m[0]) / (2.0 * h);
            let fd2 = (d0p[1] - d0m[1]) / (2.0 * h);
            assert!(
                (d[1] - fd1).length() <= 1e-6 * (1.0 + d[1].length()),
                "C' at {t}"
            );
            assert!(
                (d[2] - fd2).length() <= 1e-5 * (1.0 + d[2].length()),
                "C'' at {t}"
            );
        }
    }

    #[test]
    fn derivs_match_finite_difference() {
        check_derivs_by_finite_difference(&cubic_bezier(), &[0.1, 0.37, 0.5, 0.9]);
        check_derivs_by_finite_difference(&full_circle(), &[0.05, 0.3, 0.55, 0.8]);
    }

    #[test]
    fn rational_quarter_circle_exact() {
        let c = BSplineCurve3::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.)],
            Some(vec![1.0, FRAC_1_SQRT_2, 1.0]),
        )
        .unwrap();
        for i in 0..=100 {
            let q = c.eval(i as f64 / 100.0);
            assert!((q.to_vec().length() - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn full_rational_circle_on_unit_circle() {
        let c = full_circle();
        for i in 0..=400 {
            let q = c.eval(i as f64 / 400.0);
            assert!((q.to_vec().length() - 1.0).abs() < 1e-12);
            assert_eq!(q.z(), 0.0);
        }
    }

    #[test]
    fn span_breaks_are_distinct() {
        assert_eq!(
            full_circle().span_breaks().as_slice(),
            &[0.0, 0.25, 0.5, 0.75, 1.0]
        );
    }
}
