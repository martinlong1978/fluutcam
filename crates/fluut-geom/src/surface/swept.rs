// SPDX-License-Identifier: MIT OR Apache-2.0

//! Extrusion, revolution and offset surfaces (D02 §4.1, §4.3).

use super::{SurfDerivs, Surface};
use crate::curve::Curve3;
use crate::types::Vec3;

pub(super) fn derivs(s: &Surface, u: f64, v: f64) -> SurfDerivs {
    match s {
        Surface::Extrusion { profile, dir } => {
            let c = profile.derivs(u, 2);
            SurfDerivs {
                p: crate::types::Point3((c[0] + dir.get() * v).0),
                su: c[1],
                sv: dir.get(),
                suu: c[2],
                suv: Vec3::ZERO,
                svv: Vec3::ZERO,
            }
        }
        Surface::Revolution {
            profile,
            axis_origin,
            axis,
        } => {
            let a = axis.get();
            let c = profile.derivs(v, 2);
            let par = |x: Vec3| a * a.dot(x);
            let perp = |x: Vec3| x - par(x);
            let (su, cu) = libm::sincos(u);
            // Rotation by u about a of a vector x: par + cos·perp + sin·(a × x).
            let rot = |x: Vec3| par(x) + perp(x) * cu + a.cross(x) * su;
            // Derivative of the rotation with respect to u.
            let rot_u = |x: Vec3| perp(x) * -su + a.cross(x) * cu;
            let w = c[0] - axis_origin.to_vec();
            SurfDerivs {
                p: *axis_origin + rot(w),
                su: rot_u(w),
                sv: rot(c[1]),
                suu: -(perp(w) * cu + a.cross(w) * su),
                suv: rot_u(c[1]),
                svv: rot(c[2]),
            }
        }
        Surface::Offset { basis, dist } => {
            let first = |u: f64, v: f64| offset_first(basis, *dist, u, v);
            let (p, su0, sv0) = first(u, v);
            let hu = 1e-5 * (1.0 + u.abs());
            let hv = 1e-5 * (1.0 + v.abs());
            let (_, su_up, sv_up) = first(u + hu, v);
            let (_, su_um, sv_um) = first(u - hu, v);
            let (_, su_vp, sv_vp) = first(u, v + hv);
            let (_, su_vm, sv_vm) = first(u, v - hv);
            // Average both difference directions for the mixed term.
            let suv = ((su_vp - su_vm) / (2.0 * hv) + (sv_up - sv_um) / (2.0 * hu)) * 0.5;
            SurfDerivs {
                p,
                su: su0,
                sv: sv0,
                suu: (su_up - su_um) / (2.0 * hu),
                suv,
                svv: (sv_vp - sv_vm) / (2.0 * hv),
            }
        }
        // Not swept kinds; `Surface::derivs` dispatches them elsewhere.
        _ => super::analytic::derivs(s, u, v),
    }
}

/// Point and exact first derivatives of `B + dist·N`, using the derivative of
/// the unit normal: `N_u = (n_u − N(N·n_u)) / |n|`, `n = B_u × B_v`.
fn offset_first(basis: &Surface, dist: f64, u: f64, v: f64) -> (crate::types::Point3, Vec3, Vec3) {
    let b = basis.derivs(u, v);
    let n = b.su.cross(b.sv);
    let len = n.length();
    if len == 0.0 {
        // Singular basis point: offset direction undefined; stay on the basis.
        return (b.p, b.su, b.sv);
    }
    let nn = n / len;
    let n_u = b.suu.cross(b.sv) + b.su.cross(b.suv);
    let n_v = b.suv.cross(b.sv) + b.su.cross(b.svv);
    let unit_u = (n_u - nn * nn.dot(n_u)) / len;
    let unit_v = (n_v - nn * nn.dot(n_v)) / len;
    (b.p + nn * dist, b.su + unit_u * dist, b.sv + unit_v * dist)
}

/// Period of a curve's parameter, if it is closed and periodic.
pub(super) fn curve_period(c: &Curve3) -> Option<f64> {
    match c {
        Curve3::Circle { .. } | Curve3::Ellipse { .. } => Some(std::f64::consts::TAU),
        Curve3::BSpline(b) => b.is_periodic().then(|| b.domain().length()),
        Curve3::Offset { basis, .. } => curve_period(basis),
        Curve3::Line { .. } => None,
    }
}
