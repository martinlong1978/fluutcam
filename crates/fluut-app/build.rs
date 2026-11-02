// SPDX-License-Identifier: MIT OR Apache-2.0

//! Renders the brand SVGs in `assets/` to RGBA for the window icon and the
//! About dialog (D13 §6.1). The rasters go to `OUT_DIR` only: generated
//! images are never committed (D14 §5.2, `assets/README.md`).
//!
//! Output format: width and height as little-endian `u32`, then straight
//! (not premultiplied) RGBA rows.
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

fn main() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
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
