// SPDX-License-Identifier: MIT OR Apache-2.0

//! The inspector (S 08 §8.5): the selected item's fields, from
//! [`InspectorVm`]. Changes apply at once as edits; there is no dialog.

use super::UiState;
use crate::brand::{self, Palette, c};
use crate::jobs::Jobs;
use crate::text::tr;
use crate::vm::inspector::{
    DiagVm, FieldVm, InspectorVm, ToolpathVm, Widget, number_edit, value_edit,
};
use eframe::egui;
use fluut_model::PValue;

fn diag(ui: &mut egui::Ui, d: &DiagVm) {
    let (sev, col) = if d.error {
        ("severity-error", brand::RED)
    } else {
        ("severity-warning", brand::AMBER)
    };
    ui.add(
        egui::Label::new(
            egui::RichText::new(format!("{}: {}", tr(sev), d.text))
                .color(c(col))
                .small(),
        )
        .wrap(),
    )
    .on_hover_text(&d.code);
}

pub fn show(ui: &mut egui::Ui, jobs: &mut Jobs, state: &mut UiState) {
    let vm = InspectorVm::new(jobs.snapshot.as_deref(), &state.selection);
    let p = *Palette::get(state.dark);
    match vm {
        InspectorVm::Empty(text) => {
            ui.label(text);
        }
        InspectorVm::Setup(s) => {
            ui.heading(&s.title);
            ui.separator();
            egui::Grid::new("setup")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    let focus = state.inspector_focus.map(tr);
                    for (k, v) in &s.rows {
                        let on = focus.as_ref() == Some(k);
                        let key = if on {
                            egui::RichText::new(k).color(c(p.text)).strong()
                        } else {
                            egui::RichText::new(k).color(c(p.weak_text))
                        };
                        let r = ui.label(key);
                        if on {
                            // Stock or Work zero pointed here.
                            let rect = r.rect.expand2(egui::vec2(6.0, 2.0));
                            ui.painter().rect_stroke(
                                rect,
                                egui::CornerRadius::same(3),
                                egui::Stroke::new(1.5, c(brand::CUTTER_ORANGE)),
                                egui::StrokeKind::Outside,
                            );
                            r.scroll_to_me(None);
                        }
                        ui.label(v);
                        ui.end_row();
                    }
                });
        }
        InspectorVm::Toolpath(t) => toolpath(ui, jobs, state, &t, &p),
    }
}

fn toolpath(ui: &mut egui::Ui, jobs: &mut Jobs, state: &mut UiState, t: &ToolpathVm, p: &Palette) {
    ui.heading(&t.title);
    ui.label(egui::RichText::new(&t.subtitle).color(c(p.weak_text)));
    for d in &t.general {
        diag(ui, d);
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        for (i, tab) in t.tabs.iter().enumerate() {
            let label = if tab.errors > 0 {
                format!("{} ({})", tab.label, tab.errors)
            } else {
                tab.label.clone()
            };
            if ui
                .selectable_label(state.inspector_tab == i, label)
                .clicked()
            {
                state.inspector_tab = i;
            }
        }
    });
    ui.separator();
    let Some(tab) = t.tabs.get(state.inspector_tab) else {
        return;
    };
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new(("fields", &t.id, tab.group.key()))
            .num_columns(2)
            .spacing([12.0, 4.0])
            .min_col_width(110.0)
            .show(ui, |ui| {
                for f in tab.fields.iter().filter(|f| f.visible) {
                    field(ui, jobs, state, &t.id, f, p);
                }
            });
    });
}

fn field(
    ui: &mut egui::Ui,
    jobs: &mut Jobs,
    state: &mut UiState,
    tp: &str,
    f: &FieldVm,
    p: &Palette,
) {
    let label = ui.label(&f.label);
    ui.vertical(|ui| {
        ui.horizontal(|ui| match &f.widget {
            Widget::Number { .. } => number(ui, jobs, state, tp, f, label.id),
            Widget::Enum { options } => {
                let mut chosen = None;
                if options.len() <= 4 {
                    for (v, l) in options {
                        if ui.selectable_label(f.value == *v, l).clicked() && f.value != *v {
                            chosen = Some(v.clone());
                        }
                    }
                } else {
                    egui::ComboBox::from_id_salt(("enum", tp, &f.path))
                        .selected_text(&f.value)
                        .show_ui(ui, |ui| {
                            for (v, l) in options {
                                if ui.selectable_label(f.value == *v, l).clicked() {
                                    chosen = Some(v.clone());
                                }
                            }
                        });
                }
                if let Some(v) = chosen {
                    jobs.edit(value_edit(tp, &f.path, Some(PValue::Enum(v))));
                }
            }
            Widget::Bool => {
                let mut on = f.value == tr("inspector-yes");
                if ui.checkbox(&mut on, "").labelled_by(label.id).changed() {
                    jobs.edit(value_edit(tp, &f.path, Some(PValue::Bool(on))));
                }
            }
            Widget::ReadOnly => {
                ui.label(&f.value).on_hover_text(tr("inspector-read-only"));
            }
        });
        if f.set && f.widget != Widget::ReadOnly {
            let reset =
                egui::Button::new(egui::RichText::new(tr("inspector-reset")).small()).frame(false);
            if ui.add(reset).clicked() {
                state.entries.remove(&format!("{tp}/{}", f.path));
                jobs.edit(value_edit(tp, &f.path, None));
            }
        } else if !f.set {
            ui.label(
                egui::RichText::new(&f.default_text)
                    .small()
                    .color(c(p.weak_text)),
            );
        }
        if let Some((path, msg)) = &state.entry_error
            && *path == f.path
        {
            ui.label(egui::RichText::new(msg).small().color(c(brand::RED)));
        }
        for d in &f.diagnostics {
            diag(ui, d);
        }
    });
    ui.end_row();
}

fn unit_of(w: &Widget) -> Option<&'static str> {
    match w {
        Widget::Number { unit } => *unit,
        _ => None,
    }
}

/// A number field: shows the value, or the text being typed; commits on
/// Enter or when focus leaves.
fn number(
    ui: &mut egui::Ui,
    jobs: &mut Jobs,
    state: &mut UiState,
    tp: &str,
    f: &FieldVm,
    label: egui::Id,
) {
    let key = format!("{tp}/{}", f.path);
    let mut text = state
        .entries
        .get(&key)
        .cloned()
        .unwrap_or_else(|| f.value.clone());
    let r = ui
        .add(egui::TextEdit::singleline(&mut text).desired_width(110.0))
        .labelled_by(label)
        .on_hover_text(tr("inspector-entry-help"));
    if r.changed() {
        state.entries.insert(key.clone(), text.clone());
    }
    if let Some(u) = unit_of(&f.widget) {
        ui.label(u);
    }
    if r.lost_focus() && state.entries.contains_key(&key) {
        state.entries.remove(&key);
        if text.trim() == f.value {
            return;
        }
        let Some(snap) = jobs.snapshot.clone() else {
            return;
        };
        match number_edit(&snap, tp, &f.path, &text) {
            Ok(e) => {
                state.entry_error = None;
                jobs.edit(e);
            }
            Err(msg) => state.entry_error = Some((f.path.clone(), msg)),
        }
    }
}
