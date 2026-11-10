// SPDX-License-Identifier: MIT OR Apache-2.0

//! The "Getting started" card: a small window beside the work, not a modal
//! dialog, that walks a first job and ticks each step as the project shows
//! it done. A step's button runs the same command the ribbon runs, so file
//! paths are still chosen in the native dialogs. Closing the card is
//! remembered in the settings; the title bar reopens it.

use super::{UiState, ribbon};
use crate::brand::{self, Palette, c};
use crate::dialogs::Dialogs;
use crate::jobs::Jobs;
use crate::text::{tr, tr_args};
use crate::updates::Updates;
use crate::vm::guide::{GuideStep, GuideVm, StepVm};
use crate::vm::ribbon::{Command, RibbonCx, RibbonVm};
use eframe::egui::{self, Align2, Sense, Stroke, Vec2};

/// Width of the card.
const WIDTH: f32 = 290.0;

/// The tick box of a step: a ring, with a check mark when done and an
/// orange ring for the step to do next. A screen reader gets it as a read
/// only check box with the step's name.
fn tick(ui: &mut egui::Ui, step: &StepVm, p: &Palette) {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
    let id = if step.done {
        "guide-step-done"
    } else {
        "guide-step-todo"
    };
    let label = tr_args(id, &[("title", &step.title)]);
    r.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, step.done, &label)
    });
    let centre = rect.center();
    let painter = ui.painter();
    if step.done {
        painter.circle_filled(centre, 8.0, c(brand::GREEN));
        let ink = Stroke::new(2.0, c(brand::INK));
        painter.line_segment(
            [centre + Vec2::new(-4.0, 0.5), centre + Vec2::new(-1.0, 3.5)],
            ink,
        );
        painter.line_segment(
            [centre + Vec2::new(-1.0, 3.5), centre + Vec2::new(4.5, -3.0)],
            ink,
        );
    } else {
        let colour = if step.current {
            c(brand::CUTTER_ORANGE)
        } else {
            c(p.weak_text)
        };
        painter.circle_stroke(centre, 7.5, Stroke::new(2.0, colour));
    }
}

/// The toolpath step's two choices: the type and the tool.
fn choices(ui: &mut egui::Ui, vm: &GuideVm, state: &mut UiState) {
    let mut kind = vm.toolpath_type;
    ui.horizontal(|ui| {
        let shown = vm
            .types
            .iter()
            .find(|(t, _)| *t == kind)
            .map_or_else(String::new, |(_, l)| l.clone());
        egui::ComboBox::from_label(tr("guide-type-label"))
            .selected_text(shown)
            .width(170.0)
            .show_ui(ui, |ui| {
                for (t, label) in &vm.types {
                    ui.selectable_value(&mut kind, *t, label);
                }
            });
    });
    let mut tool = vm.tool.clone();
    ui.horizontal(|ui| {
        let shown = vm
            .tools
            .iter()
            .find(|(v, _)| *v == tool)
            .map_or_else(String::new, |(_, l)| l.clone());
        egui::ComboBox::from_label(tr("guide-tool-label"))
            .selected_text(shown)
            .width(170.0)
            .show_ui(ui, |ui| {
                for (value, label) in &vm.tools {
                    ui.selectable_value(&mut tool, value.clone(), label);
                }
            });
    });
    state.guide.toolpath_type = kind;
    state.guide.tool = (!tool.is_empty()).then_some(tool);
}

/// One step: tick, title, hint and button, and the extras of the step.
fn step_row(
    ui: &mut egui::Ui,
    step: &StepVm,
    vm: &GuideVm,
    state: &mut UiState,
    p: &Palette,
    clicked: &mut Vec<Command>,
) {
    ui.horizontal_top(|ui| {
        tick(ui, step, p);
        ui.vertical(|ui| {
            ui.set_width(WIDTH - 34.0);
            ui.label(egui::RichText::new(&step.title).strong());
            ui.label(
                egui::RichText::new(&step.hint)
                    .small()
                    .color(c(p.weak_text)),
            );
            if step.step == GuideStep::Toolpath {
                choices(ui, vm, state);
            }
            ui.horizontal(|ui| {
                let mut b = egui::Button::new(&step.button);
                if step.current && step.enabled {
                    b = b.fill(c(brand::CUTTER_ORANGE).gamma_multiply(0.35));
                }
                if ui.add_enabled(step.enabled, b).clicked() {
                    clicked.push(step.command);
                }
                if step.step == GuideStep::Setup {
                    let extras = [
                        (Command::Stock, "guide-show-stock"),
                        (Command::WorkZero, "guide-show-zero"),
                    ];
                    for (cmd, label) in extras {
                        if ui
                            .add_enabled(vm.can_show_setup, egui::Button::new(tr(label)).small())
                            .clicked()
                        {
                            clicked.push(cmd);
                        }
                    }
                }
            });
        });
    });
    ui.add_space(4.0);
}

/// The card, when it is open.
pub fn show(
    ctx: &egui::Context,
    jobs: &mut Jobs,
    state: &mut UiState,
    dialogs: &mut Dialogs,
    updates: &mut Updates,
) {
    if !state.guide.open {
        return;
    }
    let vm = GuideVm::new(jobs, &state.selection, &state.guide);
    let ribbon_vm = RibbonVm::new(
        jobs,
        RibbonCx {
            stage: state.stage,
            selection: &state.selection,
            playback: &state.playback,
        },
    );
    let p = *Palette::get(state.dark);
    let mut open = true;
    let mut clicked = Vec::new();
    let anchor = ctx.content_rect().left_bottom() + Vec2::new(12.0, -40.0);
    egui::Window::new(tr("guide-title"))
        .open(&mut open)
        .resizable(false)
        .pivot(Align2::LEFT_BOTTOM)
        .default_pos(anchor)
        .default_width(WIDTH)
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            let progress = if vm.complete {
                tr("guide-complete")
            } else {
                tr_args(
                    "guide-progress",
                    &[
                        ("done", &vm.done.to_string()),
                        ("total", &vm.steps.len().to_string()),
                    ],
                )
            };
            ui.label(egui::RichText::new(progress).color(c(p.weak_text)));
            ui.add_space(4.0);
            for step in &vm.steps {
                step_row(ui, step, &vm, state, &p, &mut clicked);
            }
        });
    if !open {
        state.guide.open = false;
        updates.set_guide_dismissed(true);
    }
    for cmd in clicked {
        if matches!(cmd, Command::AddToolpath(_)) && ribbon_vm.enabled(cmd) {
            state.guide.pending_tool = state.guide.tool.clone();
        }
        ribbon::run(cmd, &ribbon_vm, jobs, state, dialogs);
    }
}
