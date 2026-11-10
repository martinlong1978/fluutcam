// SPDX-License-Identifier: MIT OR Apache-2.0

//! B-rep body storage (design D03 §1).
//!
//! Bodies are immutable after import, so plain vectors indexed by `u32`
//! replace the slot maps of D03 §1; trim caches come later.

use fluut_geom::{Aabb3, Curve2, Curve3, Interval, Point3, Surface};

#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    pub p: Point3,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// Index into `curves`; `None` for a degenerate edge (cone apex).
    pub curve: Option<u32>,
    /// Curve parameters from `v0` to `v1` (in curve direction).
    pub range: Interval,
    pub v0: u32,
    pub v1: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Coedge {
    pub edge: u32,
    /// Traversed from `v0` to `v1`.
    pub forward: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    pub outer: bool,
    /// Coedges in loop order.
    pub coedges: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub surface: u32,
    /// The face normal is the surface normal (else reversed).
    pub same_sense: bool,
    pub loops: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shell {
    pub faces: Vec<u32>,
    pub closed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct BrepBody {
    pub name: String,
    pub tol_model: f64,
    pub surfaces: Vec<Surface>,
    pub curves: Vec<Curve3>,
    pub vertices: Vec<Vertex>,
    pub edges: Vec<Edge>,
    pub coedges: Vec<Coedge>,
    pub loops: Vec<Loop>,
    pub faces: Vec<Face>,
    pub shells: Vec<Shell>,
    /// P-curve of each coedge in its face's (u, v) plane, indexed like
    /// `coedges`; empty until computed (see [`crate::pcurve`]). A p-curve
    /// uses the parameter of its edge's 3-D curve over `Edge::range`, so
    /// `S(pc(t)) = C(t)`. `None` where it could not be computed.
    pub pcurves: Vec<Option<Curve2>>,
    /// Built from a triangle mesh: every face is one planar triangle, so
    /// views draw only the sharp edges.
    pub from_mesh: bool,
}

impl BrepBody {
    /// Start and end vertex of a coedge in its direction of travel.
    pub fn coedge_ends(&self, c: u32) -> (u32, u32) {
        let ce = &self.coedges[c as usize];
        let e = &self.edges[ce.edge as usize];
        if ce.forward {
            (e.v0, e.v1)
        } else {
            (e.v1, e.v0)
        }
    }

    /// Points along an edge, about `chord` apart where it curves.
    pub fn edge_points(&self, e: u32, chord: f64) -> Vec<Point3> {
        let ed = &self.edges[e as usize];
        match ed.curve {
            None => vec![self.vertices[ed.v0 as usize].p],
            Some(c) => self.curves[c as usize]
                .tessellate(ed.range, chord)
                .into_iter()
                .map(|(_, p)| p)
                .collect(),
        }
    }

    /// Bounding box of a face from its edges.
    ///
    /// Spike approximation: a face that bulges beyond its edges (a sphere
    /// cap) is under-estimated; D03 §4.1 bounds the surface patch as well.
    pub fn face_box(&self, f: u32, chord: f64) -> Aabb3 {
        let mut b = Aabb3::empty();
        for &l in &self.faces[f as usize].loops {
            for &c in &self.loops[l as usize].coedges {
                for p in self.edge_points(self.coedges[c as usize].edge, chord) {
                    b = b.with_point(p);
                }
            }
        }
        b
    }
}
