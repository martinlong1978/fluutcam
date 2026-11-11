// SPDX-License-Identifier: MIT OR Apache-2.0

//! **Import tools** (design D16 §10.2): the ribbon command opens the native
//! file dialog for the Fusion file, then the native Save dialog for the new
//! library; the import runs on a worker thread and a small window shows the
//! counts and warnings. There is no path text box anywhere: the paths come
//! from the system dialogs only.

use super::{UiState, tool_libs};
use crate::dialogs::{DialogKind, Dialogs, Picked};
use crate::jobs::Edit;
use crate::jobs::Jobs;
use crate::text::tr;
use crate::vm::tool_import::{self, Done};
use eframe::egui;
use std::path::PathBuf;

/// The folder of the open project's file.
fn project_path(jobs: &Jobs) -> Option<PathBuf> {
    jobs.snapshot.as_ref().map(|s| s.path.clone())
}

/// The ribbon command: asks for the file to import.
pub fn ask(jobs: &Jobs, state: &mut UiState, dialogs: &mut Dialogs) {
    if !state.tool_import.is_free() || dialogs.is_open() {
        return;
    }
    state.tool_import.close();
    state.tool_lib_note = None;
    let dir = project_path(jobs).and_then(|p| p.parent().map(PathBuf::from));
    dialogs.request(DialogKind::ImportToolsSource, dir, "");
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
        DialogKind::ImportToolsSource => {
            if let Some(name) = state.tool_import.source_picked(p.path) {
                let dir = tool_import::start_dir(project_path(jobs).as_deref());
                dialogs.request(DialogKind::ImportToolsTarget, dir, &name);
            }
        }
        DialogKind::ImportToolsTarget => {
            let wake = ctx.clone();
            let replace = dialogs.confirms_overwrite();
            state
                .tool_import
                .target_picked(p.path, replace, move || wake.request_repaint());
        }
        _ => {}
    }
}

/// Once per frame: a cancelled Save dialog ends the import; the worker's
/// result is taken.
pub fn tick(ctx: &egui::Context, state: &mut UiState, dialogs: &Dialogs) {
    if state.tool_import.awaiting_target() && !dialogs.is_open() {
        state.tool_import.cancel();
    }
    state.tool_import.poll();
    if state.tool_import.is_running() {
        ctx.request_repaint();
    }
}

/// The window: working, or the result.
pub fn show(ctx: &egui::Context, jobs: &mut Jobs, state: &mut UiState) {
    if let Some(text) = state.tool_import.working_text() {
        window(ctx).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text);
            });
        });
        return;
    }
    let Some(done) = state.tool_import.done().cloned() else {
        return;
    };
    let mut close = false;
    let mut use_it = false;
    let note = state.tool_lib_note.clone();
    window(ctx).show(ctx, |ui| {
        ui.set_max_width(520.0);
        result(ui, jobs, &done, note.as_deref(), &mut close, &mut use_it);
    });
    if use_it {
        // A library in the project's `tools` folder is loaded by the
        // project already; the others are referred to by path.
        let used = match project_path(jobs) {
            Some(p) if tool_import::loads_with_project(&p, &done.target) => {
                jobs.edit(Edit::ReloadToolLibraries);
                true
            }
            _ => tool_libs::add(jobs, state, &done.target) != tool_libs::Used::Refused,
        };
        if used {
            state.tool_import.mark_used();
        }
    }
    if close {
        state.tool_import.close();
    }
}

fn window(_ctx: &egui::Context) -> egui::Window<'static> {
    egui::Window::new(tr("tools-import-title"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
}

fn result(
    ui: &mut egui::Ui,
    jobs: &Jobs,
    done: &Done,
    note: Option<&str>,
    close: &mut bool,
    use_it: &mut bool,
) {
    ui.label(egui::RichText::new(done.headline()).strong());
    for line in done.counts() {
        ui.label(line);
    }
    let dropped = done.dropped();
    if !dropped.is_empty() {
        egui::CollapsingHeader::new(tr("tools-import-dropped-list")).show(ui, |ui| {
            for (key, n) in dropped {
                ui.label(format!("{key}: {n}"));
            }
        });
    }
    let messages = done.messages();
    if done.ok() {
        ui.separator();
        if messages.is_empty() {
            ui.label(tr("tools-import-no-warnings"));
        } else {
            ui.label(egui::RichText::new(tr("tools-import-warnings")).strong());
        }
    }
    if !messages.is_empty() {
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .id_salt("tools_import_messages")
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
            Some(p) => {
                let inside = tool_import::loads_with_project(&p, &done.target);
                ui.label(tr(if inside {
                    "tools-import-in-project"
                } else {
                    "tools-import-elsewhere"
                }));
                if done.used {
                    ui.label(egui::RichText::new(tr("tools-import-used")).strong());
                } else if ui.button(tr("tools-import-use")).clicked() {
                    *use_it = true;
                }
                if let Some(n) = note {
                    ui.label(n);
                }
            }
            None => {
                ui.label(tr("tools-import-no-project"));
            }
        }
    }
    ui.add_space(4.0);
    if ui.button(tr("tools-import-close")).clicked() {
        *close = true;
    }
}
