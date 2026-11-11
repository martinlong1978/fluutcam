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

use fluut_cam::drive::is_horizontal_plane;
use fluut_cam::geometry::{BodyGeom, SetupGeometry, edge_id, face_id};
use fluut_geom::{Aabb3, Curve3, Point2, Point3, Surface};
use fluut_path::{Motion, Segment, ToolpathResult, linearize};
use fluut_topo::BrepBody;
use fluut_topo::trim::{loop_polyline, winding};
use std::collections::BTreeMap;

/// A face mesh: triangles with a pick index per vertex.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Index into [`SetupScene::faces`] for every vertex.
    pub face: Vec<u32>,
    pub indices: Vec<u32>,
    /// Vertex range `[start, end)` of each face, by pick index.
    pub ranges: Vec<[u32; 2]>,
}

/// A line list: each pair of points is one segment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lines {
    pub points: Vec<[f32; 3]>,
    /// One value per point (both ends of a segment carry the same value).
    pub tag: Vec<u32>,
    /// Edge pick index + 1 per point, 0 for none; empty for toolpaths.
    pub pick: Vec<u32>,
    /// Point range `[start, end)` of each pickable edge, by pick index.
    pub ranges: Vec<[u32; 2]>,
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

    /// A pickable edge: the next pick index.
    fn edge(&mut self, pts: &[[f32; 3]], tag: u32) {
        let start = self.points.len() as u32;
        self.polyline(pts, tag);
        let index = self.ranges.len() as u32;
        self.pick.resize(self.points.len(), index + 1);
        self.ranges.push([start, self.points.len() as u32]);
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

/// What a pick filter needs to know about a face.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum FaceKind {
    Plane,
    Cylinder,
    #[default]
    Other,
}

impl FaceKind {
    fn of(s: &Surface) -> FaceKind {
        match s {
            Surface::Plane { .. } => FaceKind::Plane,
            Surface::Cylinder { .. } => FaceKind::Cylinder,
            _ => FaceKind::Other,
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
    /// By pick index.
    pub face_kinds: Vec<FaceKind>,
    /// Edge and wire curve ids (`e-<stem>.<i>`) by pick index.
    pub edge_ids: Vec<String>,
    /// Closed loops of edges, by pick index: loops of horizontal planes
    /// first, then other face loops and closed chains of wire curves.
    pub edge_loops: Vec<Vec<u32>>,
    pub stock: Option<Bounds>,
    pub bounds: Option<Bounds>,
    /// Model vertices and arc centres the work zero can snap to.
    pub snap: Vec<SnapPoint>,
}

/// A point of the model the work zero snaps to (S 08 §8.4): a vertex of a
/// B-rep body or wire, or the centre of a circle or arc.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SnapPoint {
    /// In the Setup's work frame W.
    pub at: [f32; 3],
    /// In the model frame M, mm: what a `point` zero holds.
    pub model: [f64; 3],
    /// Rotary and indexed Setups: its X in the aligned frame, which is what
    /// an `at` zero holds.
    pub axis_x: f64,
}

/// The most snap points a scene keeps; a model with more is a mesh or a
/// very large part, and the ones kept are the first.
pub const MAX_SNAP: usize = 4000;

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
        let (mut flat, mut other) = (Vec::new(), Vec::new());
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
                        let surface = &b.surfaces[b.faces[i as usize].surface as usize];
                        s.face_kinds.push(FaceKind::of(surface));
                        let start = s.mesh.positions.len() as u32;
                        face_mesh(b, i, pick, &mut s.mesh);
                        s.mesh.ranges.push([start, s.mesh.positions.len() as u32]);
                    }
                    let base = s.edge_ids.len() as u32;
                    let sharp = b.from_mesh.then(|| sharp_edges(b));
                    for e in 0..b.edges.len() as u32 {
                        // A mesh draws only its creases and borders; the
                        // others keep their pick index but no line.
                        let pts: Vec<[f32; 3]> = if sharp.as_ref().is_none_or(|s| s[e as usize]) {
                            b.edge_points(e, CHORD).into_iter().map(f).collect()
                        } else {
                            Vec::new()
                        };
                        s.edge_ids.push(edge_id(&body.id, e));
                        s.edges.edge(&pts, 0);
                    }
                    brep_loops(b, base, &mut flat, &mut other);
                }
                BodyGeom::Wire(w) => {
                    let base = s.edge_ids.len() as u32;
                    let mut ends = Vec::with_capacity(w.curves.len());
                    for (i, c) in w.curves.iter().enumerate() {
                        let pts: Vec<[f32; 3]> = c
                            .curve
                            .tessellate(c.range, CHORD)
                            .into_iter()
                            .map(|(_, p)| f(p))
                            .collect();
                        s.edge_ids.push(edge_id(&body.id, i as u32));
                        s.edges.edge(&pts, 1);
                        ends.push(pts.first().copied().zip(pts.last().copied()));
                    }
                    wire_loops(&ends, base, &mut other);
                }
            }
        }
        flat.extend(other);
        s.edge_loops = flat;
        let b = Bounds::of(&s.mesh.positions)
            .union(&Bounds::of(&s.edges.points))
            .union(&s.stock.unwrap_or(Bounds::EMPTY));
        s.bounds = (!b.is_empty()).then_some(b);
        s
    }
}

/// The vertices and circle or arc centres of the exact bodies of `g`, in
/// the frame of `g`, without repeats. Mesh bodies are left out: every
/// triangle corner would be one.
pub fn snap_points(g: &SetupGeometry) -> Vec<[f64; 3]> {
    let mut out: Vec<[f64; 3]> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |p: Point3| {
        let key = [p.x(), p.y(), p.z()].map(|v| (v * 1e4).round() as i64);
        if out.len() < MAX_SNAP && seen.insert(key) {
            out.push([p.x(), p.y(), p.z()]);
        }
    };
    for body in &g.bodies {
        match &body.geom {
            BodyGeom::Brep(b) if !b.from_mesh => {
                for v in &b.vertices {
                    add(v.p);
                }
                for c in &b.curves {
                    if let Curve3::Circle { frame, .. } = c {
                        add(frame.origin);
                    }
                }
            }
            BodyGeom::Wire(w) => {
                for c in &w.curves {
                    if let Curve3::Circle { frame, .. } = &c.curve {
                        add(frame.origin);
                    }
                    let pts = c.curve.tessellate(c.range, CHORD);
                    if let (Some(a), Some(z)) = (pts.first(), pts.last()) {
                        add(a.1);
                        add(z.1);
                    }
                }
            }
            BodyGeom::Brep(_) => {}
        }
    }
    out
}

/// Cosine of the angle (30 degrees) between the normals of two faces beyond
/// which the edge between them is drawn on a mesh body.
const MESH_CREASE_COS: f64 = 0.866_025_403_784_438_6;

/// For each edge of a mesh body: not between two faces that continue each
/// other (an open, shared-by-many or creased edge).
fn sharp_edges(b: &BrepBody) -> Vec<bool> {
    let mut faces: Vec<[u32; 2]> = vec![[u32::MAX; 2]; b.edges.len()];
    let mut uses = vec![0u32; b.edges.len()];
    for (fi, face) in b.faces.iter().enumerate() {
        for &l in &face.loops {
            for &c in &b.loops[l as usize].coedges {
                let e = b.coedges[c as usize].edge as usize;
                if uses[e] < 2 {
                    faces[e][uses[e] as usize] = fi as u32;
                }
                uses[e] += 1;
            }
        }
    }
    let normal = |f: u32| match &b.surfaces[b.faces[f as usize].surface as usize] {
        Surface::Plane { frame } => Some(frame.z.get()),
        _ => None,
    };
    let cos = MESH_CREASE_COS;
    (0..b.edges.len())
        .map(|e| {
            if uses[e] != 2 {
                return true;
            }
            match (normal(faces[e][0]), normal(faces[e][1])) {
                (Some(a), Some(c)) => a.dot(c) < cos,
                _ => true,
            }
        })
        .collect()
}

/// The face loops of a B-rep with more than one edge, as pick indices.
fn brep_loops(b: &BrepBody, base: u32, flat: &mut Vec<Vec<u32>>, other: &mut Vec<Vec<u32>>) {
    for face in &b.faces {
        let horizontal = is_horizontal_plane(&b.surfaces[face.surface as usize]);
        for &l in &face.loops {
            let edges: Vec<u32> = b.loops[l as usize]
                .coedges
                .iter()
                .map(|&c| base + b.coedges[c as usize].edge)
                .collect();
            if edges.len() > 1 {
                if horizontal { &mut *flat } else { &mut *other }.push(edges);
            }
        }
    }
}

/// A curve end, to the thousandth of a mm.
fn end_key(p: [f32; 3]) -> [i64; 3] {
    p.map(|v| (f64::from(v) * 1000.0).round() as i64)
}

/// Closed chains of wire curves whose ends meet, by walking from each
/// unused curve. `ends` are `(first, last)` of each curve.
fn wire_loops(ends: &[Option<([f32; 3], [f32; 3])>], base: u32, out: &mut Vec<Vec<u32>>) {
    let mut at: BTreeMap<[i64; 3], Vec<usize>> = BTreeMap::new();
    for (i, e) in ends.iter().enumerate() {
        if let Some((a, b)) = e {
            at.entry(end_key(*a)).or_default().push(i);
            at.entry(end_key(*b)).or_default().push(i);
        }
    }
    let mut used = vec![false; ends.len()];
    for start in 0..ends.len() {
        let Some((first, mut tip)) = ends[start] else {
            continue;
        };
        if used[start] || end_key(first) == end_key(tip) {
            continue;
        }
        let goal = end_key(first);
        let mut chain = vec![start];
        used[start] = true;
        while end_key(tip) != goal {
            let next = at[&end_key(tip)]
                .iter()
                .copied()
                .find(|&i| !used[i] && ends[i].is_some());
            let Some(n) = next else {
                break;
            };
            let Some((a, b)) = ends[n] else {
                break;
            };
            used[n] = true;
            chain.push(n);
            tip = if end_key(a) == end_key(tip) { b } else { a };
        }
        if end_key(tip) == goal && chain.len() > 1 {
            out.push(chain.into_iter().map(|i| base + i as u32).collect());
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_loops_find_closed_chains_and_skip_open_ones() {
        let p = |x: f32, y: f32| [x, y, 0.0];
        // A square in shuffled order, one curve reversed, and an open line.
        let ends = [
            Some((p(0.0, 0.0), p(1.0, 0.0))),
            Some((p(1.0, 1.0), p(0.0, 1.0))),
            Some((p(1.0, 1.0), p(1.0, 0.0))),
            Some((p(0.0, 1.0), p(0.0, 0.0))),
            Some((p(5.0, 5.0), p(6.0, 5.0))),
        ];
        let mut out = Vec::new();
        wire_loops(&ends, 10, &mut out);
        assert_eq!(out.len(), 1);
        let mut l = out[0].clone();
        l.sort_unstable();
        assert_eq!(l, [10, 11, 12, 13]);
    }

    #[test]
    fn edge_ranges_cover_the_points_of_their_edge() {
        let mut l = Lines::default();
        l.edge(&[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]], 0);
        l.edge(&[[0.0, 1.0, 0.0], [0.0, 2.0, 0.0]], 1);
        assert_eq!(l.ranges, [[0, 4], [4, 6]]);
        assert_eq!(l.pick, [1, 1, 1, 1, 2, 2]);
        assert_eq!(l.pick.len(), l.points.len());
    }

    #[test]
    fn mesh_bodies_draw_only_creases_and_borders() {
        use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry};
        use fluut_topo::MeshBody;
        // A flat quad of two triangles, and a third triangle folded up
        // from its far edge.
        let p = |x, y, z| Point3::new(x, y, z);
        let m = MeshBody {
            name: "m".into(),
            tol_model: 1e-4,
            positions: vec![
                p(0.0, 0.0, 0.0),
                p(1.0, 0.0, 0.0),
                p(1.0, 1.0, 0.0),
                p(0.0, 1.0, 0.0),
                p(0.5, 1.0, 1.0),
            ],
            tris: vec![[0, 1, 2], [0, 2, 3], [3, 2, 4]],
            neighbours: Vec::new(),
        };
        let mut b = fluut_topo::mesh::to_breps(&m).remove(0);
        fluut_topo::pcurve::compute(&mut b);
        let sharp = sharp_edges(&b);
        assert_eq!(sharp.len(), 7);
        // The diagonal 0-2 is flat between its two triangles; the edge
        // 2-3 is a crease; the other five are borders.
        assert_eq!(sharp.iter().filter(|&&s| !s).count(), 1);
        let g = SetupGeometry {
            bodies: vec![SetupBody {
                id: "b-1".into(),
                geom: BodyGeom::Brep(b),
            }],
        };
        let s = SetupScene::new(&g, None);
        assert_eq!(s.faces.len(), 3);
        assert_eq!(s.edge_ids.len(), 7);
        assert_eq!(s.edges.ranges.len(), 7);
        assert_eq!(s.edges.segment_count(), 6);
    }
}
