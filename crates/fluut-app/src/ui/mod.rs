// SPDX-License-Identifier: MIT OR Apache-2.0

//! The widget layer (D13 §6): `egui` with `egui_dock`.
//!
//! Widgets read view models and send requests to the job layer; they never
//! change the document themselves (D13 §1). The window repaints on input
//! and when the worker wakes it; while a job runs it repaints each frame so
//! the progress shows.

mod about;
mod browser;
mod handles;
mod icons;
mod inspector;
mod project;
mod ribbon;
mod simulation;
mod updates;
mod viewport;

use crate::bench::Bench;
use crate::brand::{self, Palette, c};
use crate::dialogs::Dialogs;
use crate::jobs::{CalcTargets, Created, Edit, JobKind, Jobs, Outcome};
use crate::text::{diagnostic, tr};
use crate::updates::Updates;
use crate::vm::Selection;
use crate::vm::pick::PickMode;
use crate::vm::playback::{Playback, StockColour};
use crate::vm::ribbon::Stage;
use crate::vm::status::StatusVm;
use eframe::egui::{self, Color32, CornerRadius, Stroke};
use eframe::egui_wgpu;
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use fluut_core::fluut_sim::Step;
use fluut_model::project::DisplayUnit;
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
    Simulation,
}

/// UI state that is not part of the document (D13 §2 `UiState`).
#[derive(Debug)]
pub struct UiState {
    /// The ribbon tab. Changing it changes the ribbon, nothing else.
    pub stage: Stage,
    pub selection: Selection,
    pub dark: bool,
    /// The post Post uses (the dropdown of the Post stage).
    pub post_id: String,
    /// The row of the Setup inspector that Stock or Work zero pointed at:
    /// the message id of its name.
    pub inspector_focus: Option<&'static str>,
    pub show_about: bool,
    pub show_settings: bool,
    /// Inspector tab per toolpath type (by group index).
    pub inspector_tab: usize,
    /// Text being typed into number fields, by field path, until it is
    /// committed.
    pub entries: BTreeMap<String, String>,
    /// Why the last entry was refused: `(field path, message)`.
    pub entry_error: Option<(String, String)>,
    /// Date and time for the post header; tests pin it.
    pub clock: fn() -> (String, String),
    /// Simulation playhead.
    pub playback: Playback,
    /// Show the simulated stock instead of the faces.
    pub show_stock: bool,
    /// How the simulated stock is coloured.
    pub stock_colour: StockColour,
    /// How far the step buttons move the playhead.
    pub step: Step,
    /// The finding looked at, in the work frame.
    pub focus: Option<[f32; 3]>,
    /// The browser row being renamed.
    pub renaming: Option<browser::Renaming>,
    /// The selection field being picked on the model.
    pub pick: Option<PickMode>,
    /// A new project waiting for its unit: its file, and the unit offered
    /// first.
    pub new_project: Option<(PathBuf, DisplayUnit)>,
    /// An Open or New waiting for the answer to "discard changes?".
    pub pending: Option<project::Pending>,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            stage: Stage::default(),
            selection: Selection::None,
            dark: true,
            post_id: "fluut/linuxcnc".into(),
            inspector_focus: None,
            show_about: false,
            show_settings: false,
            inspector_tab: 3,
            entries: BTreeMap::new(),
            entry_error: None,
            clock: crate::session::now_utc,
            playback: Playback::default(),
            show_stock: true,
            stock_colour: StockColour::Plain,
            step: Step::Move,
            focus: None,
            renaming: None,
            pick: None,
            new_project: None,
            pending: None,
        }
    }
}

/// The application.
pub struct FluutApp {
    pub jobs: Jobs,
    pub ui: UiState,
    pub view: ViewportState,
    /// The update check (spec 06 §6.4). Off until [`FluutApp::with_updates`].
    pub updates: Updates,
    /// The native file dialogs.
    pub dialogs: Dialogs,
    dock: DockState<Tab>,
    render: Option<egui_wgpu::RenderState>,
    bench: Bench,
    /// Bench mode: the project has been calculated.
    bench_ready: bool,
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
    tree.split_below(rest, 0.8, vec![Tab::Messages, Tab::Simulation]);
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
        let wake = ctx.clone();
        let dialogs = Dialogs::new(move || wake.request_repaint());
        if let Some(p) = open {
            jobs.open(p);
        }
        FluutApp {
            jobs,
            view: ViewportState::new(render.as_ref()),
            ui,
            updates: Updates::off(),
            dialogs,
            dock: default_dock(),
            render,
            bench: Bench::from_env(t0),
            bench_ready: false,
            seen: 0,
        }
    }

    /// Turns the update check on: the first-start question, the daily check
    /// once the user has opted in, and Settings. A window built without this
    /// (tests) never asks and never uses the network.
    pub fn with_updates(mut self, updates: Updates) -> FluutApp {
        self.updates = updates;
        self
    }

    /// Brings a dock tab to the front (the screenshot tool uses it).
    pub fn show_tab(&mut self, tab: Tab) {
        if let Some(found) = self.dock.find_tab(&tab) {
            let _ = self.dock.set_active_tab(found);
        }
    }

    /// Requests from the widgets go through here.
    pub fn edit(&mut self, e: Edit) {
        self.jobs.edit(e);
    }

    /// Keeps the selection valid after the document changes.
    fn on_snapshot(&mut self) {
        let Some(s) = self.jobs.snapshot.clone() else {
            return;
        };
        if s.revision == self.seen {
            return;
        }
        self.seen = s.revision;
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
        // What an add operation made becomes the selection.
        match self.jobs.created.take() {
            Some(Created::Setup(id)) => self.ui.selection = Selection::Setup(id),
            Some(Created::Toolpath(id)) => self.ui.selection = Selection::Toolpath(id),
            None => {}
        }
        self.keep_pick(ui.ctx());
        if let Some(p) = self.dialogs.poll() {
            project::picked(&mut self.jobs, &mut self.ui, p);
        }
        if self.bench.timing_frames() && !self.bench_ready && !self.jobs.is_busy() {
            // Time frames with the toolpaths on screen: calculate first.
            match &self.jobs.last {
                Some((JobKind::Open, Outcome::Opened { .. })) => {
                    self.jobs.calculate(CalcTargets::All);
                }
                _ => self.bench_ready = true,
            }
        }
        self.playback(ui.ctx());
        self.updates.tick();
        if self.updates.is_active() {
            // So a day-old session still checks without input.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(3600));
        }
        let timing = !self.bench.timing_frames() || self.bench_ready;
        if timing && self.bench.frame(&mut self.view.camera) {
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
            .show(ui, |ui| {
                ribbon::show(ui, &mut self.jobs, &mut self.ui, &mut self.dialogs);
            });
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
        project::units_prompt(ui.ctx(), &mut self.jobs, &mut self.ui);
        project::discard_prompt(ui.ctx(), &mut self.jobs, &mut self.ui);
        about::show(ui.ctx(), &mut self.ui);
        updates::settings(ui.ctx(), &mut self.ui, &mut self.updates);
        updates::prompt(ui.ctx(), &mut self.updates);
    }

    /// Pick mode ends with Esc, and when its toolpath is no longer the
    /// selection.
    fn keep_pick(&mut self, ctx: &egui::Context) {
        let Some(p) = &self.ui.pick else {
            return;
        };
        let esc = !ctx.egui_wants_keyboard_input()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let elsewhere = match &p.setup {
            Some((setup, _)) => self.ui.selection != Selection::Setup(setup.clone()),
            None => self.ui.selection != Selection::Toolpath(p.toolpath.clone()),
        };
        if esc || p.done || elsewhere {
            self.ui.pick = None;
        }
    }

    /// Moves the playhead while playing, and asks the worker for the stock
    /// at the playhead when it has none waiting (D11 §4).
    fn playback(&mut self, ctx: &egui::Context) {
        let Some(s) = self.jobs.snapshot.clone() else {
            return;
        };
        let Some(setup) = viewport::shown_setup(&s, &self.ui.selection) else {
            return;
        };
        let Some(v) = s.sims.get(&setup) else {
            return;
        };
        let pb = &mut self.ui.playback;
        if pb.setup.as_deref() != Some(&setup) || pb.duration != v.duration {
            pb.reset(&setup, v.duration);
            self.ui.focus = None;
        }
        if pb.tick(f64::from(ctx.input(|i| i.stable_dt)), v.duration) || pb.playing {
            ctx.request_repaint();
        }
        if let Some(t) = pb.wanted(self.jobs.scrubbing > 0) {
            pb.sent = Some(t);
            self.jobs.scrub(&setup, t);
        }
    }

    fn title_bar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(r.center(), 6.0, c(brand::CUTTER_ORANGE));
            ui.label(egui::RichText::new(tr("app-title")).strong());
            let name = self.jobs.snapshot.as_ref().map_or_else(
                || tr("app-untitled"),
                |s| {
                    if s.dirty {
                        format!("{} *", s.file_name())
                    } else {
                        s.file_name()
                    }
                },
            );
            ui.label(egui::RichText::new(name).color(c(p.weak_text)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("title-about")).clicked() {
                    self.ui.show_about = true;
                }
                if ui.button(tr("title-settings")).clicked() {
                    self.ui.show_settings = true;
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
                if let Some(units) = self.jobs.snapshot.as_ref().map(|s| s.project.units) {
                    let clicked = ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(units.as_str())
                                    .strong()
                                    .color(c(brand::INK)),
                            )
                            .fill(c(brand::STOCK_GREY)),
                        )
                        .on_hover_text(tr("title-units"))
                        .clicked();
                    if clicked {
                        let other = match units {
                            DisplayUnit::Mm => DisplayUnit::Inch,
                            DisplayUnit::Inch => DisplayUnit::Mm,
                        };
                        self.jobs
                            .edit(Edit::Model(fluut_model::edit::Edit::SetUnits(
                                other.as_str().to_owned(),
                            )));
                    }
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
            if updates::notice(ui, &self.updates) {
                self.ui.show_settings = true;
            }
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
            Tab::Simulation => "tab-simulation",
        })
        .into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match tab {
            Tab::Browser => browser::show(ui, self.jobs, self.ui),
            Tab::Viewport => viewport::show(ui, self.jobs, self.ui, self.view, self.render),
            Tab::Inspector => inspector::show(ui, self.jobs, self.ui),
            Tab::Messages => messages(ui, self.jobs, self.ui),
            Tab::Simulation => simulation::show(ui, self.jobs, self.ui, self.view),
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
