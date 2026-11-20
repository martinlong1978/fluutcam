// SPDX-License-Identifier: MIT OR Apache-2.0

//! Named picks in the session (spec 03 §3.5 item 4, D06 §5.1): naming a
//! pick when it is made, re-finding named picks on re-import, and the
//! `broken` toolpath state.

use crate::names::{self, Outcome};
use crate::reimport::{Matching, PickReport, belongs};
use crate::session::Session;
use crate::store::LoadedSource;
use fluut_cam::geometry::{BodyGeom, SetupBody, edge_id, face_id};
use fluut_model::edit::Edit;
use fluut_model::{Diagnostic, PValue, Params, Sel, selection_toml};
use std::collections::BTreeSet;

/// `(toolpath, field, id)` of a pick.
pub(crate) type PickKey = (String, String, String);

/// How many replaced model versions the session keeps for undo.
const HISTORY: usize = 8;

/// A named pick that no longer answers to its name: its toolpath is
/// broken until the entity is picked again.
#[derive(Clone, PartialEq, Debug)]
pub struct BrokenPick {
    pub toolpath: String,
    /// The selection field, `geometry.drive_faces`, …
    pub field: String,
    pub id: String,
    pub name: String,
    /// Where the entity was before the update that lost it, when this
    /// session made that update.
    pub at: Option<[f64; 3]>,
}

impl BrokenPick {
    /// The error `model.pick.missing`, naming the face, where it was and
    /// the toolpath.
    pub fn diagnostic(&self, toolpath_name: &str) -> Diagnostic {
        Diagnostic::error(
            "model.pick.missing",
            missing_message(
                &self.toolpath,
                toolpath_name,
                &self.field,
                &self.id,
                &self.name,
                self.at,
            ),
        )
        .on(&self.toolpath)
        .at(&self.field)
    }
}

fn what(id: &str) -> &'static str {
    if id.starts_with("e-") { "edge" } else { "face" }
}

fn missing_message(
    tp: &str,
    tp_name: &str,
    field: &str,
    id: &str,
    name: &str,
    at: Option<[f64; 3]>,
) -> String {
    let place = match at {
        Some(p) => format!("last at ({:.1}, {:.1}, {:.1}) mm", p[0], p[1], p[2]),
        None => format!("last picked as `{id}`"),
    };
    format!(
        "toolpath `{tp}` ({tp_name}): the {} `{name}` of {field}, {place}, is not in the model any more; the toolpath is broken until you pick it again",
        what(id)
    )
}

fn sel(face: bool, id: String) -> Sel {
    if face { Sel::Face(id) } else { Sel::Edge(id) }
}

fn has_names(bodies: &[SetupBody]) -> bool {
    bodies
        .iter()
        .any(|b| matches!(&b.geom, BodyGeom::Brep(x) if x.names.has_faces()))
}

/// Picks, each once, in their first place.
fn dedup(items: Vec<(Sel, Option<String>)>) -> Vec<(Sel, Option<String>)> {
    let mut seen = BTreeSet::new();
    items
        .into_iter()
        .filter(|(s, _)| seen.insert(s.clone()))
        .collect()
}

/// The finding for a pick's outcome, if it needs one.
pub(crate) fn pick_diagnostic(p: &PickReport, tp_name: &str) -> Option<Diagnostic> {
    let w = what(&p.id);
    let d = match p.outcome {
        Outcome::Exact => return None,
        Outcome::Merged => Diagnostic::warning(
            "model.pick.merged",
            format!(
                "toolpath `{}` ({tp_name}): the {w} `{}` of {} merged into another; the pick follows it",
                p.owner, p.name, p.field
            ),
        ),
        Outcome::Split => Diagnostic::warning(
            "model.pick.split",
            format!(
                "toolpath `{}` ({tp_name}): the {w} `{}` of {} is now {} pieces; the pick uses all of them, which may change what the toolpath cuts",
                p.owner,
                p.name,
                p.field,
                p.ids.len()
            ),
        ),
        Outcome::ByGeometry => Diagnostic::warning(
            "model.pick.by_geometry",
            format!(
                "toolpath `{}` ({tp_name}): the {w} `{}` of {} was found by its shape only; check this toolpath",
                p.owner, p.name, p.field
            ),
        ),
        Outcome::Missing => Diagnostic::error(
            "model.pick.missing",
            missing_message(&p.owner, tp_name, &p.field, &p.id, &p.name, p.at),
        ),
    };
    Some(d.on(&p.owner).at(&p.field))
}

impl Session {
    /// The linked models' files and the hashes the project holds, for
    /// change detection ([`crate::watch`]).
    pub fn linked_files(&self) -> Vec<crate::watch::LinkedFile> {
        crate::watch::linked_files(&self.project, &self.dir)
    }

    /// The geometry source that links the design `source_id`, if any.
    pub fn linked_source(&self, source_id: &str) -> Option<&str> {
        self.project
            .geometry
            .iter()
            .find(|g| g.link.as_ref().is_some_and(|l| l.source_id == source_id))
            .map(|g| g.id.as_str())
    }

    /// The named picks of toolpath `id` that no longer answer to their
    /// names in the loaded model. Not empty: the toolpath is broken, and
    /// calculation and posting refuse it.
    pub fn broken_picks(&self, id: &str) -> Vec<BrokenPick> {
        let Some((_, t)) = self.project.toolpath(id) else {
            return Vec::new();
        };
        t.params
            .selections()
            .into_iter()
            .filter(|(_, s)| !matches!(s, Sel::Body(_)))
            .filter_map(|(field, s)| {
                let name = t.params.pick_name(s.id())?;
                (names::pick_holds(self.store.bodies(), s.id(), name) == Some(false)).then(|| {
                    BrokenPick {
                        toolpath: id.to_owned(),
                        field,
                        id: s.id().to_owned(),
                        name: name.to_owned(),
                        at: self.places.get(s.id()).copied(),
                    }
                })
            })
            .collect()
    }

    /// The toolpath's current result, or else the last one this session
    /// had, for viewing a stale or broken toolpath.
    pub fn last_result(&self, id: &str) -> Option<std::sync::Arc<fluut_path::ToolpathResult>> {
        self.result(id).or_else(|| self.last.get(id).cloned())
    }

    /// `edit`, with names for the face and edge picks it makes on named
    /// bodies: one undo step with the pick (spec 02 §2.1).
    pub(crate) fn with_pick_names(&self, edit: Edit) -> Edit {
        if !matches!(
            edit,
            Edit::SetToolpathField { .. }
                | Edit::AddToolpath { .. }
                | Edit::DuplicateAs { .. }
                | Edit::Group { .. }
        ) {
            return edit;
        }
        let bodies: Vec<&SetupBody> = self.store.bodies().collect();
        if !bodies
            .iter()
            .any(|b| matches!(&b.geom, BodyGeom::Brep(x) if x.names.has_faces()))
        {
            return edit;
        }
        let Ok((p, changed)) = fluut_model::edit::apply(&self.project, &edit) else {
            return edit;
        };
        let mut extra = Vec::new();
        for t in p
            .setups
            .iter()
            .flat_map(|s| s.toolpaths.iter())
            .filter(|t| changed.contains(&t.id))
        {
            for (path, _) in Params::leaf_fields(t.kind()) {
                let Some(PValue::Selection(items)) = t.params.get(&path) else {
                    continue;
                };
                let mut any = false;
                let named: Vec<(Sel, Option<String>)> = items
                    .iter()
                    .map(|s| {
                        let cur = t.params.pick_name(s.id()).map(str::to_owned);
                        if cur.is_some() || matches!(s, Sel::Body(_)) {
                            return (s.clone(), cur);
                        }
                        let n = names::pick_name(bodies.iter().copied(), s.id());
                        any |= n.is_some();
                        (s.clone(), n)
                    })
                    .collect();
                if any {
                    extra.push(Edit::set_toolpath_field(
                        &t.id,
                        &path,
                        Some(&selection_toml(&named)),
                    ));
                }
            }
        }
        if extra.is_empty() {
            return edit;
        }
        let label = edit.label();
        let mut edits = vec![edit];
        edits.extend(extra);
        Edit::Group { label, edits }
    }

    /// Re-finds the named toolpath picks of `old` bodies in `new` ones
    /// (D06 §5.1): by stored name, or the old face's name. Returns the
    /// outcomes, the field edits that set the new picks (applied after
    /// the id remap), and `(toolpath, field, id)` of every pick handled by
    /// name.
    pub(crate) fn named_picks(
        &self,
        old: &[SetupBody],
        new: &[SetupBody],
        m: &Matching,
    ) -> (Vec<PickReport>, Vec<Edit>, BTreeSet<PickKey>) {
        let mut reports = Vec::new();
        let mut edits = Vec::new();
        let mut handled = BTreeSet::new();
        if !has_names(old) && !has_names(new) {
            return (reports, edits, handled);
        }
        for t in self.project.setups.iter().flat_map(|s| s.toolpaths.iter()) {
            for (path, _) in Params::leaf_fields(t.kind()) {
                let Some(PValue::Selection(items)) = t.params.get(&path) else {
                    continue;
                };
                let mut out = Vec::new();
                let mut touched = false;
                for s in items {
                    let id = s.id();
                    let stored = t.params.pick_name(id).map(str::to_owned);
                    let split = names::split_pick(id).filter(|_| belongs(id, old));
                    let Some((face, ob, _)) = split.filter(|_| !matches!(s, Sel::Body(_))) else {
                        out.push((s.clone(), stored));
                        continue;
                    };
                    let nb_id = m.new_id(&ob).unwrap_or(&ob).to_owned();
                    let name = stored.clone().or_else(|| names::pick_name(old, id));
                    let nb = names::brep(new, &nb_id).filter(|b| b.names.has_faces());
                    let (Some(name), Some(nb)) = (name, nb) else {
                        // Not named: the matching moves it, and a pick that
                        // lands on a named entity takes its name.
                        let nid = m.new_id(id).unwrap_or(id).to_owned();
                        let n = m.new_id(id).and_then(|n| names::pick_name(new, n));
                        touched |= n.is_some();
                        out.push((sel(face, nid), n.or(stored)));
                        continue;
                    };
                    let hint = m
                        .new_id(id)
                        .and_then(names::split_pick)
                        .filter(|(_, b, _)| *b == nb_id)
                        .map(|(_, _, i)| i);
                    let found = if face {
                        names::find_face(&nb.names, &name)
                    } else {
                        names::find_edge(nb, &name, hint)
                    };
                    let ids: Vec<String> = if found.outcome == Outcome::Missing {
                        vec![id.to_owned()]
                    } else {
                        found
                            .entities
                            .iter()
                            .map(|&k| {
                                if face {
                                    face_id(&nb_id, k)
                                } else {
                                    edge_id(&nb_id, k)
                                }
                            })
                            .collect()
                    };
                    out.extend(
                        ids.iter()
                            .map(|n| (sel(face, n.clone()), Some(name.clone()))),
                    );
                    reports.push(PickReport {
                        owner: t.id.clone(),
                        owner_name: t.name.clone(),
                        field: path.clone(),
                        id: id.to_owned(),
                        name,
                        outcome: found.outcome,
                        ids,
                        at: (found.outcome == Outcome::Missing)
                            .then(|| names::place(old, id))
                            .flatten(),
                    });
                    handled.insert((t.id.clone(), path.clone(), id.to_owned()));
                    touched = true;
                }
                if touched {
                    edits.push(Edit::set_toolpath_field(
                        &t.id,
                        &path,
                        Some(&selection_toml(&dedup(out))),
                    ));
                }
            }
        }
        (reports, edits, handled)
    }

    /// Keeps a model version replaced by a re-import, so undo can bring it
    /// back.
    pub(crate) fn keep_version(&mut self, id: &str, src: LoadedSource) {
        self.history
            .retain(|(i, s)| !(i == id && s.hash == src.hash));
        self.history.push((id.to_owned(), src));
        if self.history.len() > HISTORY {
            self.history.remove(0);
        }
    }

    /// After undo or redo: loads the kept version of every source whose
    /// hash in the project differs from the loaded one.
    pub(crate) fn restore_versions(&mut self) {
        let want: Vec<(String, String)> = self
            .project
            .geometry
            .iter()
            .filter_map(|g| Some((g.id.clone(), g.hash.clone()?)))
            .collect();
        for (id, hash) in want {
            if self.store.source(&id).is_some_and(|s| s.hash == hash) {
                continue;
            }
            let Some(i) = self
                .history
                .iter()
                .position(|(i, s)| *i == id && s.hash == hash)
            else {
                continue;
            };
            let (_, src) = self.history.remove(i);
            if let Some(cur) = self.store.source(&id).cloned() {
                self.keep_version(&id, cur);
            }
            self.store.insert(&id, src);
        }
    }
}
