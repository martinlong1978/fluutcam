// SPDX-License-Identifier: MIT OR Apache-2.0

//! Plane sections of single surfaces (D02 §5). Bodies are sectioned face by face
//! by the topology layer (D03 §5.1), which also clips to face trims; here the
//! result is clipped to the face's parameter box only.
//!
//! Closed forms give exact curves for planes, cylinders, spheres, and for cones
//! and tori cut perpendicular to or through their axis. Everything else is
//! marched (`march.rs`) and fitted. Tangential contacts are reported, not
//! resolved (CH-03).

use super::analytic::closest_unconstrained;
use super::{Surface, UvBox, march};
use crate::bounds::Interval;
use crate::curve::Curve3;
use crate::frame::Frame;
use crate::tol::Tol;
use crate::types::{Point3, UnitVec3, Vec3};
use std::f64::consts::{FRAC_PI_2, TAU};

/// A cutting plane: the XY plane of `frame`. Its normal is `frame.z`; 2-D
/// section coordinates use `frame.x` and `frame.y`.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct SectionPlane {
    pub frame: Frame,
}

impl SectionPlane {
    pub fn new(frame: Frame) -> SectionPlane {
        SectionPlane { frame }
    }

    /// The plane `z = height` of the world frame.
    pub fn horizontal(height: f64) -> SectionPlane {
        let mut frame = Frame::world_xy();
        frame.origin = Point3::new(0.0, 0.0, height);
        SectionPlane { frame }
    }

    pub fn normal(&self) -> UnitVec3 {
        self.frame.z
    }

    /// Signed distance of `p` from the plane (positive on the normal side).
    pub fn signed_distance(&self, p: Point3) -> f64 {
        (p - self.frame.origin).dot(self.frame.z.get())
    }
}

/// One piece of a section.
#[derive(Clone, PartialEq, Debug)]
pub struct SectionCurve {
    pub curve: Curve3,
    pub range: Interval,
    /// The piece is a closed loop (start = end).
    pub closed: bool,
    /// The plane touches the surface along this curve instead of crossing it
    /// (e.g. a plane tangent to a cylinder). Handled per CH-03.
    pub tangent: bool,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct SurfaceSection {
    pub curves: Vec<SectionCurve>,
    /// The surface lies in the plane within `tol.model` (planar face at the
    /// section height); reported instead of sectioned (D03 §5.1, CH-03).
    pub coincident: bool,
    /// Isolated tangential contacts (sphere touching the plane, cone apex).
    pub touch_points: Vec<Point3>,
}

/// Section of `s`, restricted to the parameter box `uv`, by `plane`.
pub fn plane_section(s: &Surface, uv: UvBox, plane: &SectionPlane, tol: &Tol) -> SurfaceSection {
    let analytic = match s {
        Surface::Plane { frame } => Some(plane_plane(frame, uv, plane, tol)),
        Surface::Cylinder { frame, r } => Some(cylinder(s, frame, *r, uv, plane, tol)),
        Surface::Sphere { frame, r } => Some(sphere(s, frame, *r, uv, plane, tol)),
        Surface::Cone {
            frame,
            r,
            semi_angle,
        } => cone(frame, *r, *semi_angle, uv, plane, tol),
        Surface::Torus {
            frame,
            major,
            minor,
        } => torus(s, frame, *major, *minor, uv, plane, tol),
        _ => None,
    };
    analytic.unwrap_or_else(|| march::march_section(s, uv, plane, tol))
}

/// Normal-to-axis test: `|n·z| ≥ 1 − angle`.
fn perpendicular_to_axis(n: Vec3, z: Vec3, tol: &Tol) -> bool {
    n.dot(z).abs() >= 1.0 - tol.angle
}

/// Contains-axis-direction test: `|n·z| ≤ angle`.
fn parallel_to_axis(n: Vec3, z: Vec3, tol: &Tol) -> bool {
    n.dot(z).abs() <= tol.angle
}

fn plane_plane(frame: &Frame, uv: UvBox, plane: &SectionPlane, tol: &Tol) -> SurfaceSection {
    let n = plane.normal().get();
    let d0 = plane.signed_distance(frame.origin);
    let (a, b) = (n.dot(frame.x.get()), n.dot(frame.y().get()));
    let ab2 = a * a + b * b;
    if ab2 <= tol.angle * tol.angle {
        // Parallel planes.
        return SurfaceSection {
            coincident: d0.abs() <= tol.model,
            ..SurfaceSection::default()
        };
    }
    // In (u, v): d0 + a·u + b·v = 0. Unit-speed direction (−b, a)/√(a²+b²).
    let k = ab2.sqrt();
    let (u0, v0) = (-d0 * a / ab2, -d0 * b / ab2);
    let (du, dv) = (-b / k, a / k);
    let Some(range) = liang_barsky(u0, v0, du, dv, uv) else {
        return SurfaceSection::default();
    };
    let origin = frame.origin + frame.x.get() * u0 + frame.y().get() * v0;
    let dir = frame.x.get() * du + frame.y().get() * dv;
    let curve = Curve3::line(origin, dir.normalized().unwrap_or(UnitVec3::X));
    SurfaceSection {
        curves: vec![SectionCurve {
            curve,
            range,
            closed: false,
            tangent: false,
        }],
        ..SurfaceSection::default()
    }
}

/// Parameter interval of the line `(u0 + t·du, v0 + t·dv)` inside `uv`.
fn liang_barsky(u0: f64, v0: f64, du: f64, dv: f64, uv: UvBox) -> Option<Interval> {
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for (p0, d, iv) in [(u0, du, uv.u), (v0, dv, uv.v)] {
        if d == 0.0 {
            if !iv.contains(p0) {
                return None;
            }
        } else {
            let (t1, t2) = ((iv.lo - p0) / d, (iv.hi - p0) / d);
            lo = lo.max(t1.min(t2));
            hi = hi.min(t1.max(t2));
        }
    }
    (lo < hi).then_some(Interval { lo, hi })
}

/// `(cos u·x + sin u·y)` of a frame.
fn radial(frame: &Frame, u: f64) -> Vec3 {
    let (s, c) = libm::sincos(u);
    frame.x.get() * c + frame.y().get() * s
}

/// Angle `φ` of the plane normal's component perpendicular to the frame axis,
/// and that component's length.
fn normal_angle(frame: &Frame, n: Vec3) -> (f64, f64) {
    let (nx, ny) = (n.dot(frame.x.get()), n.dot(frame.y().get()));
    (libm::atan2(ny, nx), libm::hypot(nx, ny))
}

/// Wraps `u` into `iv` if an equivalent angle lies inside it.
fn wrap_into(u: f64, iv: Interval) -> Option<f64> {
    let t = iv.lo + (u - iv.lo).rem_euclid(TAU);
    (t <= iv.hi).then_some(t)
}

/// A circle about the frame axis at height `w` along it, with parameter equal to
/// the surface's `u`; range is the box's u range.
fn axial_circle(frame: &Frame, w: f64, radius: f64, uv: UvBox, tangent: bool) -> SectionCurve {
    let f = Frame {
        origin: frame.origin + frame.z.get() * w,
        ..*frame
    };
    SectionCurve {
        curve: Curve3::Circle { frame: f, radius },
        range: uv.u,
        closed: uv.u.length() >= TAU,
        tangent,
    }
}

fn cylinder(
    s: &Surface,
    frame: &Frame,
    r: f64,
    uv: UvBox,
    plane: &SectionPlane,
    tol: &Tol,
) -> SurfaceSection {
    let n = plane.normal().get();
    let z = frame.z.get();
    let mut out = SurfaceSection::default();
    if perpendicular_to_axis(n, z, tol) {
        let w = -plane.signed_distance(frame.origin) / n.dot(z);
        if uv.v.contains(w) {
            out.curves.push(axial_circle(frame, w, r, uv, false));
        }
        return out;
    }
    let d0 = plane.signed_distance(frame.origin);
    let (phi, np) = normal_angle(frame, n);
    if parallel_to_axis(n, z, tol) {
        // Lines along the axis where r·cos(u − φ)·|n⊥| = −d0.
        let k = -d0 / (r * np);
        let gap = (k.abs() - 1.0) * r;
        if gap > tol.model {
            return out;
        }
        let tangent = gap.abs() <= tol.model;
        let us: Vec<f64> = if tangent {
            vec![if k > 0.0 {
                phi
            } else {
                phi + std::f64::consts::PI
            }]
        } else {
            let a = libm::acos(k);
            vec![phi + a, phi - a]
        };
        for u in us {
            if let Some(u) = wrap_into(u, uv.u) {
                out.curves.push(SectionCurve {
                    curve: Curve3::line(frame.origin + radial(frame, u) * r, frame.z),
                    range: uv.v,
                    closed: false,
                    tangent,
                });
            }
        }
        return out;
    }
    // Oblique: an ellipse. With θ = u − φ, m = radial(φ), m′ = z × m:
    // P(θ) = C + r·cosθ·(m − (|n⊥|/c)·z) + r·sinθ·m′, C on the axis.
    let c = n.dot(z);
    let m = radial(frame, phi);
    let m1 = z.cross(m);
    let centre = frame.origin + z * (-d0 / c);
    let major = m - z * (np / c);
    let a = r * major.length();
    let Some(ef) = Frame::new(centre, major.cross(m1), major) else {
        return out;
    };
    let curve = Curve3::Ellipse { frame: ef, a, b: r };
    // Ellipse parameter θ maps to u = θ + φ; clip numerically in v.
    let start = uv.u.lo - phi;
    let range = Interval {
        lo: start,
        hi: start + uv.u.length().min(TAU),
    };
    out.curves = clip_to_box(s, uv, curve, range, false, tol);
    out
}

fn sphere(
    s: &Surface,
    frame: &Frame,
    r: f64,
    uv: UvBox,
    plane: &SectionPlane,
    tol: &Tol,
) -> SurfaceSection {
    let n = plane.normal();
    let d = plane.signed_distance(frame.origin);
    let mut out = SurfaceSection::default();
    if d.abs() > r + tol.model {
        return out;
    }
    let foot = frame.origin - n.get() * d;
    if (d.abs() - r).abs() <= tol.model {
        if inside_box(s, uv, foot, tol) {
            out.touch_points.push(foot);
        }
        return out;
    }
    let radius = (r * r - d * d).sqrt();
    // Reference direction: the sphere's x projected into the plane, or y.
    let x_ref = if frame.x.get().cross(n.get()).length() > 1e-3 {
        frame.x.get()
    } else {
        frame.y().get()
    };
    let Some(cf) = Frame::new(foot, n.get(), x_ref) else {
        return out;
    };
    let curve = Curve3::Circle { frame: cf, radius };
    out.curves = clip_to_box(s, uv, curve, Interval { lo: 0.0, hi: TAU }, false, tol);
    out
}

fn cone(
    frame: &Frame,
    r: f64,
    semi_angle: f64,
    uv: UvBox,
    plane: &SectionPlane,
    tol: &Tol,
) -> Option<SurfaceSection> {
    let n = plane.normal().get();
    let z = frame.z.get();
    let t = libm::tan(semi_angle);
    let mut out = SurfaceSection::default();
    if perpendicular_to_axis(n, z, tol) {
        let w = -plane.signed_distance(frame.origin) / n.dot(z);
        if !uv.v.contains(w) {
            return Some(out);
        }
        let rho = r + w * t;
        if rho <= tol.model {
            if rho.abs() <= tol.model {
                out.touch_points.push(frame.origin + z * w);
            }
            return Some(out);
        }
        out.curves.push(axial_circle(frame, w, rho, uv, false));
        return Some(out);
    }
    let d0 = plane.signed_distance(frame.origin);
    if parallel_to_axis(n, z, tol) && d0.abs() <= tol.model {
        // Plane through the axis: two generatrices, at u = φ ± π/2.
        let (phi, _) = normal_angle(frame, n);
        let k = (1.0 + t * t).sqrt();
        for u in [phi + FRAC_PI_2, phi - FRAC_PI_2] {
            if let Some(u) = wrap_into(u, uv.u) {
                let e = radial(frame, u);
                let dir = (e * t + z).normalized()?;
                out.curves.push(SectionCurve {
                    curve: Curve3::line(frame.origin + e * r, dir),
                    // The line parameter is arc length: t_line = v·√(1 + tan²α).
                    range: Interval {
                        lo: uv.v.lo * k,
                        hi: uv.v.hi * k,
                    },
                    closed: false,
                    tangent: false,
                });
            }
        }
        return Some(out);
    }
    // Conics (ellipse, parabola, hyperbola) are marched for now; exact
    // rational forms per D02 §5 come with the S1 spike if marching is too coarse.
    None
}

fn torus(
    s: &Surface,
    frame: &Frame,
    major: f64,
    minor: f64,
    uv: UvBox,
    plane: &SectionPlane,
    tol: &Tol,
) -> Option<SurfaceSection> {
    let n = plane.normal().get();
    let z = frame.z.get();
    let mut out = SurfaceSection::default();
    if perpendicular_to_axis(n, z, tol) {
        let w = -plane.signed_distance(frame.origin) / n.dot(z);
        let gap = w.abs() - minor;
        if gap > tol.model {
            return Some(out);
        }
        let tangent = gap.abs() <= tol.model;
        let v1 = libm::asin((w / minor).clamp(-1.0, 1.0));
        let vs = if tangent {
            vec![v1]
        } else {
            vec![v1, std::f64::consts::PI - v1]
        };
        for v in vs {
            if wrap_into(v, uv.v).is_some() {
                let radius = major + minor * libm::cos(v);
                if radius > tol.model {
                    out.curves.push(axial_circle(frame, w, radius, uv, tangent));
                }
            }
        }
        return Some(out);
    }
    if parallel_to_axis(n, z, tol) && plane.signed_distance(frame.origin).abs() <= tol.model {
        // Plane through the axis: two meridian circles at u = φ ± π/2, with
        // parameter equal to the surface's v.
        let (phi, _) = normal_angle(frame, n);
        for u in [phi + FRAC_PI_2, phi - FRAC_PI_2] {
            if let Some(u) = wrap_into(u, uv.u) {
                let e = radial(frame, u);
                let cf = Frame::new(frame.origin + e * major, e.cross(z), e)?;
                let curve = Curve3::Circle {
                    frame: cf,
                    radius: minor,
                };
                let range = Interval {
                    lo: uv.v.lo,
                    hi: uv.v.lo + uv.v.length().min(TAU),
                };
                out.curves
                    .extend(clip_to_box(s, uv, curve, range, false, tol));
            }
        }
        return Some(out);
    }
    None
}

/// Whether surface point `p` has parameters inside `uv` (u wrapped).
fn inside_box(s: &Surface, uv: UvBox, p: Point3, tol: &Tol) -> bool {
    let (_, v, u_out) = closest_unconstrained(s, p, uv);
    let slack = tol.solver;
    !u_out && v >= uv.v.lo - slack && v <= uv.v.hi + slack
}

/// Samples per curve piece when clipping a section curve to the box.
const CLIP_SAMPLES: usize = 64;

/// Splits `curve` over `range` into the sub-ranges whose surface parameters lie
/// inside `uv`. Box crossings are found between samples and refined by
/// bisection to `tol.solver`.
fn clip_to_box(
    s: &Surface,
    uv: UvBox,
    curve: Curve3,
    range: Interval,
    tangent: bool,
    tol: &Tol,
) -> Vec<SectionCurve> {
    let inside = |t: f64| inside_box(s, uv, curve.eval(t), tol);
    let n = CLIP_SAMPLES;
    let ts: Vec<f64> = (0..=n).map(|i| range.at(i as f64 / n as f64)).collect();
    let flags: Vec<bool> = ts.iter().map(|&t| inside(t)).collect();
    let refine = |mut a: f64, mut b: f64, a_in: bool| {
        // Bisect the transition between a (state a_in) and b.
        for _ in 0..60 {
            let m = 0.5 * (a + b);
            if inside(m) == a_in {
                a = m;
            } else {
                b = m;
            }
            if (b - a).abs() < tol.solver {
                break;
            }
        }
        if a_in { a } else { b }
    };
    let mut pieces = Vec::new();
    let mut start = flags[0].then_some(range.lo);
    for i in 1..=n {
        match (flags[i - 1], flags[i]) {
            (false, true) => start = Some(refine(ts[i - 1], ts[i], false)),
            (true, false) => {
                if let Some(lo) = start.take() {
                    pieces.push(Interval {
                        lo,
                        hi: refine(ts[i - 1], ts[i], true),
                    });
                }
            }
            _ => {}
        }
    }
    if let Some(lo) = start {
        pieces.push(Interval { lo, hi: range.hi });
    }
    let full = pieces.len() == 1 && pieces[0] == range && range.length() >= TAU;
    // A closed curve cut by the box may start and end inside: join the
    // first and last pieces across the range end.
    if !full
        && pieces.len() >= 2
        && range.length() >= TAU
        && pieces[0].lo == range.lo
        && pieces[pieces.len() - 1].hi == range.hi
    {
        let first = pieces.remove(0);
        if let Some(last) = pieces.last_mut() {
            last.hi = first.hi + range.length();
        }
    }
    pieces
        .into_iter()
        .map(|r| SectionCurve {
            curve: curve.clone(),
            range: r,
            closed: full,
            tangent,
        })
        .collect()
}
