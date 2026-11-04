// SPDX-License-Identifier: MIT OR Apache-2.0

//! TEMPORARY File menu and shortcuts for the project operations (plan
//! 2026-11-04-app-project-ops). The ribbon rebuild (`ui-ribbon-icons`) wires
//! the same `Jobs` methods and dialogs to ribbon commands; when that lands,
//! this module and its call in `ui/mod.rs` are deleted.

use super::UiState;
use crate::dialogs::{DialogKind, Dialogs};
use crate::jobs::Jobs;
use crate::text::tr;
use crate::text::tr_args;
use crate::vm::Selection;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use fluut_model::ToolpathTypeId;
use fluut_model::project::{DisplayUnit, SetupKind};
use std::path::PathBuf;

/// A project-replacing action waiting for the answer to "discard changes?".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pending {
    Open(PathBuf),
    New(PathBuf, DisplayUnit),
}

const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const IMPORT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
const REDO_ALT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);

/// The Setup an Add toolpath goes to: the selected one, or the first.
pub fn target_setup(jobs: &Jobs, sel: &Selection) -> Option<String> {
    let s = jobs.snapshot.as_ref()?;
    match sel {
        Selection::Setup(id) | Selection::Face { setup: id, .. } => s.project.setup(id),
        Selection::Toolpath(t) => s.project.toolpath(t).map(|(st, _)| st),
        Selection::None => None,
    }
    .or_else(|| s.project.setups.first().map(|x| &**x))
    .map(|x| x.id.clone())
}

fn kind_label(k: SetupKind) -> String {
    tr(&format!("setup-kind-{}", k.as_str()))
}

fn dir_of(jobs: &Jobs) -> Option<PathBuf> {
    jobs.snapshot
        .as_ref()
        .and_then(|s| s.path.parent().map(PathBuf::from))
}

fn name_of(jobs: &Jobs) -> String {
    jobs.snapshot
        .as_ref()
        .map_or_else(|| "project.fluut".to_owned(), |s| s.file_name())
}

/// The menu bar; call it inside a panel.
pub fn bar(ui: &mut egui::Ui, jobs: &mut Jobs, state: &mut UiState, dialogs: &mut Dialogs) {
    let has_project = jobs.snapshot.is_some();
    let idle = !jobs.is_busy();
    let can_undo = idle && jobs.snapshot.as_ref().is_some_and(|s| s.can_undo);
    let can_redo = idle && jobs.snapshot.as_ref().is_some_and(|s| s.can_redo);
    let setup = target_setup(jobs, &state.selection);
    let (dir, name) = (dir_of(jobs), name_of(jobs));
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(tr("menu-file"), |ui| {
            let mut ask = |ui: &mut egui::Ui, id: &str, kind: DialogKind, on: bool| {
                if ui.add_enabled(on, egui::Button::new(tr(id))).clicked() {
                    dialogs.request(kind, dir.clone(), &name);
                    ui.close();
                }
            };
            ask(ui, "menu-new-mm", DialogKind::New(DisplayUnit::Mm), idle);
            ask(
                ui,
                "menu-new-inch",
                DialogKind::New(DisplayUnit::Inch),
                idle,
            );
            ask(ui, "menu-open", DialogKind::Open, idle);
            ui.separator();
            if ui
                .add_enabled(
                    idle && has_project,
                    egui::Button::new(tr("menu-save"))
                        .shortcut_text(ui.ctx().format_shortcut(&SAVE)),
                )
                .clicked()
            {
                jobs.save();
                ui.close();
            }
            ask(ui, "menu-save-as", DialogKind::SaveAs, idle && has_project);
            ask(ui, "menu-import", DialogKind::Import, idle && has_project);
            ui.separator();
            let undo_text = jobs
                .snapshot
                .as_ref()
                .and_then(|s| s.undo_label.clone())
                .filter(|_| can_undo)
                .map_or_else(
                    || tr("menu-undo"),
                    |l| tr_args("menu-undo-label", &[("label", &l)]),
                );
            if ui
                .add_enabled(
                    can_undo,
                    egui::Button::new(undo_text).shortcut_text(ui.ctx().format_shortcut(&UNDO)),
                )
                .clicked()
            {
                jobs.undo();
                ui.close();
            }
            if ui
                .add_enabled(
                    can_redo,
                    egui::Button::new(tr("menu-redo"))
                        .shortcut_text(ui.ctx().format_shortcut(&REDO)),
                )
                .clicked()
            {
                jobs.redo();
                ui.close();
            }
            ui.separator();
            ui.add_enabled_ui(idle && has_project, |ui| {
                ui.menu_button(tr("menu-add-setup"), |ui| {
                    for k in SetupKind::ALL {
                        if ui.button(kind_label(k)).clicked() {
                            jobs.add_setup(k, None);
                            ui.close();
                        }
                    }
                });
            });
            ui.add_enabled_ui(idle && setup.is_some(), |ui| {
                ui.menu_button(tr("menu-add-setup-based"), |ui| {
                    for k in SetupKind::ALL {
                        if ui.button(kind_label(k)).clicked() {
                            jobs.add_setup(k, setup.as_deref());
                            ui.close();
                        }
                    }
                });
                ui.menu_button(tr("menu-add-toolpath"), |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(420.0)
                        .show(ui, |ui| {
                            for t in ToolpathTypeId::ALL {
                                let label = tr(&format!("toolpath-type-{}", t.as_str()));
                                if ui.button(label).clicked() {
                                    if let Some(s) = &setup {
                                        jobs.add_toolpath(s, t);
                                    }
                                    ui.close();
                                }
                            }
                        });
                });
            });
        });
    });
}

/// Keyboard shortcuts of the menu.
pub fn shortcuts(ctx: &egui::Context, jobs: &mut Jobs, dialogs: &mut Dialogs) {
    let typing = ctx.egui_wants_keyboard_input();
    let (save_as, save, open, new, import, undo, redo) = ctx.input_mut(|i| {
        (
            i.consume_shortcut(&SAVE_AS),
            i.consume_shortcut(&SAVE),
            i.consume_shortcut(&OPEN),
            i.consume_shortcut(&NEW),
            i.consume_shortcut(&IMPORT),
            !typing && i.consume_shortcut(&UNDO),
            !typing && (i.consume_shortcut(&REDO) || i.consume_shortcut(&REDO_ALT)),
        )
    });
    let has_project = jobs.snapshot.is_some();
    if save && has_project {
        jobs.save();
    }
    if undo {
        jobs.undo();
    }
    if redo {
        jobs.redo();
    }
    let (dir, name) = (dir_of(jobs), name_of(jobs));
    if save_as && has_project {
        dialogs.request(DialogKind::SaveAs, dir.clone(), &name);
    }
    if open {
        dialogs.request(DialogKind::Open, dir.clone(), &name);
    }
    if new {
        dialogs.request(DialogKind::New(DisplayUnit::Mm), dir.clone(), &name);
    }
    if import && has_project {
        dialogs.request(DialogKind::Import, dir, &name);
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
pub fn picked(jobs: &mut Jobs, state: &mut UiState, p: crate::dialogs::Picked) {
    use crate::dialogs::DialogKind as K;
    let dirty = jobs.snapshot.as_ref().is_some_and(|s| s.dirty);
    let replace = match p.kind {
        K::Open => Some(Pending::Open(p.path.clone())),
        K::New(u) => Some(Pending::New(p.path.clone(), u)),
        _ => None,
    };
    match (p.kind, replace) {
        (_, Some(r)) if dirty => state.pending = Some(r),
        (_, Some(r)) => run(jobs, r),
        (K::SaveAs, None) => jobs.save_as(p.path),
        (K::Import, None) => jobs.import_model(p.path),
        _ => {}
    }
}
