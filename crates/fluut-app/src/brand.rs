// SPDX-License-Identifier: MIT OR Apache-2.0

//! Brand colours (`assets/README.md`), the dark and light themes, the bundled
//! UI font, and the rasters `build.rs` makes from the SVG logos (D13 §6.1).

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, Stroke, Visuals};
use std::sync::Arc;

/// An sRGB colour.
pub type Rgb = [u8; 3];

/// Ink: icon tile, dark theme base surface, text on light surfaces.
pub const INK: Rgb = [0x16, 0x18, 0x1D];
/// Cutter orange: the one accent, for focus and selection (S 08 §8.9).
pub const CUTTER_ORANGE: Rgb = [0xFF, 0x5B, 0x14];
/// Stock grey: model colour and secondary text.
pub const STOCK_GREY: Rgb = [0x8A, 0x93, 0xA0];
/// Paper: text on dark surfaces.
pub const PAPER: Rgb = [0xF4, 0xF2, 0xEE];
/// Flute white: light theme base surface.
pub const FLUTE_WHITE: Rgb = [0xFA, 0xF9, 0xF6];

/// Surfaces of the dark theme, one elevation step apart (from S4).
pub const PANEL: Rgb = [0x1E, 0x21, 0x28];
pub const RAISED: Rgb = [0x27, 0x2B, 0x34];
pub const LINE: Rgb = [0x33, 0x38, 0x42];
/// Surfaces of the light theme.
pub const LIGHT_PANEL: Rgb = [0xEE, 0xEC, 0xE7];
pub const LIGHT_RAISED: Rgb = [0xE3, 0xE1, 0xDB];
pub const LIGHT_LINE: Rgb = [0xCF, 0xCC, 0xC5];
/// Secondary text on light surfaces (stock grey is too light there).
pub const SLATE: Rgb = [0x56, 0x5D, 0x68];

/// Status colours (S 08 §8.6). Each status also has its own icon shape.
pub const AMBER: Rgb = [0xF2, 0xB1, 0x34];
pub const RED: Rgb = [0xE5, 0x48, 0x4D];
pub const GREEN: Rgb = [0x4C, 0xC3, 0x8A];
pub const BLUE: Rgb = [0x4C, 0x8D, 0xF6];

/// Colour-blind-safe colours for data (Okabe and Ito): the simulated stock
/// by rest class and by toolpath.
pub const OI_ORANGE: Rgb = [0xE6, 0x9F, 0x00];
pub const OI_SKY: Rgb = [0x56, 0xB4, 0xE9];
pub const OI_GREEN: Rgb = [0x00, 0x9E, 0x73];
pub const OI_YELLOW: Rgb = [0xF0, 0xE4, 0x42];
pub const OI_BLUE: Rgb = [0x00, 0x72, 0xB2];
pub const OI_VERMILLION: Rgb = [0xD5, 0x5E, 0x00];
pub const OI_PURPLE: Rgb = [0xCC, 0x79, 0xA7];

/// The stock by rest class (`fluut_sim::mesh` OFF, REST, WITHIN, GOUGE):
/// plain off the part, blue above tolerance, green within, vermillion for a
/// gouge.
pub const REST_COLOURS: [Rgb; 4] = [STOCK_GREY, OI_BLUE, OI_GREEN, OI_VERMILLION];

/// The stock by toolpath, in turn (the shader repeats them after 7).
pub const TOOLPATH_COLOURS: [Rgb; 7] = [
    OI_ORANGE,
    OI_SKY,
    OI_GREEN,
    OI_YELLOW,
    OI_BLUE,
    OI_VERMILLION,
    OI_PURPLE,
];

pub fn c(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// sRGB byte to linear light, for the viewport shader.
pub fn to_linear(c: Rgb) -> [f32; 3] {
    c.map(|b| {
        let v = f32::from(b) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    })
}

/// WCAG 2 contrast ratio of two sRGB colours.
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let lum = |c: Rgb| {
        let [r, g, b] = to_linear(c);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Colours a theme is built from.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub base: Rgb,
    pub panel: Rgb,
    pub raised: Rgb,
    pub line: Rgb,
    pub text: Rgb,
    pub weak_text: Rgb,
}

pub const DARK: Palette = Palette {
    dark: true,
    base: INK,
    panel: PANEL,
    raised: RAISED,
    line: LINE,
    text: PAPER,
    weak_text: STOCK_GREY,
};

pub const LIGHT: Palette = Palette {
    dark: false,
    base: FLUTE_WHITE,
    panel: LIGHT_PANEL,
    raised: LIGHT_RAISED,
    line: LIGHT_LINE,
    text: INK,
    weak_text: SLATE,
};

impl Palette {
    pub fn get(dark: bool) -> &'static Palette {
        if dark { &DARK } else { &LIGHT }
    }

    /// `egui` visuals with the brand colours as style tokens (D13 §6).
    pub fn visuals(&self) -> Visuals {
        let mut v = if self.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        let accent = c(CUTTER_ORANGE);
        v.override_text_color = Some(c(self.text));
        v.panel_fill = c(self.panel);
        v.window_fill = c(self.panel);
        v.extreme_bg_color = c(self.base);
        v.faint_bg_color = c(self.raised);
        v.selection.bg_fill = accent.gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0, accent);
        v.hyperlink_color = accent;
        v.widgets.noninteractive.bg_fill = c(self.panel);
        v.widgets.noninteractive.bg_stroke.color = c(self.line);
        v.widgets.noninteractive.fg_stroke.color = c(self.text);
        v.widgets.inactive.bg_fill = c(self.raised);
        v.widgets.inactive.weak_bg_fill = c(self.raised);
        v.widgets.inactive.fg_stroke.color = c(self.text);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, accent);
        v.widgets.hovered.fg_stroke.color = c(self.text);
        v.widgets.active.bg_fill = accent;
        v.widgets.active.weak_bg_fill = accent;
        v.widgets.open.weak_bg_fill = c(self.raised);
        v
    }
}

/// Applies the theme to every style of `ctx`.
pub fn apply_theme(ctx: &egui::Context, dark: bool) {
    let p = Palette::get(dark);
    ctx.set_visuals(p.visuals());
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::Vec2::new(8.0, 6.0);
    });
}

/// The bundled UI font (OFL-1.1, licence in `assets/fonts/OFL.txt`).
pub const FONT: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
const FONT_NAME: &str = "noto-sans";

/// Font definitions with Noto Sans as the only family: `default_fonts` is
/// off (D13 §6), so nothing else is loaded.
pub fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::empty();
    f.font_data
        .insert(FONT_NAME.into(), Arc::new(FontData::from_static(FONT)));
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        f.families.insert(fam, vec![FONT_NAME.into()]);
    }
    f
}

/// A raster made by `build.rs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    /// Straight RGBA.
    pub rgba: Vec<u8>,
}

impl Raster {
    fn decode(bytes: &[u8]) -> Raster {
        let word =
            |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        Raster {
            width: word(0),
            height: word(4),
            rgba: bytes[8..].to_vec(),
        }
    }

    pub fn to_color_image(&self) -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied(
            [self.width as usize, self.height as usize],
            &self.rgba,
        )
    }
}

/// The app icon (`assets/fluut-D-icon-stock.svg`, 256 px).
pub fn icon() -> Raster {
    Raster::decode(include_bytes!(concat!(env!("OUT_DIR"), "/icon.rgba")))
}

/// The full logo for the theme: the dark variant on dark surfaces.
pub fn logo(dark: bool) -> Raster {
    if dark {
        Raster::decode(include_bytes!(concat!(env!("OUT_DIR"), "/logo-dark.rgba")))
    } else {
        Raster::decode(include_bytes!(concat!(env!("OUT_DIR"), "/logo-light.rgba")))
    }
}

/// Window icon data for `eframe`.
pub fn icon_data() -> egui::IconData {
    let r = icon();
    egui::IconData {
        rgba: r.rgba,
        width: r.width,
        height: r.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_contrast_meets_wcag_aa() {
        for p in [DARK, LIGHT] {
            for surface in [p.base, p.panel, p.raised] {
                assert!(contrast(p.text, surface) >= 4.5, "{p:?}");
                assert!(contrast(p.weak_text, surface) >= 4.5, "{p:?} weak");
            }
        }
    }

    #[test]
    fn icon_is_rendered_from_the_svg() {
        let r = icon();
        assert_eq!((r.width, r.height), (256, 256));
        assert_eq!(r.rgba.len(), 256 * 256 * 4);
        // The ink tile is opaque beside the cutter; the corners are rounded off.
        let px = |x: usize, y: usize| &r.rgba[(y * 256 + x) * 4..][..4];
        assert_eq!(px(30, 30), [INK[0], INK[1], INK[2], 255]);
        assert_eq!(px(0, 0)[3], 0);
        assert_eq!(px(128, 20)[..3], CUTTER_ORANGE);
    }

    #[test]
    fn logos_differ_by_theme() {
        let (l, d) = (logo(false), logo(true));
        assert_eq!(l.height, 192);
        assert!(l.width > l.height);
        assert_ne!(l.rgba, d.rgba);
    }

    #[test]
    fn only_the_bundled_font_is_loaded() {
        let f = fonts();
        assert_eq!(f.font_data.len(), 1);
        assert_eq!(f.families[&FontFamily::Proportional], [FONT_NAME]);
    }
}
