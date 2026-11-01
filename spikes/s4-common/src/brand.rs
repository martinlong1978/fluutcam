// SPDX-License-Identifier: MIT OR Apache-2.0

//! Brand colours from `assets/README.md`, as sRGB bytes, plus the few derived
//! surface shades the prototypes use.

/// An sRGB colour.
pub type Rgb = [u8; 3];

/// Ink: icon tile, base surface of the dark theme.
pub const INK: Rgb = [0x16, 0x18, 0x1D];
/// Cutter orange: the accent for focus and selection (S 08 §8.9).
pub const CUTTER_ORANGE: Rgb = [0xFF, 0x5B, 0x14];
/// Stock grey: model colour and secondary text.
pub const STOCK_GREY: Rgb = [0x8A, 0x93, 0xA0];
/// Paper: primary text on dark surfaces.
pub const PAPER: Rgb = [0xF4, 0xF2, 0xEE];
/// Flute white.
pub const FLUTE_WHITE: Rgb = [0xFA, 0xF9, 0xF6];

/// Raised panel surface, one step lighter than ink.
pub const PANEL: Rgb = [0x1E, 0x21, 0x28];
/// Second elevation step (ribbon, headers).
pub const RAISED: Rgb = [0x27, 0x2B, 0x34];
/// Hairline separators.
pub const LINE: Rgb = [0x33, 0x38, 0x42];
/// Amber for stale and warning states.
pub const AMBER: Rgb = [0xF2, 0xB1, 0x34];
/// Red for errors.
pub const RED: Rgb = [0xE5, 0x48, 0x4D];
/// Green for done.
pub const GREEN: Rgb = [0x4C, 0xC3, 0x8A];

/// sRGB byte to linear light.
#[must_use]
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
