// SPDX-License-Identifier: MIT OR Apache-2.0

//! B-spline and NURBS curves (D02 §3). Curves up to degree 7 keep their
//! spans in Bézier form and evaluate from it (as surfaces do).

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
    /// The spans in Bézier form (degrees below [`SMALL`] only).
    bezier: Option<CurveBezier>,
}

/// Every span of a curve in Bézier form: an evaluation cache made from the
/// poles and knots.
#[derive(Clone, PartialEq, Debug)]
struct CurveBezier {
    /// Distinct knots in the domain: span `a` is `[brk[a], brk[a + 1]]`.
    brk: Vec<f64>,
    /// `1 / (brk[a + 1] - brk[a])`.
    inv: Vec<f64>,
    /// Homogeneous Bézier points `(w·P, w)`, `p + 1` per span.
    pts: Vec<DVec4>,
}

impl CurveBezier {
    fn new(c: &BSplineCurve3) -> CurveBezier {
        let p = c.degree as usize;
        let n = c.poles.len();
        let mut brk = vec![c.knots[p]];
        let mut pts = Vec::new();
        let mut d = [DVec4::ZERO; SMALL];
        for i in p..n {
            let (lo, hi) = (c.knots[i], c.knots[i + 1]);
            if hi <= lo {
                continue;
            }
            brk.push(hi);
            for j in 0..=p {
                for (k, dk) in d.iter_mut().enumerate().take(p + 1) {
                    let q = i - p + k;
                    let w = c.weights.as_ref().map_or(1.0, |w| w[q]);
                    *dk = (c.poles[q].0 * w).extend(w);
                }
                let args: SmallVec<[f64; SMALL]> =
                    (0..p).map(|r| if r < p - j { lo } else { hi }).collect();
                pts.push(blossom(&c.knots, p, i, &args, &mut d));
            }
        }
        CurveBezier {
            inv: brk.windows(2).map(|w| 1.0 / (w[1] - w[0])).collect(),
            brk,
            pts,
        }
    }

    /// Homogeneous derivatives `A[k]` of the span holding `t`, `k <= n`.
    fn homogeneous(&self, p: usize, t: f64, n: usize) -> [DVec4; 4] {
        let (a, tl, ih) = span_local(&self.brk, &self.inv, t);
        let b = bernstein_ders(p, tl, n);
        let pts = &self.pts[a * (p + 1)..(a + 1) * (p + 1)];
        let mut aw = [DVec4::ZERO; 4];
        let mut scale = 1.0;
        for (k, row) in b.iter().enumerate().take(n + 1) {
            let mut acc = DVec4::ZERO;
            for (q, &bi) in pts.iter().zip(row.iter()) {
                acc += *q * bi;
            }
            aw[k] = if k > 0 { acc * scale } else { acc };
            scale *= ih;
        }
        aw
    }
}

/// The blossom of a B-spline at the arguments `args` (`p` of them), from
/// the `p + 1` points `d` that affect span `span` (*The NURBS Book* §5.2:
/// de Boor's algorithm with one argument per level). Bézier point `j` of
/// the span is the blossom at `p - j` copies of its start and `j` of its
/// end.
pub(crate) fn blossom(
    knots: &[f64],
    p: usize,
    span: usize,
    args: &[f64],
    d: &mut [DVec4],
) -> DVec4 {
    for (r, &t) in args.iter().enumerate().take(p) {
        let r = r + 1;
        for j in (r..=p).rev() {
            let lo = knots[span + j - p];
            let hi = knots[span + 1 + j - r];
            let alpha = (t - lo) / (hi - lo);
            d[j] = d[j - 1] * (1.0 - alpha) + d[j] * alpha;
        }
    }
    d[p]
}

/// The span of `x` among the breaks `brk` (`x` clamped to them; a break
/// starts its span), the local parameter in [0, 1] and `inv` of the span
/// (1 / its length).
#[inline(always)]
pub(crate) fn span_local(brk: &[f64], inv: &[f64], x: f64) -> (usize, f64, f64) {
    let m = brk.len() - 1;
    let x = x.clamp(brk[0], brk[m]);
    let a = brk[1..m].partition_point(|&k| k <= x);
    let ih = inv[a];
    // At most 1 (the end of the span) up to rounding.
    (a, ((x - brk[a]) * ih).min(1.0), ih)
}

/// Bernstein basis of degree `p < SMALL` at `t` and its derivatives in `t`
/// up to order `n <= 3`: `[k][i]` is the k-th derivative of `B_i`.
fn bernstein_ders(p: usize, t: f64, n: usize) -> [[f64; SMALL]; 4] {
    let s = 1.0 - t;
    // low[k]: the basis of degree p - k.
    let mut low = [[0.0f64; SMALL]; 4];
    let mut b = [0.0f64; SMALL];
    b[0] = 1.0;
    for deg in 0..=p {
        if deg > 0 {
            let mut prev = 0.0;
            for x in b.iter_mut().take(deg + 1) {
                let cur = *x;
                *x = s * cur + t * prev;
                prev = cur;
            }
        }
        if p - deg <= 3 {
            low[p - deg] = b;
        }
    }
    let mut out = [[0.0f64; SMALL]; 4];
    out[0] = low[0];
    // The k-th derivative of B_i is p!/(p - k)! times the sum over j of
    // (-1)^(k - j) C(k, j) B_(i - j) at degree p - k.
    const BINOM: [[f64; 4]; 4] = [
        [1.0, 0.0, 0.0, 0.0],
        [1.0, 1.0, 0.0, 0.0],
        [1.0, 2.0, 1.0, 0.0],
        [1.0, 3.0, 3.0, 1.0],
    ];
    let mut factor = 1.0;
    for k in 1..=n.min(p) {
        factor *= (p + 1 - k) as f64;
        for i in 0..=p {
            let mut acc = 0.0;
            for j in 0..=k {
                if j <= i && i - j <= p - k {
                    let sign = if (k - j) % 2 == 0 { 1.0 } else { -1.0 };
                    acc += sign * BINOM[k][j] * low[k][i - j];
                }
            }
            out[k][i] = factor * acc;
        }
    }
    out
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
        let mut c = BSplineCurve3 {
            degree,
            knots,
            poles,
            weights,
            periodic: false,
            bezier: None,
        };
        c.bezier = c.make_bezier();
        Ok(c)
    }

    fn make_bezier(&self) -> Option<CurveBezier> {
        ((self.degree as usize) < SMALL).then(|| CurveBezier::new(self))
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
        let n = n.min(3);
        let aw = match &self.bezier {
            Some(bz) => bz.homogeneous(self.degree as usize, t, n),
            None => self.homogeneous_de_boor(t, n),
        };
        self.quotient(&aw, n)
    }

    /// [`Self::derivs`] by de Boor's algorithm (the only path above degree
    /// 7).
    #[cfg(test)]
    fn derivs_de_boor(&self, t: f64, n: usize) -> SmallVec<[Vec3; 4]> {
        let n = n.min(3);
        let aw = self.homogeneous_de_boor(t, n);
        self.quotient(&aw, n)
    }

    fn homogeneous_de_boor(&self, t: f64, n: usize) -> [DVec4; 4] {
        if (self.degree as usize) < SMALL {
            self.homogeneous_n::<SMALL>(t, n)
        } else {
            self.homogeneous_n::<{ MAX_DEGREE + 1 }>(t, n)
        }
    }

    /// Homogeneous derivatives `(w·P, w)` up to order `n`.
    fn homogeneous_n<const N: usize>(&self, t: f64, n: usize) -> [DVec4; 4] {
        let p = self.degree as usize;
        let last = self.poles.len() - 1;
        let t = self.domain().clamp(t);
        let span = find_span(last, p, t, &self.knots);
        let nders = ders_basis_funs_n::<N>(span, t, p, n, &self.knots);
        let mut aw = [DVec4::ZERO; 4];
        for (k, row) in nders.iter().enumerate().take(n + 1) {
            for (j, &b) in row.iter().enumerate().take(p + 1) {
                let i = span - p + j;
                let w = self.weights.as_ref().map_or(1.0, |w| w[i]);
                aw[k] += b * (self.poles[i].0 * w).extend(w);
            }
        }
        aw
    }

    /// `[C, C', ...]` from the homogeneous derivatives (quotient rule, A4.2).
    fn quotient(&self, aw: &[DVec4; 4], n: usize) -> SmallVec<[Vec3; 4]> {
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
        let mut c = BSplineCurve3 {
            poles: self.poles.iter().map(|&q| a.apply_point(q)).collect(),
            ..self.clone()
        };
        c.bezier = c.make_bezier();
        c
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
    fn bezier_spans_match_de_boor() {
        // Clamped, unclamped and with a double knot; degrees 1 to 5; with
        // and without weights.
        let cases: [(u8, Vec<f64>); 5] = [
            (3, vec![0., 0., 0., 0., 1., 2., 3., 3., 3., 3.]),
            (2, vec![0., 0., 0., 0.3, 0.3, 0.9, 2., 2., 2.]),
            (3, vec![0., 1., 2., 3., 4., 5., 6., 7., 8.]),
            (1, vec![0., 0., 1., 2.5, 3., 3.]),
            (5, vec![0., 0., 0., 0., 0., 0., 0.4, 1., 1., 1., 1., 1., 1.]),
        ];
        let mut n = 0;
        for (ci, (deg, k)) in cases.into_iter().enumerate() {
            let np = k.len() - deg as usize - 1;
            let poles: Vec<Point3> = (0..np)
                .map(|i| {
                    let f = i as f64 + ci as f64;
                    p(f * 2.0, libm::sin(f) * 3.0, libm::cos(1.3 * f))
                })
                .collect();
            let w: Vec<f64> = (0..np).map(|i| 0.6 + 0.3 * libm::sin(i as f64)).collect();
            for weights in [None, Some(w)] {
                let c = BSplineCurve3::new(deg, k.clone(), poles.clone(), weights).unwrap();
                assert!(c.bezier.is_some());
                let d = c.domain();
                let mut ts: Vec<f64> = (0..=200)
                    .map(|i| d.at(i as f64 / 200.0 * 1.2 - 0.1))
                    .collect();
                ts.extend(c.span_breaks());
                for t in ts {
                    let a = c.derivs(t, 3);
                    let b = c.derivs_de_boor(t, 3);
                    for (x, y) in a.iter().zip(&b) {
                        assert!(
                            (*x - *y).length() <= 1e-11 * (1.0 + y.length()),
                            "case {ci} at {t}: {x:?} against {y:?}"
                        );
                        n += 1;
                    }
                    assert_eq!(c.eval(t), Point3(a[0].0));
                }
            }
        }
        assert!(n > 3000);
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
