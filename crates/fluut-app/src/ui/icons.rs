// SPDX-License-Identifier: MIT OR Apache-2.0

//! Paints the ribbon icons (S 08 §8.9). Each icon's two masks are uploaded
//! as white textures on first use, so one set serves both themes: the ink
//! is tinted with the theme's text colour (full strength, also for the
//! small icons), the accent stays cutter orange,
//! and a disabled icon is grey throughout.

use crate::brand::{self, Palette, c};
use crate::icons::{self, Masks};
use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, TextureHandle, TextureOptions};
use std::collections::BTreeMap;

/// `(name, raster size, accent)` to its texture; `None` for an empty mask.
type Cache = BTreeMap<(&'static str, u32, bool), Option<TextureHandle>>;

fn texture(ctx: &egui::Context, m: Masks, name: &str, accent: bool) -> Option<TextureHandle> {
    let mask = if accent { m.accent } else { m.ink };
    if mask.iter().all(|&a| a == 0) {
        return None;
    }
    let pixels = mask.iter().map(|&a| Color32::from_white_alpha(a)).collect();
    let image = ColorImage::new([m.size as usize; 2], pixels);
    let label = format!("icon {name} {} {accent}", m.size);
    Some(ctx.load_texture(label, image, TextureOptions::LINEAR))
}

fn cached(
    ctx: &egui::Context,
    name: &'static str,
    size: u32,
    accent: bool,
) -> Option<TextureHandle> {
    let id = egui::Id::new("icon_textures");
    let found = ctx.data(|d| {
        d.get_temp::<std::sync::Arc<Cache>>(id)
            .and_then(|c| c.get(&(name, size, accent)).cloned())
    });
    if let Some(t) = found {
        return t;
    }
    let t = icons::masks(name, size).and_then(|m| texture(ctx, m, name, accent));
    ctx.data_mut(|d| {
        let cache = d.get_temp_mut_or_default::<std::sync::Arc<Cache>>(id);
        std::sync::Arc::make_mut(cache).insert((name, size, accent), t.clone());
    });
    t
}

/// Paints icon `name` centred in `rect`, `points` wide.
pub fn paint(
    ui: &egui::Ui,
    rect: Rect,
    name: &'static str,
    points: f32,
    enabled: bool,
    p: &Palette,
) {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let size = icons::raster_size(points, ppp);
    let rect = Rect::from_center_size(rect.center(), egui::Vec2::splat(points));
    // On whole pixels, or the lines blur.
    let min = (rect.min.to_vec2() * ppp).round() / ppp;
    let rect = Rect::from_min_size(min.to_pos2(), rect.size());
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    let (ink, accent) = if enabled {
        (c(p.text), c(brand::CUTTER_ORANGE))
    } else {
        let grey = c(p.weak_text).gamma_multiply(0.75);
        (grey, grey)
    };
    // The 16 px raster is the 24 px drawing at two thirds: its 1.75 px
    // stroke is about a pixel wide and half covered. Painting it twice
    // makes a heavier mask, so small icons read as strongly as their labels.
    let passes = if size <= 16 { 2 } else { 1 };
    for (is_accent, tint) in [(false, ink), (true, accent)] {
        if let Some(t) = cached(ctx, name, size, is_accent) {
            for _ in 0..passes {
                ui.painter().image(t.id(), rect, uv, tint);
            }
        }
    }
}
