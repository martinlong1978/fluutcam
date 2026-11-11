// SPDX-License-Identifier: MIT OR Apache-2.0

//! B-spline and NURBS surfaces (D02 §4.1): tensor-product de Boor evaluation
//! (*The NURBS Book* A3.6) and the rational quotient rule (A4.4).
//!
//! Surfaces up to degree 7 also keep every Bézier patch in Bézier form
//! ([`Bezier`]), made once by blossoming, and evaluate from it: the span is
//! found by a binary search over the distinct knots, and the Bernstein basis
//! needs no knot differences or divisions.

use super::{SurfDerivs, UvBox};
use crate::bounds::{Aabb3, Interval};
use crate::bspline::basis::{MAX_DEGREE, SMALL, ders_basis_funs_n, find_span};
use crate::bspline::{BSplineError, blossom, span_local};
use crate::transform::Affine;
use crate::types::{Point3, Vec3};
use glam::{DVec3, DVec4};

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
    /// affect it (which contain the patch, by the convex hull property),
    /// cut by the bounds of its Bézier points where there is a Bézier form
    /// (they contain it too, and are tighter).
    patches: Vec<(UvBox, Aabb3)>,
    /// The patches in Bézier form (degrees below [`SMALL`] only).
    bezier: Option<Bezier>,
}

/// Every Bézier patch of a surface in Bézier form: an evaluation cache made
/// from the poles and knots, so it adds no state of its own.
#[derive(Clone, PartialEq, Debug)]
struct Bezier {
    /// Distinct knots in the domain along u and v: patch `(a, b)` spans
    /// `[us[a], us[a + 1]] × [vs[b], vs[b + 1]]`.
    us: Vec<f64>,
    vs: Vec<f64>,
    /// `1 / (us[a + 1] − us[a])` and the same along v.
    ius: Vec<f64>,
    ivs: Vec<f64>,
    /// Bézier points per patch, patch-major (patch `a * (vs.len() − 1) + b`),
    /// then `(i, j)` at `i * (deg_v + 1) + j`; homogeneous (`w·P`) if
    /// rational.
    xyz: Vec<DVec3>,
    /// Bézier weights in the same order, if rational.
    w: Option<Vec<f64>>,
}

/// Distinct knots in `[lo, hi]` with the start index of each non-empty span.
fn spans(knots: &[f64], p: usize, n: usize) -> (Vec<f64>, Vec<usize>) {
    let mut brk = vec![knots[p]];
    let mut idx = Vec::new();
    for i in p..n {
        if knots[i + 1] > knots[i] {
            idx.push(i);
            brk.push(knots[i + 1]);
        }
    }
    (brk, idx)
}

fn inverse_lengths(brk: &[f64]) -> Vec<f64> {
    brk.windows(2).map(|w| 1.0 / (w[1] - w[0])).collect()
}

/// `a` cut by `b`; `a` where they do not overlap (not possible for two
/// bounds of the same patch, but rounding).
fn cut(a: &Aabb3, b: &Aabb3) -> Aabb3 {
    let c = Aabb3 {
        min: Point3(a.min.0.max(b.min.0)),
        max: Point3(a.max.0.min(b.max.0)),
    };
    if c.min.0.cmple(c.max.0).all() { c } else { *a }
}

impl Bezier {
    /// Bounds of the Bézier points of patch `k` (degrees `pu`, `pv`), which
    /// contain the patch (convex hull property; the weights are positive).
    fn bounds(&self, k: usize, pu: usize, pv: usize) -> Aabb3 {
        let n = (pu + 1) * (pv + 1);
        let mut b = Aabb3::empty();
        for (i, q) in self.xyz[k * n..(k + 1) * n].iter().enumerate() {
            let w = self.w.as_ref().map_or(1.0, |w| w[k * n + i]);
            b = b.with_point(Point3(*q / w));
        }
        b
    }

    fn new(s: &BSplineSurface) -> Bezier {
        let (pu, pv) = (s.deg_u as usize, s.deg_v as usize);
        let (us, su) = spans(&s.knots_u, pu, s.nu);
        let (vs, sv) = spans(&s.knots_v, pv, s.nv);
        let hom = |i: usize, j: usize| {
            let q = s.poles[i * s.nv + j].0;
            let w = s.weights.as_ref().map_or(1.0, |w| w[i * s.nv + j]);
            (q * w).extend(w)
        };
        let args = |lo: f64, hi: f64, p: usize, j: usize| -> Vec<f64> {
            (0..p).map(|r| if r < p - j { lo } else { hi }).collect()
        };
        let mut xyz = Vec::with_capacity(su.len() * sv.len() * (pu + 1) * (pv + 1));
        let mut ws = Vec::with_capacity(xyz.capacity());
        let mut d = [DVec4::ZERO; SMALL];
        for (a, &iu) in su.iter().enumerate() {
            // Blossom along u for every pole row along v the span needs.
            let mut col: Vec<[DVec4; SMALL]> = vec![[DVec4::ZERO; SMALL]; s.nv];
            for (j, c) in col.iter_mut().enumerate() {
                for (i, ci) in c.iter_mut().enumerate().take(pu + 1) {
                    for (k, dk) in d.iter_mut().enumerate().take(pu + 1) {
                        *dk = hom(iu - pu + k, j);
                    }
                    let t = args(us[a], us[a + 1], pu, i);
                    *ci = blossom(&s.knots_u, pu, iu, &t, &mut d);
                }
            }
            for (b, &iv) in sv.iter().enumerate() {
                for i in 0..=pu {
                    for j in 0..=pv {
                        for (dk, ck) in d.iter_mut().zip(&col[iv - pv..=iv]) {
                            *dk = ck[i];
                        }
                        let t = args(vs[b], vs[b + 1], pv, j);
                        let q = blossom(&s.knots_v, pv, iv, &t, &mut d);
                        xyz.push(q.truncate());
                        ws.push(q.w);
                    }
                }
            }
        }
        Bezier {
            ius: inverse_lengths(&us),
            ivs: inverse_lengths(&vs),
            us,
            vs,
            xyz,
            w: s.weights.is_some().then_some(ws),
        }
    }
}

/// For `n` equal parts of `[lo, hi]` (local parameters of a Bézier span of
/// degree `p`): `[i][m][k]`, the weight of Bézier point `k` of the span in
/// Bézier point `m` of part `i` (the blossom at `p - m` copies of the
/// part's start and `m` of its end; the weights of each point sum to 1).
fn restriction(p: usize, lo: f64, hi: f64, n: usize) -> Vec<[[f64; SMALL]; SMALL]> {
    (0..n)
        .map(|i| {
            let a = lo + (hi - lo) * i as f64 / n as f64;
            let b = lo + (hi - lo) * (i + 1) as f64 / n as f64;
            let mut out = [[0.0f64; SMALL]; SMALL];
            for (m, row) in out.iter_mut().enumerate().take(p + 1) {
                // de Casteljau with one argument per level on the unit
                // vectors (the Bézier knots are p + 1 zeros and ones).
                let mut d = [[0.0f64; SMALL]; SMALL];
                for (k, dk) in d.iter_mut().enumerate().take(p + 1) {
                    dk[k] = 1.0;
                }
                for r in 1..=p {
                    let t = if r <= p - m { a } else { b };
                    for j in (r..=p).rev() {
                        let (lower, upper) = d.split_at_mut(j);
                        for (x, &y) in upper[0].iter_mut().zip(&lower[j - 1]).take(p + 1) {
                            *x = y * (1.0 - t) + *x * t;
                        }
                    }
                }
                *row = d[p];
            }
            out
        })
        .collect()
}

/// Derivatives of `P/w` from those of the homogeneous `(w·P, w)`, for
/// `k + l ≤ D` (*The NURBS Book* A4.4).
fn rational<const D: usize>(aw: &[[DVec4; 3]; 3]) -> [[Vec3; 3]; 3] {
    const BINOM: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 2.0, 1.0]];
    let w = |k: usize, l: usize| aw[k][l].w;
    let mut skl = [[Vec3::ZERO; 3]; 3];
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
    skl
}

/// Derivatives `S_kl` (`k + l ≤ D`) from the Bézier form: degrees
/// `pu, pv < N`. Parameters outside the domain are clamped to it; a knot
/// starts its span (as `find_span`).
#[inline(always)]
fn skl_bezier<const N: usize, const D: usize>(
    bz: &Bezier,
    pu: usize,
    pv: usize,
    u: f64,
    v: f64,
) -> [[Vec3; 3]; 3] {
    let (a, tu, iu) = span_local(&bz.us, &bz.ius, u);
    let (b, tv, iv) = span_local(&bz.vs, &bz.ivs, v);
    let bu = bernstein::<N, D>(pu, tu);
    let bv = bernstein::<N, D>(pv, tv);
    let n = (pu + 1) * (pv + 1);
    let base = (a * (bz.vs.len() - 1) + b) * n;
    let pts = &bz.xyz[base..base + n];
    // Along v first, then along u, scaled by the span lengths.
    let mut t = [[DVec3::ZERO; 3]; N];
    for (i, ti) in t.iter_mut().enumerate().take(pu + 1) {
        for (l, tl) in ti.iter_mut().enumerate().take(D + 1) {
            let mut acc = DVec3::ZERO;
            for j in 0..=pv {
                acc += pts[i * (pv + 1) + j] * bv[l][j];
            }
            *tl = acc;
        }
    }
    let su = [1.0, iu, iu * iu];
    let sv = [1.0, iv, iv * iv];
    let mut skl = [[Vec3::ZERO; 3]; 3];
    for k in 0..=D {
        for l in 0..=(D - k) {
            let mut acc = DVec3::ZERO;
            for i in 0..=pu {
                acc += t[i][l] * bu[k][i];
            }
            if k + l > 0 {
                acc *= su[k] * sv[l];
            }
            skl[k][l] = Vec3(acc);
        }
    }
    let Some(ws) = &bz.w else {
        return skl;
    };
    let mut aw = [[DVec4::ZERO; 3]; 3];
    for k in 0..=D {
        for l in 0..=(D - k) {
            aw[k][l] = skl[k][l].0.extend(0.0);
        }
    }
    let ws = &ws[base..base + n];
    let mut tw = [[0.0f64; 3]; N];
    for (i, ti) in tw.iter_mut().enumerate().take(pu + 1) {
        for (l, tl) in ti.iter_mut().enumerate().take(D + 1) {
            let mut acc = 0.0;
            for j in 0..=pv {
                acc += ws[i * (pv + 1) + j] * bv[l][j];
            }
            *tl = acc;
        }
    }
    for k in 0..=D {
        for l in 0..=(D - k) {
            let mut acc = 0.0;
            for i in 0..=pu {
                acc += tw[i][l] * bu[k][i];
            }
            if k + l > 0 {
                acc *= su[k] * sv[l];
            }
            aw[k][l].w = acc;
        }
    }
    rational::<D>(&aw)
}

/// Bernstein basis of degree `p` at `t` (`[0]`) and its first and second
/// derivatives in `t` (`[1]`, `[2]`, up to order `D`). The values do not
/// depend on `D`.
#[inline(always)]
fn bernstein<const N: usize, const D: usize>(p: usize, t: f64) -> [[f64; N]; 3] {
    let s = 1.0 - t;
    if p == 3 && N >= 4 {
        // Cubic in closed form (the recurrence below up to rounding).
        let mut out = [[0.0f64; N]; 3];
        let (s2, t2, st) = (s * s, t * t, s * t);
        out[0][0] = s2 * s;
        out[0][1] = 3.0 * (st * s);
        out[0][2] = 3.0 * (st * t);
        out[0][3] = t2 * t;
        if D >= 1 {
            let q1 = 2.0 * st;
            out[1][0] = -3.0 * s2;
            out[1][1] = 3.0 * (s2 - q1);
            out[1][2] = 3.0 * (q1 - t2);
            out[1][3] = 3.0 * t2;
        }
        if D >= 2 {
            out[2][0] = 6.0 * s;
            out[2][1] = 6.0 * (t - 2.0 * s);
            out[2][2] = 6.0 * (s - 2.0 * t);
            out[2][3] = 6.0 * t;
        }
        return out;
    }
    let mut b = [0.0f64; N];
    let mut b1 = [0.0f64; N];
    let mut b2 = [0.0f64; N];
    b[0] = 1.0;
    for deg in 1..=p {
        if deg + 2 == p + 1 {
            b2 = b;
        }
        if deg == p {
            b1 = b;
        }
        let mut prev = 0.0;
        for x in b.iter_mut().take(deg + 1) {
            let cur = *x;
            *x = s * cur + t * prev;
            prev = cur;
        }
    }
    let mut out = [[0.0f64; N]; 3];
    out[0] = b;
    if D >= 1 {
        // B′ᵢ = p (Bᵢ₋₁ − Bᵢ) at degree p − 1.
        let pf = p as f64;
        for i in 0..=p {
            let l = if i >= 1 { b1[i - 1] } else { 0.0 };
            let r = if i < p { b1[i] } else { 0.0 };
            out[1][i] = pf * (l - r);
        }
    }
    if D >= 2 && p >= 2 {
        // B″ᵢ = p (p − 1) (Bᵢ₋₂ − 2 Bᵢ₋₁ + Bᵢ) at degree p − 2.
        let pf = (p * (p - 1)) as f64;
        for i in 0..=p {
            let at = |k: isize| {
                if k >= 0 && (k as usize) < p - 1 {
                    b2[k as usize]
                } else {
                    0.0
                }
            };
            let i = i as isize;
            out[2][i as usize] = pf * (at(i - 2) - 2.0 * at(i - 1) + at(i));
        }
    }
    out
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
            bezier: None,
        };
        s.bezier = s.make_bezier();
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
        if let Some(bz) = &self.bezier {
            let (pu, pv) = (self.deg_u as usize, self.deg_v as usize);
            if pu == 3 && pv == 3 {
                return skl_bezier::<4, D>(bz, 3, 3, u, v);
            }
            return skl_bezier::<SMALL, D>(bz, pu, pv, u, v);
        }
        self.skl_de_boor::<D>(u, v)
    }

    /// [`Self::skl`] by de Boor's algorithm (the only path above degree 7).
    fn skl_de_boor<const D: usize>(&self, u: f64, v: f64) -> [[Vec3; 3]; 3] {
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

        if self.weights.is_none() {
            let mut skl = [[Vec3::ZERO; 3]; 3];
            for k in 0..=D {
                for l in 0..=(D - k) {
                    skl[k][l] = Vec3(aw[k][l].truncate());
                }
            }
            skl
        } else {
            rational::<D>(&aw)
        }
    }

    /// The surface with `a` applied to its poles (exact for any affine map).
    pub fn transformed(&self, a: &Affine) -> BSplineSurface {
        let mut s = BSplineSurface {
            poles: self.poles.iter().map(|&q| a.apply_point(q)).collect(),
            ..self.clone()
        };
        s.bezier = s.make_bezier();
        s.patches = s.compute_patches();
        s
    }

    fn make_bezier(&self) -> Option<Bezier> {
        ((self.deg_u.max(self.deg_v) as usize) < SMALL).then(|| Bezier::new(self))
    }

    /// Bézier patches with the bounds of their poles.
    pub fn patches(&self) -> &[(UvBox, Aabb3)] {
        &self.patches
    }

    /// Proven bounds of `n × n` cells of the surface over `uv`, which must
    /// lie in one Bézier patch: cell `(i, j)` (`i` along u, at `i * n + j`)
    /// is bounded by the box of the Bézier points of the surface restricted
    /// to it, which contains it (convex hull property, positive weights),
    /// grown by 1e-13 relative for rounding. `None` without a Bézier form
    /// (degrees above 7).
    pub fn cell_bounds(&self, uv: UvBox, n: usize) -> Option<Vec<Aabb3>> {
        let bz = self.bezier.as_ref()?;
        let (pu, pv) = (self.deg_u as usize, self.deg_v as usize);
        let (cu, cv) = uv.centre();
        let (a, _, _) = span_local(&bz.us, &bz.ius, cu);
        let (b, _, _) = span_local(&bz.vs, &bz.ivs, cv);
        let np = (pu + 1) * (pv + 1);
        let base = (a * (bz.vs.len() - 1) + b) * np;
        let hom = |i: usize, j: usize| {
            let k = base + i * (pv + 1) + j;
            bz.xyz[k].extend(bz.w.as_ref().map_or(1.0, |w| w[k]))
        };
        let lu = |x: f64| (x - bz.us[a]) * bz.ius[a];
        let lv = |x: f64| (x - bz.vs[b]) * bz.ivs[b];
        let mu = restriction(pu, lu(uv.u.lo), lu(uv.u.hi), n);
        let mv = restriction(pv, lv(uv.v.lo), lv(uv.v.hi), n);
        // strips[i][m][j]: Bézier point m along u of strip i, pole row j.
        let mut strips = vec![[[DVec4::ZERO; SMALL]; SMALL]; n];
        for (strip, m) in strips.iter_mut().zip(&mu) {
            for (row, mrow) in strip.iter_mut().zip(m.iter()).take(pu + 1) {
                for (j, x) in row.iter_mut().enumerate().take(pv + 1) {
                    for (k, &c) in mrow.iter().enumerate().take(pu + 1) {
                        *x += hom(k, j) * c;
                    }
                }
            }
        }
        let mut out = Vec::with_capacity(n * n);
        for strip in &strips {
            for m in &mv {
                let mut bx = Aabb3::empty();
                for row in strip.iter().take(pu + 1) {
                    for mrow in m.iter().take(pv + 1) {
                        let mut q = DVec4::ZERO;
                        for (x, &c) in row.iter().zip(mrow.iter()).take(pv + 1) {
                            q += *x * c;
                        }
                        bx = bx.with_point(Point3(q.truncate() / q.w));
                    }
                }
                let e = 1e-13 * (1.0 + bx.min.0.abs().max(bx.max.0.abs()).max_element());
                out.push(bx.expand(e));
            }
        }
        Some(out)
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
                if let Some(bz) = &self.bezier {
                    // Grown by a few units in the last place: an evaluated
                    // point can round just outside the hull of its points.
                    b = cut(&b, &bz.bounds(out.len(), pu, pv));
                    let m = b.min.0.abs().max(b.max.0.abs()).max_element();
                    b = b.expand(1e-13 * (1.0 + m));
                }
                out.push((UvBox::new(iu, iv), b));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pseudo-random number in [0, 1) from a counter (no state shared
    /// between tests).
    fn rnd(k: u64) -> f64 {
        let x = k
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (x >> 11) as f64 / (1u64 << 53) as f64
    }

    #[allow(clippy::too_many_arguments)]
    fn surface(
        pu: u8,
        pv: u8,
        ku: Vec<f64>,
        kv: Vec<f64>,
        rational: bool,
        seed: u64,
    ) -> BSplineSurface {
        let nu = ku.len() - pu as usize - 1;
        let nv = kv.len() - pv as usize - 1;
        let mut poles = Vec::new();
        let mut w = Vec::new();
        for i in 0..nu {
            for j in 0..nv {
                let k = seed + (i * nv + j) as u64 * 3;
                poles.push(Point3::new(
                    i as f64 * 3.0 + rnd(k),
                    j as f64 * 2.0 + rnd(k + 1),
                    5.0 * rnd(k + 2) - 2.0,
                ));
                w.push(0.5 + rnd(k + 7));
            }
        }
        BSplineSurface::new(pu, pv, ku, kv, poles, nu, nv, rational.then_some(w)).unwrap()
    }

    #[test]
    fn bezier_form_matches_de_boor() {
        let cases = [
            // Clamped uniform bicubic.
            (
                3,
                3,
                vec![0., 0., 0., 0., 1., 2., 3., 3., 3., 3.],
                vec![0., 0., 0., 0., 0.5, 1., 1., 1., 1.],
            ),
            // Non-uniform with a double knot, degrees 2 and 3.
            (
                2,
                3,
                vec![0., 0., 0., 0.3, 0.3, 0.9, 2., 2., 2.],
                vec![-1., -1., -1., -1., 0.2, 0.7, 4., 4., 4., 4.],
            ),
            // Unclamped (domain inside the knots), degree 3 and 1.
            (
                3,
                1,
                vec![0., 1., 2., 3., 4., 5., 6., 7., 8.],
                vec![0., 0., 1., 2.5, 3., 3.],
            ),
            // Degree 5 by 4.
            (
                5,
                4,
                vec![0., 0., 0., 0., 0., 0., 0.4, 1., 1., 1., 1., 1., 1.],
                vec![0., 0., 0., 0., 0., 2., 3., 3., 3., 3., 3.],
            ),
        ];
        let mut n = 0;
        for (c, (pu, pv, ku, kv)) in cases.into_iter().enumerate() {
            for rational in [false, true] {
                let s = surface(pu, pv, ku.clone(), kv.clone(), rational, 100 * c as u64);
                assert!(s.bezier.is_some());
                let d = s.domain();
                let (bu, bv) = s.span_breaks();
                // Random points, the knots themselves and points outside.
                let mut uvs: Vec<(f64, f64)> = (0..300)
                    .map(|k| {
                        let k = 1000 + 2 * k as u64;
                        (d.u.at(rnd(k) * 1.2 - 0.1), d.v.at(rnd(k + 1) * 1.2 - 0.1))
                    })
                    .collect();
                for &u in &bu {
                    for &v in &bv {
                        uvs.push((u, v));
                    }
                }
                for (u, v) in uvs {
                    let a = s.skl::<2>(u, v);
                    let b = s.skl_de_boor::<2>(u, v);
                    for k in 0..=2 {
                        for l in 0..=(2 - k) {
                            let (x, y) = (a[k][l], b[k][l]);
                            let tol = 1e-12 * (1.0 + y.length());
                            assert!(
                                (x - y).length() <= tol,
                                "case {c} rational {rational} ({u}, {v}) S{k}{l}: {x:?} against {y:?}"
                            );
                            n += 1;
                        }
                    }
                    // The point does not depend on the order asked for.
                    assert_eq!(s.eval(u, v), Point3(a[0][0].0));
                    let d1 = s.derivs1(u, v);
                    assert_eq!((d1.1, d1.2), (a[1][0], a[0][1]));
                }
            }
        }
        assert!(n > 10_000);
    }

    #[test]
    fn patch_bounds_contain_the_patch_and_are_tighter_than_the_poles() {
        let cases = [
            (
                3,
                3,
                vec![0., 0., 0., 0., 1., 2., 3., 3., 3., 3.],
                vec![0., 0., 0., 0., 0.5, 1., 1., 1., 1.],
            ),
            (
                2,
                3,
                vec![0., 0., 0., 0.3, 0.3, 0.9, 2., 2., 2.],
                vec![-1., -1., -1., -1., 0.2, 0.7, 4., 4., 4., 4.],
            ),
            (
                3,
                1,
                vec![0., 1., 2., 3., 4., 5., 6., 7., 8.],
                vec![0., 0., 1., 2.5, 3., 3.],
            ),
        ];
        let (mut tighter, mut n) = (0, 0);
        for (c, (pu, pv, ku, kv)) in cases.into_iter().enumerate() {
            for rational in [false, true] {
                let s = surface(pu, pv, ku.clone(), kv.clone(), rational, 50 + c as u64);
                let mut plain = s.clone();
                plain.bezier = None;
                let poles = plain.compute_patches();
                for ((uv, b), (_, pb)) in s.patches().iter().zip(&poles) {
                    for i in 0..=20 {
                        for j in 0..=20 {
                            let p = s.eval(uv.u.at(i as f64 / 20.0), uv.v.at(j as f64 / 20.0));
                            assert!(
                                p.0.cmpge(b.min.0).all() && p.0.cmple(b.max.0).all(),
                                "case {c}: {p:?} outside {b:?}"
                            );
                            n += 1;
                        }
                    }
                    // Inside the pole box, up to the growth for rounding.
                    let grown = pb.expand(1e-12 * (1.0 + pb.max.0.abs().max_element()));
                    assert!(b.min.0.cmpge(grown.min.0).all() && b.max.0.cmple(grown.max.0).all());
                    let shrunk = |k: usize| {
                        b.min.0[k] > pb.min.0[k] + 1e-9 || b.max.0[k] < pb.max.0[k] - 1e-9
                    };
                    tighter += usize::from((0..3).any(shrunk));
                }
            }
        }
        assert!(n > 10_000 && tighter > 10, "{n} {tighter}");
    }

    #[test]
    fn cell_bounds_contain_their_cells() {
        let cases = [
            (
                3,
                3,
                vec![0., 0., 0., 0., 1., 2., 3., 3., 3., 3.],
                vec![0., 0., 0., 0., 0.5, 1., 1., 1., 1.],
            ),
            (
                2,
                3,
                vec![0., 0., 0., 0.3, 0.3, 0.9, 2., 2., 2.],
                vec![-1., -1., -1., -1., 0.2, 0.7, 4., 4., 4., 4.],
            ),
            (
                3,
                1,
                vec![0., 1., 2., 3., 4., 5., 6., 7., 8.],
                vec![0., 0., 1., 2.5, 3., 3.],
            ),
        ];
        let n = 4;
        let mut count = 0;
        for (c, (pu, pv, ku, kv)) in cases.into_iter().enumerate() {
            for rational in [false, true] {
                let s = surface(pu, pv, ku.clone(), kv.clone(), rational, 70 + c as u64);
                for (puv, pb) in s.patches() {
                    // The whole patch and a part of it.
                    let part = UvBox::new(
                        Interval {
                            lo: puv.u.at(0.2),
                            hi: puv.u.at(0.9),
                        },
                        Interval {
                            lo: puv.v.at(0.1),
                            hi: puv.v.at(0.6),
                        },
                    );
                    for uv in [*puv, part] {
                        let cells = s.cell_bounds(uv, n).unwrap();
                        assert_eq!(cells.len(), n * n);
                        for (k, b) in cells.iter().enumerate() {
                            let (i, j) = (k / n, k % n);
                            // Cells are tighter than the patch, up to rounding.
                            assert!(b.max.z() <= pb.max.z() + 1e-9);
                            for a in 0..=6 {
                                for e in 0..=6 {
                                    let u = uv.u.at((i as f64 + a as f64 / 6.0) / n as f64);
                                    let v = uv.v.at((j as f64 + e as f64 / 6.0) / n as f64);
                                    let p = s.eval(u, v);
                                    assert!(
                                        p.0.cmpge(b.min.0).all() && p.0.cmple(b.max.0).all(),
                                        "case {c} cell {k}: {p:?} outside {b:?}"
                                    );
                                    count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(count > 50_000, "{count}");
    }

    #[test]
    fn high_degrees_keep_de_boor() {
        let k: Vec<f64> = [0.0; 9].into_iter().chain([1.0; 9]).collect();
        let s = surface(8, 8, k.clone(), k, false, 7);
        assert!(s.bezier.is_none());
        let p = s.eval(0.3, 0.6);
        assert!(p.is_finite());
    }
}
