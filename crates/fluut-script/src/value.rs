// SPDX-License-Identifier: MIT OR Apache-2.0

//! File values ⇄ script values, one to one (D12b §1).

use crate::api::{BodyH, FaceH};
use fluut_model::params::{FieldKind, FieldMeta, Params, ToolpathTypeId};
use rhai::{Array, Dynamic, FLOAT, INT, ImmutableString, Map};
use toml::Value;

/// A TOML value as a script value.
pub fn to_dynamic(v: &Value) -> Dynamic {
    match v {
        Value::String(s) => s.clone().into(),
        Value::Integer(i) => (*i as INT).into(),
        Value::Float(x) => (*x as FLOAT).into(),
        Value::Boolean(b) => (*b).into(),
        Value::Datetime(d) => d.to_string().into(),
        Value::Array(a) => Dynamic::from_array(a.iter().map(to_dynamic).collect()),
        Value::Table(t) => {
            let m: Map = t
                .iter()
                .map(|(k, v)| (k.as_str().into(), to_dynamic(v)))
                .collect();
            Dynamic::from_map(m)
        }
    }
}

/// A length with a unit (`"6mm"`, `"0.25in"`) in mm.
pub fn unit_length(s: &str) -> Option<f64> {
    let t = s.trim();
    let (num, scale) = match t.strip_suffix("mm") {
        Some(n) => (n, 1.0),
        None => (t.strip_suffix("in")?, 25.4),
    };
    let n = num.trim();
    // A number only: no sign-only, no `inf` or `nan` spelled out.
    if n.is_empty() || !n.chars().all(|c| c.is_ascii_digit() || "+-.eE".contains(c)) {
        return None;
    }
    let v: f64 = n.parse().ok()?;
    let mm = v * scale;
    mm.is_finite().then_some(mm)
}

fn type_name(d: &Dynamic) -> &str {
    if d.is::<FaceH>() {
        "Face"
    } else if d.is::<BodyH>() {
        "Body"
    } else {
        d.type_name()
    }
}

/// A script value as a TOML value; `None` for `()`. Strings with a length
/// unit become numbers when `units` is set; faces and bodies become their
/// ids.
pub fn to_toml(d: &Dynamic, units: bool) -> Result<Option<Value>, String> {
    if d.is_unit() {
        return Ok(None);
    }
    let some = |v: Value| Ok(Some(v));
    if let Some(b) = d.clone().try_cast::<bool>() {
        return some(Value::Boolean(b));
    }
    if let Some(i) = d.clone().try_cast::<INT>() {
        return some(Value::Integer(i));
    }
    if let Some(x) = d.clone().try_cast::<FLOAT>() {
        if !x.is_finite() {
            return Err(format!("{x} is not a finite number"));
        }
        return some(Value::Float(x));
    }
    if let Some(c) = d.clone().try_cast::<char>() {
        return some(Value::String(c.to_string()));
    }
    if let Some(s) = d.clone().try_cast::<ImmutableString>() {
        if units && let Some(mm) = unit_length(&s) {
            return some(Value::Float(mm));
        }
        return some(Value::String(s.to_string()));
    }
    if let Some(f) = d.clone().try_cast::<FaceH>() {
        return some(Value::String(f.id));
    }
    if let Some(b) = d.clone().try_cast::<BodyH>() {
        return some(Value::String(b.id));
    }
    if d.is_array() {
        let a = d.clone().into_array().map_err(|t| t.to_string())?;
        let mut out = Vec::with_capacity(a.len());
        for x in &a {
            out.push(to_toml(x, units)?.ok_or("an array may not hold `()`")?);
        }
        return some(Value::Array(out));
    }
    if d.is_map() {
        let m = d.clone().cast::<Map>();
        let mut t = toml::Table::new();
        for (k, v) in &m {
            match to_toml(v, units)? {
                Some(x) => {
                    t.insert(k.to_string(), x);
                }
                None => return Err(format!("`{k}` is `()`; leave the key out instead")),
            }
        }
        return some(Value::Table(t));
    }
    Err(format!("a {} cannot be stored in a project", type_name(d)))
}

/// A toolpath parameter value: as [`to_toml`], and also by field kind:
/// plain number strings are mm in lengths, and selections take ids, faces
/// and bodies as well as the file's `{ face = "…" }` tables.
pub fn param_toml(kind: ToolpathTypeId, path: &str, d: &Dynamic) -> Result<Option<Value>, String> {
    match Params::meta(kind, path) {
        Some(m) => meta_toml(m, d),
        None => Err(format!("`{path}` is not a field of {kind}")),
    }
}

fn meta_toml(m: &FieldMeta, d: &Dynamic) -> Result<Option<Value>, String> {
    match m.kind {
        FieldKind::Length => {
            if let Some(s) = d.clone().try_cast::<ImmutableString>()
                && let Some(mm) =
                    unit_length(&s).or_else(|| fluut_model::project::length_entry(&s, mm_unit()))
            {
                return Ok(Some(Value::Float(mm)));
            }
            to_toml(d, true)
        }
        FieldKind::Selection if d.is_array() => {
            let a: Array = d.clone().into_array().map_err(|t| t.to_string())?;
            let mut out = Vec::with_capacity(a.len());
            for x in &a {
                out.push(selection_item(x)?);
            }
            Ok(Some(Value::Array(out)))
        }
        FieldKind::Table(sub) if d.is_map() => {
            let mm = d.clone().cast::<Map>();
            let mut t = toml::Table::new();
            for (k, v) in &mm {
                let v = match sub.iter().find(|s| s.key == k.as_str()) {
                    Some(s) => meta_toml(s, v)?,
                    // Unknown keys are reported by the model, in its words.
                    None => to_toml(v, true)?,
                };
                match v {
                    Some(v) => {
                        t.insert(k.to_string(), v);
                    }
                    None => return Err(format!("`{k}` is `()`; leave the key out instead")),
                }
            }
            Ok(Some(Value::Table(t)))
        }
        _ => to_toml(d, true),
    }
}

fn mm_unit() -> fluut_model::project::DisplayUnit {
    fluut_model::project::DisplayUnit::Mm
}

/// One selected entity: `"f-1.2"`, a Face or Body, or `#{ face: "f-1.2" }`.
fn selection_item(x: &Dynamic) -> Result<Value, String> {
    let one = |k: &str, id: String| {
        let mut t = toml::Table::new();
        t.insert(k.into(), Value::String(id));
        Ok(Value::Table(t))
    };
    if let Some(f) = x.clone().try_cast::<FaceH>() {
        return one("face", f.id);
    }
    if let Some(b) = x.clone().try_cast::<BodyH>() {
        return one("body", b.id);
    }
    if let Some(s) = x.clone().try_cast::<ImmutableString>() {
        let key = if s.starts_with("f-") {
            "face"
        } else if s.starts_with("e-") {
            "edge"
        } else if s.starts_with("b-") {
            "body"
        } else {
            return Err(format!(
                "`{s}` is not a face, edge or body id (`f-…`, `e-…`, `b-…`)"
            ));
        };
        return one(key, s.to_string());
    }
    to_toml(x, false)?.ok_or_else(|| "a selection may not hold `()`".into())
}

/// Inline TOML text of a value, for an edit.
pub fn inline(v: &Value) -> String {
    v.to_string()
}
