// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `craft-exchange/1` contract as FluutCAM reads it: every part name,
//! field name, capability and version rule, and the face-name syntax.
//!
//! The contract is SolidCraft RFC 0001, still a draft. Where the draft
//! leaves something open, the choice made here is marked **Reading**; a
//! change to the RFC should touch this module only.
//!
//! Manifest (`manifest.json`), as read:
//!
//! ```json
//! { "format": "craft-exchange/1",
//!   "source": { "app": "SolidCraft", "version": "0.12.0" },
//!   "source_id": "d9c1…", "revision": 7, "hash": "sha256:…", "units": "mm",
//!   "parts": [ { "path": "bodies/0.step", "sha256": "…" }, … ],
//!   "requires": ["step/ap214", "faces.names/1"] }
//! ```
//!
//! Face table (`faces.json`), as read:
//!
//! ```json
//! { "bodies": [ { "body": 0,
//!     "faces": [ { "index": 0, "name": "F2:top", "exact": true,
//!                  "surface": "plane", "aliases": [], "point": [x, y, z] } ],
//!     "edges": [ { "index": 3, "name": "F2:top|F4:side:l4", "exact": true } ] } ] }
//! ```

use serde_json::Value;

/// The format id before the `/`.
pub const FORMAT: &str = "craft-exchange";
/// The major version this reader understands. Any minor is accepted
/// (RFC 0001 §6).
pub const MAJOR: u32 = 1;

/// Part paths.
pub const MANIFEST: &str = "manifest.json";
pub const FACES: &str = "faces.json";
pub const PARAMS: &str = "params.json";
pub const META: &str = "meta.json";
pub const BOARD: &str = "board.json";
pub const BODIES: &str = "bodies/";
pub const MESH: &str = "mesh/";

/// Manifest keys.
pub mod key {
    pub const FORMAT: &str = "format";
    pub const SOURCE: &str = "source";
    pub const APP: &str = "app";
    pub const VERSION: &str = "version";
    pub const SOURCE_ID: &str = "source_id";
    pub const REVISION: &str = "revision";
    pub const HASH: &str = "hash";
    pub const UNITS: &str = "units";
    pub const PARTS: &str = "parts";
    pub const PATH: &str = "path";
    pub const SHA256: &str = "sha256";
    pub const REQUIRES: &str = "requires";
    // Face table.
    pub const BODIES: &str = "bodies";
    pub const BODY: &str = "body";
    pub const FACES: &str = "faces";
    pub const EDGES: &str = "edges";
    pub const INDEX: &str = "index";
    pub const NAME: &str = "name";
    pub const EXACT: &str = "exact";
    pub const SURFACE: &str = "surface";
    pub const ALIASES: &str = "aliases";
    pub const POINT: &str = "point";
    // Parameters and metadata.
    pub const PARAMS: &str = "params";
    pub const VALUE: &str = "value";
    pub const UNIT: &str = "unit";
    pub const EXPRESSION: &str = "expression";
    pub const MATERIAL: &str = "material";
    pub const PART_NUMBER: &str = "part_number";
}

/// What a part holds, by its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    Manifest,
    /// `bodies/<n>.step`: body `n` as STEP.
    Step(u32),
    /// `bodies/<n>.record`: body `n` as a solidcore record (not read).
    Record(u32),
    /// `mesh/<n>.bin` (not read).
    Mesh(u32),
    Faces,
    Params,
    Meta,
    /// `board.json` (PCB data, not read).
    Board,
    Unknown,
}

fn numbered(rest: &str, ext: &str) -> Option<u32> {
    let n = rest.strip_suffix(ext)?;
    (!n.is_empty() && n.len() <= 6 && n.bytes().all(|b| b.is_ascii_digit()))
        .then(|| n.parse().ok())
        .flatten()
}

impl PartKind {
    pub fn of(path: &str) -> PartKind {
        match path {
            MANIFEST => return PartKind::Manifest,
            FACES => return PartKind::Faces,
            PARAMS => return PartKind::Params,
            META => return PartKind::Meta,
            BOARD => return PartKind::Board,
            _ => {}
        }
        if let Some(r) = path.strip_prefix(BODIES) {
            if let Some(n) = numbered(r, ".step") {
                return PartKind::Step(n);
            }
            if let Some(n) = numbered(r, ".record") {
                return PartKind::Record(n);
            }
        }
        if let Some(n) = path.strip_prefix(MESH).and_then(|r| numbered(r, ".bin")) {
            return PartKind::Mesh(n);
        }
        PartKind::Unknown
    }

    /// Parts FluutCAM reads.
    pub fn is_read(self) -> bool {
        matches!(
            self,
            PartKind::Step(_) | PartKind::Faces | PartKind::Params | PartKind::Meta
        )
    }
}

/// `craft-exchange/<major>[.<minor>]` → major, or `None` when the string
/// is not this format.
pub fn format_major(s: &str) -> Option<u32> {
    let ver = s.strip_prefix(FORMAT)?.strip_prefix('/')?;
    let major = ver.split_once('.').map_or(ver, |(a, _)| a);
    if major.is_empty() || !major.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    major.parse().ok()
}

/// Capabilities this reader supports, as `(name, highest major)`. Any
/// `step/<schema>` is read (the STEP reader checks the schema).
const CAPABILITIES: [(&str, u32); 3] = [("faces.names", 1), ("params", 1), ("meta", 1)];

/// Whether a capability string such as `faces.names/1` or `step/ap214` is
/// one this reader can use.
pub fn supports(cap: &str) -> bool {
    let Some((name, ver)) = cap.split_once('/') else {
        return false;
    };
    if name == "step" {
        return !ver.is_empty();
    }
    let major = ver.split_once('.').map_or(ver, |(a, _)| a);
    let Ok(major) = major.parse::<u32>() else {
        return false;
    };
    CAPABILITIES
        .iter()
        .any(|(n, m)| *n == name && (1..=*m).contains(&major))
}

/// The hex digest of a manifest `sha256` value: bare hex or `sha256:`
/// then hex, 64 digits, compared in lower case.
pub fn sha256_hex(v: &str) -> Option<String> {
    let h = v.strip_prefix("sha256:").unwrap_or(v);
    (h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).then(|| h.to_ascii_lowercase())
}

/// A revision: **Reading** the draft gives no type, so a number or a
/// string, kept as text.
pub fn revision(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// How far a face-table point may lie from its face, in mm, besides the
/// body's modelling tolerance. **Reading**: the RFC gives points as JSON
/// numbers, whose printed digits limit their precision.
pub const POINT_TOL: f64 = 1e-4;

/// Surface type names of the face table that map to a FluutCAM surface
/// kind, for the check that a face is the one named. **Reading**: the RFC
/// lists "surface type" without values; unknown values are not checked.
pub fn surface_kind(s: &str) -> Option<SurfaceKind> {
    Some(match s.to_ascii_lowercase().as_str() {
        "plane" | "planar" => SurfaceKind::Plane,
        "cylinder" | "cylindrical" => SurfaceKind::Cylinder,
        "cone" | "conical" => SurfaceKind::Cone,
        "sphere" | "spherical" => SurfaceKind::Sphere,
        "torus" | "toroidal" => SurfaceKind::Torus,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
}

// ── Face names ─────────────────────────────────────────────────────────
//
// Names are opaque to FluutCAM, except for the two pieces of syntax the
// RFC gives: a piece of a split face is `<name>#[…]`, and an edge is
// `<face A>|<face B>`, with `~k` telling apart several edges between the
// same two faces. Names nest (`F5:blend(F2:top|F4:side:l4)`), so only
// separators outside brackets count.

/// Byte offsets of `c` outside `()` and `[]` in `s`.
fn top_level(s: &str, c: u8) -> Vec<usize> {
    let mut depth = 0i32;
    let mut out = Vec::new();
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            _ if b == c && depth == 0 => out.push(i),
            _ => {}
        }
    }
    out
}

/// For a piece of a split face (`F2:top#[F2:side:l4,F4:side:l4]`), the
/// name of the face it was cut from.
pub fn piece_of(name: &str) -> Option<&str> {
    let i = *top_level(name, b'#').last()?;
    let (base, q) = name.split_at(i);
    (q.starts_with("#[") && q.ends_with(']') && !base.is_empty()).then_some(base)
}

/// The two face names of an edge name `<face A>|<face B>[~k]`.
pub fn edge_faces(name: &str) -> Option<(&str, &str)> {
    let bars = top_level(name, b'|');
    let [i] = bars[..] else {
        return None;
    };
    let (a, b) = (&name[..i], &name[i + 1..]);
    let b = match top_level(b, b'~').last() {
        Some(&k) if b[k + 1..].bytes().all(|c| c.is_ascii_digit()) && k + 1 < b.len() => &b[..k],
        _ => b,
    };
    (!a.is_empty() && !b.is_empty()).then_some((a, b))
}

/// The name of the edge between faces `a` and `b`.
pub fn edge_name(a: &str, b: &str) -> String {
    format!("{a}|{b}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(format_major("craft-exchange/1"), Some(1));
        assert_eq!(format_major("craft-exchange/1.4"), Some(1));
        assert_eq!(format_major("craft-exchange/2"), Some(2));
        assert_eq!(format_major("craft-exchange/x"), None);
        assert_eq!(format_major("other/1"), None);
        assert!(supports("faces.names/1"));
        assert!(supports("faces.names/1.2"));
        assert!(!supports("faces.names/2"));
        assert!(supports("step/ap214"));
        assert!(supports("params/1"));
        assert!(!supports("brep.solidcore-record/1"));
        assert!(!supports("mesh/1"));
        assert!(!supports("faces.names"));
    }

    #[test]
    fn parts() {
        assert_eq!(PartKind::of("bodies/0.step"), PartKind::Step(0));
        assert_eq!(PartKind::of("bodies/12.record"), PartKind::Record(12));
        assert_eq!(PartKind::of("mesh/3.bin"), PartKind::Mesh(3));
        assert_eq!(PartKind::of("faces.json"), PartKind::Faces);
        assert_eq!(PartKind::of("bodies/a.step"), PartKind::Unknown);
        assert_eq!(PartKind::of("bodies/.step"), PartKind::Unknown);
        assert_eq!(PartKind::of("notes.txt"), PartKind::Unknown);
    }

    #[test]
    fn names() {
        assert_eq!(piece_of("F2:top#[F2:side:l4,F4:side:l4]"), Some("F2:top"));
        assert_eq!(piece_of("F2:top"), None);
        assert_eq!(piece_of("F5:blend(F2:top#[a]|F4:x)"), None);
        assert_eq!(
            edge_faces("F2:top|F4:side:l4"),
            Some(("F2:top", "F4:side:l4"))
        );
        assert_eq!(
            edge_faces("F2:top|F4:side:l4~2"),
            Some(("F2:top", "F4:side:l4"))
        );
        assert_eq!(
            edge_faces("F5:blend(F2:top|F4:side)|F2:top"),
            Some(("F5:blend(F2:top|F4:side)", "F2:top"))
        );
        assert_eq!(edge_faces("F2:top"), None);
        assert_eq!(edge_faces("a|b|c"), None);
        assert_eq!(edge_name("a", "b"), "a|b");
        assert_eq!(sha256_hex(&"A".repeat(64)), Some("a".repeat(64)));
        assert_eq!(sha256_hex("sha256:00"), None);
    }
}
