// SPDX-License-Identifier: MIT OR Apache-2.0

//! The widget layer (D13 §6): `egui` with `egui_dock`.
//!
//! Widgets read view models and send requests to the job layer; they never
//! change the document themselves (D13 §1). The window repaints on input
//! and when the worker wakes it; while a job runs it repaints each frame so
//! the progress shows.

mod about;
mod browser;
mod inspector;
mod ribbon;
mod viewport;

use crate::bench::Bench;
use crate::brand::{self, Palette, c};
use crate::jobs::{Edit, Jobs};
use crate::text::{diagnostic, tr};
use crate::vm::Selection;
use crate::vm::status::StatusVm;
use eframe::egui::{self, Color32, CornerRadius, Stroke};
use eframe::egui_wgpu;
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

pub use viewport::ViewportState;

/// Dock tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    Browser,
    Viewport,
    Inspector,
    Messages,
}

/// UI state that is not part of the document (D13 §2 `UiState`).
#[derive(Debug)]
pub struct UiState {
    pub selection: Selection,
    pub dark: bool,
    /// Text of the Open field.
    pub open_path: String,
    pub post_id: String,
    /// Text of the output file field.
    pub post_output: String,
    pub show_about: bool,
    /// Inspector tab per toolpath type (by group index).
    pub inspector_tab: usize,
    /// Text being typed into number fields, by field path, until it is
    /// committed.
    pub entries: BTreeMap<String, String>,
    /// Why the last entry was refused: `(field path, message)`.
    pub entry_error: Option<(String, String)>,
    /// Date and time for the post header; tests pin it.
    pub clock: fn() -> (String, String),
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            selection: Selection::None,
            dark: true,
            open_path: String::new(),
            post_id: "fluut/linuxcnc".into(),
            post_output: String::new(),
            show_about: false,
            inspector_tab: 3,
            entries: BTreeMap::new(),
            entry_error: None,
            clock: crate::session::now_utc,
        }
    }
}

/// The application.
pub struct FluutApp {
    pub jobs: Jobs,
    pub ui: UiState,
    pub view: ViewportState,
    dock: DockState<Tab>,
    render: Option<egui_wgpu::RenderState>,
    bench: Bench,
    /// Snapshot revision the selection was last checked against.
    seen: u64,
}

impl std::fmt::Debug for FluutApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluutApp")
            .field("ui", &self.ui)
            .finish_non_exhaustive()
    }
}

/// The default layout: browser left, inspector right, messages below the
/// viewport (S 08 §8.2).
fn default_dock() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Viewport]);
    let tree = dock.main_surface_mut();
    let [rest, _] = tree.split_left(NodeIndex::root(), 0.2, vec![Tab::Browser]);
    let [rest, _] = tree.split_right(rest, 0.72, vec![Tab::Inspector]);
    tree.split_below(rest, 0.8, vec![Tab::Messages]);
    dock
}

impl FluutApp {
    /// Builds the app. `render` is `None` without a GPU (tests): the
    /// viewport then shows a notice. `open` is a project to open at start.
    pub fn new(
        ctx: &egui::Context,
        render: Option<egui_wgpu::RenderState>,
        open: Option<PathBuf>,
        t0: Instant,
    ) -> FluutApp {
        ctx.set_fonts(brand::fonts());
        let ui = UiState::default();
        brand::apply_theme(ctx, ui.dark);
        let wake = ctx.clone();
        let mut jobs = Jobs::start(move || wake.request_repaint());
        if let Some(p) = open {
            jobs.open(p);
        }
        FluutApp {
            jobs,
            view: ViewportState::new(render.as_ref()),
            ui,
            dock: default_dock(),
            render,
            bench: Bench::from_env(t0),
            seen: 0,
        }
    }

    /// Requests from the widgets go through here.
    pub fn edit(&mut self, e: Edit) {
        self.jobs.edit(e);
    }

    /// Keeps the selection valid after the document changes, and fills the
    /// output path from the project name once.
    fn on_snapshot(&mut self) {
        let Some(s) = self.jobs.snapshot.clone() else {
            return;
        };
        if s.revision == self.seen {
            return;
        }
        let first = self.seen == 0 || s.revision < self.seen;
        self.seen = s.revision;
        if first || self.ui.post_output.is_empty() {
            let stem = s
                .path
                .file_stem()
                .map_or_else(|| "output".to_owned(), |n| n.to_string_lossy().into_owned());
            self.ui.post_output = s
                .path
                .with_file_name(format!("{stem}.ngc"))
                .display()
                .to_string();
            self.ui.open_path = s.path.display().to_string();
        }
        let valid = match &self.ui.selection {
            Selection::None => false,
            Selection::Setup(id) | Selection::Face { setup: id, .. } => {
                s.project.setup(id).is_some()
            }
            Selection::Toolpath(id) => s.project.toolpath(id).is_some(),
        };
        if !valid {
            self.ui.selection = s
                .project
                .setups
                .first()
                .map_or(Selection::None, |st| Selection::Setup(st.id.clone()));
        }
        if let Some(e) = &s.edit_error {
            self.ui.entry_error = Some((String::new(), e.clone()));
        }
    }

    /// One frame of UI. Separate from `eframe::App` so tests can drive it.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        if self.jobs.poll() {
            self.on_snapshot();
        }
        if self.bench.frame(&mut self.view.camera) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.bench.continuous() || self.jobs.is_busy() {
            ui.ctx().request_repaint();
        }
        let p = *Palette::get(self.ui.dark);
        egui::Panel::top("title")
            .frame(egui::Frame::new().fill(c(p.base)).inner_margin(6.0))
            .show(ui, |ui| self.title_bar(ui, &p));
        egui::Panel::top("ribbon")
            .frame(egui::Frame::new().fill(c(p.raised)).inner_margin(6.0))
            .show(ui, |ui| ribbon::show(ui, &mut self.jobs, &mut self.ui));
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(c(p.base)).inner_margin(6.0))
            .show(ui, |ui| self.status_bar(ui, &p));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(c(p.base)))
            .show(ui, |ui| {
                let mut tabs = Tabs {
                    jobs: &mut self.jobs,
                    ui: &mut self.ui,
                    view: &mut self.view,
                    render: self.render.as_ref(),
                };
                DockArea::new(&mut self.dock)
                    .show_close_buttons(false)
                    .show_leaf_close_all_buttons(false)
                    .tab_context_menus(false)
                    .show_inside(ui, &mut tabs);
            });
        about::show(ui.ctx(), &mut self.ui);
    }

    fn title_bar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(r.center(), 6.0, c(brand::CUTTER_ORANGE));
            ui.label(egui::RichText::new(tr("app-title")).strong());
            let name = self
                .jobs
                .snapshot
                .as_ref()
                .map_or_else(|| tr("app-untitled"), |s| s.file_name());
            ui.label(egui::RichText::new(name).color(c(p.weak_text)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("title-about")).clicked() {
                    self.ui.show_about = true;
                }
                let theme = if self.ui.dark {
                    "title-theme-light"
                } else {
                    "title-theme-dark"
                };
                if ui.button(tr(theme)).clicked() {
                    self.ui.dark = !self.ui.dark;
                    brand::apply_theme(ui.ctx(), self.ui.dark);
                }
                if let Some(s) = &self.jobs.snapshot {
                    ui.add(
                        egui::Button::new(
                            egui::RichText::new(s.project.units.as_str())
                                .strong()
                                .color(c(brand::INK)),
                        )
                        .fill(c(brand::STOCK_GREY)),
                    )
                    .on_hover_text(tr("title-units"));
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let vm = StatusVm::new(&self.jobs);
        ui.horizontal(|ui| {
            let colour = if vm.failed { c(brand::RED) } else { c(p.text) };
            if let Some(f) = vm.progress {
                ui.add(
                    egui::ProgressBar::new(f)
                        .desired_width(160.0)
                        .show_percentage(),
                );
            }
            ui.label(egui::RichText::new(&vm.text).color(colour));
            if vm.cancellable && ui.button(tr("ribbon-cancel")).clicked() {
                self.jobs.cancel();
            }
        });
    }
}

impl eframe::App for FluutApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }
}

struct Tabs<'a> {
    jobs: &'a mut Jobs,
    ui: &'a mut UiState,
    view: &'a mut ViewportState,
    render: Option<&'a egui_wgpu::RenderState>,
}

impl TabViewer for Tabs<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        tr(match tab {
            Tab::Browser => "tab-browser",
            Tab::Viewport => "tab-viewport",
            Tab::Inspector => "tab-inspector",
            Tab::Messages => "tab-messages",
        })
        .into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match tab {
            Tab::Browser => browser::show(ui, self.jobs, self.ui),
            Tab::Viewport => viewport::show(ui, self.jobs, self.ui, self.view, self.render),
            Tab::Inspector => inspector::show(ui, self.jobs, self.ui),
            Tab::Messages => messages(ui, self.jobs, self.ui),
        }
    }
}

/// Load warnings and the diagnostics of every toolpath.
fn messages(ui: &mut egui::Ui, jobs: &Jobs, state: &UiState) {
    let Some(s) = &jobs.snapshot else {
        ui.label(tr("messages-empty"));
        return;
    };
    let mut rows: Vec<(String, &fluut_model::Diagnostic)> =
        s.warnings.iter().map(|d| (String::new(), d)).collect();
    for st in &s.project.setups {
        for t in &st.toolpaths {
            rows.extend(s.diagnostics(&t.id).iter().map(|d| (t.name.clone(), d)));
        }
    }
    if rows.is_empty() {
        ui.label(tr("messages-empty"));
        return;
    }
    let p = Palette::get(state.dark);
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (who, d) in rows {
            ui.horizontal_wrapped(|ui| {
                let (sev, col) = if d.is_error() {
                    ("severity-error", brand::RED)
                } else {
                    ("severity-warning", brand::AMBER)
                };
                ui.label(egui::RichText::new(tr(sev)).color(c(col)).strong());
                if !who.is_empty() {
                    ui.label(egui::RichText::new(who).color(c(p.weak_text)));
                }
                ui.label(diagnostic(d)).on_hover_text(&d.code);
            });
        }
    });
}

/// A small rounded frame for ribbon groups.
fn group_frame(p: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(c(p.panel))
        .stroke(Stroke::new(1.0, c(p.line)))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(6.0)
}

/// Colour of a status or severity.
fn status_colour(rgb: brand::Rgb) -> Color32 {
    c(rgb)
}

#[cfg(test)]
pub(crate) mod tests;
