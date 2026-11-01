// SPDX-License-Identifier: MIT OR Apache-2.0

//! `cargo xtask icons <out-dir>`: the app icon for every platform, rendered
//! from the SVG master in `assets/` (D14 §5.2). Nothing it writes is
//! committed.
//!
//! ```text
//! <out-dir>/windows/fluutcam.ico
//! <out-dir>/macos/fluutcam.icns
//! <out-dir>/linux/hicolor/<N>x<N>/apps/fluutcam.png
//! ```
//!
//! ICO and ICNS hold PNG images, so the writers are a few header bytes
//! around the PNGs (ICO from Windows Vista, ICNS from macOS 10.7).

use resvg::{tiny_skia, usvg};
use std::path::{Path, PathBuf};

/// The master, in `assets/`: the cutter going into the stock.
pub const MASTER: &str = "fluut-D-icon-stock.svg";
/// Sizes in the Windows icon; 256 is stored as 0 in the directory.
pub const ICO_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256];
/// Sizes in the freedesktop `hicolor` theme.
pub const LINUX_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256, 512];
/// ICNS element types and the pixel size each holds (the `@2x` types hold
/// twice their nominal size).
pub const ICNS_TYPES: &[(&[u8; 4], u32)] = &[
    (b"icp4", 16),
    (b"icp5", 32),
    (b"icp6", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic13", 256),
    (b"ic14", 512),
];

/// Renders the SVG square at `px` pixels and encodes it as PNG.
pub fn render_png(svg: &[u8], px: u32) -> Result<Vec<u8>, String> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default())
        .map_err(|e| format!("bad SVG: {e}"))?;
    let size = tree.size();
    let mut pixmap = tiny_skia::Pixmap::new(px, px).ok_or("empty raster")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(px as f32 / size.width(), px as f32 / size.height()),
        &mut pixmap.as_mut(),
    );
    let mut rgba = Vec::with_capacity(pixmap.data().len());
    for p in pixmap.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, px, px);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&rgba))
        .map_err(|e| format!("PNG encoding failed: {e}"))?;
    Ok(out)
}

/// A Windows icon of PNG images, `(pixel size, PNG)`, at most 256 px each.
pub fn ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (px, png) in images {
        let dim = if *px >= 256 { 0 } else { *px as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in images {
        out.extend_from_slice(png);
    }
    out
}

/// A macOS icon from `(element type, PNG)`. Lengths are big endian and
/// include the 8-byte headers.
pub fn icns(elements: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let total: usize = 8 + elements.iter().map(|(_, d)| 8 + d.len()).sum::<usize>();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&(total as u32).to_be_bytes());
    for (ty, data) in elements {
        out.extend_from_slice(*ty);
        out.extend_from_slice(&((8 + data.len()) as u32).to_be_bytes());
        out.extend_from_slice(data);
    }
    out
}

/// Writes all icons under `out_dir`; returns the files made, sorted.
pub fn generate(assets: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let svg_path = assets.join(MASTER);
    let svg = std::fs::read(&svg_path).map_err(|e| format!("{}: {e}", svg_path.display()))?;
    let mut sizes: Vec<u32> = ICO_SIZES
        .iter()
        .chain(LINUX_SIZES)
        .copied()
        .chain(ICNS_TYPES.iter().map(|t| t.1))
        .collect();
    sizes.sort_unstable();
    sizes.dedup();
    let mut pngs = std::collections::BTreeMap::new();
    for px in sizes {
        pngs.insert(px, render_png(&svg, px)?);
    }

    let mut files = Vec::new();
    let mut write = |rel: String, bytes: &[u8]| -> Result<(), String> {
        let path = out_dir.join(rel);
        std::fs::create_dir_all(path.parent().expect("file path")).map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        files.push(path);
        Ok(())
    };
    let images: Vec<_> = ICO_SIZES.iter().map(|&s| (s, pngs[&s].clone())).collect();
    write("windows/fluutcam.ico".into(), &ico(&images))?;
    let elements: Vec<_> = ICNS_TYPES
        .iter()
        .map(|(ty, s)| (*ty, pngs[s].as_slice()))
        .collect();
    write("macos/fluutcam.icns".into(), &icns(&elements))?;
    for &s in LINUX_SIZES {
        write(
            format!("linux/hicolor/{s}x{s}/apps/fluutcam.png"),
            &pngs[&s],
        )?;
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn assets() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets")
    }

    /// Decodes a PNG to `(width, height, rgba)`.
    fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut r = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        (info.width, info.height, buf)
    }

    fn master() -> Vec<u8> {
        std::fs::read(assets().join(MASTER)).unwrap()
    }

    #[test]
    fn png_is_square_with_rounded_transparent_corners() {
        let (w, h, px) = decode(&render_png(&master(), 128).unwrap());
        assert_eq!((w, h), (128, 128));
        assert_eq!(px[3], 0, "corner pixel is transparent");
        let mid = (64 * 128 + 12) * 4; // ink tile, left of the cutter
        assert_eq!(&px[mid..mid + 4], &[0x16, 0x18, 0x1D, 255]);
    }

    #[test]
    fn rendering_is_deterministic() {
        assert_eq!(
            render_png(&master(), 48).unwrap(),
            render_png(&master(), 48).unwrap()
        );
    }

    #[test]
    fn ico_directory_points_at_the_images() {
        let imgs = vec![(16, vec![1u8; 10]), (256, vec![2u8; 20])];
        let b = ico(&imgs);
        assert_eq!(&b[..6], &[0, 0, 1, 0, 2, 0]);
        // First entry: 16 x 16, 10 bytes at offset 38.
        assert_eq!(&b[6..8], &[16, 16]);
        assert_eq!(u32::from_le_bytes(b[14..18].try_into().unwrap()), 10);
        assert_eq!(u32::from_le_bytes(b[18..22].try_into().unwrap()), 38);
        // Second: 256 is stored as 0.
        assert_eq!(&b[22..24], &[0, 0]);
        assert_eq!(u32::from_le_bytes(b[34..38].try_into().unwrap()), 48);
        assert_eq!(b.len(), 38 + 30);
        assert_eq!(&b[48..], &[2u8; 20][..]);
    }

    #[test]
    fn icns_elements_add_up_to_the_header_length() {
        let b = icns(&[(b"icp4", &[7; 5]), (b"ic07", &[9; 3])]);
        assert_eq!(&b[..4], b"icns");
        assert_eq!(
            u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize,
            b.len()
        );
        assert_eq!(&b[8..12], b"icp4");
        assert_eq!(u32::from_be_bytes(b[12..16].try_into().unwrap()), 13);
        assert_eq!(&b[21..25], b"ic07");
        assert_eq!(b.len(), 8 + 13 + 11);
    }

    #[test]
    fn generate_writes_every_platform() {
        let dir = std::env::temp_dir().join(format!("fluut-icons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let files = generate(&assets(), &dir).unwrap();
        assert_eq!(files.len(), 2 + LINUX_SIZES.len());
        let ico = std::fs::read(dir.join("windows/fluutcam.ico")).unwrap();
        assert_eq!(
            u16::from_le_bytes([ico[4], ico[5]]) as usize,
            ICO_SIZES.len()
        );
        let icns = std::fs::read(dir.join("macos/fluutcam.icns")).unwrap();
        assert_eq!(&icns[..4], b"icns");
        let big = std::fs::read(dir.join("linux/hicolor/512x512/apps/fluutcam.png")).unwrap();
        assert_eq!(decode(&big).0, 512);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn app_window_icon_uses_the_same_master() {
        let build_rs = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/fluut-app/build.rs"),
        )
        .unwrap();
        assert!(build_rs.contains(MASTER));
    }
}
