// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading a `.craftx` file: manifest, part hashes, bodies, the face
//! table, parameters and metadata.

use crate::format::{self, PartKind, key};
use crate::zip::{Archive, Limits};
use crate::{Finding, Severity};
use fluut_geom::{Interval, Point2, Point3, Surface, Tol, UvBox};
use fluut_io_step::{MappedBody, RawEntity, RawModel};
use fluut_topo::names::edge_faces;
use fluut_topo::{BrepBody, EntityName, trim};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    pub limits: Limits,
}

/// The app that wrote the file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceApp {
    pub app: Option<String>,
    pub version: Option<String>,
}

/// One exposed design parameter (`params.json`), for display only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    /// As written: a number or a string.
    pub value: String,
    pub unit: Option<String>,
    pub expression: Option<String>,
}

/// Design metadata (`meta.json`) FluutCAM shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Meta {
    pub material: Option<String>,
    pub part_number: Option<String>,
}

/// A body read from `bodies/<index>.step`. Its `step.body.names` holds the
/// face and edge names.
#[derive(Debug, Clone)]
pub struct Body {
    pub index: u32,
    pub step: MappedBody,
}

/// A read exchange file.
#[derive(Debug, Clone)]
pub struct Exchange {
    pub source: SourceApp,
    /// The design's permanent id: the key of the link.
    pub source_id: String,
    pub revision: String,
    /// The manifest's content hash, as given.
    pub content_hash: Option<String>,
    /// Display unit of the design; geometry is always in mm.
    pub units: Option<String>,
    /// In body order; a STEP part with several bodies gives several.
    pub bodies: Vec<Body>,
    pub params: Vec<Param>,
    pub meta: Meta,
    /// What the user should know: skipped parts, unconfirmed names.
    pub warnings: Vec<Finding>,
}

type Res<T> = Result<T, Finding>;

fn manifest_err(msg: impl Into<String>) -> Finding {
    Finding::error(
        "import.craftx.manifest.invalid",
        format!("manifest.json: {}", msg.into()),
    )
}

fn str_field(o: &Map<String, Value>, k: &str) -> Res<Option<String>> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(manifest_err(format!("`{k}` must be a string"))),
    }
}

/// The parts the manifest lists: path → SHA-256 (hex), in order.
fn parts(o: &Map<String, Value>) -> Res<Vec<(String, Option<String>)>> {
    let Some(Value::Array(items)) = o.get(key::PARTS) else {
        return Err(manifest_err(format!("`{}` must be a list", key::PARTS)));
    };
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for it in items {
        let p = it
            .as_object()
            .and_then(|p| p.get(key::PATH)?.as_str().map(|s| (p, s)));
        let Some((p, path)) = p else {
            return Err(manifest_err(format!(
                "each part needs a `{}` string",
                key::PATH
            )));
        };
        let sha = match p.get(key::SHA256) {
            None => None,
            Some(v) => Some(v.as_str().and_then(format::sha256_hex).ok_or_else(|| {
                manifest_err(format!("part `{path}`: `{}` is not a SHA-256", key::SHA256))
            })?),
        };
        if out.iter().any(|(q, _)| q == path) {
            return Err(manifest_err(format!("part `{path}` is listed twice")));
        }
        out.push((path.to_owned(), sha));
    }
    Ok(out)
}

/// Reads an exchange file. Refusals and the warnings so far come back as
/// `Err`; the list then holds at least one error.
pub fn read(bytes: &[u8], opts: &Options) -> Result<Exchange, Vec<Finding>> {
    let mut w = Vec::new();
    match read_in(bytes, opts, &mut w) {
        Ok(mut x) => {
            x.warnings = w;
            Ok(x)
        }
        Err(e) => {
            w.push(e);
            Err(w)
        }
    }
}

fn read_in(bytes: &[u8], opts: &Options, w: &mut Vec<Finding>) -> Res<Exchange> {
    let ar = Archive::open(bytes, &opts.limits)?;
    let man = ar.read(format::MANIFEST).ok_or_else(|| {
        Finding::error(
            "import.craftx.manifest.missing",
            "the file has no manifest.json; it is not a craft exchange file",
        )
    })??;
    let man: Value =
        serde_json::from_slice(&man).map_err(|e| manifest_err(format!("not valid JSON ({e})")))?;
    let man = man
        .as_object()
        .ok_or_else(|| manifest_err("not a JSON object"))?;
    let fmt = str_field(man, key::FORMAT)?.unwrap_or_default();
    match format::format_major(&fmt) {
        Some(format::MAJOR) => {}
        Some(m) => {
            return Err(Finding::error(
                "import.craftx.format",
                format!(
                    "`{fmt}` is version {m} of the exchange format; this FluutCAM reads version {}",
                    format::MAJOR
                ),
            ));
        }
        None => {
            return Err(Finding::error(
                "import.craftx.format",
                format!(
                    "`format` is `{fmt}`, not `{}/{}`",
                    format::FORMAT,
                    format::MAJOR
                ),
            ));
        }
    }
    let source_id = str_field(man, key::SOURCE_ID)?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| manifest_err(format!("`{}` is missing", key::SOURCE_ID)))?;
    let revision = man
        .get(key::REVISION)
        .and_then(format::revision)
        .ok_or_else(|| manifest_err(format!("`{}` is missing", key::REVISION)))?;
    let source = match man.get(key::SOURCE) {
        Some(Value::Object(s)) => SourceApp {
            app: str_field(s, key::APP)?,
            version: str_field(s, key::VERSION)?,
        },
        _ => SourceApp::default(),
    };
    let requires: Vec<String> = match man.get(key::REQUIRES) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| v.as_str().map(str::to_owned))
            .collect::<Option<_>>()
            .ok_or_else(|| manifest_err(format!("`{}` must list strings", key::REQUIRES)))?,
        Some(_) => return Err(manifest_err(format!("`{}` must be a list", key::REQUIRES))),
    };
    if let Some(c) = requires.iter().find(|c| !format::supports(c)) {
        return Err(Finding::error(
            "import.craftx.requires",
            format!("the file requires `{c}`, which this FluutCAM cannot read"),
        ));
    }
    let listed = parts(man)?;
    for n in ar.names() {
        if n != format::MANIFEST && !listed.iter().any(|(p, _)| p == n) {
            w.push(Finding::warning(
                "import.craftx.part.unlisted",
                format!("archive entry `{n}` is not in the manifest; it is ignored"),
            ));
        }
    }
    let mut steps: Vec<(u32, &str)> = Vec::new();
    let (mut faces, mut params, mut meta) = (None, None, None);
    for (path, _) in &listed {
        match PartKind::of(path) {
            PartKind::Step(n) => steps.push((n, path)),
            PartKind::Faces => faces = Some(path.as_str()),
            PartKind::Params => params = Some(path.as_str()),
            PartKind::Meta => meta = Some(path.as_str()),
            PartKind::Record(_) | PartKind::Mesh(_) | PartKind::Board => {}
            PartKind::Manifest | PartKind::Unknown => w.push(Finding::warning(
                "import.craftx.part.unknown",
                format!("part `{path}` is not one FluutCAM knows; it is skipped"),
            )),
        }
    }
    steps.sort();
    let part = |path: &str| -> Res<Vec<u8>> {
        let sha = listed
            .iter()
            .find(|(p, _)| p == path)
            .and_then(|(_, s)| s.as_deref())
            .ok_or_else(|| {
                Finding::error(
                    "import.craftx.part.unhashed",
                    format!("part `{path}` has no SHA-256 in the manifest"),
                )
            })?;
        let data = ar.read(path).ok_or_else(|| {
            Finding::error(
                "import.craftx.part.missing",
                format!("part `{path}` is in the manifest but not in the archive"),
            )
        })??;
        let got = hex(&Sha256::digest(&data));
        if got != sha {
            return Err(Finding::error(
                "import.craftx.part.hash",
                format!(
                    "part `{path}` does not match its SHA-256 in the manifest; the file is damaged or was changed"
                ),
            ));
        }
        Ok(data)
    };
    if steps.is_empty() {
        return Err(Finding::error(
            "import.craftx.body.none",
            "the file has no STEP body part, the only geometry FluutCAM reads",
        ));
    }
    let required = |c: &str| requires.iter().any(|r| r.split('/').next() == Some(c));
    let table = match faces {
        None => None,
        Some(p) => {
            let t = part(p).and_then(|d| face_table(&d));
            match t {
                Ok(t) => Some(t),
                Err(e) if e.code != "import.craftx.faces.invalid" || required("faces.names") => {
                    return Err(e);
                }
                Err(e) => {
                    w.push(Finding {
                        severity: Severity::Warning,
                        ..e
                    });
                    None
                }
            }
        }
    };
    let mut bodies = Vec::new();
    for (n, path) in steps {
        let data = part(path)?;
        let m = fluut_io_step::parse(&data).map_err(|e| {
            Finding::error(
                "import.craftx.body.step",
                format!(
                    "part `{path}` is not valid STEP (line {}, column {}): {}",
                    e.pos.line, e.pos.col, e.msg
                ),
            )
        })?;
        let mut mapped = Vec::new();
        for b in fluut_io_step::map_bodies(&m) {
            match b {
                Ok(b) => mapped.push(b),
                Err(e) => w.push(Finding::warning(
                    "import.craftx.body.skipped",
                    format!("part `{path}`: entity #{}: {}", e.id, e.msg),
                )),
            }
        }
        let entry = table
            .as_ref()
            .and_then(|t: &Vec<BodyTable>| t.iter().find(|b| b.body == n));
        if mapped.len() == 1 {
            attach(&mut mapped[0], &m, entry, path, w);
        } else if entry.is_some() {
            w.push(Finding::warning(
                "import.craftx.body.count",
                format!(
                    "part `{path}` holds {} bodies, so the face table cannot say which faces it names; the names are not used",
                    mapped.len()
                ),
            ));
        }
        for b in &mut mapped {
            derive_names(b);
        }
        bodies.extend(mapped.into_iter().map(|step| Body { index: n, step }));
    }
    if bodies.is_empty() {
        return Err(Finding::error(
            "import.craftx.body.none",
            "no body in the file could be read",
        ));
    }
    let params = match params.map(|p| part(p).and_then(|d| read_params(&d))) {
        None => Vec::new(),
        Some(Ok(p)) => p,
        Some(Err(e)) if e.code == "import.craftx.params.invalid" => {
            w.push(Finding {
                severity: Severity::Warning,
                ..e
            });
            Vec::new()
        }
        Some(Err(e)) => return Err(e),
    };
    let meta = match meta.map(|p| part(p).and_then(|d| read_meta(&d))) {
        None => Meta::default(),
        Some(Ok(m)) => m,
        Some(Err(e)) if e.code == "import.craftx.meta.invalid" => {
            w.push(Finding {
                severity: Severity::Warning,
                ..e
            });
            Meta::default()
        }
        Some(Err(e)) => return Err(e),
    };
    Ok(Exchange {
        source,
        source_id,
        revision,
        content_hash: str_field(man, key::HASH)?,
        units: str_field(man, key::UNITS)?,
        bodies,
        params,
        meta,
        warnings: Vec::new(),
    })
}

fn hex(b: &[u8]) -> String {
    use std::fmt::Write;
    b.iter().fold(String::new(), |mut s, x| {
        let _ = write!(s, "{x:02x}");
        s
    })
}

// ── Face table ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    index: u32,
    name: String,
    exact: bool,
    surface: Option<String>,
    aliases: Vec<String>,
    point: Option<Point3>,
}

#[derive(Debug, Clone, PartialEq)]
struct BodyTable {
    body: u32,
    faces: Vec<Entry>,
    edges: Vec<Entry>,
}

fn faces_err(msg: impl Into<String>) -> Finding {
    Finding::error(
        "import.craftx.faces.invalid",
        format!("faces.json: {}; the face names are not used", msg.into()),
    )
}

fn index(v: Option<&Value>) -> Option<u32> {
    v?.as_u64().and_then(|i| u32::try_from(i).ok())
}

fn entries(v: Option<&Value>, what: &str) -> Res<Vec<Entry>> {
    let items = match v {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(faces_err(format!("`{what}` must be a list"))),
    };
    let mut out = Vec::with_capacity(items.len());
    for it in items {
        let o = it
            .as_object()
            .ok_or_else(|| faces_err(format!("each of `{what}` must be an object")))?;
        let index = index(o.get(key::INDEX)).ok_or_else(|| {
            faces_err(format!("each of `{what}` needs an `{}` number", key::INDEX))
        })?;
        let name = o
            .get(key::NAME)
            .and_then(Value::as_str)
            .ok_or_else(|| faces_err(format!("{what} {index} has no `{}`", key::NAME)))?
            .to_owned();
        let exact = match o.get(key::EXACT) {
            None => true,
            Some(Value::Bool(b)) => *b,
            Some(_) => {
                return Err(faces_err(format!(
                    "{what} {index}: `exact` must be true or false"
                )));
            }
        };
        let aliases = match o.get(key::ALIASES) {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) => a
                .iter()
                .map(|x| x.as_str().map(str::to_owned))
                .collect::<Option<_>>()
                .ok_or_else(|| faces_err(format!("{what} {index}: aliases must be strings")))?,
            Some(_) => return Err(faces_err(format!("{what} {index}: aliases must be a list"))),
        };
        let point = match o.get(key::POINT) {
            None | Some(Value::Null) => None,
            Some(Value::Array(a)) => match &a[..] {
                [x, y, z] => match (x.as_f64(), y.as_f64(), z.as_f64()) {
                    (Some(x), Some(y), Some(z)) => Some(Point3::new(x, y, z)),
                    _ => return Err(faces_err(format!("{what} {index}: bad point"))),
                },
                _ => return Err(faces_err(format!("{what} {index}: a point is [x, y, z]"))),
            },
            Some(_) => return Err(faces_err(format!("{what} {index}: a point is [x, y, z]"))),
        };
        out.push(Entry {
            index,
            name,
            exact,
            surface: o
                .get(key::SURFACE)
                .and_then(Value::as_str)
                .map(str::to_owned),
            aliases,
            point,
        });
    }
    Ok(out)
}

fn face_table(data: &[u8]) -> Res<Vec<BodyTable>> {
    let v: Value =
        serde_json::from_slice(data).map_err(|e| faces_err(format!("not valid JSON ({e})")))?;
    let Some(Value::Array(bodies)) = v.get(key::BODIES) else {
        return Err(faces_err(format!("`{}` must be a list", key::BODIES)));
    };
    let mut out: Vec<BodyTable> = Vec::new();
    for b in bodies {
        let body = index(b.get(key::BODY))
            .ok_or_else(|| faces_err(format!("each body needs a `{}` number", key::BODY)))?;
        if out.iter().any(|t| t.body == body) {
            return Err(faces_err(format!("body {body} is listed twice")));
        }
        out.push(BodyTable {
            body,
            faces: entries(b.get(key::FACES), "faces")?,
            edges: entries(b.get(key::EDGES), "edges")?,
        });
    }
    Ok(out)
}

/// STEP ids of the entities of kind `kw`, in file order.
fn file_order(m: &RawModel, kw: &str) -> Vec<u64> {
    let Some(k) = m.keywords.get(kw) else {
        return Vec::new();
    };
    let mut v: Vec<(fluut_io_step::Pos, u64)> = m
        .index
        .iter()
        .filter_map(|(&id, &slot)| match &m.entities[slot as usize] {
            RawEntity::Simple { kind, pos, .. } if *kind == k => Some((*pos, id)),
            _ => None,
        })
        .collect();
    v.sort_by_key(|(p, id)| (p.line, p.col, *id));
    v.into_iter().map(|(_, id)| id).collect()
}

/// Gives the body's faces and edges the names of its face table entry.
///
/// A table entry names the face its index points at: the `index`-th
/// `ADVANCED_FACE` (edges: `EDGE_CURVE`) of the part, in file order
/// (**Reading**: RFC 0001 does not say what the index counts). A face
/// whose STEP name equals the entry's name is taken instead, and a STEP
/// name that differs from the entry's wins, with a warning. A point that
/// misses its face marks the name `exact: false`.
fn attach(
    b: &mut MappedBody,
    m: &RawModel,
    t: Option<&BodyTable>,
    path: &str,
    w: &mut Vec<Finding>,
) {
    // Names the STEP file gave: piece syntax applies to them too.
    for n in b.body.names.faces.iter_mut().flatten() {
        n.piece_of = format::piece_of(&n.name).map(str::to_owned);
    }
    for n in b.body.names.edges.iter_mut().flatten() {
        n.between = format::edge_faces(&n.name).map(|(a, c)| (a.to_owned(), c.to_owned()));
    }
    let Some(t) = t else {
        return;
    };
    let face_of: BTreeMap<u64, u32> = b
        .face_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i as u32))
        .collect();
    let edge_of: BTreeMap<u64, u32> = b
        .edge_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i as u32))
        .collect();
    let faces = file_order(m, "ADVANCED_FACE");
    let edges = file_order(m, "EDGE_CURVE");
    let step_names = b.body.names.clone();
    for (entry, is_face) in t
        .faces
        .iter()
        .map(|e| (e, true))
        .chain(t.edges.iter().map(|e| (e, false)))
    {
        let what = if is_face { "face" } else { "edge" };
        if entry.name.is_empty() {
            w.push(Finding::warning(
                "import.craftx.faces.index",
                format!(
                    "{path}: {what} {} has an empty name; it is skipped",
                    entry.index
                ),
            ));
            continue;
        }
        let (table, order, of) = if is_face {
            (&step_names.faces, &faces, &face_of)
        } else {
            (&step_names.edges, &edges, &edge_of)
        };
        let by_name = table
            .iter()
            .position(|n| n.as_ref().is_some_and(|n| n.name == entry.name))
            .map(|i| i as u32);
        let by_index = order
            .get(entry.index as usize)
            .and_then(|id| of.get(id))
            .copied();
        let target = match (by_name, by_index) {
            (Some(i), _) => i,
            (None, Some(i)) => {
                if let Some(other) = table.get(i as usize).and_then(Option::as_ref) {
                    w.push(Finding::warning(
                        "import.craftx.faces.name_mismatch",
                        format!(
                            "{path}: {what} {} is `{}` in the face table but `{}` in the STEP file; the STEP name is used",
                            entry.index, entry.name, other.name
                        ),
                    ));
                    continue;
                }
                i
            }
            (None, None) => {
                w.push(Finding::warning(
                    "import.craftx.faces.index",
                    format!(
                        "{path}: the face table names {what} {} (`{}`), which the STEP file does not have or FluutCAM could not read",
                        entry.index, entry.name
                    ),
                ));
                continue;
            }
        };
        let mut n = EntityName {
            name: entry.name.clone(),
            aliases: entry.aliases.clone(),
            exact: entry.exact,
            ..EntityName::default()
        };
        if is_face {
            n.piece_of = format::piece_of(&n.name).map(str::to_owned);
            if let Some(p) = entry.point
                && !on_face(&b.body, target, p)
            {
                n.exact = false;
                w.push(Finding::warning(
                    "import.craftx.faces.point",
                    format!(
                        "{path}: the point given for face `{}` is not on face {}; the name is kept but marked as found by geometry",
                        entry.name, entry.index
                    ),
                ));
            }
            if let Some(k) = entry.surface.as_deref().and_then(format::surface_kind)
                && !is_kind(
                    &b.body.surfaces[b.body.faces[target as usize].surface as usize],
                    k,
                )
            {
                n.exact = false;
                w.push(Finding::warning(
                    "import.craftx.faces.surface",
                    format!(
                        "{path}: face `{}` is listed as a {} but face {} is another kind of surface; the name is marked as found by geometry",
                        entry.name,
                        entry.surface.as_deref().unwrap_or_default(),
                        entry.index
                    ),
                ));
            }
            b.body.names.set_face(target, Some(n));
        } else {
            n.between = format::edge_faces(&n.name).map(|(a, c)| (a.to_owned(), c.to_owned()));
            b.body.names.set_edge(target, Some(n));
        }
    }
    let repeated = b.body.names.drop_repeated();
    if !repeated.is_empty() {
        w.push(Finding::warning(
            "import.craftx.faces.repeated",
            format!(
                "{path}: {} named more than one face or edge, so they name none: {}",
                if repeated.len() == 1 {
                    "a name is"
                } else {
                    "names are"
                },
                repeated.join(", ")
            ),
        ));
    }
}

/// Names every unnamed edge between two named faces `<face A>|<face B>`
/// (marked derived: several edges may share one).
fn derive_names(b: &mut MappedBody) {
    if !b.body.names.has_faces() {
        return;
    }
    let adj = edge_faces(&b.body);
    let names = &mut b.body.names;
    for (e, fs) in adj.iter().enumerate() {
        if names.edge(e as u32).is_some() {
            continue;
        }
        let [f0, f1] = fs[..] else {
            continue;
        };
        let (Some(a), Some(c)) = (names.face(f0), names.face(f1)) else {
            continue;
        };
        let n = EntityName {
            name: format::edge_name(&a.name, &c.name),
            exact: a.exact && c.exact,
            between: Some((a.name.clone(), c.name.clone())),
            derived: true,
            ..EntityName::default()
        };
        names.set_edge(e as u32, Some(n));
    }
}

fn is_kind(s: &Surface, k: format::SurfaceKind) -> bool {
    use format::SurfaceKind as K;
    matches!(
        (s, k),
        (Surface::Plane { .. }, K::Plane)
            | (Surface::Cylinder { .. }, K::Cylinder)
            | (Surface::Cone { .. }, K::Cone)
            | (Surface::Sphere { .. }, K::Sphere)
            | (Surface::Torus { .. }, K::Torus)
    )
}

/// Whether `p` lies on face `f`: on its surface within the modelling
/// tolerance (plus [`format::POINT_TOL`]) and inside its trim. Where the
/// trim cannot be tested in the parameter plane (a loop wraps round the
/// surface, or a p-curve is missing), the face's box stands in for it.
fn on_face(b: &BrepBody, f: u32, p: Point3) -> bool {
    let tol = b.tol_model.max(0.0) + format::POINT_TOL;
    let Some(face) = b.faces.get(f as usize) else {
        return false;
    };
    let in_box = b.face_box(f, 1e-3).expand(tol).contains(p);
    let polys: Option<Vec<trim::UvLoop>> = face
        .loops
        .iter()
        .map(|&l| trim::loop_polyline(b, f, l))
        .collect();
    let Some(polys) = polys.filter(|ps| ps.iter().any(|l| !l.points.is_empty())) else {
        return in_box;
    };
    let pts = polys.iter().flat_map(|l| l.points.iter());
    let (mut u0, mut u1, mut v0, mut v1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for q in pts {
        (u0, u1) = (u0.min(q.x()), u1.max(q.x()));
        (v0, v1) = (v0.min(q.y()), v1.max(q.y()));
    }
    let (Some(u), Some(v)) = (Interval::new(u0, u1), Interval::new(v0, v1)) else {
        return in_box;
    };
    let s = &b.surfaces[face.surface as usize];
    let c = s.closest(
        p,
        UvBox::new(u, v),
        &Tol::for_machining(b.tol_model.max(1e-7), 0.01),
    );
    if c.dist.is_nan() || c.dist > tol {
        return false;
    }
    if polys.iter().any(|l| l.wraps || l.points.len() < 3) {
        return in_box;
    }
    let q = Point2::new(c.u, c.v);
    face.loops.iter().zip(&polys).all(|(&l, lp)| {
        let inside = trim::winding(&lp.points, q) != 0;
        b.loops[l as usize].outer == inside
    })
}

// ── Parameters and metadata ────────────────────────────────────────────

fn read_params(data: &[u8]) -> Res<Vec<Param>> {
    let bad = |m: &str| {
        Finding::error(
            "import.craftx.params.invalid",
            format!("params.json: {m}; the parameters are not shown"),
        )
    };
    let v: Value = serde_json::from_slice(data).map_err(|_| bad("not valid JSON"))?;
    let Some(Value::Array(items)) = v.get(key::PARAMS) else {
        return Err(bad("`params` must be a list"));
    };
    items
        .iter()
        .map(|p| {
            let s = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_owned);
            Ok(Param {
                name: s(key::NAME).ok_or_else(|| bad("a parameter has no name"))?,
                value: match p.get(key::VALUE) {
                    Some(Value::String(x)) => x.clone(),
                    Some(Value::Number(x)) => x.to_string(),
                    Some(Value::Bool(x)) => x.to_string(),
                    _ => String::new(),
                },
                unit: s(key::UNIT),
                expression: s(key::EXPRESSION),
            })
        })
        .collect()
}

fn read_meta(data: &[u8]) -> Res<Meta> {
    let v: Value = serde_json::from_slice(data).map_err(|_| {
        Finding::error(
            "import.craftx.meta.invalid",
            "meta.json: not valid JSON; the metadata is not shown",
        )
    })?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    Ok(Meta {
        material: s(key::MATERIAL),
        part_number: s(key::PART_NUMBER),
    })
}
