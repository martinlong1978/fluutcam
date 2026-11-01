// SPDX-License-Identifier: MIT OR Apache-2.0

//! The bodies of a Setup in its work frame W, with entity ids (D08 §1
//! `SetupGeometry`, spec 03 §3.5).
//!
//! Ids: a body `b-<stem>` has faces `f-<stem>.<i>` and edges (or wire
//! curves) `e-<stem>.<i>`, where `i` is the index in the body. Matching ids
//! across re-import is M4.

use crate::cutter::geom::curve_box;
use fluut_geom::{Aabb3, Affine, Curve3, Interval, Point3};
use fluut_topo::{BrepBody, WireBody, WireCurve};

#[derive(Debug, Clone)]
pub enum BodyGeom {
    Brep(BrepBody),
    Wire(WireBody),
}

#[derive(Debug, Clone)]
pub struct SetupBody {
    pub id: String,
    pub geom: BodyGeom,
}

/// An edge or wire curve found by id.
#[derive(Debug, Clone, Copy)]
pub enum EdgeRef<'a> {
    Brep(&'a BrepBody, u32),
    Wire(&'a WireCurve),
}

#[derive(Debug, Clone, Default)]
pub struct SetupGeometry {
    pub bodies: Vec<SetupBody>,
}

fn stem(body_id: &str) -> &str {
    body_id.strip_prefix("b-").unwrap_or(body_id)
}

/// Id of face `i` of body `body_id`.
pub fn face_id(body_id: &str, i: u32) -> String {
    format!("f-{}.{i}", stem(body_id))
}

/// Id of edge (or wire curve) `i` of body `body_id`.
pub fn edge_id(body_id: &str, i: u32) -> String {
    format!("e-{}.{i}", stem(body_id))
}

/// `(body id, index)` of a face or edge id with the given prefix.
fn split_id(id: &str, prefix: &str) -> Option<(String, u32)> {
    let rest = id.strip_prefix(prefix)?;
    let (s, i) = rest.rsplit_once('.')?;
    Some((format!("b-{s}"), i.parse().ok()?))
}

impl SetupGeometry {
    /// Moves every body by `a` (model frame M → work frame W).
    pub fn transformed(bodies: &[SetupBody], a: &Affine) -> Result<SetupGeometry, String> {
        let bodies = bodies
            .iter()
            .map(|b| {
                let geom = match &b.geom {
                    BodyGeom::Brep(x) => BodyGeom::Brep(transform_brep(x, a).ok_or_else(|| {
                        format!("body {} cannot be moved into the work frame", b.id)
                    })?),
                    BodyGeom::Wire(w) => BodyGeom::Wire(w.transformed(a).ok_or_else(|| {
                        format!("body {} cannot be moved into the work frame", b.id)
                    })?),
                };
                Ok(SetupBody {
                    id: b.id.clone(),
                    geom,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(SetupGeometry { bodies })
    }

    pub fn body(&self, id: &str) -> Option<&SetupBody> {
        self.bodies.iter().find(|b| b.id == id)
    }

    pub fn face(&self, id: &str) -> Option<(&BrepBody, u32)> {
        let (b, i) = split_id(id, "f-")?;
        match &self.body(&b)?.geom {
            BodyGeom::Brep(x) if (i as usize) < x.faces.len() => Some((x, i)),
            _ => None,
        }
    }

    pub fn edge(&self, id: &str) -> Option<EdgeRef<'_>> {
        let (b, i) = split_id(id, "e-")?;
        match &self.body(&b)?.geom {
            BodyGeom::Brep(x) if (i as usize) < x.edges.len() => Some(EdgeRef::Brep(x, i)),
            BodyGeom::Wire(w) => w.curves.get(i as usize).map(EdgeRef::Wire),
            _ => None,
        }
    }

    /// Largest modelling tolerance of the bodies (at least 1e-6 mm).
    pub fn tol_model(&self) -> f64 {
        self.bodies
            .iter()
            .map(|b| match &b.geom {
                BodyGeom::Brep(x) => x.tol_model,
                BodyGeom::Wire(w) => w.tol_model,
            })
            .fold(1e-6, f64::max)
    }

    /// Bounds of every body, from vertices and edges: exact for lines,
    /// circles and ellipses, from samples `chord` apart for other curves.
    /// A face that bulges beyond its edges (a sphere cap) is not covered.
    pub fn bounds(&self, chord: f64) -> Aabb3 {
        let mut bb = Aabb3::empty();
        let mut add = |c: &Curve3, range: Interval| match c {
            Curve3::Line { .. } | Curve3::Circle { .. } | Curve3::Ellipse { .. } => {
                let b = curve_box(c, range);
                bb = bb.with_point(b.min).with_point(b.max);
            }
            _ => {
                for (_, p) in c.tessellate(range, chord) {
                    bb = bb.with_point(p);
                }
            }
        };
        for b in &self.bodies {
            match &b.geom {
                BodyGeom::Brep(x) => {
                    for e in &x.edges {
                        if let Some(c) = e.curve {
                            add(&x.curves[c as usize], e.range);
                        }
                    }
                }
                BodyGeom::Wire(w) => {
                    for c in &w.curves {
                        add(&c.curve, c.range);
                    }
                }
            }
        }
        for b in &self.bodies {
            if let BodyGeom::Brep(x) = &b.geom {
                for v in &x.vertices {
                    bb = bb.with_point(v.p);
                }
            }
        }
        bb
    }
}

/// The body moved by `a`. Faces flip sense when `a` mirrors.
fn transform_brep(b: &BrepBody, a: &Affine) -> Option<BrepBody> {
    let mut out = b.clone();
    let mut flips = Vec::with_capacity(b.surfaces.len());
    for (s, o) in b.surfaces.iter().zip(out.surfaces.iter_mut()) {
        *o = s.transformed(a)?;
        flips.push(s.transform_flips_normal(a));
    }
    for (c, o) in b.curves.iter().zip(out.curves.iter_mut()) {
        *o = c.transformed(a)?;
    }
    for v in &mut out.vertices {
        v.p = a.apply_point(v.p);
    }
    for f in &mut out.faces {
        if flips[f.surface as usize] {
            f.same_sense = !f.same_sense;
        }
    }
    Some(out)
}

/// Z range of a face, from its edges.
pub fn face_z_range(b: &BrepBody, f: u32, chord: f64) -> Interval {
    let bb = b.face_box(f, chord);
    Interval {
        lo: bb.min.z(),
        hi: bb.max.z(),
    }
}

/// Every point of an edge or wire curve, `chord` apart.
pub fn edge_points(e: EdgeRef<'_>, chord: f64) -> Vec<Point3> {
    match e {
        EdgeRef::Brep(b, i) => b.edge_points(i, chord),
        EdgeRef::Wire(c) => c
            .curve
            .tessellate(c.range, chord)
            .into_iter()
            .map(|(_, p)| p)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cutter::synth::Builder;
    use fluut_geom::{Frame, Surface, UvBox, Vec3};

    #[test]
    fn bounds_are_exact_on_arcs() {
        // A cylinder of radius 4 along X, over angles 0.3 … 2.8: its
        // top is at u = π/2, inside an arc edge, not at a vertex.
        let f = Frame::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        )
        .unwrap();
        let mut b = Builder::new("c");
        b.patch(
            Surface::cylinder(f, 4.0).unwrap(),
            UvBox::new(
                Interval { lo: 0.3, hi: 2.8 },
                Interval { lo: 0.0, hi: 10.0 },
            ),
        );
        let g = SetupGeometry {
            bodies: vec![SetupBody {
                id: "b-1".into(),
                geom: BodyGeom::Brep(b.build()),
            }],
        };
        let bb = g.bounds(1.0);
        assert_eq!(bb.max.z(), 4.0, "{bb:?}");
        assert_eq!(bb.max.x(), 10.0);
    }
}
