// SPDX-License-Identifier: MIT OR Apache-2.0

//! Re-import and entity identity (spec 03 §3.5).
//!
//! When a geometry file changes, its bodies, faces and edges are matched to
//! the ones loaded before:
//!
//! 1. by persistent id: the same id (D06 §1.1) with an equal signature;
//! 2. by signature: surface kind, parameters and bounds within the
//!    modelling tolerance;
//! 3. by surface alone, when exactly one candidate is left (a face whose
//!    neighbours moved keeps its surface but not its bounds).
//!
//! The STEP reader gives no AP242 persistent ids or face names yet, so the
//! D06 id serves as the persistent id. Selections of matched entities are
//! rewritten; selections of unmatched ones are kept and reported, never
//! dropped.

use fluut_cam::geometry::{BodyGeom, SetupBody, edge_id, face_id};
use fluut_geom::{Curve3, Interval, Point3, Surface};
use fluut_topo::BrepBody;
use std::collections::BTreeMap;

/// Chord for face bounds from edge samples, mm.
const CHORD: f64 = 1e-3;

/// How an old entity found its new one.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum MatchedBy {
    Id,
    Name,
    Signature,
    Surface,
}

/// Old id → new id and how, for every matched body, face and edge.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Matching {
    pub ids: BTreeMap<String, (String, MatchedBy)>,
}

impl Matching {
    pub fn new_id(&self, old: &str) -> Option<&str> {
        self.ids.get(old).map(|(n, _)| n.as_str())
    }

    /// The ids that change, for the remap edit.
    pub fn renames(&self) -> Vec<(String, String)> {
        self.ids
            .iter()
            .filter(|(o, (n, _))| o != &n)
            .map(|(o, (n, _))| (o.clone(), n.clone()))
            .collect()
    }
}

/// A geometric fingerprint: kind, parameters, and bounds.
#[derive(Clone, PartialEq, Debug)]
struct Sig {
    kind: u8,
    params: Vec<f64>,
    bounds: [f64; 6],
}

fn close(a: &[f64], b: &[f64], tol: f64) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

impl Sig {
    fn same(&self, o: &Sig, tol: f64) -> bool {
        self.same_shape(o, tol) && close(&self.bounds, &o.bounds, tol)
    }

    fn same_shape(&self, o: &Sig, tol: f64) -> bool {
        self.kind == o.kind && !self.params.is_empty() && close(&self.params, &o.params, tol)
    }
}

fn v3(p: Point3) -> [f64; 3] {
    [p.x(), p.y(), p.z()]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A direction with its sign fixed, so a flipped axis compares equal.
fn line_dir(d: [f64; 3]) -> [f64; 3] {
    let flip = d.iter().find(|x| x.abs() > 1e-12).is_some_and(|x| *x < 0.0);
    if flip { d.map(|x| -x) } else { d }
}

/// The point of the line through `o` along unit `d` closest to the origin.
fn foot(o: [f64; 3], d: [f64; 3]) -> [f64; 3] {
    let t = dot(o, d);
    [o[0] - t * d[0], o[1] - t * d[1], o[2] - t * d[2]]
}

fn bounds_of(points: impl Iterator<Item = Point3>) -> [f64; 6] {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in points {
        let q = v3(p);
        for i in 0..3 {
            b[i] = b[i].min(q[i]);
            b[i + 3] = b[i + 3].max(q[i]);
        }
    }
    b
}

fn curve_points(c: &Curve3, r: Interval) -> Vec<Point3> {
    match c {
        Curve3::Line { .. } => vec![c.eval(r.lo), c.eval(r.hi)],
        _ => c.tessellate(r, CHORD).into_iter().map(|(_, p)| p).collect(),
    }
}

fn face_sig(b: &BrepBody, f: usize) -> Sig {
    let face = &b.faces[f];
    let sense = if face.same_sense { 1.0 } else { -1.0 };
    let (kind, params) = match &b.surfaces[face.surface as usize] {
        Surface::Plane { frame } => {
            let n = v3_vec(frame.z.get()).map(|x| x * sense);
            (0, vec![n[0], n[1], n[2], dot(n, v3(frame.origin))])
        }
        Surface::Cylinder { frame, r } => {
            let d = line_dir(v3_vec(frame.z.get()));
            let p = foot(v3(frame.origin), d);
            (1, vec![d[0], d[1], d[2], p[0], p[1], p[2], *r, sense])
        }
        Surface::Cone {
            frame,
            r,
            semi_angle,
        } => {
            let d = v3_vec(frame.z.get());
            let o = v3(frame.origin);
            (
                2,
                vec![d[0], d[1], d[2], o[0], o[1], o[2], *r, *semi_angle, sense],
            )
        }
        Surface::Sphere { frame, r } => {
            let o = v3(frame.origin);
            (3, vec![o[0], o[1], o[2], *r, sense])
        }
        Surface::Torus {
            frame,
            major,
            minor,
        } => {
            let d = line_dir(v3_vec(frame.z.get()));
            let o = v3(frame.origin);
            (
                4,
                vec![d[0], d[1], d[2], o[0], o[1], o[2], *major, *minor, sense],
            )
        }
        // Free-form surfaces match by bounds only.
        _ => (5, Vec::new()),
    };
    Sig {
        kind,
        params,
        bounds: face_bounds(b, f),
    }
}

/// Bounds of face `f` from its edges.
pub(crate) fn face_bounds(b: &BrepBody, f: usize) -> [f64; 6] {
    let mut pts = Vec::new();
    for &l in &b.faces[f].loops {
        for &c in &b.loops[l as usize].coedges {
            let e = &b.edges[b.coedges[c as usize].edge as usize];
            match e.curve {
                Some(ci) => pts.extend(curve_points(&b.curves[ci as usize], e.range)),
                None => pts.push(b.vertices[e.v0 as usize].p),
            }
        }
    }
    bounds_of(pts.into_iter())
}

/// Bounds of a wire body's curves.
pub(crate) fn wire_bounds(w: &fluut_topo::WireBody) -> [f64; 6] {
    bounds_of(
        w.curves
            .iter()
            .flat_map(|c| curve_points(&c.curve, c.range)),
    )
}

fn v3_vec(v: fluut_geom::Vec3) -> [f64; 3] {
    [v.x(), v.y(), v.z()]
}

fn curve_sig(c: Option<&Curve3>, r: Interval, ends: (Point3, Point3)) -> Sig {
    let (kind, params) = match c {
        Some(Curve3::Line { origin, dir }) => {
            let d = line_dir(v3_vec(dir.get()));
            let p = foot(v3(*origin), d);
            (0, vec![d[0], d[1], d[2], p[0], p[1], p[2]])
        }
        Some(Curve3::Circle { frame, radius }) => {
            let d = line_dir(v3_vec(frame.z.get()));
            let o = v3(frame.origin);
            (1, vec![d[0], d[1], d[2], o[0], o[1], o[2], *radius])
        }
        Some(Curve3::Ellipse { frame, a, b }) => {
            let d = line_dir(v3_vec(frame.z.get()));
            let o = v3(frame.origin);
            (2, vec![d[0], d[1], d[2], o[0], o[1], o[2], *a, *b])
        }
        _ => (3, Vec::new()),
    };
    let pts = match c {
        Some(c) => curve_points(c, r),
        None => vec![ends.0, ends.1],
    };
    Sig {
        kind,
        params,
        bounds: bounds_of(pts.into_iter()),
    }
}

/// Signatures of a body's faces and edges (or wire curves), by index.
struct BodySigs {
    faces: Vec<Sig>,
    edges: Vec<Sig>,
    name: String,
    tol: f64,
}

fn sigs(b: &SetupBody) -> BodySigs {
    match &b.geom {
        BodyGeom::Brep(x) => BodySigs {
            faces: (0..x.faces.len()).map(|f| face_sig(x, f)).collect(),
            edges: x
                .edges
                .iter()
                .map(|e| {
                    let ends = (x.vertices[e.v0 as usize].p, x.vertices[e.v1 as usize].p);
                    curve_sig(e.curve.map(|c| &x.curves[c as usize]), e.range, ends)
                })
                .collect(),
            name: x.name.clone(),
            tol: x.tol_model.max(1e-6),
        },
        BodyGeom::Wire(w) => BodySigs {
            faces: Vec::new(),
            edges: w
                .curves
                .iter()
                .map(|c| {
                    let ends = (c.curve.eval(c.range.lo), c.curve.eval(c.range.hi));
                    curve_sig(Some(&c.curve), c.range, ends)
                })
                .collect(),
            name: w.name.clone(),
            tol: w.tol_model.max(1e-6),
        },
    }
}

/// Matches entities of one list: `old[i]` → `new[j]`, three passes.
fn match_list(
    old: &[Sig],
    new: &[Sig],
    same_ids: bool,
    tol: f64,
) -> Vec<Option<(usize, MatchedBy)>> {
    let mut out = vec![None; old.len()];
    let mut taken = vec![false; new.len()];
    if same_ids {
        for (i, s) in old.iter().enumerate() {
            if let Some(n) = new.get(i)
                && s.same(n, tol)
            {
                out[i] = Some((i, MatchedBy::Id));
                taken[i] = true;
            }
        }
    }
    for (i, s) in old.iter().enumerate() {
        if out[i].is_some() {
            continue;
        }
        if let Some(j) = (0..new.len()).find(|&j| !taken[j] && s.same(&new[j], tol)) {
            out[i] = Some((j, MatchedBy::Signature));
            taken[j] = true;
        }
    }
    for (i, s) in old.iter().enumerate() {
        if out[i].is_some() {
            continue;
        }
        let c: Vec<usize> = (0..new.len())
            .filter(|&j| !taken[j] && s.same_shape(&new[j], tol))
            .collect();
        // Unique among the new candidates, and no other old entity with
        // the same shape is left waiting for it.
        let rivals = old
            .iter()
            .enumerate()
            .filter(|(k, o)| *k != i && out[*k].is_none() && o.same_shape(s, tol))
            .count();
        if let [j] = c[..]
            && rivals == 0
        {
            out[i] = Some((j, MatchedBy::Surface));
            taken[j] = true;
        }
    }
    out
}

/// Matches the bodies of a source before and after re-import.
pub fn match_bodies(old: &[SetupBody], new: &[SetupBody]) -> Matching {
    let old_s: Vec<BodySigs> = old.iter().map(sigs).collect();
    let new_s: Vec<BodySigs> = new.iter().map(sigs).collect();
    let mut taken = vec![false; new.len()];
    let mut pairs: Vec<(usize, usize, MatchedBy)> = Vec::new();
    // Bodies: same id, then a unique same name, then most faces in common.
    for (i, b) in old.iter().enumerate() {
        if let Some(j) = new.iter().position(|n| n.id == b.id)
            && !taken[j]
        {
            taken[j] = true;
            pairs.push((i, j, MatchedBy::Id));
        }
    }
    for (i, os) in old_s.iter().enumerate() {
        if pairs.iter().any(|p| p.0 == i) || os.name.is_empty() {
            continue;
        }
        let c: Vec<usize> = (0..new.len())
            .filter(|&j| !taken[j] && new_s[j].name == os.name)
            .collect();
        if let [j] = c[..] {
            taken[j] = true;
            pairs.push((i, j, MatchedBy::Name));
        }
    }
    for (i, os) in old_s.iter().enumerate() {
        if pairs.iter().any(|p| p.0 == i) {
            continue;
        }
        let best = (0..new.len())
            .filter(|&j| !taken[j])
            .map(|j| {
                let tol = os.tol.max(new_s[j].tol);
                let n = os
                    .faces
                    .iter()
                    .chain(&os.edges)
                    .filter(|s| {
                        new_s[j]
                            .faces
                            .iter()
                            .chain(&new_s[j].edges)
                            .any(|t| s.same(t, tol))
                    })
                    .count();
                (n, std::cmp::Reverse(j))
            })
            .max();
        if let Some((n, std::cmp::Reverse(j))) = best
            && n > 0
        {
            taken[j] = true;
            pairs.push((i, j, MatchedBy::Signature));
        }
    }
    let mut m = Matching::default();
    for (i, j, by) in pairs {
        let (ob, nb) = (&old[i], &new[j]);
        m.ids.insert(ob.id.clone(), (nb.id.clone(), by));
        let tol = old_s[i].tol.max(new_s[j].tol);
        let same = ob.id == nb.id;
        for (k, r) in match_list(&old_s[i].faces, &new_s[j].faces, same, tol)
            .into_iter()
            .enumerate()
        {
            if let Some((l, by)) = r {
                m.ids
                    .insert(face_id(&ob.id, k as u32), (face_id(&nb.id, l as u32), by));
            }
        }
        for (k, r) in match_list(&old_s[i].edges, &new_s[j].edges, same, tol)
            .into_iter()
            .enumerate()
        {
            if let Some((l, by)) = r {
                m.ids
                    .insert(edge_id(&ob.id, k as u32), (edge_id(&nb.id, l as u32), by));
            }
        }
    }
    m
}

/// A selection whose entity was not found in the new file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Unmatched {
    /// The toolpath or Setup that holds the selection.
    pub owner: String,
    /// Where: `geometry.drive_faces`, `orientation`, …
    pub field: String,
    pub id: String,
}

/// The outcome of a re-import.
#[derive(Clone, PartialEq, Debug)]
pub struct ReimportReport {
    pub source: String,
    pub matching: Matching,
    /// Kept in the project and reported (spec 03 §3.5).
    pub unmatched: Vec<Unmatched>,
    /// Import findings for the new file and one warning per unmatched
    /// selection.
    pub diagnostics: Vec<fluut_model::Diagnostic>,
}

/// Every entity id the project refers to, with its owner and field.
pub fn references(p: &fluut_model::project::Project) -> Vec<Unmatched> {
    use fluut_model::project::{AxisPick, Orientation, StockSpec};
    let mut out = Vec::new();
    for s in &p.setups {
        let mut put = |field: &str, id: &str| {
            out.push(Unmatched {
                owner: s.id.clone(),
                field: field.to_owned(),
                id: id.to_owned(),
            })
        };
        for b in s
            .geometry
            .as_ref()
            .and_then(|g| g.bodies.as_ref())
            .into_iter()
            .flatten()
        {
            put("geometry", b);
        }
        let picks: Vec<&AxisPick> = match &s.orientation {
            Some(Orientation::ThreeAxis { tool_axis, x_axis }) => vec![tool_axis, x_axis],
            Some(Orientation::Rotary {
                rotary_axis,
                a_zero_toward,
            }) => std::iter::once(rotary_axis)
                .chain(a_zero_toward.as_ref())
                .collect(),
            None => Vec::new(),
        };
        for a in picks {
            if let AxisPick::Face(id) | AxisPick::Edge(id) = a {
                put("orientation", id);
            }
        }
        if let Some(StockSpec::Body { body }) = &s.stock {
            put("stock", body);
        }
        for t in &s.toolpaths {
            for (path, sel) in t.params.selections() {
                out.push(Unmatched {
                    owner: t.id.clone(),
                    field: path,
                    id: sel.id().to_owned(),
                });
            }
        }
    }
    out
}

/// Whether `id` names a body of `bodies` or one of their faces or edges.
pub fn belongs(id: &str, bodies: &[SetupBody]) -> bool {
    bodies.iter().any(|b| {
        let stem = b.id.strip_prefix("b-").unwrap_or(&b.id);
        id == b.id
            || id
                .strip_prefix("f-")
                .or_else(|| id.strip_prefix("e-"))
                .and_then(|r| r.rsplit_once('.'))
                .is_some_and(|(s, _)| s == stem)
    })
}
