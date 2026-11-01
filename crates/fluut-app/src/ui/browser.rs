// SPDX-License-Identifier: MIT OR Apache-2.0

//! The browser (S 08 §8.2): Setups and toolpaths with status icons.

use super::{UiState, status_colour};
use crate::brand::{self, Palette, c};
use crate::jobs::Jobs;
use crate::text::tr;
use crate::vm::Selection;
use crate::vm::tree::{RowStatus, TreeVm};
use eframe::egui::{self, Pos2, Sense, Shape, Stroke, Vec2};

fn colour(st: RowStatus) -> egui::Color32 {
    status_colour(match st {
        RowStatus::NotCalculated | RowStatus::Disabled => brand::STOCK_GREY,
        RowStatus::Calculating(_) | RowStatus::Done => brand::GREEN,
        RowStatus::Stale | RowStatus::Warning => brand::AMBER,
        RowStatus::Error => brand::RED,
    })
}

/// Status icon: a different shape per state, so colour is not the only
/// signal (S 08 §8.9). Painted, since the UI font has no such glyphs.
fn status_icon(ui: &mut egui::Ui, st: RowStatus, text: &str) {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    resp.on_hover_text(text);
    let (p, o, col) = (ui.painter(), r.center(), colour(st));
    match st {
        RowStatus::NotCalculated => {
            p.circle_stroke(o, 5.0, Stroke::new(1.5, col));
        }
        RowStatus::Disabled => {
            p.circle_stroke(o, 5.0, Stroke::new(1.0, col));
            p.line_segment(
                [o + Vec2::new(-4.0, 4.0), o + Vec2::new(4.0, -4.0)],
                Stroke::new(1.0, col),
            );
        }
        RowStatus::Done => {
            p.circle_filled(o, 5.0, col);
        }
        RowStatus::Stale => {
            p.circle_stroke(o, 5.0, Stroke::new(1.5, col));
            p.circle_filled(o, 2.5, col);
        }
        RowStatus::Calculating(pct) => {
            p.circle_stroke(o, 5.0, Stroke::new(1.5, c(brand::LINE)));
            let a = f32::from(pct.max(4)) / 100.0 * std::f32::consts::TAU;
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let t = a * i as f32 / 16.0 - std::f32::consts::FRAC_PI_2;
                    o + 5.0 * Vec2::new(t.cos(), t.sin())
                })
                .collect();
            p.add(Shape::line(pts, Stroke::new(2.0, col)));
        }
        RowStatus::Warning | RowStatus::Error => {
            let pts = vec![
                o + Vec2::new(0.0, -6.0),
                o + Vec2::new(6.0, 5.0),
                o + Vec2::new(-6.0, 5.0),
            ];
            if st == RowStatus::Error {
                p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
            } else {
                p.add(Shape::closed_line(pts, Stroke::new(1.5, col)));
            }
        }
    }
}

pub fn show(ui: &mut egui::Ui, jobs: &mut Jobs, state: &mut UiState) {
    let vm = TreeVm::new(
        jobs.snapshot.as_deref(),
        jobs.running.as_ref(),
        &state.selection,
    );
    if let Some(e) = &vm.empty {
        ui.label(e);
        return;
    }
    let p = Palette::get(state.dark);
    egui::ScrollArea::vertical().show(ui, |ui| {
        for s in &vm.setups {
            let id = ui.make_persistent_id(("setup", &s.id));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
                .show_header(ui, |ui| {
                    if ui.selectable_label(s.selected, &s.label).clicked() {
                        state.selection = Selection::Setup(s.id.clone());
                    }
                    ui.label(egui::RichText::new(&s.detail).color(c(p.weak_text)).small());
                })
                .body(|ui| {
                    for t in &s.toolpaths {
                        ui.horizontal(|ui| {
                            status_icon(ui, t.status, &t.status_text);
                            let r = ui.selectable_label(t.selected, &t.label);
                            if r.clicked() {
                                state.selection = Selection::Toolpath(t.id.clone());
                            }
                            r.on_hover_text(&t.detail);
                        });
                    }
                });
        }
        egui::CollapsingHeader::new(tr("browser-model"))
            .default_open(true)
            .show(ui, |ui| {
                for b in &vm.bodies {
                    ui.label(b);
                }
            });
    });
}
