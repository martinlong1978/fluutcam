// SPDX-License-Identifier: MIT OR Apache-2.0

//! Surfaces (D02 §4). Parametrisations follow STEP (ISO 10303-42) exactly, so
//! p-curves read from STEP files are valid without conversion.
//!
//! The normal is `Su × Sv` normalised. Face sense (whether the material is on the
//! normal side) is the job of the topology layer (D03), not of the surface.

mod analytic;
mod bspline;
mod march;
mod section;
mod swept;

pub use bspline::BSplineSurface;
pub use section::{SectionCurve, SectionPlane, SurfaceSection, plane_section};

use crate::bounds::Interval;
use crate::curve::Curve3;
use crate::frame::Frame;
use crate::tol::Tol;
use crate::transform::Affine;
use crate::types::{Point3, UnitVec3, Vec3};
use std::f64::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;

#[derive(Clone, PartialEq, Debug)]
pub enum Surface {
    /// `S = o + u·x + v·y`.
    Plane { frame: Frame },
    /// `S = o + r(cos u·x + sin u·y) + v·z`.
    Cylinder { frame: Frame, r: f64 },
    /// `S = o + (r + v·tan α)(cos u·x + sin u·y) + v·z`; `r` is the radius at `v = 0`.
    Cone {
        frame: Frame,
        r: f64,
        semi_angle: f64,
    },
    /// `S = o + r·cos v(cos u·x + sin u·y) + r·sin v·z`; `v` is latitude.
    Sphere { frame: Frame, r: f64 },
    /// `S = o + (R + ρ cos v)(cos u·x + sin u·y) + ρ sin v·z`.
    Torus {
        frame: Frame,
        major: f64,
        minor: f64,
    },
    /// B-spline or NURBS surface.
    BSpline(Arc<BSplineSurface>),
    /// STEP `SURFACE_OF_LINEAR_EXTRUSION`: `S = C(u) + v·dir` (`v` is a length).
    Extrusion { profile: Box<Curve3>, dir: UnitVec3 },
    /// STEP `SURFACE_OF_REVOLUTION`: profile point `C(v)` rotated by angle `u`
    /// about the axis through `axis_origin` along `axis` (right-hand rule).
    Revolution {
        profile: Box<Curve3>,
        axis_origin: Point3,
        axis: UnitVec3,
    },
    /// STEP `OFFSET_SURFACE`: `S = B + dist·N_B`, evaluated numerically. May
    /// self-intersect where `dist` exceeds a concave curvature radius (CH-04).
    Offset { basis: Box<Surface>, dist: f64 },
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SurfaceKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    BSpline,
    Extrusion,
    Revolution,
    Offset,
}

/// Point and partial derivatives up to second order.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct SurfDerivs {
    pub p: Point3,
    pub su: Vec3,
    pub sv: Vec3,
    pub suu: Vec3,
    pub suv: Vec3,
    pub svv: Vec3,
}

/// Principal curvatures `k1 ≥ k2` with their unit directions. Positive
/// curvature bends towards the normal.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Curvatures {
    pub k1: f64,
    pub k2: f64,
    pub d1: Vec3,
    pub d2: Vec3,
}

/// A rectangle in parameter space.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct UvBox {
    pub u: Interval,
    pub v: Interval,
}

impl UvBox {
    pub fn new(u: Interval, v: Interval) -> UvBox {
        UvBox { u, v }
    }

    pub fn contains(&self, u: f64, v: f64) -> bool {
        self.u.contains(u) && self.v.contains(v)
    }

    pub fn clamp(&self, u: f64, v: f64) -> (f64, f64) {
        (self.u.clamp(u), self.v.clamp(v))
    }

    pub fn centre(&self) -> (f64, f64) {
        (self.u.at(0.5), self.v.at(0.5))
    }
}

/// Result of a closest-point query.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct SurfClosest {
    pub u: f64,
    pub v: f64,
    pub point: Point3,
    pub dist: f64,
}

/// Relative threshold below which `|Su × Sv|` counts as degenerate.
const SINGULAR_REL: f64 = 1e-12;
/// Relative tolerance for treating an affine map as a similarity.
const SIMILARITY_TOL: f64 = 1e-12;

impl Surface {
    pub fn plane(frame: Frame) -> Surface {
        Surface::Plane { frame }
    }

    /// `None` unless `r` is finite and positive.
    pub fn cylinder(frame: Frame, r: f64) -> Option<Surface> {
        positive(r).then_some(Surface::Cylinder { frame, r })
    }

    /// `None` unless `r ≥ 0` and `0 < semi_angle < π/2`.
    pub fn cone(frame: Frame, r: f64, semi_angle: f64) -> Option<Surface> {
        (r.is_finite() && r >= 0.0 && semi_angle > 0.0 && semi_angle < FRAC_PI_2).then_some(
            Surface::Cone {
                frame,
                r,
                semi_angle,
            },
        )
    }

    pub fn sphere(frame: Frame, r: f64) -> Option<Surface> {
        positive(r).then_some(Surface::Sphere { frame, r })
    }

    /// `None` unless `minor > 0` and `major ≥ 0`. `major < minor` is a degenerate
    /// (spindle) torus, as STEP `DEGENERATE_TOROIDAL_SURFACE` allows.
    pub fn torus(frame: Frame, major: f64, minor: f64) -> Option<Surface> {
        (positive(minor) && major.is_finite() && major >= 0.0).then_some(Surface::Torus {
            frame,
            major,
            minor,
        })
    }

    pub fn extrusion(profile: Curve3, dir: UnitVec3) -> Surface {
        Surface::Extrusion {
            profile: Box::new(profile),
            dir,
        }
    }

    pub fn revolution(profile: Curve3, axis_origin: Point3, axis: UnitVec3) -> Surface {
        Surface::Revolution {
            profile: Box::new(profile),
            axis_origin,
            axis,
        }
    }

    pub fn kind(&self) -> SurfaceKind {
        match self {
            Surface::Plane { .. } => SurfaceKind::Plane,
            Surface::Cylinder { .. } => SurfaceKind::Cylinder,
            Surface::Cone { .. } => SurfaceKind::Cone,
            Surface::Sphere { .. } => SurfaceKind::Sphere,
            Surface::Torus { .. } => SurfaceKind::Torus,
            Surface::BSpline(_) => SurfaceKind::BSpline,
            Surface::Extrusion { .. } => SurfaceKind::Extrusion,
            Surface::Revolution { .. } => SurfaceKind::Revolution,
            Surface::Offset { .. } => SurfaceKind::Offset,
        }
    }

    pub fn eval(&self, u: f64, v: f64) -> Point3 {
        match self {
            Surface::BSpline(b) => b.eval(u, v),
            _ => self.derivs(u, v).p,
        }
    }

    /// The point and first derivatives: the same bits as
    /// [`Surface::derivs`], cheaper for B-splines.
    pub fn derivs1(&self, u: f64, v: f64) -> (Point3, Vec3, Vec3) {
        match self {
            Surface::BSpline(b) => b.derivs1(u, v),
            _ => {
                let d = self.derivs(u, v);
                (d.p, d.su, d.sv)
            }
        }
    }

    /// Point and derivatives to second order. Exact for all kinds except
    /// `Offset`, whose second derivatives are central differences of its exact
    /// first derivatives.
    pub fn derivs(&self, u: f64, v: f64) -> SurfDerivs {
        match self {
            Surface::BSpline(b) => b.derivs(u, v),
            Surface::Extrusion { .. } | Surface::Revolution { .. } | Surface::Offset { .. } => {
                swept::derivs(self, u, v)
            }
            _ => analytic::derivs(self, u, v),
        }
    }

    /// Unit normal `Su × Sv`, or `None` at singular points (sphere poles, cone
    /// apex, torus spindle points, revolution profile on the axis).
    pub fn normal(&self, u: f64, v: f64) -> Option<UnitVec3> {
        normal_from(&self.derivs(u, v))
    }

    /// Principal curvatures from the first and second fundamental forms. `None`
    /// at singular points.
    pub fn curvatures(&self, u: f64, v: f64) -> Option<Curvatures> {
        let d = self.derivs(u, v);
        let n = normal_from(&d)?.get();
        let (e, f, g) = (d.su.dot(d.su), d.su.dot(d.sv), d.sv.dot(d.sv));
        let (l, m, nn) = (d.suu.dot(n), d.suv.dot(n), d.svv.dot(n));
        let det = e * g - f * f;
        let k_gauss = (l * nn - m * m) / det;
        let h = (e * nn + g * l - 2.0 * f * m) / (2.0 * det);
        let disc = (h * h - k_gauss).max(0.0).sqrt();
        let (k1, k2) = (h + disc, h - disc);
        let dir = |k: f64, fallback: Vec3| {
            // (L − kE) du + (M − kF) dv = 0 and (M − kF) du + (N − kG) dv = 0.
            let c1 = (m - k * f, -(l - k * e));
            let c2 = (nn - k * g, -(m - k * f));
            let (du, dv) = if c1.0.abs() + c1.1.abs() >= c2.0.abs() + c2.1.abs() {
                c1
            } else {
                c2
            };
            (d.su * du + d.sv * dv)
                .normalized()
                .map_or(fallback, UnitVec3::get)
        };
        let d1 = dir(k1, d.su.normalized().map_or(Vec3::ZERO, UnitVec3::get));
        // At umbilic points every direction is principal; take d2 ⟂ d1.
        let d2 = if disc == 0.0 {
            n.cross(d1)
        } else {
            dir(k2, n.cross(d1))
        };
        Some(Curvatures { k1, k2, d1, d2 })
    }

    /// Periods in u and v (`None` = not periodic).
    pub fn periodicity(&self) -> (Option<f64>, Option<f64>) {
        match self {
            Surface::Plane { .. } => (None, None),
            Surface::Cylinder { .. } | Surface::Cone { .. } | Surface::Sphere { .. } => {
                (Some(TAU), None)
            }
            Surface::Torus { .. } => (Some(TAU), Some(TAU)),
            Surface::BSpline(b) => {
                let d = b.domain();
                let (pu, pv) = b.periodic();
                (pu.then(|| d.u.length()), pv.then(|| d.v.length()))
            }
            Surface::Extrusion { profile, .. } => (swept::curve_period(profile), None),
            Surface::Revolution { profile, .. } => (Some(TAU), swept::curve_period(profile)),
            Surface::Offset { basis, .. } => basis.periodicity(),
        }
    }

    /// Parameters inside `uv` where smoothness or shape changes (B-spline knots,
    /// quarter turns), including the box ends. Numeric algorithms work patch by
    /// patch between these.
    pub fn breaks(&self, uv: UvBox) -> (Vec<f64>, Vec<f64>) {
        let ends = |iv: Interval| vec![iv.lo, iv.hi];
        let clip = |inner: &[f64], iv: Interval| {
            let mut out = vec![iv.lo];
            out.extend(inner.iter().copied().filter(|&k| k > iv.lo && k < iv.hi));
            out.push(iv.hi);
            out
        };
        let quarters = |iv: Interval| {
            let first = (iv.lo / FRAC_PI_2).floor() as i64;
            let last = (iv.hi / FRAC_PI_2).ceil() as i64;
            let inner: Vec<f64> = (first..=last).map(|k| k as f64 * FRAC_PI_2).collect();
            clip(&inner, iv)
        };
        match self {
            Surface::Plane { .. } => (ends(uv.u), ends(uv.v)),
            Surface::Cylinder { .. } | Surface::Cone { .. } => (quarters(uv.u), ends(uv.v)),
            Surface::Sphere { .. } | Surface::Torus { .. } => (quarters(uv.u), quarters(uv.v)),
            Surface::BSpline(b) => {
                let (bu, bv) = b.span_breaks();
                (clip(&bu, uv.u), clip(&bv, uv.v))
            }
            Surface::Extrusion { profile, .. } => (profile.breaks(uv.u).to_vec(), ends(uv.v)),
            Surface::Revolution { profile, .. } => (quarters(uv.u), profile.breaks(uv.v).to_vec()),
            Surface::Offset { basis, .. } => basis.breaks(uv),
        }
    }

    /// Closest point to `p` with parameters inside `uv`.
    ///
    /// Analytic kinds: closed form in the local frame; if the unconstrained
    /// answer lies outside `uv` it is clamped and refined with damped Newton
    /// inside the box (D02 §6). Other kinds: best of a seed grid per patch (see
    /// [`Surface::breaks`]), refined the same way. D02 §4.2 control-net seeding
    /// replaces the grid if the S2 spike shows it is too slow.
    pub fn closest(&self, p: Point3, uv: UvBox, tol: &Tol) -> SurfClosest {
        let (u, v) = match self {
            Surface::BSpline(b) => {
                let seed = bspline_seed(self, b, p, uv, tol);
                newton_uv(self, p, seed, uv, tol)
            }
            Surface::Extrusion { .. } | Surface::Revolution { .. } | Surface::Offset { .. } => {
                let seed = grid_seed(self, p, uv);
                newton_uv(self, p, seed, uv, tol)
            }
            _ => {
                let (u0, v0, u_clamped) = analytic::closest_unconstrained(self, p, uv);
                if !u_clamped && uv.contains(u0, v0) {
                    (u0, v0)
                } else {
                    let (uc, vc) = uv.clamp(u0, v0);
                    newton_uv(self, p, (uc, vc), uv, tol)
                }
            }
        };
        let point = self.eval(u, v);
        SurfClosest {
            u,
            v,
            point,
            dist: point.distance(p),
        }
    }

    /// The surface offset by `d` along its normal (D02 §4.3). Analytic kinds give
    /// an exact surface of the same kind, or `None` when the offset degenerates
    /// (radius through zero), so the caller treats the face as unreachable for
    /// that tool (spec 04 §4.3.4). Other kinds give an `Offset` wrapper, with the
    /// same parametrisation as the base surface.
    pub fn offset(&self, d: f64) -> Option<Surface> {
        if !d.is_finite() {
            return None;
        }
        match self {
            Surface::Plane { frame } => Some(Surface::Plane {
                frame: Frame {
                    origin: frame.origin + frame.z.get() * d,
                    ..*frame
                },
            }),
            Surface::Cylinder { frame, r } => Surface::cylinder(*frame, r + d),
            Surface::Cone {
                frame,
                r,
                semi_angle,
            } => {
                let (s, c) = libm::sincos(*semi_angle);
                let new_r = r + d * c;
                if !positive(new_r) {
                    return None;
                }
                let frame = Frame {
                    origin: frame.origin - frame.z.get() * (d * s),
                    ..*frame
                };
                Surface::cone(frame, new_r, *semi_angle)
            }
            Surface::Sphere { frame, r } => Surface::sphere(*frame, r + d),
            Surface::Torus {
                frame,
                major,
                minor,
            } => Surface::torus(*frame, *major, minor + d),
            Surface::Offset { basis, dist } => Some(Surface::Offset {
                basis: basis.clone(),
                dist: dist + d,
            }),
            Surface::BSpline(_) | Surface::Extrusion { .. } | Surface::Revolution { .. } => {
                Some(Surface::Offset {
                    basis: Box::new(self.clone()),
                    dist: d,
                })
            }
        }
    }

    /// The surface mapped by `a`. B-splines accept any affine map; the other
    /// kinds only similarities (rotation, translation, uniform scale, mirror),
    /// otherwise `None`.
    ///
    /// The point set maps exactly; the parametrisation may change. Use
    /// [`Surface::transformed_param`] to map parameters (for p-curves) and
    /// [`Surface::transform_flips_normal`] to know whether the face sense must
    /// flip (D03 §2).
    pub fn transformed(&self, a: &Affine) -> Option<Surface> {
        if let Surface::BSpline(b) = self {
            return Some(Surface::BSpline(Arc::new(b.transformed(a))));
        }
        let s = a.uniform_scale(SIMILARITY_TOL)?;
        // Origin, x and z are mapped; y = z × x stays right-handed, so a mirror
        // reverses y relative to the mapped original.
        let map_frame = |f: &Frame| {
            let x = a.apply_vec(f.x.get()) / s;
            let z = a.apply_vec(f.z.get()) / s;
            Frame::new(a.apply_point(f.origin), z, x)
        };
        let map_dir = |d: &UnitVec3| UnitVec3::new(a.apply_vec(d.get()));
        Some(match self {
            Surface::Plane { frame } => Surface::plane(map_frame(frame)?),
            Surface::Cylinder { frame, r } => Surface::cylinder(map_frame(frame)?, r * s)?,
            Surface::Cone {
                frame,
                r,
                semi_angle,
            } => Surface::cone(map_frame(frame)?, r * s, *semi_angle)?,
            Surface::Sphere { frame, r } => Surface::sphere(map_frame(frame)?, r * s)?,
            Surface::Torus {
                frame,
                major,
                minor,
            } => Surface::torus(map_frame(frame)?, major * s, minor * s)?,
            Surface::BSpline(_) => return None,
            Surface::Extrusion { profile, dir } => {
                Surface::extrusion(profile.transformed(a)?, map_dir(dir)?)
            }
            Surface::Revolution {
                profile,
                axis_origin,
                axis,
            } => Surface::revolution(
                profile.transformed(a)?,
                a.apply_point(*axis_origin),
                map_dir(axis)?,
            ),
            Surface::Offset { basis, dist } => {
                // Keep the same point set: if the basis normal flips, so must
                // the offset direction.
                let sign = if basis.transform_flips_normal(a) {
                    -1.0
                } else {
                    1.0
                };
                Surface::Offset {
                    basis: Box::new(basis.transformed(a)?),
                    dist: dist * s * sign,
                }
            }
        })
    }

    /// Maps parameters of `self` to the parameters of `self.transformed(a)` at
    /// the mapped point: `transformed.eval(map(u, v)) = a(self.eval(u, v))`.
    /// `s` is the scale and "mirror" means `a` reverses orientation.
    ///
    /// | Kind | Map |
    /// |---|---|
    /// | Plane | `(s·u, ±s·v)` (− for a mirror) |
    /// | Cylinder, Cone | `(±u, s·v)` |
    /// | Sphere, Torus, Revolution | `(±u, v)` |
    /// | Extrusion | `(u, s·v)` |
    /// | B-spline | `(u, v)` |
    /// | Offset | as its basis |
    pub fn transformed_param(&self, a: &Affine, u: f64, v: f64) -> (f64, f64) {
        let s = a.uniform_scale(SIMILARITY_TOL).unwrap_or(1.0);
        let m = if a.reverses_orientation() { -1.0 } else { 1.0 };
        match self {
            Surface::Plane { .. } => (s * u, m * s * v),
            Surface::Cylinder { .. } | Surface::Cone { .. } => (m * u, s * v),
            Surface::Sphere { .. } | Surface::Torus { .. } | Surface::Revolution { .. } => {
                (m * u, v)
            }
            Surface::Extrusion { .. } => (u, s * v),
            Surface::BSpline(_) => (u, v),
            Surface::Offset { basis, .. } => basis.transformed_param(a, u, v),
        }
    }

    /// True when, at mapped parameters, the normal of `self.transformed(a)`
    /// points opposite to `a` applied to the normal of `self`. Then the
    /// topology layer must flip the face sense (D03 §2). Only mirrors can do
    /// this, and only for kinds whose parametrisation is not reversed by
    /// [`Surface::transformed_param`].
    pub fn transform_flips_normal(&self, a: &Affine) -> bool {
        if !a.reverses_orientation() {
            return false;
        }
        match self {
            Surface::BSpline(_) | Surface::Extrusion { .. } => true,
            Surface::Offset { basis, .. } => basis.transform_flips_normal(a),
            _ => false,
        }
    }
}

fn positive(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

fn normal_from(d: &SurfDerivs) -> Option<UnitVec3> {
    let n = d.su.cross(d.sv);
    let scale = d.su.length_squared().max(d.sv.length_squared());
    if n.length() <= SINGULAR_REL * scale {
        return None;
    }
    n.normalized()
}

/// Seeds per patch side for B-spline branch-and-bound.
const BSPLINE_PATCH_SEEDS: usize = 3;

/// Branch-and-bound over Bézier patches (D02 §4.2): patches are visited in
/// order of the distance from `p` to their pole bounds, and a patch is skipped
/// once that lower bound exceeds the best distance found. Each visited patch
/// is seeded from a small grid and refined by Newton inside the patch.
fn bspline_seed(s: &Surface, b: &BSplineSurface, p: Point3, uv: UvBox, tol: &Tol) -> (f64, f64) {
    let mut cands: Vec<(f64, UvBox)> = b
        .patches()
        .iter()
        .filter_map(|(pb, bounds)| {
            let iu = Interval::new(pb.u.lo.max(uv.u.lo), pb.u.hi.min(uv.u.hi))?;
            let iv = Interval::new(pb.v.lo.max(uv.v.lo), pb.v.hi.min(uv.v.hi))?;
            Some((bounds.distance(p), UvBox::new(iu, iv)))
        })
        .collect();
    cands.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (cu, cv) = uv.centre();
    let mut best = (cu, cv, s.eval(cu, cv).distance(p));
    for (lower_bound, pb) in cands {
        if lower_bound >= best.2 {
            break;
        }
        let mut seed = (pb.u.lo, pb.v.lo, f64::INFINITY);
        for i in 0..BSPLINE_PATCH_SEEDS {
            for j in 0..BSPLINE_PATCH_SEEDS {
                let fu = (i as f64 + 0.5) / BSPLINE_PATCH_SEEDS as f64;
                let fv = (j as f64 + 0.5) / BSPLINE_PATCH_SEEDS as f64;
                let (u, v) = (pb.u.at(fu), pb.v.at(fv));
                let d = s.eval(u, v).distance(p);
                if d < seed.2 {
                    seed = (u, v, d);
                }
            }
        }
        let (u, v) = newton_uv(s, p, (seed.0, seed.1), pb, tol);
        let d = s.eval(u, v).distance(p);
        if d < best.2 {
            best = (u, v, d);
        }
    }
    (best.0, best.1)
}

/// Seeds per patch side for numeric closest point.
const GRID_SEEDS: usize = 9;

/// Best of a `GRID_SEEDS²` grid of parameters on every patch of `uv`.
fn grid_seed(s: &Surface, p: Point3, uv: UvBox) -> (f64, f64) {
    let (bu, bv) = s.breaks(uv);
    let mut best = (uv.u.lo, uv.v.lo, f64::INFINITY);
    for wu in bu.windows(2) {
        for wv in bv.windows(2) {
            for i in 0..GRID_SEEDS {
                for j in 0..GRID_SEEDS {
                    let fu = i as f64 / (GRID_SEEDS - 1) as f64;
                    let fv = j as f64 / (GRID_SEEDS - 1) as f64;
                    let (u, v) = (wu[0] + (wu[1] - wu[0]) * fu, wv[0] + (wv[1] - wv[0]) * fv);
                    let d2 = (s.eval(u, v) - p).length_squared();
                    if d2 < best.2 {
                        best = (u, v, d2);
                    }
                }
            }
        }
    }
    (best.0, best.1)
}

/// Maximum iterations of [`newton_uv`] (D02 §6).
const NEWTON_UV_MAX_ITER: usize = 50;

/// Minimises `|S(u,v) − p|` inside `uv` from `start` with a damped Newton step on
/// the gradient of `½|S − p|²`, clamping to the box. Falls back to a gradient
/// step where the Hessian is not positive definite. Stops when the 3-D step is
/// below `tol.solver`.
pub(crate) fn newton_uv(
    s: &Surface,
    p: Point3,
    start: (f64, f64),
    uv: UvBox,
    tol: &Tol,
) -> (f64, f64) {
    let (mut u, mut v) = start;
    let mut d = s.derivs(u, v);
    let mut dist2 = (d.p - p).length_squared();
    let split = matches!(s, Surface::BSpline(_));
    for _ in 0..NEWTON_UV_MAX_ITER {
        let r = d.p - p;
        let (gu, gv) = (r.dot(d.su), r.dot(d.sv));
        let huu = d.su.dot(d.su) + r.dot(d.suu);
        let huv = d.su.dot(d.sv) + r.dot(d.suv);
        let hvv = d.sv.dot(d.sv) + r.dot(d.svv);
        let det = huu * hvv - huv * huv;
        let (mut du, mut dv) = if huu > 0.0 && det > 0.0 {
            (-(hvv * gu - huv * gv) / det, -(huu * gv - huv * gu) / det)
        } else {
            // Gradient step scaled by the metric.
            let e = d.su.length_squared().max(f64::MIN_POSITIVE);
            let g = d.sv.length_squared().max(f64::MIN_POSITIVE);
            (-gu / e, -gv / g)
        };
        // Damping: halve until the distance does not grow.
        let mut accepted = None;
        for _ in 0..30 {
            let (nu, nv) = uv.clamp(u + du, v + dv);
            // A B-spline trial point takes the point alone (most are
            // rejected): the same bits as `derivs`.
            if split && (s.eval(nu, nv) - p).length_squared() > dist2 {
                du *= 0.5;
                dv *= 0.5;
                continue;
            }
            let nd = s.derivs(nu, nv);
            let nd2 = (nd.p - p).length_squared();
            if nd2 <= dist2 {
                accepted = Some((nu, nv, nd, nd2));
                break;
            }
            du *= 0.5;
            dv *= 0.5;
        }
        let Some((nu, nv, nd, nd2)) = accepted else {
            break;
        };
        let step = nd.p.distance(d.p);
        (u, v, d, dist2) = (nu, nv, nd, nd2);
        if step < tol.solver {
            break;
        }
    }
    (u, v)
}

#[cfg(test)]
mod tests;
