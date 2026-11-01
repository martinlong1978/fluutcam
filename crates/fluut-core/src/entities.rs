// SPDX-License-Identifier: MIT OR Apache-2.0

//! Read-only descriptions of the loaded bodies and faces (spec 03 §3.2),
//! for scripts (D12b §6) and the UI: kind, surface parameters and bounds
//! in the model frame M, mm.

use crate::reimport::face_bounds;
use crate::store::ModelStore;
use fluut_cam::geometry::{BodyGeom, face_id};
use fluut_geom::{Point3, Surface, Vec3};
use fluut_topo::BrepBody;
use std::collections::BTreeMap;

/// Direction cosines within this of ±Z count as vertical.
pub const VERTICAL_TOL: f64 = 1e-9;

/// A surface parameter.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Param {
    Num(f64),
    Vec([f64; 3]),
    Bool(bool),
}

/// Axis-aligned bounds, `[min x, min y, min z, max x, max y, max z]`.
pub type Bounds = [f64; 6];

#[derive(Clone, PartialEq, Debug)]
pub struct FaceInfo {
    pub id: String,
    /// `plane`, `cylinder`, `cone`, `sphere`, `torus`, `bspline`,
    /// `extrusion`, `revolution` or `offset`.
    pub kind: &'static str,
    pub params: BTreeMap<&'static str, Param>,
    pub bounds: Bounds,
}

#[derive(Clone, PartialEq, Debug)]
pub struct BodyInfo {
    pub id: String,
    pub name: String,
    pub source: String,
    /// `solid` (B-rep) or `wire` (2-D curves).
    pub kind: &'static str,
    pub faces: Vec<FaceInfo>,
    pub bounds: Bounds,
}

/// Every body of every source, sources in id order.
pub fn bodies(store: &ModelStore) -> Vec<BodyInfo> {
    let mut out = Vec::new();
    for (source, src) in store.sources() {
        for b in &src.bodies {
            out.push(match &b.geom {
                BodyGeom::Brep(x) => {
                    let faces: Vec<FaceInfo> =
                        (0..x.faces.len()).map(|f| face_info(&b.id, x, f)).collect();
                    BodyInfo {
                        id: b.id.clone(),
                        name: x.name.clone(),
                        source: source.to_owned(),
                        kind: "solid",
                        bounds: union(faces.iter().map(|f| f.bounds)),
                        faces,
                    }
                }
                BodyGeom::Wire(w) => BodyInfo {
                    id: b.id.clone(),
                    name: w.name.clone(),
                    source: source.to_owned(),
                    kind: "wire",
                    faces: Vec::new(),
                    bounds: crate::reimport::wire_bounds(w),
                },
            });
        }
    }
    out
}

fn union(it: impl Iterator<Item = Bounds>) -> Bounds {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for x in it {
        for i in 0..3 {
            b[i] = b[i].min(x[i]);
            b[i + 3] = b[i + 3].max(x[i + 3]);
        }
    }
    b
}

fn v(p: Vec3) -> [f64; 3] {
    [p.x(), p.y(), p.z()]
}

fn pt(p: Point3) -> [f64; 3] {
    [p.x(), p.y(), p.z()]
}

fn vertical(d: [f64; 3]) -> bool {
    (d[2].abs() - 1.0).abs() <= VERTICAL_TOL
}

fn face_info(body: &str, b: &BrepBody, f: usize) -> FaceInfo {
    use Param::{Bool, Num, Vec as V};
    let face = &b.faces[f];
    // A face whose normal is reversed against a closed surface's outward
    // normal faces its axis or centre: a hole.
    let inside = !face.same_sense;
    let mut p = BTreeMap::new();
    let kind = match &b.surfaces[face.surface as usize] {
        Surface::Plane { frame } => {
            let s = if face.same_sense { 1.0 } else { -1.0 };
            let n = v(frame.z.get()).map(|x| x * s);
            let o = pt(frame.origin);
            p.insert("normal", V(n));
            p.insert("origin", V(o));
            p.insert("offset", Num(n[0] * o[0] + n[1] * o[1] + n[2] * o[2]));
            p.insert("normal_is_vertical", Bool(vertical(n)));
            "plane"
        }
        Surface::Cylinder { frame, r } => {
            let a = v(frame.z.get());
            p.insert("axis", V(a));
            p.insert("origin", V(pt(frame.origin)));
            p.insert("radius", Num(*r));
            p.insert("diameter", Num(2.0 * r));
            p.insert("axis_is_vertical", Bool(vertical(a)));
            p.insert("inside", Bool(inside));
            "cylinder"
        }
        Surface::Cone {
            frame,
            r,
            semi_angle,
        } => {
            let a = v(frame.z.get());
            p.insert("axis", V(a));
            p.insert("origin", V(pt(frame.origin)));
            p.insert("radius", Num(*r));
            p.insert("half_angle_deg", Num(semi_angle.to_degrees()));
            p.insert("axis_is_vertical", Bool(vertical(a)));
            p.insert("inside", Bool(inside));
            "cone"
        }
        Surface::Sphere { frame, r } => {
            p.insert("centre", V(pt(frame.origin)));
            p.insert("radius", Num(*r));
            p.insert("inside", Bool(inside));
            "sphere"
        }
        Surface::Torus {
            frame,
            major,
            minor,
        } => {
            let a = v(frame.z.get());
            p.insert("axis", V(a));
            p.insert("centre", V(pt(frame.origin)));
            p.insert("major_radius", Num(*major));
            p.insert("minor_radius", Num(*minor));
            p.insert("axis_is_vertical", Bool(vertical(a)));
            p.insert("inside", Bool(inside));
            "torus"
        }
        Surface::BSpline(_) => "bspline",
        Surface::Extrusion { .. } => "extrusion",
        Surface::Revolution { .. } => "revolution",
        Surface::Offset { .. } => "offset",
    };
    FaceInfo {
        id: face_id(body, f as u32),
        kind,
        params: p,
        bounds: face_bounds(b, f),
    }
}
