// SPDX-License-Identifier: MIT OR Apache-2.0

//! Renders the brand SVGs in `assets/` to RGBA for the window icon and the
//! About dialog (D13 §6.1). The rasters go to `OUT_DIR` only: generated
//! images are never committed (D14 §5.2, `assets/README.md`).
//!
//! Output format: width and height as little-endian `u32`, then straight
//! (not premultiplied) RGBA rows.
//!
//! It also rasterises the ribbon icons in `assets/icons/` (S 08 §8.9) at
//! [`ICON_SIZES`], as two coverage masks per icon: ink and accent (pixels
//! of the cutter-orange accent colour). The app tints them by theme.
//! `icons-<size>.bin` holds, icon by icon in name order, the ink mask then
//! the accent mask, `size * size` bytes each; `icons.rs` lists the names.
//!
//! On Windows it also gives `fluutcam.exe` its version resource (D14 §5.2).
//! Code signing checks the product name and version in it
//! (docs/CODE_SIGNING.md).

use resvg::{tiny_skia, usvg};
use std::path::{Path, PathBuf};

/// `(svg in assets/, output name, height in pixels)`.
const RASTERS: &[(&str, &str, u32)] = &[
    ("fluut-D-icon-stock.svg", "icon.rgba", 256),
    ("fluutcam-D-logo.svg", "logo-light.rgba", 192),
    ("fluutcam-D-logo-dark.svg", "logo-dark.rgba", 192),
];

/// Icon raster sizes in pixels: 16 and 24 at 1x, 32 and 48 at 2x.
const ICON_SIZES: [u32; 4] = [16, 24, 32, 48];

fn icons(out: &Path) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("assets/icons")
        .filter_map(|e| {
            let p = e.expect("dir entry").path();
            let stem = p.file_stem()?.to_str()?.to_owned();
            (p.extension()? == "svg").then_some(stem)
        })
        .collect();
    names.sort();
    let trees: Vec<usvg::Tree> = names
        .iter()
        .map(|n| {
            let src = dir.join(format!("{n}.svg"));
            println!("cargo:rerun-if-changed={}", src.display());
            let data = std::fs::read(&src).unwrap_or_else(|e| panic!("{}: {e}", src.display()));
            usvg::Tree::from_data(&data, &usvg::Options::default())
                .unwrap_or_else(|e| panic!("{n}.svg: {e}"))
        })
        .collect();
    for size in ICON_SIZES {
        let mut bytes = Vec::new();
        for tree in &trees {
            let scale = size as f32 / tree.size().width();
            let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("non-empty raster");
            resvg::render(
                tree,
                tiny_skia::Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            let (mut ink, mut accent) = (Vec::new(), Vec::new());
            for p in pixmap.pixels() {
                let c = p.demultiply();
                // Ink is near grey (#16181D), the accent orange (#FF5B14).
                let w = ((f32::from(c.red()) - f32::from(c.blue())) / 235.0).clamp(0.0, 1.0);
                let a = f32::from(c.alpha());
                accent.push((a * w).round() as u8);
                ink.push((a * (1.0 - w)).round() as u8);
            }
            bytes.extend(ink);
            bytes.extend(accent);
        }
        std::fs::write(out.join(format!("icons-{size}.bin")), bytes).expect("write icons");
    }
    let list: String = names
        .iter()
        .map(|n| {
            format!(
                "    {n:?},
"
            )
        })
        .collect();
    std::fs::write(
        out.join("icons.rs"),
        format!(
            "/// Icon names (file stems in `assets/icons/`), sorted.
pub const NAMES: [&str; {}] = [
{list}];
/// Raster sizes, px.
pub const SIZES: [u32; {}] = {ICON_SIZES:?};
",
            names.len(),
            ICON_SIZES.len()
        ),
    )
    .expect("write icons.rs");
}

fn main() {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    icons(&out);
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    for (svg, name, height) in RASTERS {
        let src = assets.join(svg);
        println!("cargo:rerun-if-changed={}", src.display());
        let data = std::fs::read(&src).unwrap_or_else(|e| panic!("{}: {e}", src.display()));
        let tree = usvg::Tree::from_data(&data, &usvg::Options::default())
            .unwrap_or_else(|e| panic!("{svg}: {e}"));
        let size = tree.size();
        let scale = *height as f32 / size.height();
        let width = (size.width() * scale).round() as u32;
        let mut pixmap = tiny_skia::Pixmap::new(width, *height).expect("non-empty raster");
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        let mut bytes = Vec::with_capacity(8 + pixmap.data().len());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        for p in pixmap.pixels() {
            let c = p.demultiply();
            bytes.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
        }
        std::fs::write(out.join(name), bytes).expect("write raster");
    }

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "FluutCAM")
            .set("FileDescription", "FluutCAM")
            .set("OriginalFilename", "fluutcam.exe")
            .set("LegalCopyright", "MIT OR Apache-2.0");
        res.compile().expect("compile the Windows version resource");
    }
}
