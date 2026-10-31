// SPDX-License-Identifier: MIT OR Apache-2.0

//! Arc geometry for output: principal planes, splitting and centre
//! correction (spec 05 §5.8 rule 3, D10 §4.4).

use crate::format::round_dec;
use fluut_geom::{Point3, UnitVec3, Vec3};
use fluut_path::AxisPos;

/// Normal within this of a principal axis selects that plane.
const PLANE_TOL: f64 = 1e-9;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Plane {
    /// G17, seen from +Z.
    XY,
    /// G18, seen from +Y.
    XZ,
    /// G19, seen from +X.
    YZ,
}

impl Plane {
    /// Indices of the two in-plane axes (in I/J/K order) and the plane axis.
    pub fn axes(self) -> (usize, usize, usize) {
        match self {
            Plane::XY => (0, 1, 2),
            Plane::XZ => (0, 2, 1),
            Plane::YZ => (1, 2, 0),
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Plane::XY => "G17",
            Plane::XZ => "G18",
            Plane::YZ => "G19",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Plane::XY => "XY",
            Plane::XZ => "XZ",
            Plane::YZ => "YZ",
        }
    }
}

/// The principal plane whose axis is parallel to `n`, and whether the sweep
/// is counter-clockwise seen from that axis's positive end for a positive
/// sweep about `n`.
pub fn principal_plane(n: UnitVec3) -> Option<(Plane, bool)> {
    let v = n.get();
    let c = [v.x(), v.y(), v.z()];
    [(2, Plane::XY), (1, Plane::XZ), (0, Plane::YZ)]
        .into_iter()
        .find(|&(k, _)| (c[k].abs() - 1.0).abs() <= PLANE_TOL)
        .map(|(k, p)| (p, c[k] > 0.0))
}

pub fn coords(p: Point3) -> [f64; 3] {
    [p.x(), p.y(), p.z()]
}

/// Splits an arc or helix into `n` equal parts. Endpoints are shared
/// bitwise; the first `a` and last `b` are the originals.
pub fn split(
    a: &AxisPos,
    b: &AxisPos,
    centre: Point3,
    normal: UnitVec3,
    sweep: f64,
    n: usize,
) -> Vec<(AxisPos, AxisPos, f64)> {
    let nv = normal.get();
    let rel = a.xyz - centre;
    let rise = (b.xyz - a.xyz).dot(nv);
    let point = |k: usize| -> AxisPos {
        if k == 0 {
            return *a;
        }
        if k == n {
            return *b;
        }
        let f = k as f64 / n as f64;
        let t = sweep * f;
        let (s, c) = (libm::sin(t), libm::cos(t));
        let along = nv * rel.dot(nv);
        let rot = rel * c + nv.cross(rel) * s + along * (1.0 - c);
        AxisPos {
            xyz: centre + rot + nv * (rise * f),
            a: a.a,
            b: a.b,
        }
    };
    let pts: Vec<AxisPos> = (0..=n).map(point).collect();
    pts.windows(2)
        .map(|w| (w[0], w[1], sweep / n as f64))
        .collect()
}

/// Centre after rounding (D10 §4.4): the point on the perpendicular bisector
/// of the rounded start `s` and end `e` nearest the exact centre `c`, rounded
/// to `d` decimals. With `s == e` (full circle) it is `c` rounded.
pub fn corrected_centre(s: [f64; 2], e: [f64; 2], c: [f64; 2], d: u32) -> [f64; 2] {
    let ch = [e[0] - s[0], e[1] - s[1]];
    let len = libm::hypot(ch[0], ch[1]);
    let c1 = if len == 0.0 {
        c
    } else {
        let m = [(s[0] + e[0]) / 2.0, (s[1] + e[1]) / 2.0];
        let u = [-ch[1] / len, ch[0] / len];
        let t = (c[0] - m[0]) * u[0] + (c[1] - m[1]) * u[1];
        [m[0] + t * u[0], m[1] + t * u[1]]
    };
    [round_dec(c1[0], d), round_dec(c1[1], d)]
}

/// The controller's radius check: | |c−s| − |c−e| |.
pub fn radius_error(s: [f64; 2], e: [f64; 2], c: [f64; 2]) -> f64 {
    let rs = libm::hypot(c[0] - s[0], c[1] - s[1]);
    let re = libm::hypot(c[0] - e[0], c[1] - e[1]);
    (rs - re).abs()
}

/// In-plane radius of an arc or helix start.
pub fn radius(a: &AxisPos, centre: Point3, normal: UnitVec3) -> f64 {
    let rel: Vec3 = a.xyz - centre;
    let n = normal.get();
    (rel - n * rel.dot(n)).length()
}
