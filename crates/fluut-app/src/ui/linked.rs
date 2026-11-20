// SPDX-License-Identifier: MIT OR Apache-2.0

//! Linked models in the window (spec 03 §3.5 item 4): the notice
//! "Linked model changed: Update", the update report, the link in the
//! Model panel, a broken toolpath's "Pick again", and opening a `.craftx`
//! file from the command line or from Import model.

use super::UiState;
use crate::brand::{self, Palette, c};
use crate::dialogs::{DialogKind, Dialogs};
use crate::jobs::{JobKind, Jobs, LinkRead, Outcome};
use crate::text::{data, tr, tr_args};
use crate::vm::Selection;
use crate::vm::link::{LinkPlan, ReportVm, broken_rows, panels, plan};
use crate::vm::pick::{PickMode, remove_edit};
use eframe::egui;
use fluut_model::Sel;
use fluut_model::project::DisplayUnit;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// A `.craftx` file waiting for a new project to be made for it, and the
/// count of finished jobs when that started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewForLink {
    pub path: PathBuf,
    pub since: u64,
}

/// "Update instead?" for a `.craftx` file whose design the project links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub read: LinkRead,
    pub source: String,
    /// The file came from the command line: "open in a new project" is
    /// the other answer.
    pub launch: bool,
}

fn file_label(p: &Path) -> String {
    p.file_name().map_or_else(
        || data(p.display().to_string()),
        |n| data(n.to_string_lossy().into_owned()),
    )
}

/// The work of every frame: change detection, the answers of finished
/// jobs, and the steps of opening a `.craftx` file.
pub fn tick(ctx: &egui::Context, jobs: &mut Jobs, state: &mut UiState, dialogs: &mut Dialogs) {
    let files = jobs
        .snapshot
        .as_ref()
        .map(|s| s.linked_files.clone())
        .unwrap_or_default();
    let focused = ctx.input(|i| i.focused);
    let wake = ctx.clone();
    if let Some(wait) = state.links.tick(focused, Instant::now(), &files, move || {
        wake.request_repaint()
    }) {
        ctx.request_repaint_after(wait);
    }
    if let Some(reports) = jobs.updated.take() {
        let sources: Vec<String> = reports.iter().map(|r| r.source.clone()).collect();
        state.links.updated(&sources);
        state.update_report = Some(ReportVm::new(&reports));
    }
    if let Some(read) = jobs.link_read.take() {
        let launch = state.launching.take_if(|p| *p == read.path).is_some();
        match plan(&read, jobs.snapshot.is_some(), launch) {
            LinkPlan::OfferUpdate { source } => {
                state.link_offer = Some(Offer {
                    read,
                    source,
                    launch,
                })
            }
            LinkPlan::Import => jobs.import_model(read.path),
            LinkPlan::NewProject => new_project_for(jobs, state, dialogs, read.path),
        }
    }
    // A file from the command line: read it once the project (if one was
    // given too) has opened.
    if let Some(p) = state.launching.clone()
        && state.launch_sent.is_none()
        && !jobs.is_busy()
        && !dialogs.is_open()
    {
        state.launch_sent = Some(p.clone());
        jobs.read_link(p);
    }
    // The file could not be read: the status line says why, and nothing
    // more happens with it.
    if state.launch_sent.is_some()
        && !jobs.is_busy()
        && matches!(jobs.last, Some((JobKind::ReadLink, Outcome::Failed(_))))
    {
        state.launching = None;
    }
    // The new project for a linked file: import it once made, or give up
    // when the user cancelled the dialog or the unit.
    let idle = !jobs.is_busy()
        && !dialogs.is_open()
        && state.new_project.is_none()
        && state.pending.is_none();
    if idle && let Some(n) = state.new_for_link.take() {
        let made = jobs.finished > n.since
            && matches!(jobs.last, Some((JobKind::New, Outcome::Created { .. })));
        if made {
            jobs.import_model(n.path);
        }
    }
}

/// Asks where the new project for `path` goes: beside the file, named
/// after it.
fn new_project_for(jobs: &Jobs, state: &mut UiState, dialogs: &mut Dialogs, path: PathBuf) {
    let dir = path.parent().map(Path::to_path_buf);
    let stem = path.file_stem().map_or_else(
        || "project".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    state.new_for_link = Some(NewForLink {
        path,
        since: jobs.finished,
    });
    dialogs.request(
        DialogKind::New(DisplayUnit::Mm),
        dir,
        &format!("{stem}.fluut"),
    );
}

/// The notice in the status bar while a linked file has changed.
pub fn notice(ui: &mut egui::Ui, jobs: &mut Jobs, state: &mut UiState) {
    let Some(s) = jobs.snapshot.clone() else {
        return;
    };
    let changed = state.links.changed(&s.linked_files);
    if changed.is_empty() {
        return;
    }
    let file = changed
        .iter()
        .map(|c| file_label(&c.path))
        .collect::<Vec<_>>()
        .join(", ");
    let sources: Vec<String> = changed.iter().map(|c| c.source.clone()).collect();
    ui.separator();
    ui.label(
        egui::RichText::new(tr_args("link-changed", &[("file", &file)]))
            .color(c(brand::AMBER))
            .strong(),
    );
    if ui
        .add_enabled(!jobs.is_busy(), egui::Button::new(tr("link-update")))
        .on_hover_text(tr("link-update-hover"))
        .clicked()
    {
        jobs.update(sources, None);
    }
    if ui.button(tr("link-later")).clicked() {
        state.links.later(&s.linked_files);
    }
}

/// The update report: every outcome, with the toolpath's name and a way to
/// select it.
pub fn report(ctx: &egui::Context, state: &mut UiState) {
    let Some(vm) = state.update_report.clone() else {
        return;
    };
    let mut close = false;
    let mut show = None;
    egui::Window::new(&vm.title)
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    for src in &vm.sources {
                        ui.strong(&src.heading);
                        for n in &src.notes {
                            ui.label(n);
                        }
                        egui::Grid::new(("report_grid", &src.heading))
                            .num_columns(4)
                            .spacing([10.0, 4.0])
                            .show(ui, |ui| {
                                for r in &src.rows {
                                    let col = match r.outcome {
                                        fluut_core::names::Outcome::Exact => brand::GREEN,
                                        fluut_core::names::Outcome::Missing => brand::RED,
                                        _ => brand::AMBER,
                                    };
                                    ui.label(egui::RichText::new(&r.label).color(c(col)).strong());
                                    ui.label(egui::RichText::new(&r.toolpath_name).strong());
                                    ui.add(egui::Label::new(&r.text).wrap());
                                    if ui.small_button(tr("link-report-show")).clicked() {
                                        show = Some(r.toolpath.clone());
                                    }
                                    ui.end_row();
                                }
                            });
                        ui.add_space(6.0);
                    }
                });
            ui.separator();
            ui.label(&vm.summary);
            if vm.broken {
                ui.label(tr("link-report-broken-hint"));
            }
            if ui.button(tr("link-report-close")).clicked() {
                close = true;
            }
        });
    if let Some(id) = show {
        state.selection = Selection::Toolpath(id);
    }
    if close {
        state.update_report = None;
    }
}

/// "This project already links the design: update it instead?"
pub fn offer(ctx: &egui::Context, jobs: &mut Jobs, state: &mut UiState, dialogs: &mut Dialogs) {
    let Some(o) = state.link_offer.clone() else {
        return;
    };
    let mut answer = None;
    egui::Window::new(tr("link-offer-title"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(tr_args(
                "link-offer-text",
                &[("file", &file_label(&o.read.path))],
            ));
            ui.horizontal(|ui| {
                if ui.button(tr("link-offer-update")).clicked() {
                    answer = Some(0);
                }
                if o.launch && ui.button(tr("link-offer-new")).clicked() {
                    answer = Some(1);
                }
                if ui.button(tr("link-offer-cancel")).clicked() {
                    answer = Some(2);
                }
            });
        });
    match answer {
        Some(0) => {
            state.link_offer = None;
            jobs.update(vec![o.source], Some(o.read.path));
        }
        Some(1) => {
            state.link_offer = None;
            new_project_for(jobs, state, dialogs, o.read.path);
        }
        Some(_) => state.link_offer = None,
        None => {}
    }
}

/// The linked models in the Model panel: where the design comes from, its
/// revision, when it was saved, its parameters (read only), and Update.
pub fn model_panel(ui: &mut egui::Ui, jobs: &mut Jobs, state: &mut UiState, p: &Palette) {
    let Some(s) = jobs.snapshot.clone() else {
        return;
    };
    let changed: Vec<String> = state
        .links
        .changed(&s.linked_files)
        .iter()
        .map(|c| c.source.clone())
        .collect();
    for panel in panels(&s, &changed) {
        ui.add_space(4.0);
        ui.strong(&panel.heading);
        for l in &panel.lines {
            ui.label(egui::RichText::new(l).small());
        }
        if panel.changed {
            ui.label(
                egui::RichText::new(tr("link-changed-mark"))
                    .small()
                    .color(c(brand::AMBER)),
            );
        }
        egui::CollapsingHeader::new(tr("link-params"))
            .id_salt(("params_list", &panel.source))
            .default_open(false)
            .show(ui, |ui| {
                if panel.params.is_empty() {
                    ui.label(egui::RichText::new(tr("link-params-none")).color(c(p.weak_text)));
                }
                egui::Grid::new(("params_grid", &panel.source))
                    .num_columns(2)
                    .show(ui, |ui| {
                        for (n, v) in &panel.params {
                            ui.label(n);
                            ui.label(v).on_hover_text(tr("inspector-read-only"));
                            ui.end_row();
                        }
                    });
            });
        if ui
            .add_enabled(!jobs.is_busy(), egui::Button::new(tr("link-update")))
            .on_hover_text(tr("link-update-hover"))
            .clicked()
        {
            jobs.update(vec![panel.source.clone()], None);
        }
    }
}

/// At the top of a broken toolpath's inspector: what is missing, and
/// "Pick again", which takes the lost pick out and starts picking that
/// field on the model.
pub fn broken_banner(ui: &mut egui::Ui, jobs: &mut Jobs, state: &mut UiState, tp: &str) {
    let Some(s) = jobs.snapshot.clone() else {
        return;
    };
    let rows = broken_rows(&s, tp);
    if rows.is_empty() {
        return;
    }
    let Some((_, t)) = s.project.toolpath(tp) else {
        return;
    };
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, c(brand::RED)))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(6.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(tr("broken-title"))
                    .color(c(brand::RED))
                    .strong(),
            );
            for r in &rows {
                ui.add(egui::Label::new(&r.text).wrap());
                if ui.button(tr("broken-pick-again")).clicked() {
                    let sel = if r.id.starts_with("e-") {
                        Sel::Edge(r.id.clone())
                    } else {
                        Sel::Face(r.id.clone())
                    };
                    jobs.edit(remove_edit(&s, tp, &r.field, &sel));
                    state.pick = Some(PickMode::new(t.kind(), tp, &r.field));
                    let group = r.field.split('.').next().unwrap_or_default();
                    if let Some(i) = fluut_model::Group::ALL
                        .iter()
                        .position(|g| g.key() == group)
                    {
                        state.inspector_tab = i;
                    }
                }
            }
        });
    ui.add_space(4.0);
}

/// Whether `path` is a linked model file.
pub fn is_linked_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("craftx"))
}
