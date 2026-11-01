// SPDX-License-Identifier: MIT OR Apache-2.0

//! The model store (D06 §5): geometry sources loaded once per session, with
//! entity ids.

use fluut_cam::geometry::{BodyGeom, SetupBody};
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
        if g.kind != SourceKind::Step {
            out.push(Diagnostic::error(
                "model.geometry.kind.unsupported",
                format!(
                    "geometry `{}`: {} files are not supported yet",
                    g.id,
                    g.kind.as_str()
                ),
            ));
            return out;
        }
        let model = match fluut_io_step::parse(&bytes) {
            Ok(m) => m,
            Err(e) => {
                out.push(Diagnostic::error(
                    "model.geometry.step",
                    format!(
                        "geometry `{}`: not a valid STEP file ({:?}): {}",
                        g.id, e.pos, e.msg
                    ),
                ));
                return out;
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
