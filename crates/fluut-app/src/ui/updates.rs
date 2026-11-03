// SPDX-License-Identifier: MIT OR Apache-2.0

//! Update check widgets (spec 06 §6.4): the first-start question, the
//! Settings window and the notice in the status bar.
//!
//! The widgets only read [`Updates`] and call its methods; the check itself
//! runs on its own thread. The links of an offer are shown as text with a
//! copy button: this build has no browser launcher, and FluutCAM never
//! downloads or installs an update itself.

use super::UiState;
use crate::text::{tr, tr_args};
use crate::updates::{UpdateState, Updates};
use eframe::egui;

/// "Check for updates automatically?" on first start. Escape, or a click
/// outside the dialog, is a No.
pub fn prompt(ctx: &egui::Context, updates: &mut Updates) {
    if !updates.needs_prompt() {
        return;
    }
    let shown = egui::Modal::new(egui::Id::new("update_prompt")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.heading(tr("update-prompt-title"));
        ui.add_space(4.0);
        ui.label(tr("update-prompt-text"));
        ui.add_space(8.0);
        let mut answer = None;
        ui.horizontal(|ui| {
            if ui.button(tr("update-prompt-yes")).clicked() {
                answer = Some(true);
            }
            if ui.button(tr("update-prompt-no")).clicked() {
                answer = Some(false);
            }
        });
        answer
    });
    let answer = shown
        .inner
        .or_else(|| shown.should_close().then_some(false));
    if let Some(yes) = answer {
        updates.answer(yes);
    }
}

/// The Settings window: the automatic-check toggle, "Check now" and the
/// result.
pub fn settings(ctx: &egui::Context, state: &mut UiState, updates: &mut Updates) {
    if !state.show_settings {
        return;
    }
    let mut open = true;
    egui::Window::new(tr("title-settings"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.strong(tr("settings-updates"));
            let mut auto = updates.auto_check();
            let toggle = egui::Checkbox::new(&mut auto, tr("settings-auto-check"));
            if ui.add_enabled(updates.has_key(), toggle).changed() {
                updates.answer(auto);
            }
            ui.label(egui::RichText::new(tr("settings-privacy")).small());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let idle = !updates.is_checking();
                if ui
                    .add_enabled(idle, egui::Button::new(tr("settings-check-now")))
                    .clicked()
                {
                    updates.check_now();
                }
                result(ui, updates.state());
            });
            if let UpdateState::Available(o) = updates.state() {
                ui.add_space(4.0);
                link(ui, "update-notes", &o.notes_url);
                link(ui, "update-download", &o.download_page);
                ui.label(egui::RichText::new(tr("update-download-hint")).small());
            }
        });
    state.show_settings = open;
}

/// One line about the last check.
fn result(ui: &mut egui::Ui, s: &UpdateState) {
    match s {
        UpdateState::Idle => {}
        UpdateState::Checking => {
            ui.label(tr("update-checking"));
        }
        UpdateState::UpToDate => {
            ui.label(tr("update-up-to-date"));
        }
        UpdateState::Available(o) => {
            ui.label(tr_args(
                "update-available",
                &[("version", &o.version), ("date", &o.date)],
            ));
        }
        UpdateState::NoKey => {
            ui.label(tr("update-no-key"));
        }
        UpdateState::Failed(why) => {
            ui.label(tr_args("update-failed", &[("detail", why)]));
        }
    }
}

/// A label, the address as selectable text, and a button that copies it.
fn link(ui: &mut egui::Ui, label: &str, url: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(tr(label));
        ui.add(egui::Label::new(url).selectable(true));
        if ui.small_button(tr("update-copy")).clicked() {
            ui.ctx().copy_text(url.to_owned());
        }
    });
}

/// The status-bar notice when a newer version exists. `true` when the user
/// asks for the details.
pub fn notice(ui: &mut egui::Ui, updates: &Updates) -> bool {
    let UpdateState::Available(o) = updates.state() else {
        return false;
    };
    ui.separator();
    ui.label(tr_args(
        "update-available-status",
        &[("version", &o.version)],
    ));
    ui.small_button(tr("update-details")).clicked()
}
