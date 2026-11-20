// SPDX-License-Identifier: MIT OR Apache-2.0

//! Re-import and entity identity (spec 03 §3.5).
//!
//! When a geometry file changes, its bodies, faces and edges are matched to
//! the ones loaded before:
//!
//! 0. by name, where both files name the entity (a linked `.craftx`
//!    model, or STEP face names; D06 §5.1);
//! 1. by persistent id: the same id (D06 §1.1) with an equal signature;
//! 2. by signature: surface kind, parameters and bounds within the
//!    modelling tolerance;
//! 3. by surface alone, when exactly one candidate is left (a face whose
//!    neighbours moved keeps its surface but not its bounds).
//!
//! The STEP reader gives no AP242 persistent ids, so the D06 id serves as
//! the persistent id. Selections of matched entities are rewritten;
//! selections of unmatched ones are kept and reported, never dropped.
//! Toolpath picks that store a name are re-found by that name with RFC
//! 0001's outcomes ([`PickReport`], [`crate::names`]).

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
    /// Names the file gives (not derived ones), by index.
    face_names: Vec<Option<String>>,
    edge_names: Vec<Option<String>>,
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
            face_names: (0..x.faces.len() as u32)
                .map(|f| x.names.face(f).map(|n| n.name.clone()))
                .collect(),
            edge_names: (0..x.edges.len() as u32)
                .map(|e| {
                    x.names
                        .edge(e)
                        .filter(|n| !n.derived)
                        .map(|n| n.name.clone())
                })
                .collect(),
            name: x.name.clone(),
            tol: x.tol_model.max(1e-6),
        },
        BodyGeom::Wire(w) => BodySigs {
            face_names: Vec::new(),
            edge_names: Vec::new(),
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

/// Matches entities of one list: `old[i]` → `new[j]`, by name (where
/// both name the entity; names are unique in a body), then three passes.
fn match_list(
    old: &[Sig],
    new: &[Sig],
    names: (&[Option<String>], &[Option<String>]),
    same_ids: bool,
    tol: f64,
) -> Vec<Option<(usize, MatchedBy)>> {
    let mut out = vec![None; old.len()];
    let mut taken = vec![false; new.len()];
    for (i, n) in names.0.iter().enumerate().take(old.len()) {
        let Some(n) = n else { continue };
        if let Some(j) = (0..new.len())
            .find(|&j| !taken[j] && names.1.get(j).and_then(Option::as_ref) == Some(n))
        {
            out[i] = Some((j, MatchedBy::Name));
            taken[j] = true;
        }
    }
    if same_ids {
        for (i, s) in old.iter().enumerate() {
            if out[i].is_none()
                && let Some(n) = new.get(i)
                && !taken[i]
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
        let names = (&old_s[i].face_names[..], &new_s[j].face_names[..]);
        for (k, r) in match_list(&old_s[i].faces, &new_s[j].faces, names, same, tol)
            .into_iter()
            .enumerate()
        {
            if let Some((l, by)) = r {
                m.ids
                    .insert(face_id(&ob.id, k as u32), (face_id(&nb.id, l as u32), by));
            }
        }
        let names = (&old_s[i].edge_names[..], &new_s[j].edge_names[..]);
        for (k, r) in match_list(&old_s[i].edges, &new_s[j].edges, names, same, tol)
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

/// How one named toolpath pick was found in the new file (spec 03 §3.5
/// item 4).
#[derive(Clone, PartialEq, Debug)]
pub struct PickReport {
    /// The toolpath.
    pub owner: String,
    /// The selection field, `geometry.drive_faces`, …
    pub field: String,
    /// The pick's id before the update.
    pub id: String,
    /// The toolpath's name, for the report.
    pub owner_name: String,
    /// The name it is found by.
    pub name: String,
    pub outcome: crate::names::Outcome,
    /// Its ids after the update: one, several for a split, the old id
    /// for a missing pick.
    pub ids: Vec<String>,
    /// Where the entity was (middle of its box), for a missing pick.
    pub at: Option<[f64; 3]>,
}

impl PickReport {
    /// `face` or `edge`.
    pub fn what(&self) -> &'static str {
        if self.id.starts_with("e-") {
            "edge"
        } else {
            "face"
        }
    }

    /// The outcome in plain words, as the update report shows it (the
    /// window has the same words in its message catalog).
    pub fn text(&self) -> String {
        use crate::names::Outcome as O;
        let (w, n) = (self.what(), &self.name);
        match self.outcome {
            O::Exact => format!("the {w} `{n}` was found by its name"),
            O::Merged => {
                format!("the {w} `{n}` merged into another {w}; the pick follows it")
            }
            O::Split => format!(
                "the {w} `{n}` is now {} pieces; the pick uses all of them, check what the toolpath cuts",
                self.ids.len()
            ),
            O::ByGeometry => {
                format!("the {w} `{n}` was found by its shape only; check this toolpath")
            }
            O::Missing => {
                let place = self.at.map_or_else(String::new, |p| {
                    format!(", last at ({:.1}, {:.1}, {:.1}) mm", p[0], p[1], p[2])
                });
                format!(
                    "the {w} `{n}` is not in the model any more{place}; the toolpath is broken until you pick it again"
                )
            }
        }
    }

    /// One line of the update report: outcome, toolpath, field and words.
    pub fn line(&self) -> String {
        format!(
            "{:<11} {} \"{}\" ({}): {}",
            self.outcome.as_str(),
            self.owner,
            self.owner_name,
            self.field,
            self.text()
        )
    }
}

/// The outcome of a re-import.
#[derive(Clone, PartialEq, Debug)]
pub struct ReimportReport {
    pub source: String,
    /// The source's file, as the project names it.
    pub path: String,
    /// The linked design's revision before and after, for a linked model.
    pub revision_before: Option<String>,
    pub revision: Option<String>,
    /// The file's hash did not change.
    pub unchanged: bool,
    pub matching: Matching,
    /// Kept in the project and reported (spec 03 §3.5).
    pub unmatched: Vec<Unmatched>,
    /// Every named toolpath pick of the source, with its outcome.
    pub picks: Vec<PickReport>,
    /// Import findings for the new file and one warning per unmatched
    /// selection.
    pub diagnostics: Vec<fluut_model::Diagnostic>,
}

impl ReimportReport {
    /// Toolpaths left broken: a named pick is missing.
    pub fn broken(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .picks
            .iter()
            .filter(|p| p.outcome == crate::names::Outcome::Missing)
            .map(|p| p.owner.as_str())
            .collect();
        v.dedup();
        v
    }

    /// How many picks had each outcome, in outcome order, leaving out the
    /// outcomes no pick had.
    pub fn counts(&self) -> Vec<(crate::names::Outcome, usize)> {
        use crate::names::Outcome as O;
        [O::Exact, O::Merged, O::Split, O::ByGeometry, O::Missing]
            .into_iter()
            .map(|o| (o, self.picks.iter().filter(|p| p.outcome == o).count()))
            .filter(|(_, n)| *n > 0)
            .collect()
    }

    /// The update report as text: a heading, one line per named pick and
    /// a summary (`fluut model update`, the script's `text`).
    pub fn text(&self) -> String {
        let mut out: Vec<String> = Vec::new();
        out.push(match (&self.revision_before, &self.revision) {
            (Some(a), Some(b)) if a != b => {
                format!("{} {}: revision {a} -> {b}", self.source, self.path)
            }
            (_, Some(b)) => format!("{} {}: revision {b}", self.source, self.path),
            _ => format!("{} {}", self.source, self.path),
        });
        if self.unchanged {
            out.push("  the file has not changed".into());
        }
        out.extend(self.picks.iter().map(|p| format!("  {}", p.line())));
        out.extend(self.unmatched.iter().map(|u| {
            format!(
                "  unmatched   {} ({}): `{}` is not in the new file; the selection is kept",
                u.owner, u.field, u.id
            )
        }));
        if self.picks.is_empty() {
            out.push("  no named picks".into());
        } else {
            let counts: Vec<String> = self
                .counts()
                .iter()
                .map(|(o, n)| format!("{n} {}", o.as_str()))
                .collect();
            let broken = self.broken().len();
            out.push(format!(
                "  {} picks: {}; {} toolpath{} broken",
                self.picks.len(),
                counts.join(", "),
                broken,
                if broken == 1 { "" } else { "s" }
            ));
        }
        let mut text = out.join("\n");
        text.push('\n');
        text
    }
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
