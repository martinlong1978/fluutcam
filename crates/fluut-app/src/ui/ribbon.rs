// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ribbon (S 08 §8.3): commands only. M5 has one stage with Open,
//! Calculate, Post and Cancel; the stage tabs and toolpath galleries follow.

use super::{UiState, group_frame};
use crate::brand::Palette;
use crate::jobs::{CalcTargets, Jobs, PostRequest};
use crate::text::tr;
use crate::vm::ribbon::{Command, RibbonVm};
use eframe::egui;
use std::path::PathBuf;

/// Posts the M5 ribbon offers (the built-in store).
pub const POSTS: [&str; 3] = ["fluut/linuxcnc", "fluut/grbl", "fluut/gcode"];

fn command(ui: &mut egui::Ui, vm: &RibbonVm, c: Command) -> bool {
    let enabled = vm.enabled(c);
    ui.add_enabled(
        enabled,
        egui::Button::new(c.label()).min_size(egui::vec2(72.0, 40.0)),
    )
    .clicked()
}

pub fn show(ui: &mut egui::Ui, jobs: &mut Jobs, state: &mut UiState) {
    let vm = RibbonVm::new(jobs);
    let p = Palette::get(state.dark);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("ribbon-stage-project").to_uppercase()).strong());
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        group_frame(p).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if command(ui, &vm, Command::Open) && !state.open_path.trim().is_empty() {
                        jobs.open(PathBuf::from(state.open_path.trim()));
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut state.open_path)
                            .hint_text(tr("ribbon-open-hint"))
                            .desired_width(280.0),
                    )
                    .on_hover_text(tr("ribbon-open-path"));
                });
                ui.label(egui::RichText::new(tr("ribbon-group-file")).small());
            });
        });
        group_frame(p).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if command(ui, &vm, Command::Calculate) {
                        jobs.calculate(CalcTargets::All);
                    }
                    if command(ui, &vm, Command::Simulate)
                        && let Some(s) = jobs.snapshot.clone()
                        && let Some(id) = super::viewport::shown_setup(&s, &state.selection)
                    {
                        jobs.simulate(&id);
                    }
                    if command(ui, &vm, Command::Cancel) {
                        jobs.cancel();
                    }
                });
                ui.label(egui::RichText::new(tr("ribbon-group-toolpaths")).small());
            });
        });
        group_frame(p).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if command(ui, &vm, Command::Post) && !state.post_output.trim().is_empty() {
                        let (date, time) = (state.clock)();
                        jobs.post(PostRequest {
                            post: state.post_id.clone(),
                            output: PathBuf::from(state.post_output.trim()),
                            date,
                            time,
                        });
                    }
                    egui::ComboBox::from_id_salt("post")
                        .selected_text(&state.post_id)
                        .show_ui(ui, |ui| {
                            for id in POSTS {
                                ui.selectable_value(&mut state.post_id, id.to_owned(), id);
                            }
                        })
                        .response
                        .on_hover_text(tr("ribbon-post-processor"));
                    ui.add(egui::TextEdit::singleline(&mut state.post_output).desired_width(240.0))
                        .on_hover_text(tr("ribbon-post-output"));
                });
                ui.label(egui::RichText::new(tr("ribbon-group-output")).small());
            });
        });
    });
}
