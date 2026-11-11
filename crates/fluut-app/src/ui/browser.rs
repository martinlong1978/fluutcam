// SPDX-License-Identifier: MIT OR Apache-2.0

//! The browser (S 08 §8.2): Setups and toolpaths with status icons.

use super::{UiState, status_colour, tool_libs};
use crate::brand::{self, Palette, c};
use crate::jobs::{Edit, Jobs};
use crate::session::Snapshot;
use crate::text::tr;
use crate::vm::Selection;
use crate::vm::tool_libs::ToolLibRow;
use crate::vm::tree::{
    RowStatus, TreeVm, delete_edit, drop_edit, enable_edit, name_of, rename_edit, step_edit,
};
use eframe::egui::{self, Pos2, Sense, Shape, Stroke, Vec2};

fn colour(st: RowStatus) -> egui::Color32 {
    status_colour(match st {
        RowStatus::NotCalculated | RowStatus::Disabled => brand::STOCK_GREY,
        RowStatus::Calculating(_) | RowStatus::Done => brand::GREEN,
        RowStatus::Stale | RowStatus::Warning => brand::AMBER,
        RowStatus::Error => brand::RED,
    })
}

/// A row whose name is being typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Renaming {
    pub item: Selection,
    pub text: String,
    /// Keyboard focus has been given to the field.
    pub focused: bool,
}

fn start_rename(snap: &Snapshot, state: &mut UiState, item: &Selection) {
    if let Some(text) = name_of(snap, item) {
        state.renaming = Some(Renaming {
            item: item.clone(),
            text,
            focused: false,
        });
    }
}

/// The name field of the row being renamed: Enter or leaving commits, Esc
/// cancels.
fn rename_field(ui: &mut egui::Ui, jobs: &mut Jobs, state: &mut UiState) {
    let Some(r) = state.renaming.as_mut() else {
        return;
    };
    let resp = ui.add(
        egui::TextEdit::singleline(&mut r.text)
            .desired_width(140.0)
            .hint_text(tr("browser-name")),
    );
    if !r.focused {
        r.focused = true;
        resp.request_focus();
        return;
    }
    if resp.lost_focus() {
        let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let e = rename_edit(&r.item, &r.text).filter(|_| !cancelled);
        state.renaming = None;
        if let Some(e) = e {
            jobs.edit(Edit::Model(e));
        }
    }
}

/// F2 renames the selection, Del removes it. Text fields keep the keys.
fn keys(ctx: &egui::Context, jobs: &mut Jobs, state: &mut UiState, snap: &Snapshot) {
    if ctx.egui_wants_keyboard_input() || state.renaming.is_some() || state.pick.is_some() {
        return;
    }
    let pressed = |k| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, k));
    if pressed(egui::Key::F2) {
        let item = state.selection.clone();
        start_rename(snap, state, &item);
    } else if pressed(egui::Key::Delete)
        && let Some(e) = delete_edit(&state.selection)
    {
        jobs.edit(Edit::Model(e));
    }
}

/// The context menu of a row: everything the row can do.
fn menu(
    ui: &mut egui::Ui,
    jobs: &mut Jobs,
    state: &mut UiState,
    snap: &Snapshot,
    item: &Selection,
    enabled: Option<bool>,
) {
    if ui.button(tr("browser-rename")).clicked() {
        start_rename(snap, state, item);
        ui.close();
    }
    for (id, up) in [("browser-move-up", true), ("browser-move-down", false)] {
        let e = step_edit(snap, item, up);
        if ui
            .add_enabled(e.is_some(), egui::Button::new(tr(id)))
            .clicked()
        {
            jobs.edit(Edit::Model(e.expect("the button is enabled")));
            ui.close();
        }
    }
    if let (Selection::Toolpath(id), Some(on)) = (item, enabled) {
        if ui
            .add_enabled(!jobs.is_busy(), egui::Button::new(tr("browser-duplicate")))
            .clicked()
        {
            jobs.duplicate_toolpath(id);
            ui.close();
        }
        let label = if on {
            "browser-disable"
        } else {
            "browser-enable"
        };
        if ui.button(tr(label)).clicked() {
            jobs.edit(Edit::Model(enable_edit(id, !on)));
            ui.close();
        }
    }
    if ui.button(tr("browser-delete")).clicked() {
        if let Some(e) = delete_edit(item) {
            jobs.edit(Edit::Model(e));
        }
        ui.close();
    }
}

/// The name of a Setup or toolpath row: click selects, double click
/// renames, drag moves, right click opens the menu.
fn name_row(
    ui: &mut egui::Ui,
    jobs: &mut Jobs,
    state: &mut UiState,
    snap: &Snapshot,
    row: (&Selection, &str, bool, Option<bool>),
) -> Option<egui::Response> {
    let (item, label, selected, enabled) = row;
    if state.renaming.as_ref().is_some_and(|r| r.item == *item) {
        rename_field(ui, jobs, state);
        return None;
    }
    let r = ui
        .selectable_label(selected, label)
        .interact(Sense::click_and_drag());
    if r.clicked() {
        state.selection = item.clone();
    }
    if r.double_clicked() {
        start_rename(snap, state, item);
    }
    r.dnd_set_drag_payload(item.clone());
    if r.dnd_hover_payload::<Selection>().is_some() {
        let y = r.rect.bottom();
        ui.painter().hline(
            r.rect.x_range(),
            y,
            Stroke::new(2.0, c(brand::CUTTER_ORANGE)),
        );
    }
    if let Some(from) = r.dnd_release_payload::<Selection>()
        && let Some(e) = drop_edit(snap, &from, item)
    {
        jobs.edit(Edit::Model(e));
    }
    r.context_menu(|ui| menu(ui, jobs, state, snap, item, enabled));
    Some(r)
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
    let Some(snap) = jobs.snapshot.clone() else {
        return;
    };
    keys(ui.ctx(), jobs, state, &snap);
    let p = Palette::get(state.dark);
    egui::ScrollArea::vertical().show(ui, |ui| {
        for s in &vm.setups {
            let id = ui.make_persistent_id(("setup", &s.id));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
                .show_header(ui, |ui| {
                    let item = Selection::Setup(s.id.clone());
                    name_row(ui, jobs, state, &snap, (&item, &s.label, s.selected, None));
                    ui.label(egui::RichText::new(&s.detail).color(c(p.weak_text)).small());
                })
                .body(|ui| {
                    for t in &s.toolpaths {
                        ui.horizontal(|ui| {
                            status_icon(ui, t.status, &t.status_text);
                            let item = Selection::Toolpath(t.id.clone());
                            let row = (&item, t.label.as_str(), t.selected, Some(t.enabled));
                            if let Some(r) = name_row(ui, jobs, state, &snap, row) {
                                r.on_hover_text(&t.detail);
                            }
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
        tool_libraries(ui, jobs, state, &vm.tool_libraries, p);
    });
}

/// The tool libraries the project refers to: one row each with the file
/// name (the path as written on hover), why it is unavailable if it is,
/// and Remove; then the button that opens the native file dialog.
fn tool_libraries(
    ui: &mut egui::Ui,
    jobs: &mut Jobs,
    state: &mut UiState,
    rows: &[ToolLibRow],
    p: &Palette,
) {
    egui::CollapsingHeader::new(tr("browser-tool-libraries"))
        .default_open(true)
        .show(ui, |ui| {
            if rows.is_empty() {
                ui.label(
                    egui::RichText::new(tr("browser-tool-libraries-none")).color(c(p.weak_text)),
                );
            }
            let mut remove = None;
            for (i, r) in rows.iter().enumerate() {
                ui.horizontal(|ui| {
                    if r.problem.is_some() {
                        status_icon(ui, RowStatus::Warning, &tr("status-warning"));
                    }
                    let label = ui.label(&r.name).on_hover_text(&r.reference);
                    if let Some(why) = &r.problem {
                        label.on_hover_text(why);
                    }
                    if ui.small_button(tr("browser-tool-library-remove")).clicked() {
                        remove = Some(i);
                    }
                });
                if let Some(why) = &r.problem {
                    ui.label(egui::RichText::new(why).color(c(p.weak_text)).small());
                }
            }
            if let Some(i) = remove {
                tool_libs::remove(jobs, state, i);
            }
            if ui.button(tr("browser-tool-library-add")).clicked() {
                state.add_tool_library = true;
            }
            if let Some(note) = &state.tool_lib_note {
                ui.label(egui::RichText::new(note).color(c(p.weak_text)).small());
            }
        });
}
