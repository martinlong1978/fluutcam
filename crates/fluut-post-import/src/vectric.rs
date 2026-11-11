// SPDX-License-Identifier: MIT OR Apache-2.0

//! Vectric `.pp` post processors (design D10 §6.1, spec 05 §5.10.1).
//!
//! A `.pp` file is declarative text: `KEY = value` settings, `VAR` word
//! definitions, and `begin BLOCK` sections of quoted template lines. This
//! module reads it, maps it onto a standalone `fluut.post/1` post and checks
//! that the result loads and compiles. Nothing in the file is run: a file
//! with a script section is refused.
//!
//! Every setting, word and block is either mapped, refused with an error,
//! or dropped with a warning. The mapping, the value-format reading (CH-21)
//! and the codes are listed in D10 §6.1.

use crate::toml_out::string;
use crate::{limits, slug};
use fluut_model::{DefRef, Diagnostic, MemStore};
use fluut_post::{CompiledPost, Post, resolve_post};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

#[cfg(test)]
mod tests;

/// What the user chose for the new post.
#[derive(Clone, PartialEq, Debug)]
pub struct Options {
    /// Post id, such as `vectric/mach3-mm`.
    pub post_id: String,
    /// Display name; the file's `POST_NAME` when `None`.
    pub name: Option<String>,
}

impl Options {
    /// Options with the default id for a file stem: `vectric/<slug>`.
    pub fn for_file_stem(stem: &str) -> Options {
        Options {
            post_id: format!("vectric/{}", slug(stem, 40, "post")),
            name: None,
        }
    }
}

/// Counts for the import report (spec 05 §5.10).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Report {
    /// `KEY = value` settings read (the base file's included).
    pub settings_read: usize,
    /// Settings mapped onto the post.
    pub settings_mapped: usize,
    /// `VAR` definitions read.
    pub vars_read: usize,
    /// Words written.
    pub words_written: usize,
    /// `begin` blocks read.
    pub blocks_read: usize,
    /// Vectric blocks mapped onto a post block.
    pub blocks_mapped: usize,
    /// Template lines read.
    pub lines_read: usize,
    /// Template lines written (a block mapped twice counts twice).
    pub lines_written: usize,
    /// Base files merged in through `POST_BASE`.
    pub base_files: usize,
}

/// A converted post.
#[derive(Clone, PartialEq, Debug)]
pub struct Converted {
    /// The post file text (`fluut.post/1`).
    pub post: String,
    /// The post's display name.
    pub name: String,
    /// Warnings; an import with an error returns `Err` instead.
    pub diagnostics: Vec<Diagnostic>,
    pub report: Report,
}

/// Converts a `.pp` file. A file that names a `POST_BASE` is refused; use
/// [`import_with_base`] to supply base files.
pub fn import(bytes: &[u8], opts: &Options) -> Result<Converted, Vec<Diagnostic>> {
    import_with_base(bytes, opts, &mut |_| None)
}

/// Converts a `.pp` file; `base` returns the bytes of a `POST_BASE` file by
/// the name the file gives (a plain file name), or `None` when it is not
/// found.
pub fn import_with_base(
    bytes: &[u8],
    opts: &Options,
    base: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) -> Result<Converted, Vec<Diagnostic>> {
    let mut diags = Vec::new();
    let mut src = read(bytes, &mut diags)?;
    let mut seen = BTreeSet::new();
    let mut base_files = 0;
    while let Some(name) = src.setting("POST_BASE").map(|s| unquote(s).to_owned()) {
        let key = name.to_ascii_lowercase();
        if name.is_empty()
            || name.contains(['/', '\\', ':'])
            || name.starts_with('.')
            || !seen.insert(key)
        {
            return Err(vec![err(
                "import.pp.post_base",
                format!(
                    "`POST_BASE` names `{name}`, which is not a usable base file (a plain file name, no cycle)"
                ),
            )]);
        }
        if seen.len() > limits::MAX_POST_BASE_DEPTH {
            return Err(vec![err(
                "import.pp.post_base",
                format!(
                    "more than {} `POST_BASE` levels",
                    limits::MAX_POST_BASE_DEPTH
                ),
            )]);
        }
        let Some(b) = base(&name) else {
            return Err(vec![err(
                "import.pp.post_base",
                format!(
                    "the base post `{name}` was not found; put it beside the file and import again"
                ),
            )]);
        };
        let parent = read(&b, &mut diags)?;
        src = parent.overlay(src);
        base_files += 1;
    }
    let mut c = Conv {
        diags,
        report: Report {
            base_files,
            ..Report::default()
        },
        errors: Vec::new(),
    };
    let out = c.convert(&src, opts);
    if !c.errors.is_empty() {
        return Err(c.errors);
    }
    let (post, name) = out.expect("no errors means a post");
    check_loads(&post, &opts.post_id, &mut c.diags)?;
    Ok(Converted {
        post,
        name,
        diagnostics: c.diags,
        report: c.report,
    })
}

fn err(code: &str, msg: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, msg)
}

fn warn(code: &str, msg: impl Into<String>) -> Diagnostic {
    Diagnostic::warning(code, msg)
}

/// The converted post must load and compile; anything else is a bug here.
fn check_loads(post: &str, id: &str, diags: &mut Vec<Diagnostic>) -> Result<(), Vec<Diagnostic>> {
    let internal = |m: String| {
        vec![err(
            "import.pp.internal",
            format!("the converted post does not load ({m}); please report this"),
        )]
    };
    let mut s = MemStore::new();
    s.insert_toml(post).map_err(|e| internal(e.to_string()))?;
    let r = resolve_post(
        &s,
        &DefRef::parse(id).map_err(|e| internal(e.to_string()))?,
        None,
    )
    .map_err(|e| internal(e.to_string()))?;
    let loaded = Post::load(&r).map_err(|e| internal(e.to_string()))?;
    for w in &loaded.warnings {
        diags.push(warn("import.pp.internal", w.clone()));
    }
    CompiledPost::compile(loaded).map_err(|e| {
        internal(
            e.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    Ok(())
}

// ---------------------------------------------------------------- reading

/// One `VAR NAME = [VO|WO|CS|VF|MX]` line.
#[derive(Clone, PartialEq, Debug)]
struct Var {
    vo: String,
    wo: String,
    cs: String,
    vf: String,
    mx: Option<String>,
    line: usize,
}

/// One `.pp` file as read, before mapping.
#[derive(Clone, PartialEq, Debug, Default)]
struct Source {
    /// `(KEY in upper case, raw value)`; a later key replaces an earlier.
    settings: Vec<(String, String)>,
    vars: Vec<Var>,
    /// `(NAME in upper case, raw quoted lines)`.
    blocks: Vec<(String, Vec<String>)>,
    /// The comment lines before the first setting, without the marker.
    head_comment: Vec<String>,
    /// Line numbers of lines that are none of the above.
    unknown_lines: Vec<usize>,
    /// Quoted lines outside a block.
    stray_lines: usize,
    /// Text after a closing quote or `]` that is not a comment.
    trailing: usize,
    /// Unquoted `%` lines.
    bare_percent: usize,
    /// Blocks named twice in one file.
    duplicate_blocks: BTreeSet<String>,
    /// Settings named twice in one file.
    duplicate_settings: BTreeSet<String>,
}

impl Source {
    fn setting(&self, key: &str) -> Option<&str> {
        self.settings
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn block(&self, name: &str) -> Option<&[String]> {
        self.blocks
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, l)| l.as_slice())
    }

    /// `child` laid over `self` (Vectric `POST_BASE`): settings, words (by
    /// output name) and blocks of the child replace the base's.
    fn overlay(mut self, child: Source) -> Source {
        // The base's own `POST_BASE` stays, for the next level.
        for (k, v) in child.settings {
            if k == "POST_BASE" {
                continue;
            }
            self.settings.retain(|(b, _)| *b != k);
            self.settings.push((k, v));
        }
        for v in child.vars {
            self.vars.retain(|b| b.vo != v.vo);
            self.vars.push(v);
        }
        for (n, l) in child.blocks {
            match self.blocks.iter_mut().find(|(b, _)| *b == n) {
                Some(b) => b.1 = l,
                None => self.blocks.push((n, l)),
            }
        }
        if !child.head_comment.is_empty() {
            self.head_comment = child.head_comment;
        }
        self.unknown_lines.extend(child.unknown_lines);
        self.stray_lines += child.stray_lines;
        self.trailing += child.trailing;
        self.bare_percent += child.bare_percent;
        self.duplicate_blocks.extend(child.duplicate_blocks);
        self.duplicate_settings.extend(child.duplicate_settings);
        self
    }
}

/// Decodes the file: UTF-8 (with or without a byte-order mark), else
/// Windows-1252 read as Latin-1, with a warning.
fn decode(bytes: &[u8], diags: &mut Vec<Diagnostic>) -> Result<String, Vec<Diagnostic>> {
    if bytes.len() as u64 > limits::MAX_POST_BYTES {
        return Err(vec![err(
            "import.limit.size",
            format!(
                "the file is {} bytes; the limit for a post is {}",
                bytes.len(),
                limits::MAX_POST_BYTES
            ),
        )]);
    }
    if bytes.contains(&0) {
        return Err(vec![err(
            "import.file.invalid",
            "the file holds NUL bytes; it is not a text post processor",
        )]);
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    Ok(match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => {
            diags.push(warn(
                "import.file.encoding",
                "the file is not UTF-8; read as Latin-1 (accented letters in names may differ)",
            ));
            bytes.iter().map(|&b| b as char).collect()
        }
    })
}

/// `"text"` → `text`; a value without quotes is trimmed.
fn unquote(v: &str) -> &str {
    let v = v.trim();
    match v.strip_prefix('"') {
        Some(r) => r.split('"').next().unwrap_or(""),
        None => v,
    }
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn read(bytes: &[u8], diags: &mut Vec<Diagnostic>) -> Result<Source, Vec<Diagnostic>> {
    let text = decode(bytes, diags)?;
    let mut src = Source::default();
    let mut block: Option<usize> = None;
    let mut seen_blocks = BTreeSet::new();
    let mut seen_settings = BTreeSet::new();
    let mut in_head = true;
    let mut n_lines = 0usize;
    for (i, raw) in text.lines().enumerate() {
        n_lines += 1;
        if n_lines > limits::MAX_POST_LINES {
            return Err(vec![err(
                "import.limit.lines",
                format!("the file has more than {} lines", limits::MAX_POST_LINES),
            )]);
        }
        let line = raw.trim();
        let no = i + 1;
        if line.is_empty() {
            continue;
        }
        if let Some(c) = line.strip_prefix('+').or_else(|| line.strip_prefix('|')) {
            if in_head && src.head_comment.len() < 100 {
                src.head_comment.push(c.trim_end().to_owned());
            }
            continue;
        }
        in_head = false;
        if let Some(rest) = line.strip_prefix('"') {
            let Some(end) = rest.find('"') else {
                src.unknown_lines.push(no);
                continue;
            };
            let after = rest[end + 1..].trim();
            if !after.is_empty() && !after.starts_with('+') {
                src.trailing += 1;
            }
            match block {
                Some(b) => {
                    if src.blocks[b].1.len() >= limits::MAX_BLOCK_LINES {
                        return Err(vec![err(
                            "import.limit.lines",
                            format!(
                                "block {} has more than {} lines",
                                src.blocks[b].0,
                                limits::MAX_BLOCK_LINES
                            ),
                        )]);
                    }
                    src.blocks[b].1.push(rest[..end].to_owned());
                }
                None => src.stray_lines += 1,
            }
            continue;
        }
        let mut words = line.split_whitespace();
        let first = words.next().unwrap_or("");
        let first_up = first.to_ascii_uppercase();
        if first_up == "SCRIPT" || first_up == "ENDSCRIPT" {
            return Err(vec![err(
                "import.pp.script",
                format!(
                    "line {no} starts a script section; FluutCAM imports declarative posts only and runs no code"
                ),
            )]);
        }
        if first_up == "BEGIN" {
            let name = words.next().unwrap_or("").to_ascii_uppercase();
            if !is_name(&name) {
                src.unknown_lines.push(no);
                block = None;
                continue;
            }
            if !seen_blocks.insert(name.clone()) {
                src.duplicate_blocks.insert(name.clone());
                src.blocks.retain(|(n, _)| *n != name);
            }
            src.blocks.push((name, Vec::new()));
            block = Some(src.blocks.len() - 1);
            continue;
        }
        if first_up == "VAR" || first_up.starts_with("VAR\t") {
            block = None;
            match read_var(&line[3..], no) {
                Some((v, trailing)) => {
                    if src.vars.len() >= limits::MAX_POST_VARS {
                        return Err(vec![err(
                            "import.limit.vars",
                            format!("more than {} `VAR` lines", limits::MAX_POST_VARS),
                        )]);
                    }
                    src.trailing += usize::from(trailing);
                    src.vars.push(v);
                }
                None => src.unknown_lines.push(no),
            }
            continue;
        }
        if let Some((k, v)) = line.split_once('=')
            && is_name(k.trim())
        {
            block = None;
            let k = k.trim().to_ascii_uppercase();
            if !seen_settings.insert(k.clone()) {
                src.duplicate_settings.insert(k.clone());
            }
            src.settings.retain(|(b, _)| *b != k);
            src.settings.push((k, v.trim().to_owned()));
            continue;
        }
        if line == "%" {
            src.bare_percent += 1;
        } else {
            src.unknown_lines.push(no);
        }
    }
    Ok(src)
}

/// `NAME = [VO|WO|CS|VF]` or `…|MX]`, after the `VAR` keyword. The flag
/// says text that is not a comment followed the `]`.
fn read_var(rest: &str, line: usize) -> Option<(Var, bool)> {
    let (_, def) = rest.split_once('=')?;
    let def = def.trim().strip_prefix('[')?;
    let close = def.rfind(']')?;
    let after = def[close + 1..].trim();
    let f: Vec<&str> = def[..close].split('|').collect();
    if !(4..=5).contains(&f.len()) {
        return None;
    }
    Some((
        Var {
            vo: f[0].trim().to_owned(),
            wo: f[1].trim().to_owned(),
            cs: f[2].to_owned(),
            vf: f[3].trim().to_owned(),
            mx: f.get(4).map(|m| m.trim().to_owned()),
            line,
        },
        !after.is_empty() && !after.starts_with('+'),
    ))
}

// ------------------------------------------------------- value formats

/// A Vectric value format (`VF`), read as a C `printf` conversion
/// `%[flags][width].[decimals]f` with the decimal separator spelled out
/// (CH-21, decided 2026-11-11):
///
/// - `+` first: every value carries a sign (`sign = "always"`).
/// - A width that starts with `0` (and has more digits): pad with zeros.
/// - Width: the least number of characters of the whole number (sign,
///   digits, separator, decimals). A width no larger than the shortest
///   possible number has no effect, which is the common case (`1.3`).
/// - `.` or `,`: the decimal separator.
/// - Decimals: digits after the separator, always written (trailing zeros
///   kept, a leading `0` before the separator).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ValueFormat {
    pub plus: bool,
    pub zero_pad: bool,
    pub width: u32,
    pub separator: char,
    pub decimals: u32,
}

impl ValueFormat {
    /// Reads a `VF` field such as `1.3`, `+1,4`, `04.0` or `.1`.
    pub fn parse(s: &str) -> Result<ValueFormat, String> {
        let (plus, rest) = match s.strip_prefix('+') {
            Some(r) => (true, r),
            None => (false, s),
        };
        let sep_at = rest
            .find(['.', ','])
            .ok_or_else(|| format!("`{s}` has no decimal separator (`.` or `,`)"))?;
        let (w, d) = (&rest[..sep_at], &rest[sep_at + 1..]);
        let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
        if !digits(w) || d.is_empty() || !digits(d) {
            return Err(format!("`{s}` is not [+][width](.|,)decimals"));
        }
        let width: u32 = if w.is_empty() {
            0
        } else {
            w.parse().map_err(|_| format!("`{s}`: width too large"))?
        };
        let decimals: u32 = d.parse().map_err(|_| format!("`{s}`: bad decimals"))?;
        if decimals > 9 || width > 30 {
            return Err(format!("`{s}`: more than 9 decimals or 30 characters"));
        }
        Ok(ValueFormat {
            plus,
            zero_pad: w.len() > 1 && w.starts_with('0'),
            width,
            separator: if rest.as_bytes()[sep_at] == b',' {
                ','
            } else {
                '.'
            },
            decimals,
        })
    }

    /// Characters of a value's non-integer part: separator and decimals,
    /// and the `+` sign.
    fn fixed_chars(self) -> u32 {
        (if self.decimals > 0 {
            self.decimals + 1
        } else {
            0
        }) + u32::from(self.plus)
    }

    /// Zero padding as integer digits (`pad_width`), or 0.
    pub fn pad_width(self) -> u32 {
        if self.zero_pad {
            self.width.saturating_sub(self.fixed_chars())
        } else {
            0
        }
    }

    /// Whether the width asks for space padding, which FluutCAM does not
    /// write.
    pub fn space_padded(self) -> bool {
        !self.zero_pad && self.width > 1 + self.fixed_chars()
    }
}

// ---------------------------------------------------------- mapping

/// Values the FluutCAM emitter supplies for every program (spec 05
/// §5.4.2), by the names Vectric uses for them.
const SUPPLIED: [&str; 44] = [
    "X",
    "Y",
    "Z",
    "A",
    "B",
    "I",
    "J",
    "IA",
    "JA",
    "F",
    "FP",
    "FC",
    "S",
    "T",
    "N",
    "TOOLNAME",
    "TDIA",
    "SAFEZ",
    "XH",
    "YH",
    "ZH",
    "XMIN",
    "YMIN",
    "ZMIN",
    "XMAX",
    "YMAX",
    "ZMAX",
    "DATE",
    "TIME",
    "TOOLPATH_NAME",
    "FILE_NOTES",
    "Radius",
    "DWELL",
    "LBRACKET",
    "RBRACKET",
    "PLANE",
    "MACHINE_NAME",
    "SETUP_NAME",
    "WORK_OFFSET",
    "POST_ID",
    "FLUUT_VERSION",
    "K",
    "KA",
    "R",
];

/// Supplied with a different meaning from Vectric's.
const APPROXIMATE: [&str; 6] = ["XMIN", "YMIN", "ZMIN", "XMAX", "YMAX", "ZMAX"];

/// Vectric blocks with a direct FluutCAM event.
const DIRECT: [(&str, &str); 22] = [
    ("HEADER", "header"),
    ("FOOTER", "footer"),
    ("RAPID_MOVE", "rapid"),
    ("FIRST_RAPID_MOVE", "first_rapid"),
    ("FEED_MOVE", "feed"),
    ("FIRST_FEED_MOVE", "first_feed"),
    ("PLUNGE_MOVE", "plunge"),
    ("FIRST_PLUNGE_MOVE", "first_plunge"),
    ("RETRACT_MOVE", "retract"),
    ("FIRST_RETRACT_MOVE", "first_retract"),
    ("CW_ARC_MOVE", "arc_cw"),
    ("FIRST_CW_ARC_MOVE", "first_arc_cw"),
    ("CCW_ARC_MOVE", "arc_ccw"),
    ("FIRST_CCW_ARC_MOVE", "first_arc_ccw"),
    ("DWELL_MOVE", "dwell"),
    ("FEED_RATE_CHANGE", "feed_change"),
    ("SPINDLE_ON", "spindle_on"),
    ("CUTTER_COMP_LEFT", "cutter_comp_left"),
    ("CUTTER_COMP_RIGHT", "cutter_comp_right"),
    ("CUTTER_COMP_OFF", "cutter_comp_off"),
    ("TOOLCHANGE", "tool_change"),
    ("NEW_SEGMENT", "toolpath_start"),
];

/// Helical blocks: (Vectric, plunge variant, FluutCAM event).
const HELICAL: [(&str, &str, &str); 4] = [
    (
        "CW_HELICAL_ARC_MOVE",
        "CW_HELICAL_ARC_PLUNGE_MOVE",
        "helix_cw",
    ),
    (
        "FIRST_CW_HELICAL_ARC_MOVE",
        "FIRST_CW_HELICAL_ARC_PLUNGE_MOVE",
        "first_helix_cw",
    ),
    (
        "CCW_HELICAL_ARC_MOVE",
        "CCW_HELICAL_ARC_PLUNGE_MOVE",
        "helix_ccw",
    ),
    (
        "FIRST_CCW_HELICAL_ARC_MOVE",
        "FIRST_CCW_HELICAL_ARC_PLUNGE_MOVE",
        "first_helix_ccw",
    ),
];

/// Settings mapped or deliberately ignored (with a warning) below.
const KNOWN_SETTINGS: [&str; 22] = [
    "POST_NAME",
    "FILE_EXTENSION",
    "UNITS",
    "LINE_ENDING",
    "SUBSTITUTE",
    "LINE_NUMBER_START",
    "LINE_NUMBER_INCREMENT",
    "LINE_NUMBER_MAXIMUM",
    "DIRECT_OUTPUT",
    "PRINT_DIRECT",
    "POST_BASE",
    "ROTARY_WRAP_X",
    "ROTARY_WRAP_Y",
    "INVERSE_TIME_MODE",
    "TAPE_SPLITTING",
    "LASER_SUPPORT",
    "RAPID_PLUNGE_TO_STARTZ",
    "SPINDLE_SPEED_RANGE",
    "MIN_ARC_RADIUS",
    "MAX_ARC_RADIUS",
    "MAXIMUM_ROTATION",
    "MAXIMUM_LENGTH",
];

/// A word as written to `[words.<key>]`.
#[derive(Clone, PartialEq, Debug)]
struct Word {
    placeholder: Option<String>,
    prefix: String,
    changed: bool,
    fmt: ValueFormat,
    scale: f64,
}

/// A template line as written: text and an optional condition.
#[derive(Clone, PartialEq, Debug)]
struct OutLine {
    when: Option<&'static str>,
    text: String,
}

#[derive(Clone, PartialEq, Debug)]
struct Wrap {
    /// `X` or `Y`: the Vectric axis wrapped around the cylinder.
    axis: &'static str,
    /// `A` or `B`.
    letter: &'static str,
    negative: bool,
}

/// A block line after placeholder handling.
enum Rendered {
    Lines(Vec<String>),
    /// Dropped because it needs a value FluutCAM does not have.
    Dropped,
}

struct Conv {
    diags: Vec<Diagnostic>,
    errors: Vec<Diagnostic>,
    report: Report,
}

/// Placeholder use, for the summary warnings.
#[derive(Default)]
struct Uses {
    unsupported: BTreeMap<String, usize>,
    approximate: BTreeSet<String>,
    dropped_lines: BTreeMap<String, usize>,
    control_chars: usize,
    whitespace: usize,
    blank: usize,
    line_number: bool,
}

impl Conv {
    fn warn(&mut self, code: &str, msg: impl Into<String>) {
        self.diags.push(warn(code, msg));
    }

    fn refuse(&mut self, code: &str, msg: impl Into<String>) {
        self.errors.push(err(code, msg));
    }

    fn convert(&mut self, src: &Source, opts: &Options) -> Option<(String, String)> {
        self.report.settings_read = src.settings.len();
        self.report.vars_read = src.vars.len();
        self.report.blocks_read = src.blocks.len();
        self.report.lines_read = src.blocks.iter().map(|(_, l)| l.len()).sum();
        self.reading_findings(src);
        let set = |k: &str| src.setting(k).map(|v| unquote(v).to_owned());
        let yes = |k: &str| set(k).is_some_and(|v| v.eq_ignore_ascii_case("yes"));

        // Refusals first: the post would write wrong G-code.
        if src.blocks.is_empty() && src.setting("POST_NAME").is_none() {
            self.refuse(
                "import.file.invalid",
                "no `POST_NAME` and no blocks: this is not a Vectric post processor",
            );
            return None;
        }
        let units = match set("UNITS").map(|u| u.to_ascii_uppercase()) {
            Some(u) if u == "MM" => "mm",
            Some(u) if u == "INCHES" || u == "INCH" => "inch",
            Some(u) => {
                self.refuse(
                    "import.pp.units",
                    format!("`UNITS = {u}` is neither MM nor INCHES"),
                );
                "mm"
            }
            None => {
                self.refuse(
                    "import.pp.units",
                    "no `UNITS`: the output units are unknown, and guessing could scale every move",
                );
                "mm"
            }
        };
        if yes("INVERSE_TIME_MODE") {
            self.refuse(
                "import.pp.inverse_time",
                "`INVERSE_TIME_MODE` posts write G93 feeds from their own blocks; FluutCAM switches G93 itself around rotary moves, so the conversion would mix feed modes",
            );
        }
        if src.setting("SPINDLE_SPEED_RANGE").is_some() {
            self.refuse(
                "import.pp.spindle_range",
                "`SPINDLE_SPEED_RANGE` maps spindle speeds onto speed steps; FluutCAM writes the speed itself, which this machine would misread",
            );
        }
        let wrap = self.wrap(src);
        if !self.errors.is_empty() {
            return None;
        }

        let mut name = opts
            .name
            .clone()
            .or_else(|| set("POST_NAME"))
            .unwrap_or_default();
        name = name
            .chars()
            .filter(|c| !c.is_control())
            .take(limits::MAX_NAME_CHARS)
            .collect::<String>()
            .trim()
            .to_owned();
        if name.is_empty() {
            name = opts.post_id.clone();
        }

        let mut words = self.words(src, wrap.as_ref());
        let decimal_char = self.decimal_char(src);
        let units_inch = units == "inch";
        let mm = |v: f64| if units_inch { v * 25.4 } else { v };

        let mut output: Vec<(String, String)> = vec![
            ("units".into(), string(units)),
            ("decimal_char".into(), string(&decimal_char.to_string())),
        ];
        self.report.settings_mapped += 1;
        if let Some(n) = set("POST_NAME")
            && !n.is_empty()
        {
            self.report.settings_mapped += 1;
        }
        let ext = set("FILE_EXTENSION").unwrap_or_default();
        let ext_ok = !ext.is_empty()
            && ext.len() <= 16
            && ext.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if ext_ok {
            output.push(("extension".into(), string(&ext)));
            self.report.settings_mapped += 1;
        } else if src.setting("FILE_EXTENSION").is_some() {
            self.warn(
                "import.pp.bad_value",
                format!("`FILE_EXTENSION = {ext}` is not a plain extension; `nc` is used"),
            );
        }
        output.push(("line_ending".into(), string(self.line_ending(src))));
        let (sub, pairs) = self.substitute(src);
        if let Some(sub) = sub {
            output.push(("substitute_values".into(), sub));
        }
        // `[POST_NAME]` is written into the lines as text, substituted like
        // the names the emitter fills in.
        let inline_name: String = name
            .chars()
            .map(|c| pairs.iter().find(|p| p.0 == c).map_or(c, |p| p.1))
            .collect();

        // Settings with no FluutCAM meaning.
        let ignored: [(&str, &str); 5] = [
            (
                "DIRECT_OUTPUT",
                "sends the output to the control software; FluutCAM writes a file",
            ),
            (
                "PRINT_DIRECT",
                "prints the output directly; FluutCAM writes a file",
            ),
            (
                "TAPE_SPLITTING",
                "splits the output by line count; set `output.max_file_size` (bytes) by hand if the controller needs it",
            ),
            (
                "RAPID_PLUNGE_TO_STARTZ",
                "is a Vectric toolpath option; FluutCAM toolpaths set their own rapid and plunge heights",
            ),
            (
                "LASER_SUPPORT",
                "marks laser output; FluutCAM has no laser toolpaths, the laser blocks are left out",
            ),
        ];
        for (k, why) in ignored {
            let on = match k {
                "LASER_SUPPORT" => yes(k),
                _ => src.setting(k).is_some(),
            };
            if on {
                self.warn("import.pp.setting_ignored", format!("`{k}` {why}"));
            }
        }
        let unknown: Vec<&str> = src
            .settings
            .iter()
            .map(|(k, _)| k.as_str())
            .filter(|k| !KNOWN_SETTINGS.contains(k))
            .collect();
        if !unknown.is_empty() {
            self.warn(
                "import.pp.setting_unknown",
                format!(
                    "settings FluutCAM does not know were left out: {}",
                    unknown.join(", ")
                ),
            );
        }

        // Line numbers.
        let num = |k: &str, this: &mut Conv| -> Option<u64> {
            let v = set(k)?;
            match v
                .split_whitespace()
                .next()
                .and_then(|t| t.parse::<u64>().ok())
            {
                Some(n) => {
                    this.report.settings_mapped += 1;
                    Some(n)
                }
                None => {
                    this.warn(
                        "import.pp.bad_value",
                        format!("`{k} = {v}` is not a whole number; left out"),
                    );
                    None
                }
            }
        };
        let ln_start = num("LINE_NUMBER_START", self);
        let mut ln_inc = num("LINE_NUMBER_INCREMENT", self);
        let ln_max = num("LINE_NUMBER_MAXIMUM", self);
        if ln_inc == Some(0) {
            self.warn(
                "import.pp.bad_value",
                "`LINE_NUMBER_INCREMENT = 0`; 1 is used",
            );
            ln_inc = Some(1);
        }

        // Arc and rotary limits.
        let fnum = |k: &str, this: &mut Conv| -> Option<f64> {
            let v = set(k)?;
            match v.parse::<f64>().ok().filter(|x| x.is_finite() && *x >= 0.0) {
                Some(x) => {
                    this.report.settings_mapped += 1;
                    Some(x)
                }
                None => {
                    this.warn(
                        "import.pp.bad_value",
                        format!("`{k} = {v}` is not a number >= 0; left out"),
                    );
                    None
                }
            }
        };
        let min_r = fnum("MIN_ARC_RADIUS", self).map(mm);
        let max_r = fnum("MAX_ARC_RADIUS", self).map(mm);
        let max_rot = fnum("MAXIMUM_ROTATION", self).filter(|x| *x > 0.0);
        let max_len = fnum("MAXIMUM_LENGTH", self).map(mm);
        if let Some(l) = max_len {
            output.push(("max_segment_length".into(), num_str(l)));
        }

        // Blocks.
        let comment = comment_syntax(src);
        if comment.2 {
            self.warn(
                "import.pp.comment_syntax",
                "the post writes no comments and does not look like G-code; FluutCAM's provenance lines are written as `( … )`: check the controller accepts them, or set `output.comment_start`",
            );
        }
        output.push(("comment_start".into(), string(comment.0)));
        output.push(("comment_end".into(), string(comment.1)));

        let mut uses = Uses::default();
        let mut blocks: BTreeMap<String, Vec<OutLine>> = BTreeMap::new();
        let mut arcs_ok = true;
        let mut helix_ok = true;
        let mut r_mode = false;
        let mut absolute = false;
        let first_tool_only: Option<&'static str> = Some("!state.first_tool");
        for (vname, event) in DIRECT {
            let Some(lines) = src.block(vname) else {
                continue;
            };
            let arc = event.contains("arc_");
            let out =
                self.block_lines(vname, lines, &words, wrap.as_ref(), &inline_name, &mut uses);
            if arc {
                let text: String = out.join(" ");
                r_mode |= text.contains("[Radius]");
                absolute |= text.contains("[IA]") || text.contains("[JA]");
                if !has_centre(&text) {
                    arcs_ok = false;
                    continue;
                }
            }
            let when = match event {
                "tool_change" | "toolpath_start" => first_tool_only,
                _ => None,
            };
            self.report.blocks_mapped += 1;
            blocks.insert(
                event.to_owned(),
                out.into_iter().map(|text| OutLine { when, text }).collect(),
            );
        }
        // Helices: the plunge variants stand in when the plain ones are
        // missing.
        for (vname, plunge, event) in HELICAL {
            let (from, lines) = match (src.block(vname), src.block(plunge)) {
                (Some(l), p) => {
                    if p.is_some() {
                        self.warn(
                            "import.pp.block_fallback",
                            format!(
                                "`{plunge}` is left out: FluutCAM writes every helix with `{vname}`"
                            ),
                        );
                    }
                    (vname, l)
                }
                (None, Some(l)) => {
                    self.warn(
                        "import.pp.block_fallback",
                        format!("`{plunge}` is used for every helix (there is no `{vname}`)"),
                    );
                    (plunge, l)
                }
                (None, None) => continue,
            };
            let out = self.block_lines(from, lines, &words, wrap.as_ref(), &inline_name, &mut uses);
            if !has_centre(&out.join(" ")) {
                helix_ok = false;
                continue;
            }
            self.report.blocks_mapped += 1;
            blocks.insert(
                event.to_owned(),
                out.into_iter()
                    .map(|text| OutLine { when: None, text })
                    .collect(),
            );
        }
        // INITIAL_RAPID_MOVE: the program's first rapid. It becomes the
        // first lines of `first_rapid`, guarded by `state.moved`.
        if let Some(lines) = src.block("INITIAL_RAPID_MOVE") {
            let init = self.block_lines(
                "INITIAL_RAPID_MOVE",
                lines,
                &words,
                wrap.as_ref(),
                &inline_name,
                &mut uses,
            );
            let later: Vec<OutLine> = blocks
                .get("first_rapid")
                .or_else(|| blocks.get("rapid"))
                .cloned()
                .unwrap_or_default();
            let mut b: Vec<OutLine> = init
                .into_iter()
                .map(|text| OutLine {
                    when: Some("!state.moved"),
                    text,
                })
                .collect();
            b.extend(later.into_iter().map(|l| OutLine {
                when: Some("state.moved"),
                text: l.text,
            }));
            self.report.blocks_mapped += 1;
            blocks.insert("first_rapid".into(), b);
        }
        // Blocks with no FluutCAM event.
        let mapped: BTreeSet<&str> = DIRECT
            .iter()
            .map(|(v, _)| *v)
            .chain(HELICAL.iter().flat_map(|(a, b, _)| [*a, *b]))
            .chain(["INITIAL_RAPID_MOVE"])
            .collect();
        let left: Vec<&str> = src
            .blocks
            .iter()
            .map(|(n, _)| n.as_str())
            .filter(|n| !mapped.contains(n))
            .collect();
        if !left.is_empty() {
            self.warn(
                "import.pp.block_unsupported",
                format!(
                    "blocks with no FluutCAM event were left out: {}",
                    left.join(", ")
                ),
            );
        }
        if !arcs_ok {
            for b in ["arc_cw", "arc_ccw", "first_arc_cw", "first_arc_ccw"] {
                blocks.remove(b);
            }
            self.warn(
                "import.pp.arcs_linearized",
                "an arc block writes no arc centre FluutCAM supplies ([I] [J], [IA] [JA] or [Radius]); arcs are written as line moves",
            );
        }
        if !helix_ok {
            for b in ["helix_cw", "helix_ccw", "first_helix_cw", "first_helix_ccw"] {
                blocks.remove(b);
            }
            self.warn(
                "import.pp.arcs_linearized",
                "a helical block writes no arc centre FluutCAM supplies ([I] [J], [IA] [JA] or [Radius]); helices are written as line moves",
            );
        }
        // Empty motion blocks would make a move disappear: FluutCAM treats
        // them as absent and falls back.
        let empty: Vec<String> = blocks
            .iter()
            .filter(|(k, v)| v.is_empty() && is_motion(k))
            .map(|(k, _)| k.clone())
            .collect();
        for k in &empty {
            blocks.remove(k);
        }
        if !empty.is_empty() {
            self.warn(
                "import.pp.block_fallback",
                format!(
                    "empty motion blocks are treated as absent, so their moves use the plain block: {}",
                    empty.join(", ")
                ),
            );
        }
        // Required blocks.
        if !blocks.contains_key("feed") {
            match blocks.get("first_feed").cloned() {
                Some(l) => {
                    self.warn(
                        "import.pp.block_fallback",
                        "no `FEED_MOVE`; `FIRST_FEED_MOVE` is used for every feed move",
                    );
                    blocks.insert("feed".into(), l);
                }
                None => {
                    self.refuse(
                        "import.pp.block_missing",
                        "no usable `FEED_MOVE` (missing, empty, or made only of values FluutCAM does not supply): the post cannot write cutting moves",
                    );
                    return None;
                }
            }
        }
        if !blocks.contains_key("rapid") {
            let l = blocks["feed"].clone();
            self.warn(
                "import.pp.block_fallback",
                "no `RAPID_MOVE`; rapid moves are written with the `FEED_MOVE` lines",
            );
            blocks.insert("rapid".into(), l);
        }
        for (k, v) in [("header", "HEADER"), ("footer", "FOOTER")] {
            if !blocks.contains_key(k) {
                self.warn(
                    "import.pp.block_missing",
                    format!("no `{v}` block; the program has none"),
                );
            }
        }
        let arcs_on = arcs_ok && blocks.contains_key("arc_cw") && blocks.contains_key("arc_ccw");
        if arcs_ok && (blocks.contains_key("arc_cw") != blocks.contains_key("arc_ccw")) {
            self.warn(
                "import.pp.arcs_linearized",
                "only one arc direction has a block; arcs are written as line moves",
            );
        }
        if !arcs_on {
            for b in ["arc_cw", "arc_ccw", "first_arc_cw", "first_arc_ccw"] {
                blocks.remove(b);
            }
        }
        let helix_on =
            helix_ok && blocks.contains_key("helix_cw") && blocks.contains_key("helix_ccw");
        if !helix_on {
            for b in ["helix_cw", "helix_ccw", "first_helix_cw", "first_helix_ccw"] {
                blocks.remove(b);
            }
        }
        // Rotary moves of a wrapped post use the feed lines.
        if wrap.is_some() {
            let l = blocks
                .get("first_feed")
                .or_else(|| blocks.get("feed"))
                .cloned()
                .unwrap_or_default();
            blocks.insert("rotary".into(), l);
        }
        // `[Radius]` in arc blocks: radius-mode arcs.
        if r_mode && arcs_on {
            let w = words.remove("Radius");
            words.insert(
                "R".into(),
                Word {
                    placeholder: Some("Radius".into()),
                    ..w.unwrap_or(Word {
                        placeholder: None,
                        prefix: String::new(),
                        changed: false,
                        fmt: ValueFormat {
                            plus: false,
                            zero_pad: false,
                            width: 0,
                            separator: decimal_char,
                            decimals: 3,
                        },
                        scale: 1.0,
                    })
                },
            );
        }
        if uses.line_number && !words.contains_key("N") {
            self.warn(
                "import.pp.var_missing",
                "`[N]` is used but no `VAR` defines it; written as `N` and a whole number",
            );
            words.insert(
                "N".into(),
                Word {
                    placeholder: None,
                    prefix: "N".into(),
                    changed: false,
                    fmt: ValueFormat::parse("1.0").expect("a valid format"),
                    scale: 1.0,
                },
            );
        }
        self.use_findings(&uses);
        self.report.words_written = words.len();
        self.report.lines_written = blocks.values().map(Vec::len).sum();

        // Write.
        let mut o = String::new();
        let _ = writeln!(
            o,
            "# Imported from a Vectric post processor (fluut posts import)."
        );
        let _ = writeln!(
            o,
            "# Review it before use: imported posts are marked origin = \"imported\"."
        );
        if !src.head_comment.is_empty() {
            let _ = writeln!(o, "#\n# The source file's opening comment:");
            for c in &src.head_comment {
                let c: String = c
                    .chars()
                    .filter(|c| !c.is_control() || *c == '\t')
                    .collect();
                let _ = writeln!(o, "#   {}", c.trim_end());
            }
        }
        let _ = writeln!(o, "format  = \"fluut.post/1\"");
        let _ = writeln!(o, "id      = {}", string(&opts.post_id));
        let _ = writeln!(o, "version = \"1.0\"");
        let _ = writeln!(o, "name    = {}", string(&name));
        let _ = writeln!(o, "origin  = \"imported\"");
        let _ = writeln!(o, "\n[output]");
        for (k, v) in &output {
            let _ = writeln!(o, "{k} = {v}");
        }
        if ln_start.is_some() || ln_inc.is_some() || ln_max.is_some() {
            let _ = writeln!(o, "\n[line_numbering]");
            let _ = writeln!(o, "enabled = false   # `[N]` in the lines numbers them");
            if let Some(v) = ln_start {
                let _ = writeln!(o, "start = {v}");
            }
            if let Some(v) = ln_inc {
                let _ = writeln!(o, "increment = {v}");
            }
            if let Some(v) = ln_max {
                let _ = writeln!(o, "maximum = {v}");
            }
        }
        for (k, w) in &words {
            let _ = writeln!(o, "\n[words.{}]", toml_key(k));
            if let Some(p) = &w.placeholder {
                let _ = writeln!(o, "placeholder = {}", string(p));
            }
            let _ = writeln!(o, "prefix = {}", string(&w.prefix));
            let _ = writeln!(
                o,
                "when = \"{}\"",
                if w.changed { "changed" } else { "always" }
            );
            let _ = writeln!(o, "decimals = {}", w.fmt.decimals);
            let _ = writeln!(o, "trailing_zeros = true");
            let _ = writeln!(o, "leading_zero = true");
            if w.fmt.plus {
                let _ = writeln!(o, "sign = \"always\"");
            }
            if w.fmt.pad_width() > 0 {
                let _ = writeln!(o, "pad_width = {}", w.fmt.pad_width());
            }
            if w.scale != 1.0 {
                let _ = writeln!(o, "scale = {}", num_str(w.scale));
            }
        }
        let _ = writeln!(o, "\n[arcs]");
        let _ = writeln!(o, "enabled = {arcs_on}");
        let _ = writeln!(o, "planes = [\"XY\"]");
        let mode = if r_mode && arcs_on {
            "r"
        } else if absolute {
            "ijk_absolute"
        } else {
            "ijk_incremental"
        };
        let _ = writeln!(o, "centre_mode = \"{mode}\"");
        if mode == "r" {
            let _ = writeln!(o, "max_sweep_deg = 90");
        }
        if let Some(r) = min_r {
            let _ = writeln!(o, "min_radius = {}", num_str(r));
        }
        if let Some(r) = max_r {
            let _ = writeln!(o, "max_radius = {}", num_str(r));
        }
        let _ = writeln!(o, "\n[helix]");
        let _ = writeln!(o, "enabled = {helix_on}");
        let _ = writeln!(o, "planes = [\"XY\"]");
        let _ = writeln!(o, "\n[cycles]");
        let _ = writeln!(o, "enabled = false");
        if wrap.is_some() {
            let _ = writeln!(o, "\n[rotary]");
            let _ = writeln!(o, "enabled = true");
            if let Some(m) = max_rot {
                let _ = writeln!(o, "max_angle_per_line = {}", num_str(m));
            }
            let _ = writeln!(o, "\n[feed]");
            let _ = writeln!(o, "rotary_method = \"linear_only\"");
        }
        for (k, lines) in &blocks {
            let _ = writeln!(o, "\n[blocks.{k}]");
            if lines.is_empty() {
                let _ = writeln!(o, "lines = []");
                continue;
            }
            let _ = writeln!(o, "lines = [");
            for l in lines {
                match l.when {
                    Some(w) => {
                        let _ = writeln!(
                            o,
                            "  {{ when = {}, line = {} }},",
                            string(w),
                            string(&l.text)
                        );
                    }
                    None => {
                        let _ = writeln!(o, "  {},", string(&l.text));
                    }
                }
            }
            let _ = writeln!(o, "]");
        }
        Some((o, name))
    }

    /// Warnings about what the reader skipped.
    fn reading_findings(&mut self, src: &Source) {
        if src.bare_percent > 0 {
            self.warn(
                "import.pp.unknown_line",
                format!(
                    "{} unquoted `%` lines were left out (only quoted lines are output)",
                    src.bare_percent
                ),
            );
        }
        if !src.unknown_lines.is_empty() {
            let first: Vec<String> = src
                .unknown_lines
                .iter()
                .take(10)
                .map(ToString::to_string)
                .collect();
            self.warn(
                "import.pp.unknown_line",
                format!(
                    "{} lines are neither settings, `VAR`, blocks nor quoted lines and were left out (lines {}{})",
                    src.unknown_lines.len(),
                    first.join(", "),
                    if src.unknown_lines.len() > 10 { ", …" } else { "" }
                ),
            );
        }
        if src.stray_lines > 0 {
            self.warn(
                "import.pp.unknown_line",
                format!(
                    "{} quoted lines stand outside any block and were left out",
                    src.stray_lines
                ),
            );
        }
        if src.trailing > 0 {
            self.warn(
                "import.pp.unknown_line",
                format!(
                    "{} lines have text after the closing quote or `]` that is not a `+` comment; it was left out",
                    src.trailing
                ),
            );
        }
        if !src.duplicate_blocks.is_empty() {
            self.warn(
                "import.pp.block_duplicate",
                format!(
                    "blocks defined twice; the later one is used: {}",
                    src.duplicate_blocks
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
        if !src.duplicate_settings.is_empty() {
            self.warn(
                "import.pp.setting_duplicate",
                format!(
                    "settings given twice; the later value is used: {}",
                    src.duplicate_settings
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
    }

    /// `ROTARY_WRAP_X` / `ROTARY_WRAP_Y`.
    fn wrap(&mut self, src: &Source) -> Option<Wrap> {
        let x = src.setting("ROTARY_WRAP_X").map(unquote);
        let y = src.setting("ROTARY_WRAP_Y").map(unquote);
        let (axis, v) = match (x, y) {
            (None, None) => return None,
            (Some(_), Some(_)) => {
                self.refuse(
                    "import.pp.rotary_wrap",
                    "both `ROTARY_WRAP_X` and `ROTARY_WRAP_Y` are set",
                );
                return None;
            }
            (Some(v), None) => ("X", v),
            (None, Some(v)) => ("Y", v),
        };
        let (negative, l) = match v.strip_prefix('-') {
            Some(l) => (true, l),
            None => (false, v),
        };
        let letter = match l.trim().to_ascii_uppercase().as_str() {
            "A" => "A",
            "B" => "B",
            _ => {
                self.refuse(
                    "import.pp.rotary_wrap",
                    format!(
                        "`ROTARY_WRAP_{axis} = {v}` names no rotary axis letter (A or B, optionally with `-`)"
                    ),
                );
                return None;
            }
        };
        self.report.settings_mapped += 1;
        self.warn(
            "import.pp.rotary_wrap",
            format!(
                "wrapped output: FluutCAM rotary moves write {letter} next to {axis} (in degrees{}), and feeds as given; the machine's rotary axis must lie along {}",
                if negative { ", sign reversed" } else { "" },
                if axis == "X" { "Y" } else { "X" }
            ),
        );
        Some(Wrap {
            axis,
            letter,
            negative,
        })
    }

    /// `VAR` lines → words, keyed by their output name.
    fn words(&mut self, src: &Source, wrap: Option<&Wrap>) -> BTreeMap<String, Word> {
        let mut words = BTreeMap::new();
        let mut dup = BTreeSet::new();
        let mut bad_fmt = Vec::new();
        let mut space = BTreeSet::new();
        let mut odd_mode = BTreeSet::new();
        let mut neg_pad = BTreeSet::new();
        for v in &src.vars {
            if !is_name(&v.vo) {
                bad_fmt.push(format!("line {}: output name `{}`", v.line, v.vo));
                continue;
            }
            let changed = match v.wo.as_str() {
                "C" | "c" => true,
                "A" | "a" => false,
                _ => {
                    odd_mode.insert(v.wo.clone());
                    false
                }
            };
            let fmt = match ValueFormat::parse(&v.vf) {
                Ok(f) => f,
                Err(e) => {
                    bad_fmt.push(format!("line {}: {e}", v.line));
                    ValueFormat::parse("1.3").expect("a valid format")
                }
            };
            if fmt.space_padded() {
                space.insert(v.vo.clone());
            }
            if fmt.pad_width() > 0
                && !["N", "T", "S", "F", "FP", "FC", "DWELL"].contains(&v.vo.as_str())
            {
                neg_pad.insert(v.vo.clone());
            }
            let scale = match &v.mx {
                None => 1.0,
                Some(m) if m.is_empty() => 1.0,
                Some(m) => match m.parse::<f64>() {
                    Ok(x) if x.is_finite() && x != 0.0 => x,
                    _ => {
                        bad_fmt.push(format!("line {}: multiplier `{m}`", v.line));
                        1.0
                    }
                },
            };
            let w = Word {
                placeholder: None,
                prefix: expand_plain(&v.cs),
                changed,
                fmt,
                scale,
            };
            let key = canonical(&v.vo);
            if words.insert(key.clone(), w).is_some() {
                dup.insert(key);
            }
        }
        if !dup.is_empty() {
            self.warn(
                "import.pp.var_duplicate",
                format!(
                    "output names defined by two `VAR` lines; the later is used: {}",
                    dup.into_iter().collect::<Vec<_>>().join(", ")
                ),
            );
        }
        if !bad_fmt.is_empty() {
            self.warn(
                "import.pp.var_format",
                format!(
                    "`VAR` fields not understood (3 decimals or no multiplier used instead): {}",
                    bad_fmt.join("; ")
                ),
            );
        }
        if !space.is_empty() {
            self.warn(
                "import.pp.var_format",
                format!(
                    "field widths that pad with spaces are not written (numbers are as short as their decimals allow): {}",
                    space.into_iter().collect::<Vec<_>>().join(", ")
                ),
            );
        }
        if !neg_pad.is_empty() {
            self.warn(
                "import.pp.var_format",
                format!(
                    "zero-padded words that can be negative are one character longer than Vectric's when negative (the sign is not counted in the width): {}",
                    neg_pad.into_iter().collect::<Vec<_>>().join(", ")
                ),
            );
        }
        if !odd_mode.is_empty() {
            self.warn(
                "import.pp.var_format",
                format!(
                    "output modes other than A and C ({}) are written always",
                    odd_mode.into_iter().collect::<Vec<_>>().join(", ")
                ),
            );
        }
        // Unsupported output names stay out; their uses are reported where
        // the blocks use them.
        words.retain(|k, _| SUPPLIED.contains(&k.as_str()));
        if let Some(w) = wrap {
            let base = words.get(w.axis).cloned().unwrap_or(Word {
                placeholder: None,
                prefix: String::new(),
                changed: true,
                fmt: ValueFormat::parse("1.3").expect("a valid format"),
                scale: 1.0,
            });
            words.insert(
                w.letter.to_owned(),
                Word {
                    prefix: w.letter.to_owned(),
                    scale: if w.negative { -1.0 } else { 1.0 },
                    ..base
                },
            );
        }
        words
    }

    /// The decimal separator of the words that have decimals.
    fn decimal_char(&mut self, src: &Source) -> char {
        let mut count: BTreeMap<char, usize> = BTreeMap::new();
        for v in &src.vars {
            if let Ok(f) = ValueFormat::parse(&v.vf)
                && f.decimals > 0
            {
                *count.entry(f.separator).or_default() += 1;
            }
        }
        let best = count
            .iter()
            .max_by_key(|(c, n)| (**n, **c == '.'))
            .map_or('.', |(c, _)| *c);
        if count.len() > 1 {
            self.warn(
                "import.pp.decimal_char",
                format!(
                    "words use both `.` and `,` as decimal separator; `{best}` is used for all"
                ),
            );
        }
        best
    }

    fn line_ending(&mut self, src: &Source) -> &'static str {
        let Some(raw) = src.setting("LINE_ENDING") else {
            return "crlf";
        };
        self.report.settings_mapped += 1;
        let v = expand_plain(unquote(raw));
        match v.as_str() {
            "\r\n" => "crlf",
            "\n" => "lf",
            "\r" => "cr",
            _ => {
                self.warn(
                    "import.pp.line_ending",
                    format!(
                        "`LINE_ENDING` is {} characters that are not CR LF, LF or CR; CR LF is used",
                        v.chars().count()
                    ),
                );
                "crlf"
            }
        }
    }

    /// `SUBSTITUTE = "abcd"`: a→b, c→d in user text. The TOML inline
    /// table and the pairs.
    fn substitute(&mut self, src: &Source) -> (Option<String>, Vec<(char, char)>) {
        let Some(raw) = src.setting("SUBSTITUTE") else {
            return (None, Vec::new());
        };
        let chars: Vec<char> = expand_plain(unquote(raw)).chars().collect();
        if chars.len() % 2 == 1 {
            self.warn(
                "import.pp.bad_value",
                "`SUBSTITUTE` has an odd number of characters; the last one is left out",
            );
        }
        let pairs: Vec<(char, char)> = chars
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| (p[0], p[1]))
            .collect();
        if pairs.is_empty() {
            return (None, pairs);
        }
        let text: Vec<String> = pairs
            .iter()
            .map(|(a, b)| format!("{} = {}", string(&a.to_string()), string(&b.to_string())))
            .collect();
        self.report.settings_mapped += 1;
        (Some(format!("{{ {} }}", text.join(", "))), pairs)
    }

    /// Converts one block's lines.
    fn block_lines(
        &mut self,
        block: &str,
        lines: &[String],
        words: &BTreeMap<String, Word>,
        wrap: Option<&Wrap>,
        post_name: &str,
        uses: &mut Uses,
    ) -> Vec<String> {
        let motion = ["RAPID", "FEED", "PLUNGE", "RETRACT", "ARC"]
            .iter()
            .any(|m| block.contains(m));
        let mut out = Vec::new();
        for raw in lines {
            match render(raw, words, wrap.filter(|_| motion), post_name, uses) {
                Rendered::Lines(ls) => {
                    for l in ls {
                        let t = l.trim();
                        if t.is_empty() {
                            uses.blank += 1;
                            continue;
                        }
                        if t.len() != l.len() {
                            uses.whitespace += 1;
                        }
                        out.push(t.to_owned());
                    }
                }
                Rendered::Dropped => {
                    *uses.dropped_lines.entry(block.to_owned()).or_default() += 1;
                }
            }
        }
        out
    }

    fn use_findings(&mut self, u: &Uses) {
        if !u.unsupported.is_empty() {
            let list: Vec<String> = u
                .unsupported
                .iter()
                .map(|(k, n)| format!("[{k}] x{n}"))
                .collect();
            self.warn(
                "import.pp.placeholder",
                format!(
                    "placeholders FluutCAM has no value for were removed (comment lines made only of them were left out): {}",
                    list.join(", ")
                ),
            );
        }
        if !u.dropped_lines.is_empty() {
            let list: Vec<String> = u
                .dropped_lines
                .iter()
                .map(|(k, n)| format!("{k} x{n}"))
                .collect();
            self.warn(
                "import.pp.line_dropped",
                format!(
                    "lines that need a value FluutCAM does not supply were left out: {}",
                    list.join(", ")
                ),
            );
        }
        if !u.approximate.is_empty() {
            self.warn(
                "import.pp.placeholder_approx",
                format!(
                    "{} are the extents of the toolpaths, not of the material",
                    u.approximate
                        .iter()
                        .map(|p| format!("[{p}]"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
        if u.control_chars > 0 {
            self.warn(
                "import.pp.control_char",
                format!(
                    "{} control characters given as [code] were left out",
                    u.control_chars
                ),
            );
        }
        if u.whitespace > 0 || u.blank > 0 {
            self.warn(
                "import.pp.whitespace",
                format!(
                    "FluutCAM trims lines: {} lines lose leading or trailing spaces, {} empty lines are not written",
                    u.whitespace, u.blank
                ),
            );
        }
    }
}

/// A placeholder name in FluutCAM's spelling: Vectric matches names
/// regardless of case (its own posts write both `[Radius]` and `[RADIUS]`).
fn canonical(name: &str) -> String {
    SUPPLIED
        .iter()
        .find(|s| s.eq_ignore_ascii_case(name))
        .map_or_else(|| name.to_owned(), |s| (*s).to_owned())
}

/// Whether an arc block's text names a centre FluutCAM supplies.
fn has_centre(text: &str) -> bool {
    (text.contains("[I]") && text.contains("[J]"))
        || (text.contains("[IA]") && text.contains("[JA]"))
        || text.contains("[Radius]")
}

fn is_motion(event: &str) -> bool {
    let e = event.strip_prefix("first_").unwrap_or(event);
    matches!(
        e,
        "rapid" | "feed" | "plunge" | "retract" | "arc_cw" | "arc_ccw" | "helix_cw" | "helix_ccw"
    )
}

/// A TOML bare key, or a quoted one.
fn toml_key(k: &str) -> String {
    if is_name(k) { k.to_owned() } else { string(k) }
}

/// A float for TOML: shortest round-trip form, always with a `.` or an
/// exponent.
fn num_str(x: f64) -> String {
    let s = format!("{x:?}");
    if s.contains(['.', 'e', 'E']) {
        s
    } else {
        format!("{s}.0")
    }
}

/// Expands `[nn]` character codes in a setting or prefix; other brackets
/// stay as they are.
fn expand_plain(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find(']') {
            Some(close) if close > 0 && after[..close].bytes().all(|b| b.is_ascii_digit()) => {
                match after[..close].parse::<u32>().ok().and_then(char::from_u32) {
                    Some(c) if (c as u32) < 256 => out.push(c),
                    _ => out.push_str(&rest[open..open + close + 2]),
                }
                rest = &after[close + 1..];
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The comment style of the post's own comment lines: `( … )`, `;` or
/// `'`; the flag says none was found.
fn comment_syntax(src: &Source) -> (&'static str, &'static str, bool) {
    let mut paren = 0;
    let mut semi = 0;
    let mut quote = 0;
    for (_, lines) in &src.blocks {
        for l in lines {
            let t = l.trim_start();
            let t = t.strip_prefix("[N]").unwrap_or(t).trim_start();
            if t.starts_with('(') && t.trim_end().ends_with(')') {
                paren += 1;
            } else if t.starts_with(';') {
                semi += 1;
            } else if t.starts_with('\'') {
                quote += 1;
            }
        }
    }
    if paren == 0 && semi == 0 && quote == 0 {
        // A G-code post with no comments of its own: `( … )` is the
        // RS-274 comment, no need to warn.
        let gcode = src.blocks.iter().flat_map(|(_, l)| l).any(|l| {
            let t = l.trim_start();
            let t = t.strip_prefix("[N]").unwrap_or(t).trim_start().as_bytes();
            t.len() > 1 && matches!(t[0], b'G' | b'g' | b'M' | b'm') && t[1].is_ascii_digit()
        });
        ("(", ")", !gcode)
    } else if paren >= semi && paren >= quote {
        ("(", ")", false)
    } else if semi >= quote {
        (";", "", false)
    } else {
        ("'", "", false)
    }
}

/// Whether a template line is a comment: it starts (after `[N]`) with `(`,
/// `;` or `'`.
fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    let t = t.strip_prefix("[N]").unwrap_or(t).trim_start();
    t.starts_with(['(', ';', '\''])
}

/// One template line: `[nn]` codes expanded (`[` written as
/// `[LBRACKET]`), `[POST_NAME]` replaced by the name, placeholders checked,
/// the wrapped axis followed by the rotary word. A placeholder FluutCAM has
/// no value for is removed with the letters written directly before it
/// (as the emitter does for an absent value); a line left with nothing, or
/// a comment made only of such placeholders, is dropped.
fn render(
    raw: &str,
    words: &BTreeMap<String, Word>,
    wrap: Option<&Wrap>,
    post_name: &str,
    uses: &mut Uses,
) -> Rendered {
    let comment = is_comment(raw);
    let mut text = String::new();
    let mut supported = 0;
    let mut unsupported = 0;
    let mut rest = raw;
    while let Some(open) = rest.find('[') {
        text.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let inner = after.find(']').map(|c| &after[..c]);
        match inner {
            Some(d) if !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()) => {
                match d.parse::<u32>().ok().and_then(char::from_u32) {
                    Some('[') => text.push_str("[LBRACKET]"),
                    Some(c @ ('\r' | '\n')) => text.push(c),
                    Some(c) if c.is_control() && c != '\t' => uses.control_chars += 1,
                    Some(c) if (c as u32) < 256 => text.push(c),
                    _ => uses.control_chars += 1,
                }
                rest = &after[d.len() + 1..];
            }
            Some("POST_NAME") => {
                supported += 1;
                text.push_str(&post_name.replace('[', "[LBRACKET]"));
                rest = &after["POST_NAME".len() + 1..];
            }
            Some(raw_name) if is_name(raw_name) => {
                let n = canonical(raw_name);
                let n = n.as_str();
                if SUPPLIED.contains(&n) || words.contains_key(n) {
                    supported += usize::from(n != "N");
                    if n == "N" {
                        uses.line_number = true;
                    }
                    if APPROXIMATE.contains(&n) {
                        uses.approximate.insert(n.to_owned());
                    }
                    let spaced = text.ends_with(' ');
                    let _ = write!(text, "[{n}]");
                    if let Some(w) = wrap
                        && n == w.axis
                    {
                        let _ = write!(text, "{}[{}]", if spaced { " " } else { "" }, w.letter);
                    }
                } else {
                    unsupported += 1;
                    *uses.unsupported.entry(n.to_owned()).or_default() += 1;
                    if !comment {
                        while text.ends_with(|c: char| c.is_ascii_alphabetic()) {
                            text.pop();
                        }
                        if text.ends_with(' ') {
                            text.pop();
                        }
                    }
                }
                rest = &after[raw_name.len() + 1..];
            }
            _ => {
                text.push_str("[LBRACKET]");
                rest = after;
            }
        }
    }
    text.push_str(rest);
    if unsupported > 0 {
        let body = text.trim_start();
        let body = body.strip_prefix("[N]").unwrap_or(body).trim();
        if (comment && supported == 0) || body.is_empty() {
            return Rendered::Dropped;
        }
    }
    let lines = text
        .replace("\r\n", "\n")
        .split(['\r', '\n'])
        .map(str::to_owned)
        .collect();
    Rendered::Lines(lines)
}
