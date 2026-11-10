// SPDX-License-Identifier: MIT OR Apache-2.0

//! The model store (D06 §5): geometry sources loaded once per session, with
//! entity ids. STEP, STL, DXF and SVG give bodies.

use fluut_cam::geometry::{BodyGeom, SetupBody};
use fluut_io_2d::Unit;
use fluut_model::Diagnostic;
use fluut_model::hash::sha256_hex;
use fluut_model::project::{GeometrySource, SourceKind};
use std::collections::BTreeMap;
use std::path::Path;

/// The bodies of one geometry source and the hash of its file.
#[derive(Debug, Clone)]
pub struct LoadedSource {
    pub hash: String,
    pub bodies: Vec<SetupBody>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelStore {
    sources: BTreeMap<String, LoadedSource>,
}

impl ModelStore {
    pub fn new() -> ModelStore {
        ModelStore::default()
    }

    /// Adds a source built in memory (tests, scripts).
    pub fn insert(&mut self, id: &str, src: LoadedSource) {
        self.sources.insert(id.to_owned(), src);
    }

    pub fn source(&self, id: &str) -> Option<&LoadedSource> {
        self.sources.get(id)
    }

    /// Forgets source `id`.
    pub fn remove(&mut self, id: &str) {
        self.sources.remove(id);
    }

    /// Every source, by id.
    pub fn sources(&self) -> impl Iterator<Item = (&str, &LoadedSource)> {
        self.sources.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Every body of every source, by source id.
    pub fn bodies(&self) -> impl Iterator<Item = &SetupBody> {
        self.sources.values().flat_map(|s| s.bodies.iter())
    }

    /// Loads a source from its file, relative to `dir`. A changed file is a
    /// warning (spec 02 §2.6.3); a missing or unreadable one an error.
    pub fn load(&mut self, g: &GeometrySource, dir: &Path) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let path = dir.join(&g.path);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                out.push(Diagnostic::error(
                    "model.geometry.missing",
                    format!("geometry `{}`: cannot read {}: {e}", g.id, path.display()),
                ));
                return out;
            }
        };
        let hash = sha256_hex(&bytes);
        if let Some(h) = &g.hash
            && *h != hash
        {
            out.push(Diagnostic::warning(
                "model.geometry.changed",
                format!(
                    "geometry `{}`: {} changed since it was imported",
                    g.id, g.path
                ),
            ));
        }
        let bodies = match g.kind {
            SourceKind::Step => load_step(g, &bytes, &mut out),
            SourceKind::Dxf | SourceKind::Svg => load_2d(g, &bytes, &mut out),
            SourceKind::Stl => load_stl(g, &bytes, &mut out),
            SourceKind::Bitmap => {
                out.push(Diagnostic::error(
                    "model.geometry.kind.unsupported",
                    format!(
                        "geometry `{}`: {} files are not supported yet",
                        g.id,
                        g.kind.as_str()
                    ),
                ));
                None
            }
        };
        let Some(bodies) = bodies else {
            return out;
        };
        for e in &g.bodies {
            if !bodies.iter().any(|b| b.id == e.id) {
                out.push(Diagnostic::warning(
                    "model.geometry.body_missing",
                    format!(
                        "geometry `{}`: body `{}` is no longer in the file",
                        g.id, e.id
                    ),
                ));
            }
        }
        self.sources
            .insert(g.id.clone(), LoadedSource { hash, bodies });
        out
    }
}

fn load_step(
    g: &GeometrySource,
    bytes: &[u8],
    out: &mut Vec<Diagnostic>,
) -> Option<Vec<SetupBody>> {
    let model = match fluut_io_step::parse(bytes) {
        Ok(m) => m,
        Err(e) => {
            out.push(Diagnostic::error(
                "model.geometry.step",
                format!(
                    "geometry `{}`: not a valid STEP file ({:?}): {}",
                    g.id, e.pos, e.msg
                ),
            ));
            return None;
        }
    };
    let mut bodies = Vec::new();
    for b in fluut_io_step::map_bodies(&model) {
        match b {
            Ok(b) => {
                if !b.skipped.is_empty() {
                    out.push(Diagnostic::warning(
                        "model.geometry.faces_skipped",
                        format!(
                            "geometry `{}`: body #{} lost {} faces it could not read",
                            g.id,
                            b.id,
                            b.skipped.len()
                        ),
                    ));
                }
                bodies.push(SetupBody {
                    id: format!("b-{}", b.id),
                    geom: BodyGeom::Brep(b.body),
                });
            }
            Err(e) => out.push(Diagnostic::warning(
                "model.geometry.body_skipped",
                format!("geometry `{}`: entity #{}: {}", g.id, e.id, e.msg),
            )),
        }
    }
    Some(bodies)
}

/// Modelling tolerance of imported 2-D and mesh bodies, in mm.
const TOL_IMPORT: f64 = 1e-4;

/// The project's `unit` as mm per file unit: `Ok(None)` when not given, an
/// error when not a unit.
fn unit_scale(g: &GeometrySource, out: &mut Vec<Diagnostic>) -> Result<Option<f64>, ()> {
    let Some(u) = g.unit.as_deref() else {
        return Ok(None);
    };
    match Unit::parse(u) {
        Some(u) => Ok(Some(u.mm())),
        None => {
            let known: Vec<&str> = Unit::ALL.iter().map(|u| u.as_str()).collect();
            out.push(Diagnostic::error(
                "model.geometry.unit.unknown",
                format!(
                    "geometry `{}`: unit `{u}` is not one of {}",
                    g.id,
                    known.join(", ")
                ),
            ));
            Err(())
        }
    }
}

fn unit_missing(g: &GeometrySource, suggested: Option<Unit>) -> Diagnostic {
    let hint = suggested.map_or(String::new(), |u| {
        format!(" (the file suggests `{}`)", u.as_str())
    });
    Diagnostic::error(
        "model.geometry.unit.missing",
        format!(
            "geometry `{}`: {} files carry no reliable unit; set `unit`{hint}",
            g.id,
            g.kind.as_str()
        ),
    )
}

/// DXF and SVG: one wire body per DXF layer, or one for the SVG file, with
/// ids `b-1`, `b-2`, … in file order.
fn load_2d(g: &GeometrySource, bytes: &[u8], out: &mut Vec<Diagnostic>) -> Option<Vec<SetupBody>> {
    let scale = unit_scale(g, out).ok()?;
    let opts = fluut_io_2d::Options {
        scale,
        tol_model: TOL_IMPORT,
        limits: fluut_io_2d::Limits::default(),
    };
    let dxf = g.kind == SourceKind::Dxf;
    let read = if dxf {
        fluut_io_2d::read_dxf(bytes, &opts)
    } else {
        fluut_io_2d::read_svg(bytes, &opts)
    };
    let r = match read {
        Ok(r) => r,
        Err(e) => {
            out.push(Diagnostic::error(
                if dxf {
                    "model.geometry.dxf"
                } else {
                    "model.geometry.svg"
                },
                format!(
                    "geometry `{}`: not a readable {} file: {e}",
                    g.id,
                    g.kind.as_str()
                ),
            ));
            return None;
        }
    };
    if dxf && scale.is_none() {
        out.push(unit_missing(g, r.report.unit));
        return None;
    }
    let rep = &r.report;
    let mut notes: Vec<String> = Vec::new();
    if rep.dropped > 0 {
        notes.push(format!("{} malformed curves dropped", rep.dropped));
    }
    for (what, list) in [("unsupported", &rep.unsupported), ("ignored", &rep.ignored)] {
        if !list.is_empty() {
            let items: Vec<String> = list.iter().map(|(k, n)| format!("{n} {k}")).collect();
            notes.push(format!("{what}: {}", items.join(", ")));
        }
    }
    notes.extend(rep.warnings.iter().cloned());
    if rep.more_warnings > 0 {
        notes.push(format!("{} more warnings", rep.more_warnings));
    }
    for n in notes {
        out.push(Diagnostic::warning(
            "model.geometry.import",
            format!("geometry `{}`: {n}", g.id),
        ));
    }
    Some(
        r.wires
            .into_iter()
            .enumerate()
            .map(|(i, w)| SetupBody {
                id: format!("b-{}", i + 1),
                geom: BodyGeom::Wire(w),
            })
            .collect(),
    )
}

/// STL: one body per connected shell, every triangle a planar face (D05 §2.5),
/// with ids `b-1`, `b-2`, … in order of their first triangle.
fn load_stl(g: &GeometrySource, bytes: &[u8], out: &mut Vec<Diagnostic>) -> Option<Vec<SetupBody>> {
    let scale = unit_scale(g, out).ok()?;
    let Some(scale) = scale else {
        out.push(unit_missing(g, None));
        return None;
    };
    let opts = fluut_io_mesh::Options {
        scale: Some(scale),
        tol_model: TOL_IMPORT,
        limits: fluut_io_mesh::Limits::default(),
    };
    let m = match fluut_io_mesh::read_stl(bytes, &opts) {
        Ok(m) => m,
        Err(e) => {
            out.push(Diagnostic::error(
                "model.geometry.stl",
                format!("geometry `{}`: not a readable STL file: {e}", g.id),
            ));
            return None;
        }
    };
    let r = &m.report;
    let mut notes: Vec<String> = Vec::new();
    if r.dropped > 0 {
        notes.push(format!("{} degenerate triangles dropped", r.dropped));
    }
    if r.flipped {
        notes.push("triangle order reversed to match the file normals".into());
    }
    if r.open_edges > 0 {
        notes.push(format!(
            "{} open edges in {} holes",
            r.open_edges, r.boundary_loops
        ));
    }
    if r.non_manifold_edges > 0 {
        notes.push(format!("{} non-manifold edges", r.non_manifold_edges));
    }
    if r.inconsistent_edges > 0 {
        notes.push(format!(
            "{} edges between triangles of opposite orientation",
            r.inconsistent_edges
        ));
    }
    notes.extend(r.warnings.iter().cloned());
    for n in notes {
        out.push(Diagnostic::warning(
            "model.geometry.import",
            format!("geometry `{}`: {n}", g.id),
        ));
    }
    if !r.is_closed() {
        out.push(Diagnostic::warning(
            "model.geometry.mesh.open",
            format!(
                "geometry `{}`: the mesh is not a closed, consistent surface; toolpaths follow its triangles, but results near the gaps may be wrong",
                g.id
            ),
        ));
    }
    let mut bodies = Vec::new();
    for (i, mut b) in fluut_topo::mesh::to_breps(&m.body).into_iter().enumerate() {
        fluut_topo::pcurve::compute(&mut b);
        bodies.push(SetupBody {
            id: format!("b-{}", i + 1),
            geom: BodyGeom::Brep(b),
        });
    }
    Some(bodies)
}
