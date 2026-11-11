// SPDX-License-Identifier: MIT OR Apache-2.0

//! Importers for definitions from other CAM products (spec 05 §5.10,
//! design D16).
//!
//! Everything here reads untrusted files (spec 01 §1.7). The importers are
//! static: they parse data, never run it, and stop at the limits in
//! [`limits`]. This release imports Autodesk Fusion tool-library JSON
//! ([`fusion`]). Vectric `.vtdb` is designed (D16 §7) and not built.

pub mod fusion;
mod toml_out;

use fluut_model::Diagnostic;
use std::collections::BTreeMap;

/// Limits on an imported file (D16 §6).
pub mod limits {
    /// Largest file read, bytes.
    pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
    /// Most tools taken from one file.
    pub const MAX_TOOLS: usize = 10_000;
    /// Most presets taken from one tool.
    pub const MAX_PRESETS: usize = 200;
    /// Most holder segments taken from one tool.
    pub const MAX_SEGMENTS: usize = 64;
    /// Longest name kept, characters.
    pub const MAX_NAME_CHARS: usize = 200;
    /// Largest length accepted, mm.
    pub const MAX_LENGTH_MM: f64 = 10_000.0;
    /// Largest feed accepted, mm/min.
    pub const MAX_FEED_MM_MIN: f64 = 1_000_000.0;
    /// Largest spindle speed accepted, rev/min.
    pub const MAX_RPM: f64 = 1_000_000.0;
}

/// What the user chose for the new library.
#[derive(Clone, PartialEq, Debug)]
pub struct ImportOptions {
    /// Library id, such as `fusion/shop-tools`.
    pub library_id: String,
    /// Library display name.
    pub library_name: String,
}

/// Counts for the import report (spec 05 §5.10).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ImportReport {
    /// Entries in the source file.
    pub tools_read: usize,
    /// Tools in the new library.
    pub tools_written: usize,
    /// Entries left out, with a warning each.
    pub tools_skipped: usize,
    /// Source keys that were not used, by dotted key, with their count.
    pub dropped_keys: BTreeMap<String, usize>,
}

/// A converted library.
#[derive(Clone, PartialEq, Debug)]
pub struct ImportResult {
    /// The library file text (`fluut.tools/1`).
    pub library: String,
    /// Warnings; an import with an error returns `Err` instead.
    pub diagnostics: Vec<Diagnostic>,
    pub report: ImportReport,
}

/// A library id that `DefRef` accepts and that names no version.
pub fn valid_library_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id.split('/').all(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        })
}

/// A lower-case id piece from free text: ASCII letters and digits, other
/// runs become one `-`; at most `max` characters; `fallback` when empty.
pub fn slug(text: &str, max: usize, fallback: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= max {
            break;
        }
    }
    let out = out.trim_matches('-').to_owned();
    if out.is_empty() {
        fallback.to_owned()
    } else {
        out
    }
}
