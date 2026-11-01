// SPDX-License-Identifier: MIT OR Apache-2.0

//! Plane sections of the Setup's solids as exact 2-D regions (spec 04
//! §4.4, D03 §5.1, D08 §7.1).
//!
//! Each face's surface is cut by the plane `z` (D02 §5). The section curve
//! is split where the face's edges cross the plane, and the pieces whose
//! middle lies inside the face's trim are kept. The pieces of every face
//! are projected onto XY as lines and arcs and joined end to end; the
//! split points are the shared edge crossings, so the joins are exact.
//! Closed chains nest into a region by the even–odd rule.
//!
//! A plane at the height of a horizontal face touches it everywhere
//! (CH-03): callers take the section a little above such levels.

use crate::chain::{Piece, chain_pieces, project_curve};
use crate::cutter::trim::Trim;
use crate::drive::nest;
use crate::geometry::{BodyGeom, SetupGeometry, face_id};
use crate::region::Region2;
use fluut_geom::{
    Curve3, Interval, Point2, Point3, SectionPlane, Surface, Tol, UvBox, plane_section,
};
use fluut_topo::BrepBody;

/// One face, ready to be sectioned.
#[derive(Debug)]
struct SFace {
    id: String,
    surface: Surface,
    trim: Trim,
    /// The trim's parameter box, grown so section curves reach past the
    /// face's edges.
    uv: UvBox,
    edges: Vec<u32>,
}

#[derive(Debug)]
struct SBody<'a> {
    body: &'a BrepBody,
    faces: Vec<SFace>,
}

/// The faces of a Setup's solids, prepared once for many sections.
#[derive(Debug)]
pub struct Sectioner<'a> {
    bodies: Vec<SBody<'a>>,
    tol: f64,
}

/// A section: the material region and the pieces it is bounded by, with
/// the id of the face each comes from.
#[derive(Debug, Clone, Default)]
pub struct Section {
    pub region: Region2,
    pub pieces: Vec<(Piece, String)>,
}

impl<'a> Sectioner<'a> {
    /// Prepares every face of every solid of `g`; `tol` is the chord
    /// tolerance of non-circular section curves.
    pub fn new(g: &'a SetupGeometry, tol: f64) -> Result<Sectioner<'a>, String> {
        let mut bodies = Vec::new();
        for b in &g.bodies {
            let BodyGeom::Brep(body) = &b.geom else {
                continue;
            };
            let mut faces = Vec::new();
            for (fi, f) in body.faces.iter().enumerate() {
                let trim = Trim::build(body, fi as u32, (tol / 4.0).max(1e-4)).map_err(|e| {
                    format!(
                        "face {} cannot be sectioned: {e:?}",
                        face_id(&b.id, fi as u32)
                    )
                })?;
                let t = trim.uv_box();
                let grow = |i: Interval| {
                    let g = 1e-2 * i.length() + 1e-6;
                    Interval {
                        lo: i.lo - g,
                        hi: i.hi + g,
                    }
                };
                let mut edges = Vec::new();
                for &l in &f.loops {
                    for &c in &body.loops[l as usize].coedges {
                        edges.push(body.coedges[c as usize].edge);
                    }
                }
                edges.sort_unstable();
                edges.dedup();
                faces.push(SFace {
                    id: face_id(&b.id, fi as u32),
                    surface: body.surfaces[f.surface as usize].clone(),
                    uv: UvBox::new(grow(t.u), grow(t.v)),
                    trim,
                    edges,
                });
            }
            bodies.push(SBody { body, faces });
        }
        Ok(Sectioner {
            bodies,
            tol: tol.max(1e-4),
        })
    }

    /// The section at height `z`. Fails when the pieces do not close
    /// (an open shell, or a face the plane only touches).
    pub fn section(&self, z: f64) -> Result<Section, String> {
        let plane = SectionPlane::horizontal(z);
        let mut region = Region2::empty();
        let mut all = Vec::new();
        for b in &self.bodies {
            let tol = Tol::for_machining(b.body.tol_model.max(1e-6), self.tol);
            let crossings: Vec<Vec<Point3>> = (0..b.body.edges.len() as u32)
                .map(|e| edge_crossings(b.body, e, z))
                .collect();
            let mut pieces = Vec::new();
            for f in &b.faces {
                let s = plane_section(&f.surface, f.uv, &plane, &tol);
                for c in s.curves.iter().filter(|c| !c.tangent) {
                    for (range, ends) in split(&c.curve, c.range, f, &crossings, &tol) {
                        let mut p = project_curve(&c.curve, range, self.tol);
                        snap(&mut p, ends);
                        if !p.segs.is_empty() {
                            pieces.push((p, f.id.clone()));
                        }
                    }
                }
            }
            let chains = chain_pieces(
                pieces.iter().map(|(p, _)| p.clone()).collect(),
                (1e3 * b.body.tol_model).max(1e-5),
            );
            if chains.iter().any(|c| !c.path.closed) {
                return Err(format!(
                    "the section of {} at Z {z} is not closed",
                    b.body.name
                ));
            }
            let r = nest(chains.iter().map(|c| c.ccw_loop()).collect());
            region = if region.is_empty() {
                r
            } else {
                region.union(&r)
            };
            all.extend(pieces);
        }
        Ok(Section {
            region: snapped(&region),
            pieces: all,
        })
    }
}

/// Points where edge `e` crosses the plane `z`.
fn edge_crossings(b: &BrepBody, e: u32, z: f64) -> Vec<Point3> {
    let ed = &b.edges[e as usize];
    let Some(ci) = ed.curve else {
        return Vec::new();
    };
    let c = &b.curves[ci as usize];
    let pts = c.tessellate(ed.range, 1e-3);
    let mut out = Vec::new();
    for w in pts.windows(2) {
        let (t0, p0) = w[0];
        let (t1, p1) = w[1];
        let (f0, f1) = (p0.z() - z, p1.z() - z);
        if f0 == 0.0 {
            out.push(p0);
            continue;
        }
        if f0 * f1 >= 0.0 {
            continue;
        }
        let (mut lo, mut hi, mut flo) = (t0, t1, f0);
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            let fm = c.eval(mid).z() - z;
            if (fm < 0.0) == (flo < 0.0) {
                lo = mid;
                flo = fm;
            } else {
                hi = mid;
            }
        }
        let p = c.eval(0.5 * (lo + hi));
        out.push(Point3::new(p.x(), p.y(), z));
    }
    if let Some(&(_, p)) = pts.last()
        && p.z() == z
    {
        out.push(p);
    }
    out
}

/// Sub-ranges of a section curve that lie on face `f`, with the exact
/// edge crossings at their ends where there are any.
fn split(
    c: &Curve3,
    range: Interval,
    f: &SFace,
    crossings: &[Vec<Point3>],
    tol: &Tol,
) -> Vec<(Interval, [Option<Point3>; 2])> {
    let mut cuts: Vec<(f64, Point3)> = Vec::new();
    for &e in &f.edges {
        for &p in &crossings[e as usize] {
            let (t, d) = c.closest(p, range, tol);
            if d <= 1e-6 + 10.0 * tol.model {
                cuts.push((t, p));
            }
        }
    }
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    cuts.dedup_by(|a, b| (a.0 - b.0).abs() <= 1e-12);
    let closed = c.eval(range.lo).distance(c.eval(range.hi)) <= 1e-9;
    let mut spans: Vec<(f64, f64, Option<Point3>, Option<Point3>)> = Vec::new();
    if cuts.is_empty() {
        spans.push((range.lo, range.hi, None, None));
    } else if closed {
        let period = range.hi - range.lo;
        for i in 0..cuts.len() {
            let (t0, p0) = cuts[i];
            let (mut t1, p1) = cuts[(i + 1) % cuts.len()];
            if t1 <= t0 {
                t1 += period;
            }
            spans.push((t0, t1, Some(p0), Some(p1)));
        }
    } else {
        let mut prev = (range.lo, None);
        for &(t, p) in &cuts {
            spans.push((prev.0, t, prev.1, Some(p)));
            prev = (t, Some(p));
        }
        spans.push((prev.0, range.hi, prev.1, None));
    }
    spans
        .into_iter()
        .filter(|(a, b, _, _)| b - a > 1e-12)
        .filter(|&(a, b, _, _)| {
            let m = c.eval(0.5 * (a + b));
            let cp = f.surface.closest(m, f.uv, tol);
            cp.dist <= 1e-6 + 10.0 * tol.model && f.trim.contains(cp.u, cp.v)
        })
        .map(|(a, b, p0, p1)| (Interval { lo: a, hi: b }, [p0, p1]))
        .collect()
}

/// The region with vertices on a 1e-9 mm grid and bulges to 1e-12.
/// Rounding differences from the work-frame transform then vanish, so a
/// part moved with its work zero gives bit-identical sections and paths
/// (spec 04 §4.12.7 (3)).
fn snapped(r: &Region2) -> Region2 {
    let q = |v: f64, k: f64| (v * k).round() / k + 0.0;
    Region2 {
        loops: r
            .loops
            .iter()
            .map(|l| crate::region::Loop2 {
                verts: l
                    .verts
                    .iter()
                    .map(|v| crate::region::Vertex {
                        p: Point2::new(q(v.p.x(), 1e9), q(v.p.y(), 1e9)),
                        bulge: q(v.bulge, 1e12),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Moves a piece's ends onto the exact edge crossings.
fn snap(p: &mut Piece, ends: [Option<Point3>; 2]) {
    let xy = |q: Point3| Point2::new(q.x(), q.y());
    if let (Some(q), Some(s)) = (ends[0], p.segs.first_mut()) {
        s.a = xy(q);
    }
    if let (Some(q), Some(s)) = (ends[1], p.segs.last_mut()) {
        s.b = xy(q);
    }
}
