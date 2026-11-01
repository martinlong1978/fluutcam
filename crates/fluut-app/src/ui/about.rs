// SPDX-License-Identifier: MIT OR Apache-2.0

//! The About dialog (D13 §6.1): the logo for the current theme, version and
//! licences.

use super::UiState;
use crate::brand;
use crate::text::{tr, tr_args};
use eframe::egui;

/// Logo textures, loaded once per theme.
fn logo(ctx: &egui::Context, dark: bool) -> egui::TextureHandle {
    let id = egui::Id::new(("fluut-logo", dark));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let t = ctx.load_texture(
        if dark { "logo-dark" } else { "logo-light" },
        brand::logo(dark).to_color_image(),
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

pub fn show(ctx: &egui::Context, state: &mut UiState) {
    if !state.show_about {
        return;
    }
    let mut open = true;
    let mut close = false;
    egui::Window::new(tr("title-about"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                let t = logo(ctx, state.dark);
                let size = t.size_vec2() * (96.0 / t.size_vec2().y);
                ui.add(egui::Image::new((t.id(), size)).alt_text(tr("app-title")));
                ui.add_space(8.0);
                ui.label(tr("about-tagline"));
                ui.label(tr_args(
                    "about-version",
                    &[("version", env!("CARGO_PKG_VERSION"))],
                ));
                ui.add_space(8.0);
                ui.label(egui::RichText::new(tr("about-licence")).small());
                ui.label(egui::RichText::new(tr("about-font")).small());
                ui.add_space(8.0);
                if ui.button(tr("about-close")).clicked() {
                    close = true;
                }
            });
        });
    state.show_about = open && !close;
}
