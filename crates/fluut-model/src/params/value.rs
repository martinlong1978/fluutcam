// SPDX-License-Identifier: MIT OR Apache-2.0

//! Parameter values as stored in the document (D15 §1.3–§1.4).

use super::meta::{FieldKind, FieldMeta};
use super::tables::HEIGHT_REFS;
use std::fmt::Write;

/// A number as written in the file. `int` remembers the TOML integer form,
/// so a file round-trips byte for byte (D06 §7.2).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Num {
    pub v: f64,
    pub int: bool,
}

impl Num {
    pub fn float(v: f64) -> Num {
        Num { v, int: false }
    }

    pub fn int(i: i64) -> Num {
        Num {
            v: i as f64,
            int: true,
        }
    }

    /// Shortest round-trip form; floats always show a decimal point or an
    /// exponent, so they read back as floats.
    pub fn to_toml(self) -> String {
        if self.int {
            format!("{}", self.v as i64)
        } else {
            format!("{:?}", self.v)
        }
    }

    fn from_toml(v: &toml::Value) -> Result<Num, String> {
        match v {
            toml::Value::Integer(i) => Ok(Num::int(*i)),
            toml::Value::Float(x) if x.is_finite() => Ok(Num::float(*x)),
            toml::Value::Float(_) => Err("expected a finite number".into()),
            _ => Err(format!("expected a number, found {}", v.type_str())),
        }
    }
}

/// One selected entity (D15 §2.3.1).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Sel {
    Body(String),
    Face(String),
    Edge(String),
}

impl Sel {
    pub fn id(&self) -> &str {
        match self {
            Sel::Body(s) | Sel::Face(s) | Sel::Edge(s) => s,
        }
    }

    fn key(&self) -> &'static str {
        match self {
            Sel::Body(_) => "body",
            Sel::Face(_) => "face",
            Sel::Edge(_) => "edge",
        }
    }
}

/// `{ ref, offset }` (D15 §2.4).
#[derive(Clone, PartialEq, Debug)]
pub struct HeightSpec {
    pub reference: String,
    pub offset: Option<Num>,
}

impl HeightSpec {
    pub fn new(reference: &str, offset: f64) -> HeightSpec {
        HeightSpec {
            reference: reference.to_owned(),
            offset: (offset != 0.0).then_some(Num::float(offset)),
        }
    }

    pub fn offset(&self) -> f64 {
        self.offset.map_or(0.0, |n| n.v)
    }
}

/// A stored parameter value.
#[derive(Clone, PartialEq, Debug)]
pub enum PValue {
    Num(Num),
    Bool(bool),
    Enum(String),
    Height(HeightSpec),
    Selection(Vec<Sel>),
    Point([Num; 3]),
    Points(Vec<[Num; 3]>),
    Table(ParamMap),
}

impl PValue {
    pub fn num(&self) -> Option<f64> {
        match self {
            PValue::Num(n) => Some(n.v),
            _ => None,
        }
    }
}

/// Values of one group (or inline table), in field order. Absent keys are
/// not stored.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ParamMap {
    entries: Vec<(&'static str, PValue)>,
}

/// Whether unknown keys are errors or warnings (D15 §1.4).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Unknown {
    Error,
    Warn,
}

impl ParamMap {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&PValue> {
        self.entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &PValue)> {
        self.entries.iter().map(|(k, v)| (*k, v))
    }

    /// Sets or clears `key`, keeping field order. `fields` lists the fields
    /// this map may hold.
    pub(crate) fn put<'a>(
        &mut self,
        fields: impl Iterator<Item = &'a FieldMeta>,
        key: &str,
        value: Option<PValue>,
    ) -> Result<(), String> {
        let order: Vec<&'static str> = fields.map(|m| m.key).collect();
        let Some(&k) = order.iter().find(|&&k| k == key) else {
            return Err(format!("unknown key `{key}`"));
        };
        self.entries.retain(|(e, _)| *e != k);
        if let Some(v) = value {
            self.entries.push((k, v));
            let pos = |k: &str| order.iter().position(|&o| o == k).unwrap_or(usize::MAX);
            self.entries.sort_by_key(|(k, _)| pos(k));
        }
        Ok(())
    }

    /// Reads a TOML table against `fields`. `path` prefixes messages.
    pub(crate) fn from_toml<'a>(
        fields: impl Iterator<Item = &'a FieldMeta> + Clone,
        t: &toml::Table,
        path: &str,
        unknown: Unknown,
        known_elsewhere: &dyn Fn(&str) -> bool,
        warnings: &mut Vec<String>,
    ) -> Result<ParamMap, String> {
        for k in t.keys() {
            if fields.clone().any(|m| m.key == k) {
                continue;
            }
            if known_elsewhere(k) {
                return Err(format!("`{path}.{k}` is not used by this toolpath type"));
            }
            match unknown {
                Unknown::Error => return Err(format!("unknown key `{path}.{k}`")),
                Unknown::Warn => warnings.push(format!("unknown key `{path}.{k}` ignored")),
            }
        }
        let mut entries = Vec::new();
        for m in fields {
            if let Some(v) = t.get(m.key) {
                let p = format!("{path}.{}", m.key);
                let val =
                    convert(m, v, &p, unknown, warnings).map_err(|e| format!("`{p}`: {e}"))?;
                entries.push((m.key, val));
            }
        }
        Ok(ParamMap { entries })
    }

    /// `{ k = v, … }` for an inline table.
    pub(crate) fn inline(&self) -> String {
        if self.entries.is_empty() {
            return "{}".into();
        }
        let parts: Vec<String> = self
            .entries
            .iter()
            .map(|(k, v)| format!("{k} = {}", write_value(v)))
            .collect();
        format!("{{ {} }}", parts.join(", "))
    }

    /// `key = value` lines with aligned `=`.
    pub(crate) fn write_lines(&self, out: &mut String, indent: &str) {
        let w = self.entries.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        for (k, v) in &self.entries {
            let _ = writeln!(out, "{indent}{k:<w$} = {}", write_value(v));
        }
    }
}

fn convert(
    m: &FieldMeta,
    v: &toml::Value,
    path: &str,
    unknown: Unknown,
    warnings: &mut Vec<String>,
) -> Result<PValue, String> {
    use toml::Value as V;
    let found = || format!("found {}", v.type_str());
    Ok(match m.kind {
        FieldKind::Length
        | FieldKind::Feed
        | FieldKind::Speed
        | FieldKind::Angle
        | FieldKind::Duration => PValue::Num(Num::from_toml(v)?),
        FieldKind::Count => match v {
            V::Integer(i) => PValue::Num(Num::int(*i)),
            _ => return Err(format!("expected an integer, {}", found())),
        },
        FieldKind::Bool => match v {
            V::Boolean(b) => PValue::Bool(*b),
            _ => return Err(format!("expected true or false, {}", found())),
        },
        FieldKind::Enum(allowed) => match v {
            V::String(s) if allowed.contains(&s.as_str()) => PValue::Enum(s.clone()),
            V::String(s) => {
                return Err(format!("`{s}` is not one of {}", allowed.join(", ")));
            }
            _ => return Err(format!("expected a string, {}", found())),
        },
        FieldKind::Height => PValue::Height(height(v)?),
        FieldKind::Selection => PValue::Selection(selection(v)?),
        FieldKind::Point => PValue::Point(point(v)?),
        FieldKind::Points => match v {
            V::Array(a) => PValue::Points(a.iter().map(point).collect::<Result<_, _>>()?),
            _ => return Err(format!("expected an array of points, {}", found())),
        },
        FieldKind::Table(sub) => match v {
            V::Table(t) => PValue::Table(ParamMap::from_toml(
                sub.iter(),
                t,
                path,
                unknown,
                &|_| false,
                warnings,
            )?),
            _ => return Err(format!("expected an inline table, {}", found())),
        },
    })
}

fn height(v: &toml::Value) -> Result<HeightSpec, String> {
    let toml::Value::Table(t) = v else {
        return Err(format!(
            "expected {{ ref, offset }}, found {}",
            v.type_str()
        ));
    };
    if let Some(k) = t.keys().find(|k| *k != "ref" && *k != "offset") {
        return Err(format!("unknown key `{k}` in a height"));
    }
    let reference = match t.get("ref") {
        Some(toml::Value::String(s)) if HEIGHT_REFS.contains(&s.as_str()) => s.clone(),
        Some(toml::Value::String(s)) => {
            return Err(format!(
                "`{s}` is not a height reference ({})",
                HEIGHT_REFS.join(", ")
            ));
        }
        _ => return Err("a height needs `ref`".into()),
    };
    let offset = t.get("offset").map(Num::from_toml).transpose()?;
    Ok(HeightSpec { reference, offset })
}

fn selection(v: &toml::Value) -> Result<Vec<Sel>, String> {
    let toml::Value::Array(items) = v else {
        return Err(format!("expected an array, found {}", v.type_str()));
    };
    items
        .iter()
        .map(|it| {
            let t = it
                .as_table()
                .filter(|t| t.len() == 1)
                .ok_or("each item must be { body | face | edge = id }")?;
            let (k, id) = t.iter().next().ok_or("empty item")?;
            let id = id.as_str().ok_or("ids are strings")?.to_owned();
            match k.as_str() {
                "body" => Ok(Sel::Body(id)),
                "face" => Ok(Sel::Face(id)),
                "edge" => Ok(Sel::Edge(id)),
                _ => Err(format!("`{k}` is not body, face or edge")),
            }
        })
        .collect()
}

fn point(v: &toml::Value) -> Result<[Num; 3], String> {
    match v.as_array().map(Vec::as_slice) {
        Some([x, y, z]) => Ok([Num::from_toml(x)?, Num::from_toml(y)?, Num::from_toml(z)?]),
        _ => Err("expected a point [x, y, z]".into()),
    }
}

/// TOML basic string.
pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn write_point(p: &[Num; 3]) -> String {
    format!(
        "[{}, {}, {}]",
        p[0].to_toml(),
        p[1].to_toml(),
        p[2].to_toml()
    )
}

pub(crate) fn write_value(v: &PValue) -> String {
    match v {
        PValue::Num(n) => n.to_toml(),
        PValue::Bool(b) => b.to_string(),
        PValue::Enum(s) => quote(s),
        PValue::Height(h) => match h.offset {
            None => format!("{{ ref = {} }}", quote(&h.reference)),
            Some(o) => format!(
                "{{ ref = {}, offset = {} }}",
                quote(&h.reference),
                o.to_toml()
            ),
        },
        PValue::Selection(items) => {
            let parts: Vec<String> = items
                .iter()
                .map(|s| format!("{{ {} = {} }}", s.key(), quote(s.id())))
                .collect();
            format!("[{}]", parts.join(", "))
        }
        PValue::Point(p) => write_point(p),
        PValue::Points(ps) => {
            let parts: Vec<String> = ps.iter().map(write_point).collect();
            format!("[{}]", parts.join(", "))
        }
        PValue::Table(m) => m.inline(),
    }
}
