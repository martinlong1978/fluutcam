// SPDX-License-Identifier: MIT OR Apache-2.0

//! What happens with the answer of a file dialog, and the "discard unsaved
//! changes?" question that a project-replacing answer can raise. The ribbon
//! asks the dialog (`ui::ribbon`); the frame passes the path here.

use super::UiState;
use crate::dialogs::{DialogKind, Dialogs, Picked};
use crate::jobs::{Jobs, PostRequest};
use crate::text::tr;
use eframe::egui;
use fluut_model::project::DisplayUnit;
use std::path::PathBuf;

/// A project-replacing action waiting for the answer to "discard changes?".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pending {
    Open(PathBuf),
    New(PathBuf, DisplayUnit),
}

/// The folder of the open project's file.
fn dir_of(jobs: &Jobs) -> Option<PathBuf> {
    jobs.snapshot
        .as_ref()
        .and_then(|s| s.path.parent().map(PathBuf::from))
}

/// The name a save dialog offers for `kind`.
fn name_for(jobs: &Jobs, state: &UiState, kind: DialogKind) -> String {
    let project = || {
        jobs.snapshot
            .as_ref()
            .map_or_else(|| "project.fluut".to_owned(), |s| s.file_name())
    };
    match kind {
        DialogKind::PostOutput => {
            let stem = jobs
                .snapshot
                .as_ref()
                .and_then(|s| s.path.file_stem().map(|n| n.to_string_lossy().into_owned()));
            let ext = crate::posts::extension(&state.post_id);
            format!("{}.{ext}", stem.unwrap_or_else(|| "output".to_owned()))
        }
        _ => project(),
    }
}

/// Opens the dialog of `kind`, in the project's folder.
pub fn ask(jobs: &Jobs, state: &UiState, dialogs: &mut Dialogs, kind: DialogKind) {
    let name = name_for(jobs, state, kind);
    dialogs.request(kind, dir_of(jobs), &name);
}

/// The unit of a new project: the native dialog cannot ask, so a small
/// window does, after it. The unit asked for comes first.
pub fn units_prompt(ctx: &egui::Context, jobs: &mut Jobs, state: &mut UiState) {
    let Some((path, first)) = state.new_project.clone() else {
        return;
    };
    let units = if first == DisplayUnit::Inch {
        [DisplayUnit::Inch, DisplayUnit::Mm]
    } else {
        [DisplayUnit::Mm, DisplayUnit::Inch]
    };
    let mut answer = None;
    let mut cancelled = false;
    egui::Window::new(tr("units-title"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(tr("units-text"));
            ui.horizontal(|ui| {
                for u in units {
                    let id = match u {
                        DisplayUnit::Mm => "units-mm",
                        DisplayUnit::Inch => "units-inch",
                    };
                    if ui.button(tr(id)).clicked() {
                        answer = Some(u);
                    }
                }
                cancelled = ui.button(tr("units-cancel")).clicked();
            });
        });
    if cancelled {
        state.new_project = None;
    } else if let Some(u) = answer {
        state.new_project = None;
        let new = Pending::New(path, u);
        if jobs.snapshot.as_ref().is_some_and(|s| s.dirty) {
            state.pending = Some(new);
        } else {
            run(jobs, new);
        }
    }
}

/// The "discard unsaved changes?" window for a pending Open or New.
pub fn discard_prompt(ctx: &egui::Context, jobs: &mut Jobs, state: &mut UiState) {
    let Some(p) = state.pending.clone() else {
        return;
    };
    let mut answer = None;
    egui::Window::new(tr("discard-title"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(tr("discard-text"));
            ui.horizontal(|ui| {
                if ui.button(tr("discard-yes")).clicked() {
                    answer = Some(true);
                }
                if ui.button(tr("discard-no")).clicked() {
                    answer = Some(false);
                }
            });
        });
    match answer {
        Some(true) => {
            state.pending = None;
            run(jobs, p);
        }
        Some(false) => state.pending = None,
        None => {}
    }
}

fn run(jobs: &mut Jobs, p: Pending) {
    match p {
        Pending::Open(path) => jobs.open(path),
        Pending::New(path, units) => jobs.new_project(path, units),
    }
}

/// Acts on a path the user chose in a dialog. Replacing a project with
/// unsaved changes asks first.
pub fn picked(jobs: &mut Jobs, state: &mut UiState, p: Picked) {
    use DialogKind as K;
    let dirty = jobs.snapshot.as_ref().is_some_and(|s| s.dirty);
    let replace = match p.kind {
        K::Open => Some(Pending::Open(p.path.clone())),
        _ => None,
    };
    match (p.kind, replace) {
        (K::New(u), _) => state.new_project = Some((p.path, u)),
        (_, Some(r)) if dirty => state.pending = Some(r),
        (_, Some(r)) => run(jobs, r),
        (K::SaveAs, None) => jobs.save_as(p.path),
        (K::Import, None) => jobs.import_model(p.path),
        (K::PostOutput, None) => {
            let mut output = p.path;
            if output.extension().is_none() {
                output.set_extension(crate::posts::extension(&state.post_id));
            }
            let (date, time) = (state.clock)();
            jobs.post(PostRequest {
                post: state.post_id.clone(),
                output,
                date,
                time,
            });
        }
        _ => {}
    }
}
