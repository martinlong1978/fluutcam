// SPDX-License-Identifier: MIT OR Apache-2.0

//! Body validity checks (design D04 §6).

use crate::brep::BrepBody;
use fluut_geom::{Interval, Surface, Tol, UvBox};
use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_2;

#[derive(Debug, Clone, PartialEq)]
pub enum Issue {
    /// A face without exactly one outer loop.
    OuterLoops { face: u32, count: usize },
    /// Consecutive coedges of a loop do not meet.
    LoopGap { face: u32, coedge: u32, gap: f64 },
    /// An edge's curve does not pass through its vertex.
    VertexOffCurve { edge: u32, dist: f64 },
    /// An edge's midpoint is not on the surface of a face that uses it.
    EdgeOffSurface { face: u32, edge: u32, dist: f64 },
    /// An edge of a closed shell not used exactly once in each direction.
    NotManifold {
        shell: u32,
        edge: u32,
        forward: usize,
        backward: usize,
    },
}

/// Parameter box used for closest-point checks on a surface.
fn search_box(s: &Surface) -> Option<UvBox> {
    let big = Interval::new(-1e7, 1e7)?;
    let lat = Interval::new(-FRAC_PI_2, FRAC_PI_2)?;
    Some(match s {
        Surface::BSpline(b) => b.domain(),
        Surface::Sphere { .. } => UvBox::new(big, lat),
        Surface::Extrusion { profile, .. } => UvBox::new(profile.natural_range()?, big),
        Surface::Revolution { profile, .. } => UvBox::new(big, profile.natural_range()?),
        Surface::Offset { basis, .. } => search_box(basis)?,
        _ => UvBox::new(big, big),
    })
}

/// All issues found in a body. `tol` is the gap allowed between entities
/// that should coincide.
pub fn check(b: &BrepBody, tol: f64) -> Vec<Issue> {
    let mut out = Vec::new();
    // Solve closest points far below the gap being tested.
    let gtol = Tol::for_machining(1e-7, 1e-4);
    for (ei, e) in b.edges.iter().enumerate() {
        let Some(c) = e.curve else { continue };
        let curve = &b.curves[c as usize];
        for (t, v) in [(e.range.lo, e.v0), (e.range.hi, e.v1)] {
            let d = curve.eval(t).distance(b.vertices[v as usize].p);
            if d > tol {
                out.push(Issue::VertexOffCurve {
                    edge: ei as u32,
                    dist: d,
                });
            }
        }
    }
    for (fi, f) in b.faces.iter().enumerate() {
        let fi = fi as u32;
        let outer = f
            .loops
            .iter()
            .filter(|&&l| b.loops[l as usize].outer)
            .count();
        if outer != 1 {
            out.push(Issue::OuterLoops {
                face: fi,
                count: outer,
            });
        }
        let surf = &b.surfaces[f.surface as usize];
        let sbox = search_box(surf);
        for &l in &f.loops {
            let cs = &b.loops[l as usize].coedges;
            for (i, &c) in cs.iter().enumerate() {
                let next = cs[(i + 1) % cs.len()];
                let (_, end) = b.coedge_ends(c);
                let (start, _) = b.coedge_ends(next);
                if end != start {
                    let gap = b.vertices[end as usize]
                        .p
                        .distance(b.vertices[start as usize].p);
                    if gap > tol {
                        out.push(Issue::LoopGap {
                            face: fi,
                            coedge: c,
                            gap,
                        });
                    }
                }
                let edge = b.coedges[c as usize].edge;
                let e = &b.edges[edge as usize];
                if let (Some(curve), Some(sb)) = (e.curve, sbox) {
                    let mid = b.curves[curve as usize].eval(e.range.at(0.5));
                    let d = surf.closest(mid, sb, &gtol).dist;
                    if d > tol {
                        out.push(Issue::EdgeOffSurface {
                            face: fi,
                            edge,
                            dist: d,
                        });
                    }
                }
            }
        }
    }
    for (si, s) in b.shells.iter().enumerate() {
        if !s.closed {
            continue;
        }
        let mut uses: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
        for &f in &s.faces {
            let face = &b.faces[f as usize];
            for &l in &face.loops {
                for &c in &b.loops[l as usize].coedges {
                    let ce = &b.coedges[c as usize];
                    let u = uses.entry(ce.edge).or_default();
                    if ce.forward {
                        u.0 += 1;
                    } else {
                        u.1 += 1;
                    }
                }
            }
        }
        for (edge, (fw, bw)) in uses {
            // Seam edges are used once each way by the same face.
            if fw != 1 || bw != 1 {
                out.push(Issue::NotManifold {
                    shell: si as u32,
                    edge,
                    forward: fw,
                    backward: bw,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brep::{Coedge, Edge, Face, Loop, Shell, Vertex};
    use fluut_geom::{Curve3, Frame, Point3, UnitVec3, Vec3};

    /// The unit cube as a closed shell of six planar faces.
    pub(crate) fn cube() -> BrepBody {
        let mut b = BrepBody {
            name: "cube".into(),
            tol_model: 1e-6,
            ..Default::default()
        };
        for i in 0..8 {
            let p = Point3::new((i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64);
            b.vertices.push(Vertex { p });
        }
        // Faces as outward-CCW vertex cycles.
        let faces: [[u32; 4]; 6] = [
            [0, 2, 3, 1], // z = 0, normal −z
            [4, 5, 7, 6], // z = 1
            [0, 1, 5, 4], // y = 0
            [2, 6, 7, 3], // y = 1
            [0, 4, 6, 2], // x = 0
            [1, 3, 7, 5], // x = 1
        ];
        let mut edge_of: BTreeMap<(u32, u32), u32> = BTreeMap::new();
        for f in faces {
            let pts: Vec<Point3> = f.iter().map(|&v| b.vertices[v as usize].p).collect();
            let n = (pts[1] - pts[0]).cross(pts[2] - pts[1]);
            let frame = Frame::new(pts[0], n, pts[1] - pts[0]).unwrap();
            b.surfaces.push(Surface::plane(frame));
            let mut cs = Vec::new();
            for k in 0..4 {
                let (a, c) = (f[k], f[(k + 1) % 4]);
                let key = (a.min(c), a.max(c));
                let e = *edge_of.entry(key).or_insert_with(|| {
                    let (pa, pc) = (b.vertices[key.0 as usize].p, b.vertices[key.1 as usize].p);
                    let d: Vec3 = pc - pa;
                    b.curves.push(Curve3::line(pa, UnitVec3::new(d).unwrap()));
                    b.edges.push(Edge {
                        curve: Some(b.curves.len() as u32 - 1),
                        range: Interval::new(0.0, d.length()).unwrap(),
                        v0: key.0,
                        v1: key.1,
                    });
                    b.edges.len() as u32 - 1
                });
                b.coedges.push(Coedge {
                    edge: e,
                    forward: a == key.0,
                });
                cs.push(b.coedges.len() as u32 - 1);
            }
            b.loops.push(Loop {
                outer: true,
                coedges: cs,
            });
            b.faces.push(Face {
                surface: b.surfaces.len() as u32 - 1,
                same_sense: true,
                loops: vec![b.loops.len() as u32 - 1],
            });
        }
        b.shells.push(Shell {
            faces: (0..6).collect(),
            closed: true,
        });
        b
    }

    #[test]
    fn cube_is_valid() {
        let b = cube();
        assert_eq!(b.edges.len(), 12);
        assert_eq!(check(&b, 1e-6), vec![]);
    }

    #[test]
    fn flipped_coedge_is_found() {
        let mut b = cube();
        b.coedges[0].forward = !b.coedges[0].forward;
        let issues = check(&b, 1e-6);
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, Issue::NotManifold { .. }))
        );
        assert!(issues.iter().any(|i| matches!(i, Issue::LoopGap { .. })));
    }

    #[test]
    fn moved_vertex_is_found() {
        let mut b = cube();
        b.vertices[7].p = Point3::new(1.0, 1.0, 1.001);
        let issues = check(&b, 1e-6);
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, Issue::VertexOffCurve { .. }))
        );
        assert!(
            !issues
                .iter()
                .any(|i| matches!(i, Issue::NotManifold { .. }))
        );
    }

    #[test]
    fn missing_face_breaks_closed_shell() {
        let mut b = cube();
        b.shells[0].faces.pop();
        let n = check(&b, 1e-6)
            .iter()
            .filter(|i| matches!(i, Issue::NotManifold { .. }))
            .count();
        assert_eq!(n, 4);
    }
}
