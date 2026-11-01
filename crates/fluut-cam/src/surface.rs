// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Setup's bodies as drop-query geometry for 3-D types (D07, D08 §4):
//! drive, check and excluded faces, the stock to leave as a grown cutter
//! profile, the slope at a contact, and a bound for links.

use crate::cutter::geom::{BuildOptions, QueryGeometry};
use crate::cutter::solve::{Contact, ContactOn, SolveStats};
use crate::cutter::{Cutter, DropOptions, drop};
use crate::geometry::{BodyGeom, SetupGeometry, face_id};
use fluut_geom::{Aabb3, Point2, Point3};
use std::cell::Cell;
use std::collections::BTreeSet;
use std::f64::consts::FRAC_PI_2;

/// One body's query geometry and which of its primitives belong to drive
/// faces.
#[derive(Debug)]
struct BodyQuery<'a> {
    q: QueryGeometry<'a>,
    drive_face: Vec<bool>,
    drive_edge: Vec<bool>,
    drive_vertex: Vec<bool>,
}

/// A CL point: where the tool tip stops, and what it touches.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Hit {
    /// Tool tip Z.
    pub tip: f64,
    /// Angle between +Z and the normal of the tool's lower surface at the
    /// contact, in radians.
    pub slope: f64,
    /// The contact is on a drive face, or on an edge or vertex of one.
    pub drive: bool,
}

/// Drop queries on the drive and check faces of a Setup (excluded faces
/// left out), for a cutter with `stock_to_leave` applied.
#[derive(Debug)]
pub struct SurfaceQuery<'a> {
    bodies: Vec<BodyQuery<'a>>,
    /// The cutter grown by the stock to leave.
    cutter: Cutter,
    stock: f64,
    opts: DropOptions,
    drops: Cell<u64>,
}

/// Which faces a query uses, by id. `drive` empty means every face.
#[derive(Clone, Debug, Default)]
pub struct FaceSets {
    pub drive: BTreeSet<String>,
    /// `None`: every face that is not a drive or excluded face.
    pub check: Option<BTreeSet<String>>,
    pub excluded: BTreeSet<String>,
}

impl FaceSets {
    pub fn is_drive(&self, id: &str) -> bool {
        !self.excluded.contains(id) && (self.drive.is_empty() || self.drive.contains(id))
    }

    /// The face is in the query: a drive or check face, not excluded.
    pub fn is_queried(&self, id: &str) -> bool {
        !self.excluded.contains(id)
            && (self.is_drive(id) || self.check.as_ref().is_none_or(|c| c.contains(id)))
    }
}

/// A contact this close to the edge of the flat bottom counts as on the
/// flat (ball and bull tools) or on the rim (flat tools).
const FLAT_EPS: f64 = 1e-9;

impl<'a> SurfaceQuery<'a> {
    /// `cutter` is the tool; `stock` the stock to leave, above `−corner`
    /// (or ≥ 0 for a flat tool). `None` if the grown cutter is not valid.
    pub fn new(
        g: &'a SetupGeometry,
        sets: &FaceSets,
        cutter: Cutter,
        stock: f64,
    ) -> Option<SurfaceQuery<'a>> {
        if cutter.corner() + stock < 0.0 {
            return None;
        }
        let grown = Cutter::new(2.0 * (cutter.radius() + stock), cutter.corner() + stock)?;
        let opts = BuildOptions::default();
        let mut bodies = Vec::new();
        for b in &g.bodies {
            let BodyGeom::Brep(body) = &b.geom else {
                continue;
            };
            let ids: Vec<String> = (0..body.faces.len() as u32)
                .map(|i| face_id(&b.id, i))
                .collect();
            let q = QueryGeometry::with_faces(body, &opts, |f| sets.is_queried(&ids[f as usize]));
            let drive_face: Vec<bool> = ids.iter().map(|id| sets.is_drive(id)).collect();
            let mut drive_edge = vec![false; body.edges.len()];
            for (fi, f) in body.faces.iter().enumerate() {
                if !drive_face[fi] {
                    continue;
                }
                for &l in &f.loops {
                    for &c in &body.loops[l as usize].coedges {
                        drive_edge[body.coedges[c as usize].edge as usize] = true;
                    }
                }
            }
            let mut drive_vertex = vec![false; body.vertices.len()];
            for (ei, e) in body.edges.iter().enumerate() {
                if drive_edge[ei] {
                    drive_vertex[e.v0 as usize] = true;
                    drive_vertex[e.v1 as usize] = true;
                }
            }
            bodies.push(BodyQuery {
                q,
                drive_face,
                drive_edge,
                drive_vertex,
            });
        }
        Some(SurfaceQuery {
            bodies,
            cutter: grown,
            stock,
            opts: DropOptions::default(),
            drops: Cell::new(0),
        })
    }

    /// The cutter with the stock to leave applied.
    pub fn cutter(&self) -> &Cutter {
        &self.cutter
    }

    /// Number of drops so far.
    pub fn drops(&self) -> u64 {
        self.drops.get()
    }

    /// The highest tool position with its axis through `(x, y)`; `None`
    /// if nothing is under the tool.
    pub fn drop(&self, x: f64, y: f64) -> Option<Hit> {
        self.drops.set(self.drops.get() + 1);
        let mut best: Option<(&BodyQuery, Contact)> = None;
        let mut st = SolveStats::default();
        for b in &self.bodies {
            let Some(c) = drop(&b.q, &self.cutter, x, y, &self.opts, &mut st) else {
                continue;
            };
            if best.is_none_or(|(_, t)| c.tau > t.tau) {
                best = Some((b, c));
            }
        }
        let (b, c) = best?;
        let drive = match c.on {
            ContactOn::Face { face, .. } => b.drive_face[face as usize],
            ContactOn::Edge { edge, .. } => b.drive_edge[edge as usize],
            ContactOn::Vertex(v) => b.drive_vertex[v as usize],
        };
        Some(Hit {
            tip: c.tau + self.stock,
            slope: self.slope(b, x, y, &c),
            drive,
        })
    }

    /// Slope at contact `c` of the grown cutter with its axis at `(x, y)`.
    fn slope(&self, b: &BodyQuery, x: f64, y: f64, c: &Contact) -> f64 {
        let cut = &self.cutter;
        let p = c.point;
        let e = libm::hypot(p.x() - x, p.y() - y) - cut.flat_radius();
        if cut.corner() > 0.0 {
            if e <= FLAT_EPS {
                return 0.0;
            }
            return libm::asin((e / cut.corner()).clamp(0.0, 1.0));
        }
        if e < -FLAT_EPS {
            return 0.0;
        }
        // On the rim of a flat tool: the face's own slope; for an edge, the
        // angle at which it rises (0 if it lies flat under the tool);
        // vertical for a vertex.
        let (face, u, v) = match c.on {
            ContactOn::Face { face, u, v } => (face, u, v),
            ContactOn::Edge { edge, t } => {
                let ed = &b.q.body.edges[edge as usize];
                let Some(ci) = ed.curve else {
                    return FRAC_PI_2;
                };
                let d = b.q.body.curves[ci as usize].derivs(t, 1)[1];
                let horiz = libm::hypot(d.x(), d.y());
                return libm::atan2(d.z().abs(), horiz);
            }
            ContactOn::Vertex(_) => return FRAC_PI_2,
        };
        let f = &b.q.body.faces[face as usize];
        let Some(n) = b.q.body.surfaces[f.surface as usize].normal(u, v) else {
            return FRAC_PI_2;
        };
        let nz = if f.same_sense {
            n.get().z()
        } else {
            -n.get().z()
        };
        libm::acos(nz.clamp(-1.0, 1.0))
    }

    /// An upper bound of the tip height that clears all geometry under the
    /// tool along the straight XY move from `a` to `b` (with the stock to
    /// leave), from the primitive boxes. −∞ if nothing is near.
    pub fn link_bound(&self, a: Point2, b: Point2) -> f64 {
        let grow = self.cutter.radius() - self.stock + self.stock.max(0.0);
        let q = Aabb3 {
            min: Point3::new(a.x().min(b.x()) - grow, a.y().min(b.y()) - grow, -1e300),
            max: Point3::new(a.x().max(b.x()) + grow, a.y().max(b.y()) + grow, 1e300),
        };
        let mut z = f64::NEG_INFINITY;
        for body in &self.bodies {
            for i in body.q.bvh.query(&q) {
                let bx = body.q.prim_box(i);
                if seg_box_distance(a, b, bx) <= grow {
                    z = z.max(bx.max.z());
                }
            }
        }
        z + self.stock.max(0.0)
    }

    /// Conservative XY box of the drive faces (from their primitive boxes),
    /// or `None` if there are none.
    pub fn drive_box(&self) -> Option<(Point2, Point2)> {
        let mut bb = Aabb3::empty();
        for b in &self.bodies {
            for (i, p) in b.q.prims.iter().enumerate() {
                if let crate::cutter::geom::Prim::Face { face, .. } = p
                    && b.drive_face[*face as usize]
                {
                    let bx = b.q.prim_box(i as u32);
                    bb = bb.with_point(bx.min).with_point(bx.max);
                }
            }
        }
        (!bb.is_empty()).then(|| {
            (
                Point2::new(bb.min.x(), bb.min.y()),
                Point2::new(bb.max.x(), bb.max.y()),
            )
        })
    }
}

/// Lateral distance from the segment `a`–`b` to the XY projection of `bx`
/// (0 if they overlap); a lower bound good enough for pruning.
fn seg_box_distance(a: Point2, b: Point2, bx: &Aabb3) -> f64 {
    let (x0, y0, x1, y1) = (bx.min.x(), bx.min.y(), bx.max.x(), bx.max.y());
    let inside = |p: Point2| p.x() >= x0 && p.x() <= x1 && p.y() >= y0 && p.y() <= y1;
    if inside(a) || inside(b) {
        return 0.0;
    }
    let corners = [
        Point2::new(x0, y0),
        Point2::new(x1, y0),
        Point2::new(x1, y1),
        Point2::new(x0, y1),
    ];
    let seg_pt = |p: Point2, q: Point2, c: Point2| -> f64 {
        let d = q - p;
        let l2 = d.dot(d);
        let t = if l2 > 0.0 {
            ((c - p).dot(d) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (p + d * t - c).length()
    };
    let mut best = f64::INFINITY;
    for k in 0..4 {
        let (p, q) = (corners[k], corners[(k + 1) % 4]);
        best = best
            .min(seg_pt(a, b, p))
            .min(seg_pt(p, q, a))
            .min(seg_pt(p, q, b));
        if segments_cross(a, b, p, q) {
            return 0.0;
        }
    }
    best
}

fn segments_cross(a: Point2, b: Point2, c: Point2, d: Point2) -> bool {
    let o = |p: Point2, q: Point2, r: Point2| (q - p).perp_dot(r - p);
    let (d1, d2) = (o(c, d, a), o(c, d, b));
    let (d3, d4) = (o(a, b, c), o(a, b, d));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

#[cfg(test)]
mod tests;
