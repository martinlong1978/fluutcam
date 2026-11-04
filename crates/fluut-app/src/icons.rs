// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ribbon icon rasters `build.rs` makes from `assets/icons/` (S 08
//! §8.9): an ink mask and an accent mask per icon and size. The widget
//! layer tints them by theme (`ui::icons`).

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

/// Bytes of one raster size: all icons, ink mask then accent mask.
fn blob(size: u32) -> Option<&'static [u8]> {
    match size {
        16 => Some(include_bytes!(concat!(env!("OUT_DIR"), "/icons-16.bin"))),
        24 => Some(include_bytes!(concat!(env!("OUT_DIR"), "/icons-24.bin"))),
        32 => Some(include_bytes!(concat!(env!("OUT_DIR"), "/icons-32.bin"))),
        48 => Some(include_bytes!(concat!(env!("OUT_DIR"), "/icons-48.bin"))),
        _ => None,
    }
}

/// One icon at one size: coverage masks, `size * size` bytes each.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Masks {
    pub size: u32,
    pub ink: &'static [u8],
    pub accent: &'static [u8],
}

/// The masks of icon `name` at a size of [`SIZES`].
pub fn masks(name: &str, size: u32) -> Option<Masks> {
    let i = NAMES.binary_search(&name).ok()?;
    let n = (size * size) as usize;
    let at = blob(size)?.get(2 * i * n..2 * (i + 1) * n)?;
    let (ink, accent) = at.split_at(n);
    Some(Masks { size, ink, accent })
}

/// The raster size for an icon shown at `points`, on a display with
/// `pixels_per_point`: the smallest that is sharp, else the largest.
pub fn raster_size(points: f32, pixels_per_point: f32) -> u32 {
    let px = points * pixels_per_point;
    let largest = SIZES[SIZES.len() - 1];
    SIZES
        .into_iter()
        .find(|&s| s as f32 >= px - 0.5)
        .unwrap_or(largest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::ribbon::Command;
    use std::path::Path;

    fn dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons")
    }

    #[test]
    fn every_command_has_an_icon_with_ink_at_every_size() {
        for c in Command::all() {
            for size in SIZES {
                let m = masks(c.icon(), size).unwrap_or_else(|| panic!("{}", c.icon()));
                assert_eq!(m.ink.len(), (size * size) as usize);
                assert_eq!(m.accent.len(), m.ink.len());
                let covered = m.ink.iter().filter(|&&a| a > 48).count();
                assert!(covered > 0, "{} at {size}", c.icon());
            }
        }
    }

    #[test]
    fn every_master_is_used() {
        let used: Vec<_> = Command::all().iter().map(|c| c.icon()).collect();
        for n in NAMES {
            assert!(used.contains(&n), "{n}.svg is not used");
        }
    }

    #[test]
    fn unknown_icons_and_sizes_have_no_masks() {
        assert!(masks("no-such-icon", 24).is_none());
        assert!(masks(NAMES[0], 25).is_none());
    }

    #[test]
    fn accent_and_ink_land_in_their_own_masks() {
        // `tp-face`: the tool position is orange, the stock outline ink.
        let m = masks("tp-face", 48).unwrap();
        let at = |mask: &[u8], x: usize, y: usize| mask[y * 48 + x];
        assert!(at(m.accent, 35, 32) > 200);
        assert!(at(m.ink, 35, 32) < 40);
        assert!(at(m.ink, 6, 24) > 200);
        assert!(at(m.accent, 6, 24) < 40);
    }

    #[test]
    fn raster_sizes_follow_the_display() {
        assert_eq!(raster_size(16.0, 1.0), 16);
        assert_eq!(raster_size(24.0, 1.0), 24);
        assert_eq!(raster_size(16.0, 2.0), 32);
        assert_eq!(raster_size(24.0, 2.0), 48);
        assert_eq!(raster_size(24.0, 1.5), 48);
        assert_eq!(raster_size(24.0, 3.0), 48);
    }

    /// The style rules of `assets/icons/README.md`.
    #[test]
    fn masters_parse_and_keep_the_style() {
        let mut n = 0;
        for e in std::fs::read_dir(dir()).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_none_or(|x| x != "svg") {
                continue;
            }
            n += 1;
            let name = p.display().to_string();
            let s = std::fs::read_to_string(&p).unwrap();
            let tree = resvg::usvg::Tree::from_str(&s, &resvg::usvg::Options::default())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(tree.size().width(), 24.0, "{name}");
            assert_eq!(tree.size().height(), 24.0, "{name}");
            let root = s.split('>').next().unwrap();
            for attr in [
                r#"viewBox="0 0 24 24""#,
                r#"fill="none""#,
                r##"stroke="#16181D""##,
                r#"stroke-width="1.75""#,
                r#"stroke-linecap="round""#,
                r#"stroke-linejoin="round""#,
            ] {
                assert!(root.contains(attr), "{name}: {attr}");
            }
            assert_eq!(s.matches("stroke-width").count(), 1, "{name}");
            assert!(s.matches(r##"fill="#FF5B14""##).count() <= 2, "{name}");
            for colour in s.split('#').skip(1) {
                let c = &colour[..6];
                assert!(c == "16181D" || c == "FF5B14", "{name}: #{c}");
            }
        }
        assert_eq!(n, NAMES.len());
    }
}
