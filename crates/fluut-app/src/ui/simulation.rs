// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Simulation tab (D13 §7, S 08 §8.7): play, pause, step and scrub
//! the program, its toolpath clips, the stock colouring with its legend,
//! and the verification findings. Clicking a finding centres the view on
//! it.

use super::{UiState, ViewportState, viewport::shown_setup};
use crate::brand::{self, Palette, c};
use crate::jobs::Jobs;
use crate::text::tr;
use crate::vm::playback::{PlaybackVm, StockColour};
use eframe::egui;
use fluut_core::fluut_sim::Step;

pub fn show(ui: &mut egui::Ui, jobs: &Jobs, state: &mut UiState, view: &mut ViewportState) {
    let Some(s) = &jobs.snapshot else {
        ui.label(tr("sim-none"));
        return;
    };
    let Some(v) = shown_setup(s, &state.selection).and_then(|id| s.sims.get(&id)) else {
        ui.label(tr("sim-none"));
        return;
    };
    let p = Palette::get(state.dark);
    let vm = PlaybackVm::new(v, &state.playback, state.stock_colour);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(vm.can_back, egui::Button::new(tr("sim-back")))
            .clicked()
        {
            state.playback.step(v, state.step, false);
        }
        if ui.button(&vm.play_label).clicked() {
            state.playback.toggle(vm.duration);
        }
        if ui
            .add_enabled(vm.can_forward, egui::Button::new(tr("sim-forward")))
            .clicked()
        {
            state.playback.step(v, state.step, true);
        }
        ui.label(&vm.time);
        ui.checkbox(&mut state.show_stock, tr("sim-show-stock"));
        ui.label(egui::RichText::new(&vm.summary).color(c(p.weak_text)));
    });
    ui.horizontal(|ui| {
        ui.label(tr("sim-step"));
        for (unit, id) in [
            (Step::Move, "sim-step-move"),
            (Step::Pass, "sim-step-pass"),
            (Step::Toolpath, "sim-step-toolpath"),
        ] {
            ui.selectable_value(&mut state.step, unit, tr(id));
        }
        ui.separator();
        ui.label(tr("sim-colour"));
        for (colour, id, on) in [
            (StockColour::Plain, "sim-colour-plain", true),
            (StockColour::Rest, "sim-colour-rest", vm.has_part),
            (StockColour::Toolpath, "sim-colour-toolpath", true),
        ] {
            let b = egui::Button::selectable(state.stock_colour == colour, tr(id));
            if ui.add_enabled(on, b).clicked() {
                state.stock_colour = colour;
            }
        }
    });
    // What the stock colours mean.
    if !vm.legend.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for row in &vm.legend {
                let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, c(row.colour));
                ui.label(&row.label);
                ui.add_space(6.0);
            }
        });
    }
    let mut t = state.playback.t;
    let w = ui.available_width();
    ui.spacing_mut().slider_width = (w - 16.0).max(40.0);
    if ui
        .add(egui::Slider::new(&mut t, 0.0..=vm.duration.max(1e-9)).show_value(false))
        .changed()
    {
        state.playback.seek(t, vm.duration);
    }
    // Clips: one button per toolpath, as wide as its share of the time.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for clip in &vm.clips {
            let width = ((clip.to - clip.from) * (w - 4.0 * vm.clips.len() as f32)).max(24.0);
            let b = egui::Button::new(egui::RichText::new(&clip.name).small())
                .selected(clip.current)
                .min_size(egui::vec2(width, 18.0));
            if ui.add(b).on_hover_text(&clip.name).clicked() {
                state.playback.seek(clip.start, vm.duration);
            }
        }
    });
    ui.separator();
    ui.label(egui::RichText::new(tr("sim-findings")).strong());
    if vm.findings.is_empty() {
        ui.label(tr("sim-no-findings"));
        return;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for f in &vm.findings {
            let col = if f.error { brand::RED } else { brand::AMBER };
            let r = ui.selectable_label(
                state.focus == Some(f.position),
                egui::RichText::new(&f.text).color(c(col)),
            );
            if r.clicked() {
                state.focus = Some(f.position);
                view.camera.target = glam::Vec3::from(f.position);
            }
        }
    });
}
