// SPDX-License-Identifier: MIT OR Apache-2.0

//! Drive geometry of the 2-D types from the Geometry group (D15 §2.3, §3,
//! §4): curve selections, vertical wall faces and planar floor faces.

use crate::chain::{Piece, chain_pieces, project_curve};
use crate::geometry::{BodyGeom, EdgeRef, SetupGeometry, edge_points, face_id, face_z_range};
use crate::region::{Inside, Loop2, Region2};
use fluut_geom::{Interval, Point2, Surface};
use fluut_model::{Diagnostic, Sel};
use fluut_topo::BrepBody;

/// Below this, a normal component counts as zero (vertical walls) or a unit
/// one as exactly ±1 (horizontal faces).
const ANGLE: f64 = 1e-9;

fn missing(id: &str, field: &str) -> Diagnostic {
    Diagnostic::error(
        "param.geometry.selection.missing",
        format!("`{id}` is not in the model"),
    )
    .at(field)
}

/// Faces named by a face selection (bodies expand to all their faces),
/// minus `excluded`. `(body, face index, id)`.
pub fn faces<'a>(
    g: &'a SetupGeometry,
    sels: &[Sel],
    excluded: &[Sel],
    field: &str,
) -> Result<Vec<(&'a BrepBody, u32, String)>, Diagnostic> {
    let mut out = Vec::new();
    for s in sels {
        match s {
            Sel::Face(id) => {
                let (b, i) = g.face(id).ok_or_else(|| missing(id, field))?;
                out.push((b, i, id.clone()));
            }
            Sel::Body(id) => match &g.body(id).ok_or_else(|| missing(id, field))?.geom {
                BodyGeom::Brep(b) => {
                    for i in 0..b.faces.len() as u32 {
                        out.push((b, i, face_id(id, i)));
                    }
                }
                BodyGeom::Wire(_) => {
                    return Err(Diagnostic::error(
                        "param.geometry.drive_faces.kind",
                        format!("`{id}` is a wire body and has no faces"),
                    )
                    .at(field));
                }
            },
            Sel::Edge(id) => {
                return Err(Diagnostic::error(
                    "param.geometry.drive_faces.kind",
                    format!("`{id}` is an edge, not a face"),
                )
                .at(field));
            }
        }
    }
    let skip: Vec<&str> = excluded.iter().map(Sel::id).collect();
    out.retain(|(_, _, id)| !skip.contains(&id.as_str()));
    Ok(out)
}

fn edge_piece(e: EdgeRef<'_>, tol: f64) -> Option<Piece> {
    match e {
        EdgeRef::Brep(b, i) => {
            let ed = &b.edges[i as usize];
            let c = &b.curves[ed.curve? as usize];
            Some(project_curve(c, ed.range, tol))
        }
        EdgeRef::Wire(c) => Some(project_curve(&c.curve, c.range, tol)),
    }
}

fn reversed(p: Piece) -> Piece {
    Piece {
        segs: p
            .segs
            .iter()
            .rev()
            .map(crate::path2::seg_reversed)
            .collect(),
        z: p.z,
    }
}

/// The edges of one face loop as pieces in loop order.
fn loop_pieces(b: &BrepBody, l: u32, tol: f64) -> Vec<Piece> {
    b.loops[l as usize]
        .coedges
        .iter()
        .filter_map(|&c| {
            let ce = &b.coedges[c as usize];
            let p = edge_piece(EdgeRef::Brep(b, ce.edge), tol)?;
            Some(if ce.forward { p } else { reversed(p) })
        })
        .collect()
}

/// Pieces of a curve selection (`drive_curves`, `boundary.curves`): wire
/// curves, edges, and the loops of planar faces.
pub fn curve_pieces(
    g: &SetupGeometry,
    sels: &[Sel],
    tol: f64,
    field: &str,
) -> Result<Vec<Piece>, Diagnostic> {
    let mut out = Vec::new();
    for s in sels {
        match s {
            Sel::Edge(id) => {
                let e = g.edge(id).ok_or_else(|| missing(id, field))?;
                out.extend(edge_piece(e, tol));
            }
            Sel::Face(id) => {
                let (b, f) = g.face(id).ok_or_else(|| missing(id, field))?;
                for &l in &b.faces[f as usize].loops {
                    out.extend(loop_pieces(b, l, tol));
                }
            }
            Sel::Body(id) => match &g.body(id).ok_or_else(|| missing(id, field))?.geom {
                BodyGeom::Wire(w) => {
                    for c in &w.curves {
                        out.push(project_curve(&c.curve, c.range, tol));
                    }
                }
                BodyGeom::Brep(b) => {
                    for f in &b.faces {
                        if is_horizontal_plane(&b.surfaces[f.surface as usize]) {
                            for &l in &f.loops {
                                out.extend(loop_pieces(b, l, tol));
                            }
                        }
                    }
                }
            },
        }
    }
    Ok(out)
}

pub fn is_horizontal_plane(s: &Surface) -> bool {
    matches!(s, Surface::Plane { frame } if frame.z.get().z().abs() > 1.0 - ANGLE)
}

/// A face whose surface contains the tool axis direction: a plane with a
/// horizontal normal, a cylinder about Z, or an extrusion along Z.
pub fn is_vertical_wall(s: &Surface) -> bool {
    match s {
        Surface::Plane { frame } => frame.z.get().z().abs() < ANGLE,
        Surface::Cylinder { frame, .. } => frame.z.get().z().abs() > 1.0 - ANGLE,
        Surface::Extrusion { dir, .. } => dir.get().z().abs() > 1.0 - ANGLE,
        _ => false,
    }
}

/// Bottom edges of vertical wall faces (`contour_2d` drive faces).
pub fn wall_pieces(
    g: &SetupGeometry,
    sels: &[Sel],
    excluded: &[Sel],
    tol: f64,
) -> Result<Vec<Piece>, Diagnostic> {
    wall_edges(g, sels, excluded, tol, false)
}

/// Bottom (or, with `top`, top) edges of vertical wall faces.
pub fn wall_edges(
    g: &SetupGeometry,
    sels: &[Sel],
    excluded: &[Sel],
    tol: f64,
    top: bool,
) -> Result<Vec<Piece>, Diagnostic> {
    let mut out = Vec::new();
    let ztol = g.tol_model().max(1e-6) * 10.0;
    for (b, f, id) in faces(g, sels, excluded, "geometry.drive_faces")? {
        let face = &b.faces[f as usize];
        if !is_vertical_wall(&b.surfaces[face.surface as usize]) {
            return Err(Diagnostic::error(
                "param.geometry.drive_faces.kind",
                format!("face `{id}` is not a vertical wall"),
            )
            .at("geometry.drive_faces"));
        }
        let zr = face_z_range(b, f, tol);
        let lo = if top { zr.hi } else { zr.lo };
        for &l in &face.loops {
            for &c in &b.loops[l as usize].coedges {
                let ce = &b.coedges[c as usize];
                let pts = edge_points(EdgeRef::Brep(b, ce.edge), tol);
                if pts.iter().all(|p| (p.z() - lo).abs() <= ztol)
                    && let Some(p) = edge_piece(EdgeRef::Brep(b, ce.edge), tol)
                {
                    out.push(if ce.forward { p } else { reversed(p) });
                }
            }
        }
    }
    Ok(out)
}

/// Regions and Z of planar horizontal floor faces (`pocket_2d`).
pub fn floor_regions(
    g: &SetupGeometry,
    sels: &[Sel],
    excluded: &[Sel],
    tol: f64,
) -> Result<Vec<(Region2, Interval)>, Diagnostic> {
    let join = g.tol_model().max(1e-5);
    let mut out = Vec::new();
    for (b, f, id) in faces(g, sels, excluded, "geometry.drive_faces")? {
        let face = &b.faces[f as usize];
        if !is_horizontal_plane(&b.surfaces[face.surface as usize]) {
            return Err(Diagnostic::error(
                "param.geometry.drive_faces.kind",
                format!("face `{id}` is not planar and horizontal"),
            )
            .at("geometry.drive_faces"));
        }
        let mut loops = Vec::new();
        let mut z = Interval {
            lo: f64::INFINITY,
            hi: f64::NEG_INFINITY,
        };
        for &l in &face.loops {
            for ch in chain_pieces(loop_pieces(b, l, tol), join) {
                if ch.path.closed {
                    z = Interval {
                        lo: z.lo.min(ch.z.lo),
                        hi: z.hi.max(ch.z.hi),
                    };
                    loops.push(ch.ccw_loop());
                }
            }
        }
        out.push((nest(loops), z));
    }
    Ok(out)
}

/// A region from simple, non-crossing loops: a loop inside an odd number of
/// others is a hole.
pub fn nest(loops: Vec<Loop2>) -> Region2 {
    let probe = |l: &Loop2| -> Point2 {
        l.segs()
            .next()
            .map_or(Point2::new(0.0, 0.0), |s| s.point_at(0.5))
    };
    let depth: Vec<usize> = loops
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let p = probe(l);
            loops
                .iter()
                .enumerate()
                .filter(|&(j, o)| {
                    j != i
                        && Region2 {
                            loops: vec![if o.is_ccw() { o.clone() } else { o.reversed() }],
                        }
                        .contains(p)
                            == Inside::In
                })
                .count()
        })
        .collect();
    let loops = loops
        .into_iter()
        .zip(depth)
        .map(|(l, d)| {
            let ccw = l.is_ccw();
            if (d % 2 == 0) == ccw { l } else { l.reversed() }
        })
        .collect();
    Region2 { loops }
}

/// Groups a region into areas: each outer loop with the holes inside it.
pub fn areas(r: &Region2) -> Vec<Region2> {
    let mut out: Vec<Region2> = r
        .loops
        .iter()
        .filter(|l| l.is_ccw())
        .map(|l| Region2 {
            loops: vec![l.clone()],
        })
        .collect();
    for h in r.loops.iter().filter(|l| !l.is_ccw()) {
        let p = h.verts[0].p;
        if let Some(a) = out
            .iter_mut()
            .filter(|a| a.contains(p) != Inside::Out)
            .min_by(|a, b| a.area().total_cmp(&b.area()))
        {
            a.loops.push(h.clone());
        }
    }
    out
}
