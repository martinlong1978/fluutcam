// SPDX-License-Identifier: MIT OR Apache-2.0

//! Closed-form evaluation and closest point for analytic surfaces (D02 §4.2).

use super::{SurfDerivs, Surface, UvBox};
use crate::bounds::Interval;
use crate::types::{Point3, Vec3};
use std::f64::consts::TAU;

pub(super) fn derivs(s: &Surface, u: f64, v: f64) -> SurfDerivs {
    match s {
        Surface::BSpline(b) => b.derivs(u, v),
        Surface::Extrusion { .. } | Surface::Revolution { .. } | Surface::Offset { .. } => {
            super::swept::derivs(s, u, v)
        }
        Surface::Plane { frame } => SurfDerivs {
            p: frame.origin + frame.x.get() * u + frame.y().get() * v,
            su: frame.x.get(),
            sv: frame.y().get(),
            suu: Vec3::ZERO,
            suv: Vec3::ZERO,
            svv: Vec3::ZERO,
        },
        Surface::Cylinder { frame, r } => {
            let (e, e1) = radial_dirs(frame, u);
            let z = frame.z.get();
            SurfDerivs {
                p: frame.origin + e * *r + z * v,
                su: e1 * *r,
                sv: z,
                suu: -e * *r,
                suv: Vec3::ZERO,
                svv: Vec3::ZERO,
            }
        }
        Surface::Cone {
            frame,
            r,
            semi_angle,
        } => {
            let (e, e1) = radial_dirs(frame, u);
            let z = frame.z.get();
            let t = libm::tan(*semi_angle);
            let rho = r + v * t;
            SurfDerivs {
                p: frame.origin + e * rho + z * v,
                su: e1 * rho,
                sv: e * t + z,
                suu: -e * rho,
                suv: e1 * t,
                svv: Vec3::ZERO,
            }
        }
        Surface::Sphere { frame, r } => {
            let (e, e1) = radial_dirs(frame, u);
            let z = frame.z.get();
            let (sv, cv) = libm::sincos(v);
            SurfDerivs {
                p: frame.origin + (e * cv + z * sv) * *r,
                su: e1 * (r * cv),
                sv: (z * cv - e * sv) * *r,
                suu: -e * (r * cv),
                suv: -e1 * (r * sv),
                svv: -(e * cv + z * sv) * *r,
            }
        }
        Surface::Torus {
            frame,
            major,
            minor,
        } => {
            let (e, e1) = radial_dirs(frame, u);
            let z = frame.z.get();
            let (sv, cv) = libm::sincos(v);
            let rho = major + minor * cv;
            SurfDerivs {
                p: frame.origin + e * rho + z * (minor * sv),
                su: e1 * rho,
                sv: (z * cv - e * sv) * *minor,
                suu: -e * rho,
                suv: -e1 * (minor * sv),
                svv: -(e * cv + z * sv) * *minor,
            }
        }
    }
}

/// `(cos u·x + sin u·y, −sin u·x + cos u·y)`.
fn radial_dirs(frame: &crate::frame::Frame, u: f64) -> (Vec3, Vec3) {
    let (s, c) = libm::sincos(u);
    let (x, y) = (frame.x.get(), frame.y().get());
    (x * c + y * s, y * c - x * s)
}

/// Wraps angle `theta` into `range`. When no equivalent angle lies in the range,
/// returns the angularly nearer end and `true`.
fn wrap_angle(theta: f64, range: Interval) -> (f64, bool) {
    let t = range.lo + (theta - range.lo).rem_euclid(TAU);
    if t <= range.hi {
        (t, false)
    } else {
        let past_hi = t - range.hi;
        let before_lo = range.lo + TAU - t;
        (
            if past_hi <= before_lo {
                range.hi
            } else {
                range.lo
            },
            true,
        )
    }
}

/// Closest parameters ignoring the v limits: `(u, v, u_clamped)`. `u` is already
/// wrapped into `uv.u`; `u_clamped` says it had to be moved to an end.
pub(super) fn closest_unconstrained(s: &Surface, p: Point3, uv: UvBox) -> (f64, f64, bool) {
    let angle = |lx: f64, ly: f64| {
        if lx == 0.0 && ly == 0.0 {
            // On the axis every u is equally close.
            (uv.u.lo, false)
        } else {
            wrap_angle(libm::atan2(ly, lx), uv.u)
        }
    };
    match s {
        // Not closed form; `Surface::closest` handles B-splines separately.
        Surface::BSpline(_)
        | Surface::Extrusion { .. }
        | Surface::Revolution { .. }
        | Surface::Offset { .. } => {
            let (u, v) = uv.centre();
            (u, v, true)
        }
        Surface::Plane { frame } => {
            let l = frame.to_local(p);
            (l.x(), l.y(), false)
        }
        Surface::Cylinder { frame, .. } => {
            let l = frame.to_local(p);
            let (u, c) = angle(l.x(), l.y());
            (u, l.z(), c)
        }
        Surface::Cone {
            frame,
            r,
            semi_angle,
        } => {
            let l = frame.to_local(p);
            let (u, c) = angle(l.x(), l.y());
            let t = libm::tan(*semi_angle);
            // Project (radial, z) onto the generatrix (r + v·t, v).
            let radial = libm::hypot(l.x(), l.y());
            let v = ((radial - r) * t + l.z()) / (1.0 + t * t);
            // Do not pass the apex (ρ = 0 at v = −r/t).
            (u, v.max(-r / t), c)
        }
        Surface::Sphere { frame, .. } => {
            let l = frame.to_local(p);
            let (u, c) = angle(l.x(), l.y());
            let radial = libm::hypot(l.x(), l.y());
            let v = if radial == 0.0 && l.z() == 0.0 {
                0.0
            } else {
                libm::atan2(l.z(), radial)
            };
            (u, v, c)
        }
        Surface::Torus { frame, major, .. } => {
            let l = frame.to_local(p);
            let (u, c) = angle(l.x(), l.y());
            let dr = libm::hypot(l.x(), l.y()) - major;
            let (v, cv) = if dr == 0.0 && l.z() == 0.0 {
                (uv.v.lo, false)
            } else {
                wrap_angle(libm::atan2(l.z(), dr), uv.v)
            };
            (u, v, c || cv)
        }
    }
}
