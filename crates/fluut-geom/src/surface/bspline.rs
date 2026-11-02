// SPDX-License-Identifier: MIT OR Apache-2.0

//! B-spline and NURBS surfaces (D02 §4.1): tensor-product de Boor evaluation
//! (*The NURBS Book* A3.6) and the rational quotient rule (A4.4).

use super::{SurfDerivs, UvBox};
use crate::bounds::{Aabb3, Interval};
use crate::bspline::BSplineError;
use crate::bspline::basis::{MAX_DEGREE, SMALL, ders_basis_funs_n, find_span};
use crate::transform::Affine;
use crate::types::{Point3, Vec3};
use glam::DVec4;

#[derive(Clone, PartialEq, Debug)]
pub struct BSplineSurface {
    deg_u: u8,
    deg_v: u8,
    knots_u: Vec<f64>,
    knots_v: Vec<f64>,
    /// Row-major: pole `(i, j)` (i along u, j along v) is at `i * nv + j`.
    poles: Vec<Point3>,
    nu: usize,
    nv: usize,
    weights: Option<Vec<f64>>,
    periodic_u: bool,
    periodic_v: bool,
    /// Per Bézier patch: its parameter box and the bounds of the poles that
    /// affect it (which contain the patch, by the convex hull property).
    patches: Vec<(UvBox, Aabb3)>,
}

impl BSplineSurface {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        deg_u: u8,
        deg_v: u8,
        knots_u: Vec<f64>,
        knots_v: Vec<f64>,
        poles: Vec<Point3>,
        nu: usize,
        nv: usize,
        weights: Option<Vec<f64>>,
    ) -> Result<BSplineSurface, BSplineError> {
        let (pu, pv) = (deg_u as usize, deg_v as usize);
        if pu == 0 || pv == 0 || pu > MAX_DEGREE || pv > MAX_DEGREE {
            return Err(BSplineError::DegreeZeroOrTooHigh);
        }
        if nu < pu + 1 || nv < pv + 1 || poles.len() != nu * nv {
            return Err(BSplineError::TooFewPoles);
        }
        if knots_u.len() != nu + pu + 1 || knots_v.len() != nv + pv + 1 {
            return Err(BSplineError::BadKnotCount);
        }
        for k in [&knots_u, &knots_v] {
            if k.iter().any(|x| !x.is_finite()) || k.windows(2).any(|w| w[1] < w[0]) {
                return Err(BSplineError::KnotsDecreasing);
            }
        }
        if knots_u[pu] >= knots_u[nu] || knots_v[pv] >= knots_v[nv] {
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
        let mut s = BSplineSurface {
            deg_u,
            deg_v,
            knots_u,
            knots_v,
            poles,
            nu,
            nv,
            weights,
            periodic_u: false,
            periodic_v: false,
            patches: Vec::new(),
        };
        s.patches = s.compute_patches();
        Ok(s)
    }

    pub fn with_periodic(mut self, periodic_u: bool, periodic_v: bool) -> BSplineSurface {
        self.periodic_u = periodic_u;
        self.periodic_v = periodic_v;
        self
    }

    pub fn degrees(&self) -> (u8, u8) {
        (self.deg_u, self.deg_v)
    }
    pub fn pole_counts(&self) -> (usize, usize) {
        (self.nu, self.nv)
    }
    pub fn pole(&self, i: usize, j: usize) -> Point3 {
        self.poles[i * self.nv + j]
    }
    pub fn is_rational(&self) -> bool {
        self.weights.is_some()
    }
    pub fn periodic(&self) -> (bool, bool) {
        (self.periodic_u, self.periodic_v)
    }

    pub fn domain(&self) -> UvBox {
        let (pu, pv) = (self.deg_u as usize, self.deg_v as usize);
        UvBox::new(
            Interval {
                lo: self.knots_u[pu],
                hi: self.knots_u[self.nu],
            },
            Interval {
                lo: self.knots_v[pv],
                hi: self.knots_v[self.nv],
            },
        )
    }

    /// Distinct knot values inside the domain along u and v: the Bézier patch
    /// boundaries.
    pub fn span_breaks(&self) -> (Vec<f64>, Vec<f64>) {
        let d = self.domain();
        let breaks = |knots: &[f64], iv: Interval| {
            let mut out: Vec<f64> = Vec::new();
            for &k in knots {
                if k >= iv.lo && k <= iv.hi && out.last() != Some(&k) {
                    out.push(k);
                }
            }
            out
        };
        (breaks(&self.knots_u, d.u), breaks(&self.knots_v, d.v))
    }

    /// Point and derivatives to second order. Parameters outside the domain are
    /// clamped to it.
    pub fn derivs(&self, u: f64, v: f64) -> SurfDerivs {
        let skl = self.skl::<2>(u, v);
        SurfDerivs {
            p: Point3(skl[0][0].0),
            su: skl[1][0],
            sv: skl[0][1],
            suu: skl[2][0],
            suv: skl[1][1],
            svv: skl[0][2],
        }
    }

    /// The point and first derivatives: the same bits as [`Self::derivs`].
    pub fn derivs1(&self, u: f64, v: f64) -> (Point3, Vec3, Vec3) {
        let skl = self.skl::<1>(u, v);
        (Point3(skl[0][0].0), skl[1][0], skl[0][1])
    }

    /// The point: the same bits as [`Self::derivs`].
    pub fn eval(&self, u: f64, v: f64) -> Point3 {
        Point3(self.skl::<0>(u, v)[0][0].0)
    }

    /// Derivatives `S_kl` for `k + l ≤ D` (others zero). An entry does not
    /// depend on `D`.
    fn skl<const D: usize>(&self, u: f64, v: f64) -> [[Vec3; 3]; 3] {
        if (self.deg_u.max(self.deg_v) as usize) < SMALL {
            self.skl_n::<SMALL, D>(u, v)
        } else {
            self.skl_n::<{ MAX_DEGREE + 1 }, D>(u, v)
        }
    }

    fn skl_n<const N: usize, const D: usize>(&self, u: f64, v: f64) -> [[Vec3; 3]; 3] {
        let (pu, pv) = (self.deg_u as usize, self.deg_v as usize);
        let d = self.domain();
        let (u, v) = d.clamp(u, v);
        let su = find_span(self.nu - 1, pu, u, &self.knots_u);
        let sv = find_span(self.nv - 1, pv, v, &self.knots_v);
        let nu_d = ders_basis_funs_n::<N>(su, u, pu, D, &self.knots_u);
        let nv_d = ders_basis_funs_n::<N>(sv, v, pv, D, &self.knots_v);

        // Homogeneous derivatives aw[k][l] for k + l ≤ D.
        let mut aw = [[DVec4::ZERO; 3]; 3];
        for k in 0..=D {
            for l in 0..=(D - k) {
                let mut acc = DVec4::ZERO;
                for (a, &bu) in nu_d[k].iter().enumerate().take(pu + 1) {
                    if bu == 0.0 {
                        continue;
                    }
                    let i = su - pu + a;
                    for (b, &bv) in nv_d[l].iter().enumerate().take(pv + 1) {
                        let j = sv - pv + b;
                        let idx = i * self.nv + j;
                        let w = self.weights.as_ref().map_or(1.0, |w| w[idx]);
                        acc += (bu * bv) * (self.poles[idx].0 * w).extend(w);
                    }
                }
                aw[k][l] = acc;
            }
        }

        let mut skl = [[Vec3::ZERO; 3]; 3];
        if self.weights.is_none() {
            for k in 0..=D {
                for l in 0..=(D - k) {
                    skl[k][l] = Vec3(aw[k][l].truncate());
                }
            }
        } else {
            const BINOM: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 2.0, 1.0]];
            let w = |k: usize, l: usize| aw[k][l].w;
            for k in 0..=D {
                for l in 0..=(D - k) {
                    let mut v3 = Vec3(aw[k][l].truncate());
                    for j in 1..=l {
                        v3 -= skl[k][l - j] * (BINOM[l][j] * w(0, j));
                    }
                    for i in 1..=k {
                        v3 -= skl[k - i][l] * (BINOM[k][i] * w(i, 0));
                        let mut v2 = Vec3::ZERO;
                        for j in 1..=l {
                            v2 += skl[k - i][l - j] * (BINOM[l][j] * w(i, j));
                        }
                        v3 -= v2 * BINOM[k][i];
                    }
                    skl[k][l] = v3 / w(0, 0);
                }
            }
        }
        skl
    }

    /// The surface with `a` applied to its poles (exact for any affine map).
    pub fn transformed(&self, a: &Affine) -> BSplineSurface {
        let mut s = BSplineSurface {
            poles: self.poles.iter().map(|&q| a.apply_point(q)).collect(),
            ..self.clone()
        };
        s.patches = s.compute_patches();
        s
    }

    /// Bézier patches with the bounds of their poles.
    pub fn patches(&self) -> &[(UvBox, Aabb3)] {
        &self.patches
    }

    fn compute_patches(&self) -> Vec<(UvBox, Aabb3)> {
        let (pu, pv) = (self.deg_u as usize, self.deg_v as usize);
        // Spans with non-zero length: (span index, interval).
        let spans = |knots: &[f64], p: usize, n: usize| {
            (p..n)
                .filter(|&i| knots[i + 1] > knots[i])
                .map(|i| {
                    (
                        i,
                        Interval {
                            lo: knots[i],
                            hi: knots[i + 1],
                        },
                    )
                })
                .collect::<Vec<_>>()
        };
        let su = spans(&self.knots_u, pu, self.nu);
        let sv = spans(&self.knots_v, pv, self.nv);
        let mut out = Vec::with_capacity(su.len() * sv.len());
        for &(i, iu) in &su {
            for &(j, iv) in &sv {
                let mut b = Aabb3::empty();
                for a in i - pu..=i {
                    for c in j - pv..=j {
                        b = b.with_point(self.poles[a * self.nv + c]);
                    }
                }
                out.push((UvBox::new(iu, iv), b));
            }
        }
        out
    }
}
