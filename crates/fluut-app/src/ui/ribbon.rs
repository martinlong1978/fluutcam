// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ribbon (S 08 §8.3): stage tabs and their groups of commands. What
//! each stage holds, and what is enabled, comes from
//! [`RibbonVm`]; this file paints it and runs the commands.

use super::{UiState, group_frame, icons};
use crate::brand::{self, Palette, c};
use crate::jobs::{CalcTargets, Jobs, PostRequest};
use crate::text::tr;
use crate::vm::ribbon::{Command, CommandVm, GroupId, GroupVm, RibbonCx, RibbonVm, Stage};
use eframe::egui::{self, Align2, CornerRadius, FontId, Sense, Stroke, Vec2};
use fluut_core::fluut_sim::Step;
use std::path::PathBuf;

/// Posts the ribbon offers (the built-in store).
pub const POSTS: [&str; 3] = ["fluut/linuxcnc", "fluut/grbl", "fluut/gcode"];

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
/// Space above a single text field, to centre it in the row.
const FIELD_TOP: f32 = 27.0;

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

/// The Open field: a path, until the file dialog replaces it.
fn open_field(ui: &mut egui::Ui, state: &mut UiState) {
    ui.allocate_ui(Vec2::new(240.0, LARGE_HEIGHT), |ui| {
        ui.vertical(|ui| {
            ui.add_space(FIELD_TOP);
            ui.add(
                egui::TextEdit::singleline(&mut state.open_path)
                    .hint_text(tr("ribbon-open-hint"))
                    .desired_width(240.0),
            )
            .on_hover_text(tr("ribbon-open-path"));
        });
    });
}

/// What Post acts on: the post and the output file.
fn output_fields(ui: &mut egui::Ui, state: &mut UiState) {
    ui.allocate_ui(Vec2::new(280.0, LARGE_HEIGHT), |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.add_space(12.0);
            egui::ComboBox::from_id_salt("post")
                .selected_text(&state.post_id)
                .width(280.0)
                .show_ui(ui, |ui| {
                    for id in POSTS {
                        ui.selectable_value(&mut state.post_id, id.to_owned(), id);
                    }
                })
                .response
                .on_hover_text(tr("ribbon-post-processor"));
            ui.add(egui::TextEdit::singleline(&mut state.post_output).desired_width(280.0))
                .on_hover_text(tr("ribbon-post-output"));
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
                match g.id {
                    GroupId::File => {
                        ui.add_space(6.0);
                        open_field(ui, state);
                    }
                    GroupId::Output => {
                        ui.add_space(6.0);
                        output_fields(ui, state);
                    }
                    _ => {}
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

fn run(c: Command, vm: &RibbonVm, jobs: &mut Jobs, state: &mut UiState) {
    if !vm.enabled(c) {
        return;
    }
    match c {
        Command::Open if !state.open_path.trim().is_empty() => {
            jobs.open(PathBuf::from(state.open_path.trim()));
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
        Command::Post if !state.post_output.trim().is_empty() => {
            let (date, time) = (state.clock)();
            jobs.post(PostRequest {
                post: state.post_id.clone(),
                output: PathBuf::from(state.post_output.trim()),
                date,
                time,
            });
        }
        Command::Cancel => jobs.cancel(),
        _ => {}
    }
}

/// The shortcuts of [`Command::shortcut`], even of disabled commands, so
/// the keys stay ours. The ones with Shift go first: without Shift, a
/// chord also matches the key pressed with Shift.
fn shortcuts(ctx: &egui::Context, vm: &RibbonVm, jobs: &mut Jobs, state: &mut UiState) {
    let mut keyed: Vec<_> = Command::all()
        .into_iter()
        .filter_map(|cmd| cmd.shortcut().map(|s| (cmd, s)))
        .collect();
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
        if ctx.input_mut(|i| i.consume_shortcut(&chord)) {
            run(cmd, vm, jobs, state);
        }
    }
}

pub fn show(ui: &mut egui::Ui, jobs: &mut Jobs, state: &mut UiState) {
    let vm = RibbonVm::new(
        jobs,
        RibbonCx {
            stage: state.stage,
            selection: &state.selection,
            playback: &state.playback,
        },
    );
    let p = Palette::get(state.dark);
    shortcuts(ui.ctx(), &vm, jobs, state);
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
        run(cmd, &vm, jobs, state);
    }
}
