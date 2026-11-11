// SPDX-License-Identifier: MIT OR Apache-2.0

//! **Import post** (design D10 §6.1): the ribbon command opens the native
//! file dialog for the Vectric `.pp` file, then the native Save dialog for
//! the new post; the import runs on a worker thread and a small window
//! shows the counts and warnings. There is no path text box anywhere: the
//! paths come from the system dialogs only.

use super::UiState;
use crate::dialogs::{DialogKind, Dialogs, Picked};
use crate::jobs::Edit;
use crate::jobs::Jobs;
use crate::text::tr;
use crate::vm::post_import::{self, Done};
use eframe::egui;
use std::path::PathBuf;

/// The open project's file.
fn project_path(jobs: &Jobs) -> Option<PathBuf> {
    jobs.snapshot.as_ref().map(|s| s.path.clone())
}

/// The ribbon command: asks for the file to import.
pub fn ask(jobs: &Jobs, state: &mut UiState, dialogs: &mut Dialogs) {
    if !state.post_import.is_free() || dialogs.is_open() {
        return;
    }
    state.post_import.close();
    let dir = project_path(jobs).and_then(|p| p.parent().map(PathBuf::from));
    dialogs.request(DialogKind::ImportPostSource, dir, "");
}

/// A dialog of the import gave a path.
pub fn picked(
    ctx: &egui::Context,
    jobs: &Jobs,
    state: &mut UiState,
    dialogs: &mut Dialogs,
    p: Picked,
) {
    match p.kind {
        DialogKind::ImportPostSource => {
            if let Some(name) = state.post_import.source_picked(p.path) {
                let dir = post_import::start_dir(project_path(jobs).as_deref());
                dialogs.request(DialogKind::ImportPostTarget, dir, &name);
            }
        }
        DialogKind::ImportPostTarget => {
            let wake = ctx.clone();
            let replace = dialogs.confirms_overwrite();
            state
                .post_import
                .target_picked(p.path, replace, move || wake.request_repaint());
        }
        _ => {}
    }
}

/// Once per frame: a cancelled Save dialog ends the import; the worker's
/// result is taken.
pub fn tick(ctx: &egui::Context, state: &mut UiState, dialogs: &Dialogs) {
    if state.post_import.awaiting_target() && !dialogs.is_open() {
        state.post_import.cancel();
    }
    state.post_import.poll();
    if state.post_import.is_running() {
        ctx.request_repaint();
    }
}

/// The window: working, or the result.
pub fn show(ctx: &egui::Context, jobs: &mut Jobs, state: &mut UiState) {
    if let Some(text) = state.post_import.working_text() {
        window(ctx).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text);
            });
        });
        return;
    }
    let Some(done) = state.post_import.done().cloned() else {
        return;
    };
    let mut close = false;
    let mut use_it = false;
    window(ctx).show(ctx, |ui| {
        ui.set_max_width(520.0);
        result(ui, jobs, &done, &mut close, &mut use_it);
    });
    if use_it {
        // The project finds the post once its `posts` folder is read again;
        // Post then offers it and it is the chosen one.
        jobs.edit(Edit::ReloadDefinitions);
        state.post_id = done.post_id.clone();
        state.post_import.mark_used();
    }
    if close {
        state.post_import.close();
    }
}

fn window(_ctx: &egui::Context) -> egui::Window<'static> {
    egui::Window::new(tr("posts-import-title"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
}

fn result(ui: &mut egui::Ui, jobs: &Jobs, done: &Done, close: &mut bool, use_it: &mut bool) {
    ui.label(egui::RichText::new(done.headline()).strong());
    for line in done.counts() {
        ui.label(line);
    }
    let messages = done.messages();
    if done.ok() {
        ui.separator();
        if messages.is_empty() {
            ui.label(tr("posts-import-no-warnings"));
        } else {
            ui.label(egui::RichText::new(tr("posts-import-warnings")).strong());
        }
    }
    if !messages.is_empty() {
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .id_salt("posts_import_messages")
            .show(ui, |ui| {
                for (error, code, text) in messages {
                    ui.horizontal_wrapped(|ui| {
                        let sev = if error {
                            "severity-error"
                        } else {
                            "severity-warning"
                        };
                        ui.label(egui::RichText::new(tr(sev)).strong());
                        ui.label(text).on_hover_text(&code);
                        ui.label(egui::RichText::new(format!("[{code}]")).weak());
                    });
                }
            });
    }
    if done.ok() {
        ui.separator();
        match project_path(jobs) {
            Some(p) if post_import::loads_with_project(&p, &done.target) => {
                ui.label(tr("posts-import-in-project"));
                if done.used {
                    ui.label(egui::RichText::new(tr("posts-import-used")).strong());
                } else if ui.button(tr("posts-import-use")).clicked() {
                    *use_it = true;
                }
            }
            Some(_) => {
                ui.label(tr("posts-import-elsewhere"));
            }
            None => {
                ui.label(tr("posts-import-no-project"));
            }
        }
    }
    ui.add_space(4.0);
    if ui.button(tr("posts-import-close")).clicked() {
        *close = true;
    }
}
