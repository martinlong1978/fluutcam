// SPDX-License-Identifier: MIT OR Apache-2.0

//! The inspector (S 08 §8.5): the selected item's fields, from
//! [`InspectorVm`]. Changes apply at once as edits; there is no dialog.

use super::UiState;
use crate::brand::{self, Palette, c};
use crate::jobs::Edit;
use crate::jobs::Jobs;
use crate::text::tr;
use crate::vm::inspector::{
    DiagVm, FieldVm, InspectorVm, ToolChoiceVm, ToolpathVm, Widget, height_edit, number_edit,
    points_edit, tool_edit, value_edit,
};
use crate::vm::setup::{RowVm, RowWidget, SetupVm, reset_edit, setup_edit};
use eframe::egui;
use fluut_model::{Group, PValue};

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
        InspectorVm::Setup(s) => setup(ui, jobs, state, &s, &p),
        InspectorVm::Toolpath(t) => toolpath(ui, jobs, state, &t, &p),
    }
}

/// The Setup editor: one titled grid per group. The group Stock or Work
/// zero pointed at is outlined and scrolled to.
fn setup(ui: &mut egui::Ui, jobs: &mut Jobs, state: &mut UiState, s: &SetupVm, p: &Palette) {
    ui.heading(&s.title);
    for d in &s.diagnostics {
        diag(ui, d);
    }
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for g in &s.groups {
            let head = ui.label(egui::RichText::new(&g.label).color(c(p.text)).strong());
            if state.inspector_focus == Some(g.id) {
                let rect = head.rect.expand2(egui::vec2(6.0, 2.0));
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::same(3),
                    egui::Stroke::new(1.5, c(brand::CUTTER_ORANGE)),
                    egui::StrokeKind::Outside,
                );
                head.scroll_to_me(Some(egui::Align::TOP));
            }
            egui::Grid::new(("setup", &s.id, g.id))
                .num_columns(2)
                .spacing([12.0, 4.0])
                .min_col_width(110.0)
                .show(ui, |ui| {
                    for r in &g.rows {
                        setup_row(ui, jobs, state, &s.id, r, p);
                    }
                });
            ui.add_space(8.0);
        }
        egui::Grid::new(("info", &s.id))
            .num_columns(2)
            .spacing([12.0, 4.0])
            .min_col_width(110.0)
            .show(ui, |ui| {
                for (k, v) in &s.info {
                    ui.label(egui::RichText::new(k).color(c(p.weak_text)));
                    ui.label(v);
                    ui.end_row();
                }
            });
    });
}

fn setup_row(
    ui: &mut egui::Ui,
    jobs: &mut Jobs,
    state: &mut UiState,
    id: &str,
    r: &RowVm,
    p: &Palette,
) {
    let label = ui.label(&r.label);
    let path = format!("setup/{id}/{}", r.key);
    ui.vertical(|ui| {
        let mut commit = None;
        ui.horizontal(|ui| match &r.widget {
            RowWidget::ReadOnly => {
                ui.label(&r.value);
            }
            RowWidget::Text => commit = entry(ui, state, &path, &r.value, None, label.id, ""),
            RowWidget::Number { unit } => {
                let help = tr("inspector-entry-help");
                commit = entry(ui, state, &path, &r.value, *unit, label.id, &help);
            }
            RowWidget::Choice(options) => {
                let shown = options
                    .iter()
                    .find(|(v, _)| *v == r.value)
                    .map_or(r.value.as_str(), |(_, l)| l.as_str());
                egui::ComboBox::from_id_salt(&path)
                    .selected_text(shown)
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for (v, l) in options {
                            if ui.selectable_label(*v == r.value, l).clicked() && *v != r.value {
                                commit = Some(v.clone());
                            }
                        }
                    })
                    .response
                    .labelled_by(label.id);
            }
        });
        if let Some(text) = commit
            && let Some(snap) = jobs.snapshot.clone()
        {
            match setup_edit(&snap, id, &r.key, &text) {
                Ok(e) => {
                    state.entry_error = None;
                    jobs.edit(Edit::Model(e));
                }
                Err(msg) => state.entry_error = Some((path.clone(), msg)),
            }
        }
        if let Some(text) = &r.origin.inherited {
            ui.label(egui::RichText::new(text).small().color(c(p.weak_text)));
        }
        if let Some(field) = r.origin.reset {
            let reset = egui::Button::new(egui::RichText::new(tr("setup-reset-inherited")).small())
                .frame(false);
            if ui.add(reset).clicked() {
                jobs.edit(Edit::Model(reset_edit(id, field)));
            }
        }
        entry_message(ui, state, &path);
    });
    ui.end_row();
}

fn entry_message(ui: &mut egui::Ui, state: &UiState, path: &str) {
    if let Some((at, msg)) = &state.entry_error
        && at == path
    {
        ui.label(egui::RichText::new(msg).small().color(c(brand::RED)));
    }
}

/// A text entry that shows `value`, or the text being typed, and returns
/// the text once, on Enter or when focus leaves, if it changed.
fn entry(
    ui: &mut egui::Ui,
    state: &mut UiState,
    key: &str,
    value: &str,
    unit: Option<&str>,
    label: egui::Id,
    help: &str,
) -> Option<String> {
    let mut text = state
        .entries
        .get(key)
        .cloned()
        .unwrap_or_else(|| value.to_owned());
    let mut r = ui
        .add(egui::TextEdit::singleline(&mut text).desired_width(110.0))
        .labelled_by(label);
    if !help.is_empty() {
        r = r.on_hover_text(help);
    }
    if r.changed() {
        state.entries.insert(key.to_owned(), text.clone());
    }
    if let Some(u) = unit {
        ui.label(u);
    }
    if r.lost_focus() && state.entries.remove(key).is_some() && text.trim() != value {
        return Some(text);
    }
    None
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
    if tab.group == Group::ToolParams {
        tool_row(ui, jobs, &t.id, &t.tool);
        ui.separator();
    }
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

/// The tool dropdown, with why the tool does not suit under it.
fn tool_row(ui: &mut egui::Ui, jobs: &mut Jobs, tp: &str, v: &ToolChoiceVm) {
    let shown = v
        .options
        .iter()
        .find(|(r, _)| *r == v.value)
        .map_or(v.value.as_str(), |(_, l)| l.as_str());
    let mut chosen = None;
    ui.horizontal(|ui| {
        let label = ui.label(tr("inspector-tool"));
        egui::ComboBox::from_id_salt(("tool", tp))
            .selected_text(shown)
            .width(260.0)
            .show_ui(ui, |ui| {
                for (r, l) in &v.options {
                    if ui.selectable_label(*r == v.value, l).clicked() && *r != v.value {
                        chosen = Some(r.clone());
                    }
                }
            })
            .response
            .labelled_by(label.id);
    });
    if let Some(r) = chosen {
        jobs.edit(Edit::Model(tool_edit(tp, &r)));
    }
    for d in &v.diagnostics {
        diag(ui, d);
    }
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
            Widget::Height {
                refs,
                reference,
                offset,
                unit,
            } => height(
                ui,
                jobs,
                state,
                tp,
                f,
                label.id,
                (refs, reference, offset, *unit),
            ),
            Widget::Text => {
                let key = format!("{tp}/{}", f.path);
                if let Some(t) = entry(ui, state, &key, &f.value, None, label.id, "") {
                    let v = (!t.is_empty()).then_some(PValue::Enum(t));
                    jobs.edit(value_edit(tp, &f.path, v));
                }
            }
            Widget::Points { unit } => {
                let key = format!("{tp}/{}", f.path);
                let help = tr("inspector-points-help");
                let shown = entry(ui, state, &key, &f.value, *unit, label.id, &help);
                if let (Some(t), Some(snap)) = (shown, jobs.snapshot.clone()) {
                    finish(jobs, state, &f.path, points_edit(&snap, tp, &f.path, &t));
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
        entry_message(ui, state, &f.path);
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
    let help = tr("inspector-entry-help");
    let Some(text) = entry(ui, state, &key, &f.value, unit_of(&f.widget), label, &help) else {
        return;
    };
    let Some(snap) = jobs.snapshot.clone() else {
        return;
    };
    finish(jobs, state, &f.path, number_edit(&snap, tp, &f.path, &text));
}

/// Sends the edit of an entry, or shows why there is none.
fn finish(jobs: &mut Jobs, state: &mut UiState, path: &str, r: Result<Edit, String>) {
    match r {
        Ok(e) => {
            state.entry_error = None;
            jobs.edit(e);
        }
        Err(msg) => state.entry_error = Some((path.to_owned(), msg)),
    }
}

/// `refs`, reference, offset and unit of a height.
type HeightView<'a> = (
    &'a [(String, String)],
    &'a str,
    &'a str,
    Option<&'static str>,
);

/// A height: its reference, and its offset beside it.
fn height(
    ui: &mut egui::Ui,
    jobs: &mut Jobs,
    state: &mut UiState,
    tp: &str,
    f: &FieldVm,
    label: egui::Id,
    (refs, reference, offset, unit): HeightView<'_>,
) {
    let Some(snap) = jobs.snapshot.clone() else {
        return;
    };
    let shown = refs
        .iter()
        .find(|(v, _)| v == reference)
        .map_or(reference, |(_, l)| l.as_str());
    let mut chosen = None;
    egui::ComboBox::from_id_salt(("height", tp, &f.path))
        .selected_text(shown)
        .width(130.0)
        .show_ui(ui, |ui| {
            for (v, l) in refs {
                if ui.selectable_label(v == reference, l).clicked() && v != reference {
                    chosen = Some(v.clone());
                }
            }
        })
        .response
        .labelled_by(label);
    if let Some(r) = chosen {
        let e = height_edit(&snap, tp, &f.path, Some(&r), None);
        finish(jobs, state, &f.path, e);
    }
    let key = format!("{tp}/{}/offset", f.path);
    let help = tr("inspector-height-offset");
    let off = ui.label("+");
    if let Some(t) = entry(ui, state, &key, offset, unit, off.id, &help) {
        let e = height_edit(&snap, tp, &f.path, None, Some(&t));
        finish(jobs, state, &f.path, e);
    }
}
