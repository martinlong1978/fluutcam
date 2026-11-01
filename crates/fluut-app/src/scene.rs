// SPDX-License-Identifier: MIT OR Apache-2.0

//! Display geometry (D13 §5.1): what the viewport draws for a Setup and its
//! toolpaths, in the Setup's work frame W, in `f32`.
//!
//! Built on the worker thread, so the UI thread only uploads buffers. Display
//! values never reach an output, so `f32` and coarse sampling are fine here.
//!
//! Faces are sampled on a (u, v) grid refined near the trim loops and kept
//! where the loops' polylines contain the sample (`fluut_topo::trim`). Their
//! borders are therefore stepped at the finest grid level; the exact edges
//! are drawn on top. A trimmed tessellation (D03 §7) replaces this later.

use fluut_cam::geometry::{BodyGeom, SetupGeometry, face_id};
use fluut_geom::{Aabb3, Point2, Point3};
use fluut_path::{Motion, Segment, ToolpathResult, linearize};
use fluut_topo::BrepBody;
use fluut_topo::trim::{loop_polyline, winding};

/// A face mesh: triangles with a pick index per vertex.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Index into [`SetupScene::faces`] for every vertex.
    pub face: Vec<u32>,
    pub indices: Vec<u32>,
}

/// A line list: each pair of points is one segment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lines {
    pub points: Vec<[f32; 3]>,
    /// One value per point (both ends of a segment carry the same value).
    pub tag: Vec<u32>,
}

impl Lines {
    fn push(&mut self, a: [f32; 3], b: [f32; 3], tag: u32) {
        self.points.extend([a, b]);
        self.tag.extend([tag, tag]);
    }

    fn polyline(&mut self, pts: &[[f32; 3]], tag: u32) {
        for w in pts.windows(2) {
            self.push(w[0], w[1], tag);
        }
    }

    pub fn segment_count(&self) -> usize {
        self.points.len() / 2
    }
}

/// An axis-aligned box in `f32`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds {
    pub const EMPTY: Bounds = Bounds {
        min: [f32::INFINITY; 3],
        max: [f32::NEG_INFINITY; 3],
    };

    pub fn is_empty(&self) -> bool {
        (0..3).any(|i| self.min[i] > self.max[i])
    }

    pub fn add(&mut self, p: [f32; 3]) {
        for (i, v) in p.into_iter().enumerate() {
            self.min[i] = self.min[i].min(v);
            self.max[i] = self.max[i].max(v);
        }
    }

    pub fn union(mut self, o: &Bounds) -> Bounds {
        if !o.is_empty() {
            self.add(o.min);
            self.add(o.max);
        }
        self
    }

    fn of(points: &[[f32; 3]]) -> Bounds {
        let mut b = Bounds::EMPTY;
        for p in points {
            b.add(*p);
        }
        b
    }

    fn from_aabb(a: &Aabb3) -> Bounds {
        Bounds {
            min: f(a.min),
            max: f(a.max),
        }
    }
}

/// Everything the viewport draws for one Setup, except toolpaths.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupScene {
    pub mesh: Mesh,
    /// B-rep edges (tag 0) and wire curves (tag 1).
    pub edges: Lines,
    /// Face ids (`f-<stem>.<i>`) by pick index.
    pub faces: Vec<String>,
    pub stock: Option<Bounds>,
    pub bounds: Option<Bounds>,
}

/// Motion classes for toolpath colours (S 08 §8.9 "by motion type").
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum MotionClass {
    Rapid = 0,
    Cut = 1,
    Plunge = 2,
    Lead = 3,
}

impl MotionClass {
    pub fn of(m: Motion) -> MotionClass {
        match m {
            Motion::Rapid | Motion::Retract => MotionClass::Rapid,
            Motion::Cut | Motion::Link => MotionClass::Cut,
            Motion::Plunge | Motion::Ramp => MotionClass::Plunge,
            Motion::LeadIn | Motion::LeadOut => MotionClass::Lead,
        }
    }
}

/// A toolpath as lines tagged with their [`MotionClass`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolpathLines {
    pub lines: Lines,
    pub bounds: Option<Bounds>,
}

/// Chord for edge polylines and linearised arcs, mm.
const CHORD: f64 = 0.02;
/// Cells along the longer side of a face's (u, v) box, before refinement.
const GRID: usize = 24;
/// Refinement levels near the trim loops (each halves the cell).
const REFINE: u32 = 3;

fn f(p: Point3) -> [f32; 3] {
    [p.x() as f32, p.y() as f32, p.z() as f32]
}

impl ToolpathLines {
    pub fn new(r: &ToolpathResult) -> ToolpathLines {
        let mut lines = Lines::default();
        for s in r.segments() {
            let class = s
                .motion()
                .map_or(MotionClass::Cut, |m| MotionClass::of(m.motion));
            let parts = match s {
                Segment::Arc { .. } | Segment::Helix { .. } => linearize(s, CHORD),
                _ => vec![s.clone()],
            };
            for p in &parts {
                if let (Some(a), Some(b)) = (p.start(), p.end()) {
                    lines.push(f(a.xyz), f(b.xyz), class as u32);
                }
            }
        }
        let b = Bounds::of(&lines.points);
        ToolpathLines {
            bounds: (!b.is_empty()).then_some(b),
            lines,
        }
    }
}

impl SetupScene {
    /// The scene of a Setup's bodies, already in its work frame, with its
    /// stock box.
    pub fn new(geometry: &SetupGeometry, stock: Option<&Aabb3>) -> SetupScene {
        let mut s = SetupScene {
            stock: stock.map(Bounds::from_aabb),
            ..SetupScene::default()
        };
        for body in &geometry.bodies {
            match &body.geom {
                BodyGeom::Brep(b) => {
                    // Moving a body into W drops its p-curves; the trim
                    // polylines need them.
                    let mut owned;
                    let b = if b.pcurves.len() == b.coedges.len() {
                        b
                    } else {
                        owned = b.clone();
                        fluut_topo::pcurve::compute(&mut owned);
                        &owned
                    };
                    for i in 0..b.faces.len() as u32 {
                        let pick = s.faces.len() as u32;
                        s.faces.push(face_id(&body.id, i));
                        face_mesh(b, i, pick, &mut s.mesh);
                    }
                    for e in 0..b.edges.len() as u32 {
                        let pts: Vec<[f32; 3]> =
                            b.edge_points(e, CHORD).into_iter().map(f).collect();
                        s.edges.polyline(&pts, 0);
                    }
                }
                BodyGeom::Wire(w) => {
                    for c in &w.curves {
                        let pts: Vec<[f32; 3]> = c
                            .curve
                            .tessellate(c.range, CHORD)
                            .into_iter()
                            .map(|(_, p)| f(p))
                            .collect();
                        s.edges.polyline(&pts, 1);
                    }
                }
            }
        }
        let b = Bounds::of(&s.mesh.positions)
            .union(&Bounds::of(&s.edges.points))
            .union(&s.stock.unwrap_or(Bounds::EMPTY));
        s.bounds = (!b.is_empty()).then_some(b);
        s
    }
}

/// The trim region of a face in (u, v).
enum Region {
    /// Closed loop polylines; a point is inside when an odd number of them
    /// wind round it.
    Loops(Vec<Vec<Point2>>),
    /// A loop wraps round the surface (a band): the whole (u, v) box.
    Band,
}

impl Region {
    fn contains(&self, p: Point2) -> bool {
        match self {
            Region::Band => true,
            Region::Loops(ls) => ls.iter().filter(|l| winding(l, p) != 0).count() % 2 == 1,
        }
    }
}

/// Adds the triangles of face `fi` to `mesh`. Faces whose loops have no
/// p-curves are skipped (their edges still show).
fn face_mesh(b: &BrepBody, fi: u32, pick: u32, mesh: &mut Mesh) {
    let face = &b.faces[fi as usize];
    let Some(loops) = face
        .loops
        .iter()
        .map(|&l| loop_polyline(b, fi, l))
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    let pts: Vec<Point2> = loops
        .iter()
        .flat_map(|l| l.points.iter().copied())
        .collect();
    if pts.len() < 3 {
        return;
    }
    let region = if loops.iter().any(|l| l.wraps) {
        Region::Band
    } else {
        Region::Loops(
            loops
                .into_iter()
                .map(|l| l.points)
                .filter(|p| p.len() >= 3)
                .collect(),
        )
    };
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in &pts {
        lo = [lo[0].min(p.x()), lo[1].min(p.y())];
        hi = [hi[0].max(p.x()), hi[1].max(p.y())];
    }
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    if !(span > 0.0 && span.is_finite()) {
        return;
    }
    let cell = span / GRID as f64;
    let (nu, nv) = (
        ((hi[0] - lo[0]) / cell).ceil().max(1.0) as usize,
        ((hi[1] - lo[1]) / cell).ceil().max(1.0) as usize,
    );
    let surface = &b.surfaces[face.surface as usize];
    let flip = !face.same_sense;
    let mut emit = |u0: f64, v0: f64, du: f64, dv: f64| {
        let corners = [(u0, v0), (u0 + du, v0), (u0 + du, v0 + dv), (u0, v0 + dv)];
        for tri in [[0, 1, 2], [0, 2, 3]] {
            let (cu, cv) = tri.iter().fold((0.0, 0.0), |(a, c), &k| {
                (a + corners[k].0 / 3.0, c + corners[k].1 / 3.0)
            });
            if !region.contains(Point2::new(cu, cv)) {
                continue;
            }
            for &k in &tri {
                let (u, v) = corners[k];
                let n = surface.normal(u, v).map_or([0.0, 0.0, 1.0], |n| {
                    let n = n.get();
                    let s = if flip { -1.0 } else { 1.0 };
                    [(s * n.x()) as f32, (s * n.y()) as f32, (s * n.z()) as f32]
                });
                mesh.indices.push(mesh.positions.len() as u32);
                mesh.positions.push(f(surface.eval(u, v)));
                mesh.normals.push(n);
                mesh.face.push(pick);
            }
        }
    };
    for i in 0..nu {
        for j in 0..nv {
            let (u0, v0) = (lo[0] + i as f64 * cell, lo[1] + j as f64 * cell);
            refine(&region, u0, v0, cell, REFINE, &mut emit);
        }
    }
}

/// Emits the cell whole when its corners and centre agree, else splits it
/// into four, down to `depth` levels.
fn refine(
    region: &Region,
    u0: f64,
    v0: f64,
    size: f64,
    depth: u32,
    emit: &mut impl FnMut(f64, f64, f64, f64),
) {
    let samples = [
        (u0, v0),
        (u0 + size, v0),
        (u0, v0 + size),
        (u0 + size, v0 + size),
        (u0 + size / 2.0, v0 + size / 2.0),
    ];
    let inside = samples
        .iter()
        .filter(|&&(u, v)| region.contains(Point2::new(u, v)))
        .count();
    if depth == 0 || inside == samples.len() {
        // `emit` keeps the triangles whose centroid is inside.
        emit(u0, v0, size, size);
    } else if inside > 0 || touches_boundary(region, u0, v0, size) {
        split(region, u0, v0, size, depth, emit);
    }
}

fn split(
    region: &Region,
    u0: f64,
    v0: f64,
    size: f64,
    depth: u32,
    emit: &mut impl FnMut(f64, f64, f64, f64),
) {
    let h = size / 2.0;
    for (du, dv) in [(0.0, 0.0), (h, 0.0), (0.0, h), (h, h)] {
        refine(region, u0 + du, v0 + dv, h, depth - 1, emit);
    }
}

/// Whether a loop vertex lies in the cell (a small hole or spike between the
/// samples).
fn touches_boundary(region: &Region, u0: f64, v0: f64, size: f64) -> bool {
    match region {
        Region::Band => false,
        Region::Loops(ls) => ls
            .iter()
            .flatten()
            .any(|p| p.x() >= u0 && p.x() <= u0 + size && p.y() >= v0 && p.y() <= v0 + size),
    }
}
