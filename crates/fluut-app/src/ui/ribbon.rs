// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ribbon (S 08 §8.3): stage tabs and their groups of commands. What
//! each stage holds, and what is enabled, comes from
//! [`RibbonVm`]; this file paints it and runs the commands.

use super::{UiState, group_frame, icons, project};
use crate::brand::{self, Palette, c};
use crate::dialogs::{DialogKind, Dialogs};
use crate::jobs::{CalcTargets, Jobs};
use crate::text::tr;
use crate::vm::Selection;
use crate::vm::ribbon::{Command, CommandVm, GroupId, GroupVm, RibbonCx, RibbonVm, Stage};
use eframe::egui::{self, Align2, CornerRadius, FontId, Sense, Stroke, Vec2};
use fluut_core::fluut_sim::Step;
use fluut_model::project::DisplayUnit;

/// Height of a small button; three of them stack beside a large one.
const ROW: f32 = 24.0;
const ROW_GAP: f32 = 2.0;
const LARGE_HEIGHT: f32 = 3.0 * ROW + 2.0 * ROW_GAP;
const LARGE_MIN_WIDTH: f32 = 60.0;
const LARGE_ICON: f32 = 24.0;
const SMALL_ICON: f32 = 16.0;
const LABEL_SIZE: f32 = 12.0;
const GROUP_LABEL_SIZE: f32 = 11.0;
const GROUP_LABEL_HEIGHT: f32 = 15.0;
/// Width of the post dropdown.
const POST_WIDTH: f32 = 200.0;

fn label_font() -> FontId {
    FontId::proportional(LABEL_SIZE)
}

/// The width a small button needs for its label.
fn small_width(ui: &egui::Ui, label: &str) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), label_font(), c(brand::INK));
    SMALL_ICON + galley.size().x + 16.0
}

/// A command button: large with the icon over the label, small with the
/// icon before it.
fn button(ui: &mut egui::Ui, item: &CommandVm, width: f32, p: &Palette) -> bool {
    let text = if item.enabled { p.text } else { p.weak_text };
    let galley = ui
        .painter()
        .layout_no_wrap(item.label.clone(), label_font(), c(text));
    let size = if item.large {
        Vec2::new((galley.size().x + 14.0).max(LARGE_MIN_WIDTH), LARGE_HEIGHT)
    } else {
        Vec2::new(width, ROW)
    };
    let sense = if item.enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, r) = ui.allocate_exact_size(size, sense);
    r.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, item.enabled, &item.label)
    });
    if ui.is_rect_visible(rect) {
        if item.enabled && (r.hovered() || r.has_focus()) {
            let fill = if r.is_pointer_button_down_on() {
                c(brand::CUTTER_ORANGE).gamma_multiply(0.35)
            } else {
                c(p.raised)
            };
            let stroke = Stroke::new(1.0, c(brand::CUTTER_ORANGE));
            let corner = CornerRadius::same(4);
            ui.painter()
                .rect(rect, corner, fill, stroke, egui::StrokeKind::Inside);
        }
        if item.large {
            let top = rect.center_top() + Vec2::new(0.0, 14.0);
            let icon = egui::Rect::from_min_size(
                top - Vec2::new(LARGE_ICON / 2.0, 0.0),
                Vec2::splat(LARGE_ICON),
            );
            icons::paint(ui, icon, item.icon, LARGE_ICON, item.enabled, p);
            let at =
                rect.center_bottom() - Vec2::new(galley.size().x / 2.0, galley.size().y + 12.0);
            ui.painter().galley(at.round(), galley, c(text));
        } else {
            let icon = egui::Rect::from_min_size(
                rect.left_center() + Vec2::new(5.0, -SMALL_ICON / 2.0),
                Vec2::splat(SMALL_ICON),
            );
            icons::paint(ui, icon, item.icon, SMALL_ICON, item.enabled, p);
            let at = rect.left_center() + Vec2::new(SMALL_ICON + 10.0, -galley.size().y / 2.0);
            ui.painter().galley(at.round(), galley, c(text));
        }
    }
    r.on_hover_text(&item.tooltip).clicked()
}

/// The commands of a group: large ones side by side, runs of small ones
/// stacked in columns of three, with one width per column.
fn commands(ui: &mut egui::Ui, g: &GroupVm, p: &Palette) -> Vec<Command> {
    let mut clicked = Vec::new();
    let mut i = 0;
    while i < g.items.len() {
        if g.items[i].large {
            if button(ui, &g.items[i], 0.0, p) {
                clicked.push(g.items[i].command);
            }
            i += 1;
            continue;
        }
        let run = g.items[i..].iter().take_while(|x| !x.large).count();
        for column in g.items[i..i + run].chunks(3) {
            let width = column
                .iter()
                .map(|x| small_width(ui, &x.label))
                .fold(0.0, f32::max);
            ui.allocate_ui(Vec2::new(width, LARGE_HEIGHT), |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = ROW_GAP;
                    for item in column {
                        if button(ui, item, width, p) {
                            clicked.push(item.command);
                        }
                    }
                });
            });
        }
        i += run;
    }
    clicked
}

/// What Post acts on: the post. The output file is chosen in the dialog
/// that Post opens.
fn post_choice(ui: &mut egui::Ui, state: &mut UiState) {
    ui.allocate_ui(Vec2::new(POST_WIDTH, LARGE_HEIGHT), |ui| {
        ui.vertical(|ui| {
            ui.add_space(27.0);
            egui::ComboBox::from_id_salt("post")
                .selected_text(&state.post_id)
                .width(POST_WIDTH)
                .show_ui(ui, |ui| {
                    for choice in crate::posts::choices() {
                        ui.selectable_value(&mut state.post_id, choice.id.clone(), &choice.id);
                    }
                })
                .response
                .on_hover_text(tr("ribbon-post-processor"));
        });
    });
}

fn group(ui: &mut egui::Ui, g: &GroupVm, state: &mut UiState, p: &Palette) -> Vec<Command> {
    let mut clicked = Vec::new();
    group_frame(p).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let row = ui.horizontal(|ui| {
                ui.set_min_height(LARGE_HEIGHT);
                ui.spacing_mut().item_spacing.x = 2.0;
                clicked = commands(ui, g, p);
                if g.id == GroupId::Output {
                    ui.add_space(6.0);
                    post_choice(ui, state);
                }
            });
            let galley = ui.painter().layout_no_wrap(
                g.label.clone(),
                FontId::proportional(GROUP_LABEL_SIZE),
                c(p.weak_text),
            );
            let width = row.response.rect.width().max(galley.size().x);
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(width, GROUP_LABEL_HEIGHT), Sense::hover());
            ui.painter().text(
                rect.center_bottom(),
                Align2::CENTER_BOTTOM,
                &g.label,
                FontId::proportional(GROUP_LABEL_SIZE),
                c(p.weak_text),
            );
        });
    });
    clicked
}

/// The stage tabs: upper case, the current one underlined in the accent.
fn stage_tabs(ui: &mut egui::Ui, state: &mut UiState, p: &Palette) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        // The underline shows the stage; no selection fill behind the text.
        ui.visuals_mut().selection.bg_fill = egui::Color32::TRANSPARENT;
        ui.visuals_mut().selection.stroke = Stroke::NONE;
        for s in Stage::ALL {
            let on = state.stage == s;
            let label = s.label().to_uppercase();
            let colour = if on { p.text } else { p.weak_text };
            let text = egui::RichText::new(label).size(13.0).color(c(colour));
            let r = ui.add(egui::Button::selectable(on, text).frame_when_inactive(false));
            if on {
                let y = r.rect.bottom() + 1.0;
                ui.painter().hline(
                    r.rect.x_range(),
                    y,
                    Stroke::new(2.0, c(brand::CUTTER_ORANGE)),
                );
            }
            if r.clicked() {
                state.stage = s;
            }
        }
    });
}

/// Runs a command, if it is enabled. The guide runs its steps through here.
pub(super) fn run(
    c: Command,
    vm: &RibbonVm,
    jobs: &mut Jobs,
    state: &mut UiState,
    dialogs: &mut Dialogs,
) {
    if !vm.enabled(c) {
        return;
    }
    match c {
        Command::NewProject => {
            let units = jobs
                .snapshot
                .as_ref()
                .map_or(DisplayUnit::Mm, |s| s.project.units);
            project::ask(jobs, state, dialogs, DialogKind::New(units));
        }
        Command::Open => project::ask(jobs, state, dialogs, DialogKind::Open),
        Command::Save => jobs.save(),
        Command::SaveAs => project::ask(jobs, state, dialogs, DialogKind::SaveAs),
        Command::ImportModel => project::ask(jobs, state, dialogs, DialogKind::Import),
        Command::Reimport => jobs.reimport(),
        Command::Undo => jobs.undo(),
        Command::Redo => jobs.redo(),
        Command::NewSetup(kind) => jobs.add_setup(kind, None),
        Command::NewSetupBasedOn => {
            if let (Some(id), Some(kind)) = (&vm.setup, vm.setup_kind) {
                jobs.add_setup(kind, Some(id));
            }
        }
        // Parameters live in the inspector: show the Setup, at that group.
        Command::Stock | Command::WorkZero => {
            if let Some(id) = &vm.setup {
                state.selection = Selection::Setup(id.clone());
                state.inspector_focus = Some(if c == Command::Stock {
                    "setup-stock"
                } else {
                    "setup-zero"
                });
            }
        }
        Command::AddToolpath(t) => {
            if let Some(setup) = &vm.setup {
                jobs.add_toolpath(setup, t);
            }
        }
        Command::CalculateSelected => {
            jobs.calculate(CalcTargets::Toolpaths(vm.selected_toolpaths.clone()));
        }
        Command::Calculate => jobs.calculate(CalcTargets::All),
        Command::Simulate => {
            if let Some(s) = jobs.snapshot.clone()
                && let Some(id) = super::viewport::shown_setup(&s, &state.selection)
            {
                jobs.simulate(&id);
            }
        }
        Command::PlayPause | Command::StepBack | Command::StepForward => {
            let Some(s) = jobs.snapshot.clone() else {
                return;
            };
            let setup = super::viewport::shown_setup(&s, &state.selection);
            let Some(v) = setup.and_then(|id| s.sims.get(&id).cloned()) else {
                return;
            };
            let unit: Step = state.step;
            match c {
                Command::PlayPause => state.playback.toggle(v.duration),
                _ => state.playback.step(&v, unit, c == Command::StepForward),
            }
        }
        Command::Post => project::ask(jobs, state, dialogs, DialogKind::PostOutput),
        Command::Cancel => jobs.cancel(),
        _ => {}
    }
}

/// The shortcuts of [`Command::shortcut`], even of disabled commands, so
/// the keys stay ours. The ones with Shift go first: without Shift, a
/// chord also matches the key pressed with Shift.
fn shortcuts(
    ctx: &egui::Context,
    vm: &RibbonVm,
    jobs: &mut Jobs,
    state: &mut UiState,
    dialogs: &mut Dialogs,
) {
    let typing = ctx.egui_wants_keyboard_input();
    let mut keyed = Vec::new();
    for cmd in Command::all() {
        keyed.extend(cmd.shortcut().into_iter().map(|s| (cmd, s)));
        keyed.extend(cmd.alt_shortcut().into_iter().map(|s| (cmd, s)));
    }
    keyed.sort_by_key(|(_, s)| !s.shift);
    for (cmd, s) in keyed {
        let Some(key) = egui::Key::from_name(&s.key.to_string()) else {
            continue;
        };
        let mut m = egui::Modifiers::NONE;
        if s.command {
            m |= egui::Modifiers::COMMAND;
        }
        if s.shift {
            m |= egui::Modifiers::SHIFT;
        }
        let chord = egui::KeyboardShortcut::new(m, key);
        // A text field keeps Ctrl+Z for its own undo.
        if cmd.yields_to_text() && typing {
            continue;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&chord)) {
            run(cmd, vm, jobs, state, dialogs);
        }
    }
}

pub fn show(ui: &mut egui::Ui, jobs: &mut Jobs, state: &mut UiState, dialogs: &mut Dialogs) {
    let vm = RibbonVm::new(
        jobs,
        RibbonCx {
            stage: state.stage,
            selection: &state.selection,
            playback: &state.playback,
        },
    );
    let p = Palette::get(state.dark);
    shortcuts(ui.ctx(), &vm, jobs, state, dialogs);
    stage_tabs(ui, state, p);
    ui.add_space(6.0);
    let mut clicked = Vec::new();
    egui::ScrollArea::horizontal()
        .id_salt("ribbon_groups")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for g in &vm.groups {
                    clicked.extend(group(ui, g, state, p));
                }
            });
        });
    for cmd in clicked {
        run(cmd, &vm, jobs, state, dialogs);
    }
}
