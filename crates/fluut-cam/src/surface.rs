// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Setup's bodies as drop-query geometry for 3-D types (D07, D08 §4):
//! drive, check and excluded faces, the stock to leave as a grown cutter
//! profile, the slope at a contact, and a bound for links.

use crate::cutter::geom::{BuildOptions, QueryGeometry, curve_box};
use crate::cutter::solve::{Contact, ContactOn, SolveStats};
use crate::cutter::{Cutter, DropOptions, drop};
use crate::geometry::{BodyGeom, SetupGeometry, face_id};
use fluut_geom::{Aabb3, Curve3, Interval, Point2, Point3, Vec2};
use std::collections::BTreeSet;
use std::f64::consts::FRAC_PI_2;
use std::sync::atomic::{AtomicU64, Ordering};

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

/// A drop with what a waterline needs.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Probe {
    pub tip: f64,
    /// As [`Hit::slope`], except that an edge or vertex on the rim of a
    /// flat tool counts as vertical.
    pub slope: f64,
    pub drive: bool,
    /// Unit XY vector from the tool axis to the contact (zero when the
    /// contact is on the axis): τ rises along it.
    pub toward: Vec2,
}

/// A drop that decides the side of a level ([`SurfaceQuery::above`]).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Side {
    /// The tip is above the level.
    pub inside: bool,
    /// Tip − level, clamped to the window `[−w, w]`.
    pub g: f64,
    /// The probe, where the tip is within the window (exact there).
    pub probe: Option<Probe>,
    /// When inside: a contact that holds the tip above the level (above
    /// the window, not necessarily the highest one).
    pub contact: Option<Point3>,
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
    drops: AtomicU64,
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
            drops: AtomicU64::new(0),
        })
    }

    /// The cutter with the stock to leave applied.
    pub fn cutter(&self) -> &Cutter {
        &self.cutter
    }

    /// Number of drops so far.
    pub fn drops(&self) -> u64 {
        self.drops.load(Ordering::Relaxed)
    }

    /// The highest tool position with its axis through `(x, y)`; `None`
    /// if nothing is under the tool.
    pub fn drop(&self, x: f64, y: f64) -> Option<Hit> {
        let (b, c) = self.best(x, y)?;
        Some(Hit {
            tip: c.tau + self.stock,
            slope: self.slope(b, x, y, &c),
            drive: self.is_drive(b, &c),
        })
    }

    /// As [`SurfaceQuery::drop`], with the contact direction and the slope
    /// a waterline uses (D09 §4.4).
    pub fn probe(&self, x: f64, y: f64) -> Option<Probe> {
        let (b, c) = self.best(x, y)?;
        Some(self.probe_of(b, x, y, &c))
    }

    fn probe_of(&self, b: &BodyQuery, x: f64, y: f64, c: &Contact) -> Probe {
        let (dx, dy) = (c.point.x() - x, c.point.y() - y);
        let d = libm::hypot(dx, dy);
        let toward = if d > 1e-12 {
            Vec2::new(dx / d, dy / d)
        } else {
            Vec2::new(0.0, 0.0)
        };
        let mut slope = self.slope(b, x, y, c);
        // On the rim of a flat tool an edge or a vertex is a wall: τ jumps
        // there.
        if self.cutter.corner() == 0.0
            && d - self.cutter.radius() >= -FLAT_EPS
            && !matches!(c.on, ContactOn::Face { .. })
        {
            slope = FRAC_PI_2;
        }
        Probe {
            tip: c.tau + self.stock,
            slope,
            drive: self.is_drive(b, c),
            toward,
        }
    }

    /// The face the side of the tool touches when its tip is at `z` with
    /// the axis at `p`, facing `n` (unit XY, towards the material): for a
    /// waterline at a cliff, where the drop's contact is on top of the wall,
    /// not at the level. The nearest face point within `eps` of the side of
    /// the (grown) cutter at the top of its corner. `(slope, drive)`.
    pub fn wall_at(&self, p: Point2, n: Vec2, z: f64, eps: f64) -> Option<(f64, bool)> {
        let c = &self.cutter;
        let q = Point3::new(
            p.x() + n.x() * c.radius(),
            p.y() + n.y() * c.radius(),
            z - self.stock + c.corner(),
        );
        let bx = Aabb3 {
            min: Point3::new(q.x() - eps, q.y() - eps, q.z() - eps),
            max: Point3::new(q.x() + eps, q.y() + eps, q.z() + eps),
        };
        let tol = fluut_geom::Tol::for_machining(1e-6, 1e-3);
        let mut best: Option<(f64, f64, bool)> = None;
        for b in &self.bodies {
            for i in b.q.bvh.query(&bx) {
                let crate::cutter::geom::Prim::Face { face, uv } = b.q.prims[i as usize] else {
                    continue;
                };
                let Some(qf) = &b.q.faces[face as usize] else {
                    continue;
                };
                let f = &b.q.body.faces[face as usize];
                let srf = &b.q.body.surfaces[f.surface as usize];
                let cp = srf.closest(q, uv, &tol);
                // The trim takes analytic parameters in their base period.
                let (u, v) = match srf {
                    fluut_geom::Surface::BSpline(_)
                    | fluut_geom::Surface::Extrusion { .. }
                    | fluut_geom::Surface::Revolution { .. }
                    | fluut_geom::Surface::Offset { .. } => (cp.u, cp.v),
                    _ => crate::cutter::solve::analytic_uv(srf, cp.point),
                };
                if cp.dist > eps || !qf.trim.contains(u, v) {
                    continue;
                }
                if best.is_some_and(|(d, _, _)| d <= cp.dist) {
                    continue;
                }
                let slope = srf.normal(cp.u, cp.v).map_or(FRAC_PI_2, |nn| {
                    let nz = if f.same_sense {
                        nn.get().z()
                    } else {
                        -nn.get().z()
                    };
                    libm::acos(nz.clamp(-1.0, 1.0))
                });
                best = Some((cp.dist, slope, b.drive_face[face as usize]));
            }
        }
        best.map(|(_, s, d)| (s, d))
    }

    /// Whether the tip with the axis through `(x, y)` is above `z`, and
    /// `tip − z` clamped to `[−w, w]` (−w if nothing is under the tool),
    /// with the probe when the tip is within `w` of `z` (it is exact
    /// there). Cheaper than [`SurfaceQuery::probe`] away from `z`: the
    /// search stops at the first contact above `z + w` and skips everything
    /// below `z − w`. `w > 0`.
    pub fn above(&self, x: f64, y: f64, z: f64, w: f64) -> Side {
        let opts = DropOptions {
            floor: z - w - self.stock,
            ceiling: z + w - self.stock,
            ..self.opts
        };
        match self.best_with(x, y, &opts) {
            Some((b, c)) => {
                let g = c.tau + self.stock - z;
                Side {
                    inside: g > 0.0,
                    g: g.clamp(-w, w),
                    probe: (g.abs() < w).then(|| self.probe_of(b, x, y, &c)),
                    contact: (g > 0.0).then_some(c.point),
                }
            }
            None => Side {
                inside: false,
                g: -w,
                probe: None,
                contact: None,
            },
        }
    }

    /// The tip height of the tool with its axis through `(x, y)` touching
    /// `p`, if `p` is in the footprint (with the stock to leave).
    pub fn tip_touching(&self, x: f64, y: f64, p: Point3) -> Option<f64> {
        let d = libm::hypot(p.x() - x, p.y() - y);
        let r = self.cutter.radius();
        (d <= r).then(|| p.z() - self.cutter.h(d) + self.stock)
    }

    /// Radius of the cutter grown by the stock to leave.
    pub fn radius(&self) -> f64 {
        self.cutter.radius()
    }

    fn best(&self, x: f64, y: f64) -> Option<(&BodyQuery<'a>, Contact)> {
        self.best_with(x, y, &self.opts)
    }

    fn best_with(&self, x: f64, y: f64, opts: &DropOptions) -> Option<(&BodyQuery<'a>, Contact)> {
        self.drops.fetch_add(1, Ordering::Relaxed);
        let mut best: Option<(&BodyQuery, Contact)> = None;
        let mut st = SolveStats::default();
        for b in &self.bodies {
            let Some(c) = drop(&b.q, &self.cutter, x, y, opts, &mut st) else {
                continue;
            };
            if best.is_none_or(|(_, t)| c.tau > t.tau) {
                best = Some((b, c));
            }
        }
        best
    }

    fn is_drive(&self, b: &BodyQuery, c: &Contact) -> bool {
        match c.on {
            ContactOn::Face { face, .. } => b.drive_face[face as usize],
            ContactOn::Edge { edge, .. } => b.drive_edge[edge as usize],
            ContactOn::Vertex(v) => b.drive_vertex[v as usize],
        }
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

    /// XY box of the drive faces from their edges: exact for lines,
    /// circles and ellipses. A face that bulges sideways beyond its edges
    /// is not covered. `None` if there are no drive faces.
    pub fn drive_box(&self) -> Option<(Point2, Point2)> {
        let mut bb = Aabb3::empty();
        for b in &self.bodies {
            let body = b.q.body;
            for (fi, f) in body.faces.iter().enumerate() {
                if !b.drive_face[fi] || b.q.faces[fi].is_none() {
                    continue;
                }
                for &l in &f.loops {
                    for &c in &body.loops[l as usize].coedges {
                        let e = &body.edges[body.coedges[c as usize].edge as usize];
                        let eb = match e.curve {
                            Some(ci) => edge_box(&body.curves[ci as usize], e.range),
                            None => Aabb3::from_points(&[body.vertices[e.v0 as usize].p]),
                        };
                        bb = bb.with_point(eb.min).with_point(eb.max);
                    }
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

/// Box of an edge: exact for lines and conics, sampled otherwise.
fn edge_box(c: &Curve3, range: Interval) -> Aabb3 {
    match c {
        Curve3::Line { .. } | Curve3::Circle { .. } | Curve3::Ellipse { .. } => curve_box(c, range),
        _ => {
            let pts: Vec<Point3> = c.tessellate(range, 1e-4).into_iter().map(|x| x.1).collect();
            Aabb3::from_points(&pts)
        }
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
