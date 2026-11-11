// SPDX-License-Identifier: MIT OR Apache-2.0

//! The typed post: a resolved post table read into structs with spec 05
//! defaults (§5.3, §5.5), plus validation (§5.2.5).

use fluut_model::Resolved;
use serde::Deserialize;
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

#[derive(Clone, PartialEq, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Post {
    pub name: String,
    /// `"imported"` until reviewed (spec 05 §5.9).
    pub origin: Option<String>,
    pub output: Output,
    pub line_numbering: LineNumbering,
    pub words: BTreeMap<String, WordFmt>,
    pub blocks: BTreeMap<String, Block>,
    pub arcs: Arcs,
    pub helix: Helix,
    pub rotary: Rotary,
    pub spline: Spline,
    pub cycles: Cycles,
    pub feed: Feed,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    #[default]
    Mm,
    Inch,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEnding {
    #[default]
    Lf,
    Crlf,
    Cr,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }
}

#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(default)]
pub struct Output {
    pub extension: String,
    pub units: Units,
    pub line_ending: LineEnding,
    pub decimal_char: String,
    /// Text replacements applied to every output line.
    pub substitute: BTreeMap<String, String>,
    /// Text replacements applied only to user text put into a line: tool,
    /// toolpath, Setup and machine names and the file notes (Vectric
    /// `SUBSTITUTE`, which keeps such names from breaking a comment).
    pub substitute_values: BTreeMap<String, String>,
    /// Bytes per file before splitting; 0 = no split.
    pub max_file_size: u64,
    /// Longest line move before splitting, mm; 0 = no limit.
    pub max_segment_length: f64,
    pub comment_start: String,
    pub comment_end: String,
}

impl Default for Output {
    fn default() -> Output {
        Output {
            extension: "nc".into(),
            units: Units::Mm,
            line_ending: LineEnding::Lf,
            decimal_char: ".".into(),
            substitute: BTreeMap::new(),
            substitute_values: BTreeMap::new(),
            max_file_size: 0,
            max_segment_length: 0.0,
            comment_start: "(".into(),
            comment_end: ")".into(),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(default)]
pub struct LineNumbering {
    pub enabled: bool,
    pub start: u64,
    pub increment: u64,
    /// After this, numbering restarts at `start`.
    pub maximum: u64,
    pub prefix: String,
}

impl Default for LineNumbering {
    fn default() -> LineNumbering {
        LineNumbering {
            enabled: false,
            start: 10,
            increment: 10,
            maximum: 99_999,
            prefix: "N".into(),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum When {
    #[default]
    Always,
    Changed,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sign {
    /// Only negative values carry a sign.
    #[default]
    Negative,
    Always,
}

/// Word format (spec 05 §5.3).
#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(default)]
pub struct WordFmt {
    /// The `[NAME]` the word answers to; defaults to the table key.
    pub placeholder: Option<String>,
    pub when: When,
    pub prefix: String,
    pub decimals: u32,
    pub scale: f64,
    pub sign: Sign,
    pub trailing_zeros: bool,
    pub leading_zero: bool,
    pub pad_width: u32,
    /// Emitted only when its modal group changes (spec 05 §5.3.1).
    pub modal: bool,
}

impl Default for WordFmt {
    fn default() -> WordFmt {
        WordFmt {
            placeholder: None,
            when: When::Always,
            prefix: String::new(),
            decimals: 3,
            scale: 1.0,
            sign: Sign::Negative,
            trailing_zeros: false,
            leading_zero: true,
            pad_width: 0,
            modal: false,
        }
    }
}

#[derive(Clone, PartialEq, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Block {
    pub lines: Vec<Line>,
}

/// A merged block line (see `merge`): every line is a table there.
#[derive(Clone, PartialEq, Debug, Default, Deserialize)]
pub struct Line {
    pub id: Option<String>,
    /// Condition expression (spec 05 §5.4.3).
    pub when: Option<String>,
    pub line: String,
    /// Layer that supplied or last changed the line.
    pub layer: Option<String>,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Deserialize)]
pub enum ArcPlane {
    XY,
    XZ,
    YZ,
    #[serde(rename = "ANY")]
    Any,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CentreMode {
    #[default]
    IjkIncremental,
    IjkAbsolute,
    R,
}

#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(default)]
pub struct Arcs {
    pub enabled: bool,
    pub planes: Vec<ArcPlane>,
    pub centre_mode: CentreMode,
    pub min_radius: f64,
    pub max_radius: f64,
    pub max_sweep_deg: f64,
    /// The controller's start/end radius check (spec 05 §5.8).
    pub radius_tolerance: f64,
}

impl Default for Arcs {
    fn default() -> Arcs {
        Arcs {
            enabled: true,
            planes: vec![ArcPlane::XY, ArcPlane::XZ, ArcPlane::YZ],
            centre_mode: CentreMode::IjkIncremental,
            min_radius: 0.0,
            max_radius: 1e5,
            max_sweep_deg: 360.0,
            radius_tolerance: 0.002,
        }
    }
}

#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(default)]
pub struct Helix {
    pub enabled: bool,
    pub planes: Vec<ArcPlane>,
}

impl Default for Helix {
    fn default() -> Helix {
        Helix {
            enabled: false,
            planes: vec![ArcPlane::XY],
        }
    }
}

#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(default)]
pub struct Rotary {
    pub enabled: bool,
    pub max_angle_per_line: f64,
    /// X/Z interpolated in the same line as A.
    pub linear_with_rotary: bool,
}

impl Default for Rotary {
    fn default() -> Rotary {
        Rotary {
            enabled: false,
            max_angle_per_line: 90.0,
            linear_with_rotary: true,
        }
    }
}

#[derive(Clone, PartialEq, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Spline {
    pub enabled: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CycleName {
    Drill,
    Spot,
    Peck,
    ChipBreak,
    Tap,
    Ream,
    Bore,
}

#[derive(Clone, PartialEq, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Cycles {
    pub enabled: bool,
    pub supported: Vec<CycleName>,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotaryMethod {
    #[default]
    Degrees,
    InverseTime,
    LinearOnly,
}

#[derive(Clone, PartialEq, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Feed {
    pub rotary_method: RotaryMethod,
}

/// Block names (spec 05 §5.4.1).
pub const EVENTS: [&str; 33] = [
    "header",
    "footer",
    "tool_change",
    "setup_start",
    "rotary_index",
    "rotary_clamp",
    "rotary_unclamp",
    "setup_end",
    "toolpath_start",
    "toolpath_end",
    "spindle_on",
    "spindle_off",
    "coolant_on",
    "coolant_off",
    "rapid",
    "feed",
    "plunge",
    "retract",
    "arc_cw",
    "arc_ccw",
    "helix_cw",
    "helix_ccw",
    "rotary",
    "spline",
    "feed_change",
    "dwell",
    "cutter_comp_left",
    "cutter_comp_right",
    "cutter_comp_off",
    "cycle_start",
    "cycle_point",
    "cycle_end",
    "manual_nc",
];

/// Events that may also have a `first_*` block (spec 05 §5.4.1).
pub const FIRST_EVENTS: [&str; 9] = [
    "rapid",
    "feed",
    "plunge",
    "retract",
    "arc_cw",
    "arc_ccw",
    "helix_cw",
    "helix_ccw",
    "rotary",
];

/// `v > 0`, false for NaN.
fn positive(v: f64) -> bool {
    v > 0.0
}

fn known_block(name: &str) -> bool {
    name == "comment"
        || EVENTS.contains(&name)
        || name
            .strip_prefix("first_")
            .is_some_and(|e| FIRST_EVENTS.contains(&e))
}

#[derive(Clone, PartialEq, Debug, thiserror::Error)]
pub enum PostError {
    #[error("post: {0}")]
    Read(String),
    #[error("post is invalid:\n  {}", .0.join("\n  "))]
    Invalid(Vec<String>),
}

/// A typed, validated post with the warnings found while reading it.
#[derive(Clone, PartialEq, Debug)]
pub struct LoadedPost {
    pub post: Post,
    pub warnings: Vec<String>,
}

impl Post {
    /// Reads and validates a resolved post. Unknown keys are warnings (spec
    /// 02 §2.0); validation failures are errors (spec 05 §5.2.5).
    pub fn load(resolved: &Resolved) -> Result<LoadedPost, PostError> {
        let mut warnings = Vec::new();
        let post: Post =
            serde_ignored::deserialize(toml::Value::Table(resolved.table.clone()), |path| {
                warnings.push(format!("unknown key `{path}`"))
            })
            .map_err(|e| PostError::Read(e.to_string()))?;
        let errors = post.validate();
        if errors.is_empty() {
            Ok(LoadedPost { post, warnings })
        } else {
            Err(PostError::Invalid(errors))
        }
    }

    /// Checks limits and required blocks; placeholders are checked when
    /// templates compile.
    pub fn validate(&self) -> Vec<String> {
        let mut e = Vec::new();
        for name in self.blocks.keys() {
            if !known_block(name) {
                e.push(format!("unknown block `{name}`"));
            }
        }
        let mut need = |cond: bool, blocks: &[&str], why: &str| {
            if cond {
                for b in blocks {
                    if !self.blocks.contains_key(*b) {
                        e.push(format!("block `{b}` is required ({why})"));
                    }
                }
            }
        };
        need(true, &["rapid", "feed"], "every post");
        need(self.arcs.enabled, &["arc_cw", "arc_ccw"], "arcs.enabled");
        need(
            self.helix.enabled,
            &["helix_cw", "helix_ccw"],
            "helix.enabled",
        );
        need(self.rotary.enabled, &["rotary"], "rotary.enabled");
        need(
            self.cycles.enabled,
            &["cycle_start", "cycle_point", "cycle_end"],
            "cycles.enabled",
        );
        let a = &self.arcs;
        if a.centre_mode == CentreMode::R && a.max_sweep_deg > 180.0 {
            e.push("arcs.centre_mode = \"r\" needs arcs.max_sweep_deg <= 180".into());
        }
        if !(a.max_sweep_deg > 0.0 && a.max_sweep_deg <= 360.0) {
            e.push("arcs.max_sweep_deg must be in (0, 360]".into());
        }
        if !(a.min_radius >= 0.0 && a.min_radius <= a.max_radius) {
            e.push("arcs.min_radius must be >= 0 and <= arcs.max_radius".into());
        }
        if !positive(a.radius_tolerance) {
            e.push("arcs.radius_tolerance must be > 0".into());
        }
        if !positive(self.rotary.max_angle_per_line) {
            e.push("rotary.max_angle_per_line must be > 0".into());
        }
        if self.output.max_segment_length < 0.0 {
            e.push("output.max_segment_length must be >= 0".into());
        }
        if self.output.decimal_char.chars().count() != 1 {
            e.push("output.decimal_char must be one character".into());
        }
        if self.line_numbering.increment == 0 {
            e.push("line_numbering.increment must be > 0".into());
        }
        for (k, w) in &self.words {
            if w.decimals > 9 {
                e.push(format!("words.{k}.decimals must be <= 9"));
            }
            if !(w.scale.is_finite() && w.scale != 0.0) {
                e.push(format!("words.{k}.scale must be finite and non-zero"));
            }
        }
        e
    }
}
