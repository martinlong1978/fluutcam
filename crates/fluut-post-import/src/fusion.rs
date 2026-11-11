// SPDX-License-Identifier: MIT OR Apache-2.0

//! Autodesk Fusion tool-library JSON (design D16 §3-§5).
//!
//! The file is `{"data": [tool, ...], "version": n}`; a bare array of tools
//! is accepted too. The keys used are listed in D16 §3.1. Everything else
//! is counted and dropped.

use crate::toml_out::{num, string};
use crate::{ImportOptions, ImportReport, ImportResult, limits, slug};
use fluut_model::Diagnostic;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

const INCH: f64 = 25.4;
const MAX_LEN: f64 = limits::MAX_LENGTH_MM;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    Flat,
    Ball,
    Bull,
    V,
    Taper,
    Drill,
}

impl Shape {
    fn name(self) -> &'static str {
        match self {
            Shape::Flat => "flat",
            Shape::Ball => "ball",
            Shape::Bull => "bull",
            Shape::V => "v",
            Shape::Taper => "taper",
            Shape::Drill => "drill",
        }
    }
}

#[derive(Default, Debug)]
struct Preset {
    material: String,
    rpm: Option<f64>,
    feed: Option<f64>,
    plunge: Option<f64>,
    stepover: Option<f64>,
    step_down: Option<f64>,
}

#[derive(Debug)]
struct Tool {
    id: String,
    name: String,
    number: u32,
    shape: Shape,
    diameter: f64,
    corner_radius: Option<f64>,
    flute_length: Option<f64>,
    half_angle: Option<f64>,
    tip_diameter: Option<f64>,
    point_angle: Option<f64>,
    pitch: Option<f64>,
    overall_length: Option<f64>,
    shank_diameter: Option<f64>,
    holder: Option<(f64, f64)>,
    presets: Vec<Preset>,
}

/// Fusion `type` text (lower case) to a shape; the flag says the mapping
/// is an approximation. `Err` when the type has no FluutCAM shape.
fn shape_of(kind: &str) -> Result<(Shape, bool), ()> {
    Ok(match kind {
        "flat end mill" => (Shape::Flat, false),
        "ball end mill" => (Shape::Ball, false),
        "bull nose end mill" | "radius mill" => (Shape::Bull, false),
        "chamfer mill" | "counter sink" => (Shape::V, false),
        "tapered mill" => (Shape::Taper, true),
        "drill" | "center drill" | "spot drill" => (Shape::Drill, false),
        "tap right hand" | "tap left hand" => (Shape::Drill, false),
        "face mill" | "slot mill" => (Shape::Flat, true),
        _ => return Err(()),
    })
}

fn unit_factor(u: &str) -> Option<f64> {
    Some(match u {
        "millimeters" => 1.0,
        "centimeters" => 10.0,
        "meters" => 1000.0,
        "inches" => INCH,
        "feet" => 12.0 * INCH,
        _ => return None,
    })
}

fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(limits::MAX_NAME_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

struct Ctx<'a> {
    diags: Vec<Diagnostic>,
    dropped: &'a mut BTreeMap<String, usize>,
    label: String,
}

impl Ctx<'_> {
    fn warn(&mut self, code: &str, msg: &str) {
        self.diags
            .push(Diagnostic::warning(code, format!("{}: {msg}", self.label)));
    }

    /// A number in `0..=max`, times `factor`. A bad value is a warning
    /// and `None`; a missing key is `None`.
    fn num(&mut self, obj: &Map<String, Value>, key: &str, factor: f64, max: f64) -> Option<f64> {
        let v = obj.get(key)?;
        let Some(x) = v.as_f64().map(|x| x * factor).filter(|x| x.is_finite()) else {
            self.warn(
                "import.tool.bad_value",
                &format!("`{key}` is not a number; dropped"),
            );
            return None;
        };
        if !(0.0..=max).contains(&x) {
            self.warn(
                "import.tool.bad_value",
                &format!("`{key}` is out of range ({x}); dropped"),
            );
            return None;
        }
        Some(x)
    }

    fn drop_unused(&mut self, prefix: &str, obj: &Map<String, Value>, used: &[&str]) {
        for k in obj.keys() {
            if !used.contains(&k.as_str()) {
                *self.dropped.entry(format!("{prefix}{k}")).or_default() += 1;
            }
        }
    }
}

fn obj<'a>(v: &'a Value, key: &str) -> Option<&'a Map<String, Value>> {
    v.get(key)?.as_object()
}

/// Converts one tool entry. `None` when the entry is left out; a warning
/// then says why.
fn convert(
    entry: &Value,
    index: usize,
    dropped: &mut BTreeMap<String, usize>,
) -> (Option<Tool>, Vec<Diagnostic>) {
    let top = entry.as_object();
    let desc = top
        .and_then(|o| o.get("description"))
        .and_then(Value::as_str)
        .map(clean)
        .unwrap_or_default();
    let label = if desc.is_empty() {
        format!("tool {}", index + 1)
    } else {
        format!("tool {} \"{desc}\"", index + 1)
    };
    let mut c = Ctx {
        diags: Vec::new(),
        dropped,
        label,
    };
    let Some(top) = top else {
        c.warn("import.tool.skipped", "the entry is not an object; skipped");
        return (None, c.diags);
    };
    let kind = top
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let Ok((shape, approx)) = shape_of(&kind) else {
        c.warn(
            "import.tool.unsupported_type",
            &format!(
                "the type `{}` has no FluutCAM tool shape; skipped",
                clean(&kind)
            ),
        );
        return (None, c.diags);
    };
    if approx {
        c.warn(
            "import.tool.approximated",
            &format!(
                "`{kind}` is imported as a {} tool; check its size",
                shape.name()
            ),
        );
    }
    let unit = top
        .get("unit")
        .and_then(Value::as_str)
        .unwrap_or("millimeters");
    let Some(f) = unit_factor(unit) else {
        c.warn(
            "import.tool.bad_value",
            &format!("unit `{}` is not known; skipped", clean(unit)),
        );
        return (None, c.diags);
    };
    if !top.contains_key("unit") {
        c.warn("import.tool.bad_value", "no `unit`; millimeters assumed");
    }
    let empty = Map::new();
    let geo = obj(entry, "geometry").unwrap_or(&empty);
    let Some(diameter) = c.num(geo, "DC", f, MAX_LEN).filter(|d| *d > 0.0) else {
        c.warn(
            "import.tool.bad_value",
            "no usable diameter `geometry.DC`; skipped",
        );
        return (None, c.diags);
    };
    let corner = c.num(geo, "RE", f, MAX_LEN);
    let flute = c.num(geo, "LCF", f, MAX_LEN).filter(|x| *x > 0.0);
    let overall = c.num(geo, "OAL", f, MAX_LEN).filter(|x| *x > 0.0);
    let shank = c.num(geo, "SFDM", f, MAX_LEN).filter(|x| *x > 0.0);
    let ta = c.num(geo, "TA", 1.0, 90.0);
    let sig = c.num(geo, "SIG", 1.0, 180.0).filter(|x| *x > 0.0);
    let pitch = c.num(geo, "TP", f, MAX_LEN).filter(|x| *x > 0.0);
    let tip = c.num(geo, "tip-diameter", f, MAX_LEN);
    c.drop_unused(
        "geometry.",
        geo,
        &[
            "DC",
            "RE",
            "LCF",
            "OAL",
            "SFDM",
            "TA",
            "SIG",
            "TP",
            "tip-diameter",
        ],
    );

    let mut t = Tool {
        id: String::new(),
        name: desc,
        number: 0,
        shape,
        diameter,
        corner_radius: None,
        flute_length: flute,
        half_angle: None,
        tip_diameter: None,
        point_angle: None,
        pitch: None,
        overall_length: overall,
        shank_diameter: shank,
        holder: None,
        presets: Vec::new(),
    };
    match shape {
        Shape::Bull => {
            t.corner_radius = corner.filter(|r| *r > 0.0);
            if t.corner_radius.is_none() {
                c.warn(
                    "import.tool.approximated",
                    "no corner radius `RE`; imported as a flat tool",
                );
                t.shape = Shape::Flat;
            }
        }
        Shape::V | Shape::Taper => {
            let half = if kind == "counter sink" {
                sig.map(|s| s / 2.0)
            } else {
                ta
            };
            let Some(a) = half.filter(|a| *a > 0.0 && *a < 90.0) else {
                c.warn(
                    "import.tool.bad_value",
                    "no usable taper angle `TA`; skipped",
                );
                return (None, c.diags);
            };
            t.half_angle = Some(a);
            t.tip_diameter = tip.filter(|x| *x < diameter);
            if shape == Shape::Taper {
                t.corner_radius = corner.filter(|r| *r > 0.0);
            }
        }
        Shape::Drill => {
            t.point_angle = sig;
            if kind.starts_with("tap") {
                t.pitch = pitch;
            }
        }
        Shape::Flat | Shape::Ball => {}
    }

    // Post-process: the tool number.
    if let Some(pp) = obj(entry, "post-process") {
        if let Some(n) = pp.get("number") {
            match n.as_u64().filter(|n| *n <= 99_999) {
                Some(n) => t.number = n as u32,
                None => c.warn(
                    "import.tool.bad_value",
                    "`post-process.number` is not 0 to 99999; set to 0",
                ),
            }
        }
        c.drop_unused("post-process.", pp, &["number"]);
    }

    // Holder: the widest segment diameter, the total height.
    if let Some(h) = obj(entry, "holder") {
        let hf = h
            .get("unit")
            .and_then(Value::as_str)
            .and_then(unit_factor)
            .unwrap_or(f);
        let segs = h.get("segments").and_then(Value::as_array);
        let (mut dia, mut len) = (0.0_f64, 0.0_f64);
        let mut ok = true;
        for s in segs.into_iter().flatten().take(limits::MAX_SEGMENTS) {
            let g = |k: &str| s.get(k).and_then(Value::as_f64).map(|x| x * hf);
            match (g("height"), g("lower-diameter"), g("upper-diameter")) {
                (Some(hh), Some(a), Some(b))
                    if [hh, a, b]
                        .iter()
                        .all(|x| x.is_finite() && (0.0..=MAX_LEN).contains(x)) =>
                {
                    len += hh;
                    dia = dia.max(a).max(b);
                }
                _ => ok = false,
            }
        }
        if ok && dia > 0.0 && len > 0.0 {
            t.holder = Some((dia, len));
        } else {
            c.warn(
                "import.tool.holder_dropped",
                "the holder has no usable segments; dropped",
            );
        }
        c.drop_unused("holder.", h, &["segments", "unit"]);
    }
    if t.overall_length.is_none() {
        c.warn(
            "import.tool.length_missing",
            "no overall length `OAL`; FluutCAM warns `cam.tool.length_missing` where the length is needed",
        );
    }

    // Presets.
    if let Some(sv) = obj(entry, "start-values") {
        let list = sv.get("presets").and_then(Value::as_array);
        let mut seen = BTreeSet::new();
        for (i, p) in list.into_iter().flatten().enumerate() {
            if i >= limits::MAX_PRESETS {
                c.warn(
                    "import.preset.dropped",
                    &format!(
                        "more than {} presets; the rest dropped",
                        limits::MAX_PRESETS
                    ),
                );
                break;
            }
            let Some(po) = p.as_object() else {
                c.warn(
                    "import.preset.dropped",
                    &format!("preset {} is not an object; dropped", i + 1),
                );
                continue;
            };
            let name = po
                .get("name")
                .and_then(Value::as_str)
                .map(clean)
                .unwrap_or_default();
            let material = if name.is_empty() {
                "default".to_owned()
            } else {
                name
            };
            if !seen.insert(material.clone()) {
                c.warn(
                    "import.preset.dropped",
                    &format!("a second preset named \"{material}\"; dropped"),
                );
                continue;
            }
            let pr = Preset {
                rpm: c.num(po, "n", 1.0, limits::MAX_RPM),
                feed: c.num(po, "v_f", f, limits::MAX_FEED_MM_MIN),
                plunge: c.num(po, "v_f_plunge", f, limits::MAX_FEED_MM_MIN),
                stepover: c.num(po, "stepover", f, MAX_LEN),
                step_down: c.num(po, "stepdown", f, MAX_LEN),
                material,
            };
            c.drop_unused(
                "start-values.presets.",
                po,
                &["name", "n", "v_f", "v_f_plunge", "stepover", "stepdown"],
            );
            t.presets.push(pr);
        }
        c.drop_unused("start-values.", sv, &["presets"]);
    }
    c.drop_unused(
        "",
        top,
        &[
            "type",
            "description",
            "unit",
            "geometry",
            "post-process",
            "holder",
            "start-values",
        ],
    );
    (Some(t), c.diags)
}

fn err(code: &str, msg: impl Into<String>) -> Vec<Diagnostic> {
    vec![Diagnostic::error(code, msg)]
}

/// Converts Fusion tool-library JSON bytes.
///
/// `Err` holds the errors (the file is not JSON, is not a tool library,
/// is too large or holds too many tools). Other problems are warnings in
/// the result.
pub fn import(bytes: &[u8], opts: &ImportOptions) -> Result<ImportResult, Vec<Diagnostic>> {
    if bytes.len() as u64 > limits::MAX_FILE_BYTES {
        return Err(err(
            "import.file.too_large",
            format!(
                "the file is larger than {} MiB",
                limits::MAX_FILE_BYTES >> 20
            ),
        ));
    }
    if !crate::valid_library_id(&opts.library_id) {
        return Err(err(
            "import.library.bad_id",
            format!(
                "`{}` is not a library id (lower-case letters, digits, `-`, `_`, `/`)",
                opts.library_id
            ),
        ));
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes)
        .map_err(|_| err("import.file.invalid", "the file is not UTF-8 text"))?;
    let root: Value = serde_json::from_str(text).map_err(|e| {
        err(
            "import.file.invalid",
            format!("the file is not valid JSON: {e}"),
        )
    })?;
    let (entries, version) = match &root {
        Value::Array(a) => (a, None),
        Value::Object(o) => match o.get("data").and_then(Value::as_array) {
            Some(a) => (a, o.get("version").and_then(Value::as_u64)),
            None => {
                return Err(err(
                    "import.file.invalid",
                    "no `data` list: this is not a Fusion tool library",
                ));
            }
        },
        _ => return Err(err("import.file.invalid", "the JSON is not a tool library")),
    };
    if entries.len() > limits::MAX_TOOLS {
        return Err(err(
            "import.limit.tools",
            format!(
                "{} tools; the limit is {}",
                entries.len(),
                limits::MAX_TOOLS
            ),
        ));
    }
    let mut report = ImportReport {
        tools_read: entries.len(),
        ..ImportReport::default()
    };
    let mut diags = Vec::new();
    if version.is_none() {
        diags.push(Diagnostic::warning(
            "import.file.version",
            "no library `version`; read as a plain tool list",
        ));
    }
    let mut tools: Vec<Tool> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let (t, d) = convert(e, i, &mut report.dropped_keys);
        diags.extend(d);
        match t {
            Some(t) => tools.push(t),
            None => report.tools_skipped += 1,
        }
    }
    // Ids and numbers.
    let mut ids = BTreeSet::new();
    let mut numbers: BTreeMap<u32, usize> = BTreeMap::new();
    for t in &mut tools {
        let base = if t.name.is_empty() {
            format!(
                "{}-{}",
                t.shape.name(),
                num(t.diameter).trim_end_matches(".0")
            )
        } else {
            t.name.clone()
        };
        let mut id = slug(&base, 40, "tool");
        if t.number > 0 {
            id = format!("t{}-{id}", t.number);
        }
        let first = id.clone();
        let mut n = 2;
        while !ids.insert(id.clone()) {
            id = format!("{first}-{n}");
            n += 1;
        }
        if id != first {
            diags.push(Diagnostic::warning(
                "import.tool.id_duplicate",
                format!("two tools make the id `{first}`; the next is `{id}`"),
            ));
        }
        t.id = id;
        if t.number > 0 {
            *numbers.entry(t.number).or_default() += 1;
        }
    }
    for (n, k) in &numbers {
        if *k > 1 {
            diags.push(Diagnostic::warning(
                "import.tool.number_duplicate",
                format!("tool number {n} is used by {k} tools; all kept"),
            ));
        }
    }
    report.tools_written = tools.len();
    if !report.dropped_keys.is_empty() {
        let total: usize = report.dropped_keys.values().sum();
        let mut top: Vec<_> = report.dropped_keys.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let list: Vec<String> = top
            .iter()
            .take(8)
            .map(|(k, n)| format!("{k} x{n}"))
            .collect();
        diags.push(Diagnostic::warning(
            "import.fields.dropped",
            format!(
                "{total} source fields of {} kinds were not used: {}{}",
                top.len(),
                list.join(", "),
                if top.len() > 8 { ", ..." } else { "" }
            ),
        ));
    }
    Ok(ImportResult {
        library: write(&tools, opts),
        diagnostics: diags,
        report,
    })
}

fn write(tools: &[Tool], opts: &ImportOptions) -> String {
    let mut o = String::new();
    let _ = writeln!(
        o,
        "# Imported from an Autodesk Fusion tool library (fluut tools import)."
    );
    let _ = writeln!(o, "format  = \"fluut.tools/1\"");
    let _ = writeln!(o, "id      = {}", string(&opts.library_id));
    let _ = writeln!(o, "version = \"1.0\"");
    let _ = writeln!(o, "name    = {}", string(&opts.library_name));
    for t in tools {
        let _ = writeln!(o, "\n[[tool]]");
        let _ = writeln!(o, "id       = {}", string(&t.id));
        let _ = writeln!(o, "name     = {}", string(&t.name));
        let _ = writeln!(o, "number   = {}", t.number);
        let _ = writeln!(o, "shape    = \"{}\"", t.shape.name());
        let _ = writeln!(o, "diameter = {}", num(t.diameter));
        for (k, v) in [
            ("corner_radius", t.corner_radius),
            ("flute_length", t.flute_length),
            ("half_angle_deg", t.half_angle),
            ("tip_diameter", t.tip_diameter),
            ("point_angle_deg", t.point_angle),
            ("pitch", t.pitch),
            ("overall_length", t.overall_length),
            ("shank_diameter", t.shank_diameter),
        ] {
            if let Some(v) = v {
                let _ = writeln!(o, "{k} = {}", num(v));
            }
        }
        if let Some((d, l)) = t.holder {
            let _ = writeln!(
                o,
                "holder = {{ diameter = {}, length = {} }}",
                num(d),
                num(l)
            );
        }
        for p in &t.presets {
            let _ = writeln!(o, "\n  [[tool.preset]]");
            let _ = writeln!(o, "  material = {}", string(&p.material));
            for (k, v) in [
                ("rpm", p.rpm),
                ("feed", p.feed),
                ("plunge", p.plunge),
                ("stepover", p.stepover),
                ("step_down", p.step_down),
            ] {
                if let Some(v) = v {
                    let _ = writeln!(o, "  {k} = {}", num(v));
                }
            }
        }
    }
    o
}
