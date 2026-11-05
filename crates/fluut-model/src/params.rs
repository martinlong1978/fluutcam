// SPDX-License-Identifier: MIT OR Apache-2.0

//! Toolpath parameters: field metadata, stored values, load and save
//! (D06 §3, D15).
//!
//! Every group is stored as a [`ParamMap`] checked against the type's field
//! table. Load, save, validation and the result hash all walk the same
//! table, so they cannot disagree about which fields exist.

mod meta;
mod tables;
pub(crate) mod value;

#[cfg(test)]
mod tests;

pub use meta::{Class, ConstVal, DefaultRule, FieldKind, FieldMeta, Group, ToolpathTypeId};
pub use tables::{FEED_REFS, HEIGHT_REFS, X_REFS};
pub use value::{HeightSpec, Num, PValue, ParamMap, Sel};

use std::fmt::Write;
use value::Unknown;

/// All parameter groups of one toolpath.
#[derive(Clone, PartialEq, Debug)]
pub struct Params {
    kind: ToolpathTypeId,
    groups: [ParamMap; 5],
}

impl Params {
    /// No local values: every field takes its default.
    pub fn new(kind: ToolpathTypeId) -> Params {
        Params {
            kind,
            groups: Default::default(),
        }
    }

    pub fn kind(&self) -> ToolpathTypeId {
        self.kind
    }

    pub fn group(&self, g: Group) -> &ParamMap {
        &self.groups[g.index()]
    }

    /// The field at `path` (`group.key` or `group.key.sub`), if the type
    /// declares it.
    pub fn meta(kind: ToolpathTypeId, path: &str) -> Option<&'static FieldMeta> {
        let mut it = path.split('.');
        let g = Group::from_key(it.next()?)?;
        let key = it.next()?;
        let m = kind.group_fields(g).find(|m| m.key == key)?;
        match it.next() {
            None => Some(m),
            Some(sub) if it.next().is_none() => m.sub_fields().iter().find(|s| s.key == sub),
            Some(_) => None,
        }
    }

    /// Every leaf field of the type with its path, in canonical order.
    pub fn leaf_fields(kind: ToolpathTypeId) -> Vec<(String, &'static FieldMeta)> {
        let mut out = Vec::new();
        for m in kind.fields() {
            let base = format!("{}.{}", m.group.key(), m.key);
            match m.kind {
                FieldKind::Table(sub) => {
                    for s in sub {
                        out.push((format!("{base}.{}", s.key), s));
                    }
                }
                _ => out.push((base, m)),
            }
        }
        out
    }

    /// The local value at `path`, or `None` when absent.
    pub fn get(&self, path: &str) -> Option<&PValue> {
        let mut it = path.split('.');
        let g = Group::from_key(it.next()?)?;
        let v = self.groups[g.index()].get(it.next()?)?;
        match it.next() {
            None => Some(v),
            Some(sub) => match v {
                PValue::Table(t) => t.get(sub),
                _ => None,
            },
        }
    }

    /// The toolpath starts from the in-process stock of the earlier
    /// toolpaths in its Setup (`rest_from = "previous"`, D15 §2.3.2).
    pub fn reads_in_process(&self) -> bool {
        matches!(self.get("geometry.rest_from"), Some(PValue::Enum(s)) if s == "previous")
    }

    /// The result depends on the in-process stock: rest machining, or
    /// links that stop above it (D08 §8.2).
    pub fn reads_stock(&self) -> bool {
        self.reads_in_process() || self.kind.links_over_stock()
    }

    pub fn is_set(&self, path: &str) -> bool {
        self.get(path).is_some()
    }

    /// Sets (`Some`) or clears (`None`) a local value. The value must fit
    /// the field kind; ranges are checked by validation, not here.
    pub fn set(&mut self, path: &str, value: Option<PValue>) -> Result<(), String> {
        let meta = Params::meta(self.kind, path)
            .ok_or_else(|| format!("`{path}` is not a field of {}", self.kind))?;
        if let Some(v) = &value
            && !fits(meta, v)
        {
            return Err(format!("`{path}`: wrong kind of value"));
        }
        let mut it = path.split('.');
        let g = Group::from_key(it.next().unwrap_or_default()).ok_or("bad group")?;
        let key = it.next().unwrap_or_default();
        let fields = self.kind.group_fields(g);
        let map = &mut self.groups[g.index()];
        match it.next() {
            None => map.put(fields, key, value),
            Some(sub) => {
                let parent = self
                    .kind
                    .group_fields(g)
                    .find(|m| m.key == key)
                    .ok_or("bad key")?;
                let mut t = match map.get(key) {
                    Some(PValue::Table(t)) => t.clone(),
                    _ => ParamMap::default(),
                };
                t.put(parent.sub_fields().iter(), sub, value)?;
                let v = (!t.is_empty()).then_some(PValue::Table(t));
                map.put(fields, key, v)
            }
        }
    }

    /// Sets or clears the value at `path` from its file form. Unknown keys
    /// inside an inline table are errors here.
    pub fn set_toml(&mut self, path: &str, value: Option<&toml::Value>) -> Result<(), String> {
        let meta = Params::meta(self.kind, path)
            .ok_or_else(|| format!("`{path}` is not a field of {}", self.kind))?;
        let v = match value {
            None => None,
            Some(v) => {
                let mut w = Vec::new();
                Some(
                    value::convert(meta, v, path, Unknown::Error, &mut w)
                        .map_err(|e| format!("`{path}`: {e}"))?,
                )
            }
        };
        self.set(path, v)
    }

    /// Calls `f` on every selected entity; `f` may change its id.
    pub fn map_selections(&mut self, f: &mut dyn FnMut(&mut Sel)) {
        for g in &mut self.groups {
            g.visit_mut(&mut |v| {
                if let PValue::Selection(items) = v {
                    items.iter_mut().for_each(&mut *f);
                }
            });
        }
    }

    /// Every selected entity, in field order.
    pub fn selections(&self) -> Vec<(String, Sel)> {
        let mut out = Vec::new();
        for (path, _) in Params::leaf_fields(self.kind) {
            if let Some(PValue::Selection(items)) = self.get(&path) {
                out.extend(items.iter().map(|s| (path.clone(), s.clone())));
            }
        }
        out
    }

    /// Reads the group tables of a toolpath table. Other keys of `tp` are
    /// left to the caller.
    pub fn from_toml(
        kind: ToolpathTypeId,
        tp: &toml::Table,
        warnings: &mut Vec<String>,
    ) -> Result<Params, String> {
        let mut p = Params::new(kind);
        for g in Group::ALL {
            let Some(v) = tp.get(g.key()) else { continue };
            let t = v
                .as_table()
                .ok_or_else(|| format!("`{}` must be a table", g.key()))?;
            let unknown = if g == Group::Passes {
                Unknown::Error
            } else {
                Unknown::Warn
            };
            p.groups[g.index()] = ParamMap::from_toml(
                kind.group_fields(g),
                t,
                g.key(),
                unknown,
                &|k| tables::known_in_any_type(g, k),
                warnings,
            )?;
        }
        Ok(p)
    }

    /// Writes the non-empty group tables of a toolpath in a Setup.
    pub fn write(&self, out: &mut String, indent: &str) {
        for g in Group::ALL {
            let m = &self.groups[g.index()];
            if m.is_empty() {
                continue;
            }
            let _ = writeln!(out, "\n{indent}[setup.toolpath.{}]", g.key());
            m.write_lines(out, indent);
        }
    }

    /// Canonical text of every field of class `class`, one `path = value`
    /// line each, absent fields as `path = -`. The result hash uses the R
    /// lines (D15 §1.8).
    pub fn class_text(&self, class: Class) -> String {
        let mut out = String::new();
        for (path, m) in Params::leaf_fields(self.kind) {
            if m.class != class {
                continue;
            }
            match self.get(&path) {
                Some(v) => {
                    let _ = writeln!(out, "{path} = {}", value::write_value(v));
                }
                None => {
                    let _ = writeln!(out, "{path} = -");
                }
            }
        }
        out
    }
}

fn fits(meta: &FieldMeta, v: &PValue) -> bool {
    match (meta.kind, v) {
        (
            FieldKind::Length
            | FieldKind::Feed
            | FieldKind::Speed
            | FieldKind::Angle
            | FieldKind::Duration,
            PValue::Num(_),
        ) => true,
        (FieldKind::Count, PValue::Num(n)) => n.int,
        (FieldKind::Bool, PValue::Bool(_)) => true,
        (FieldKind::Text, PValue::Enum(_)) => true,
        (FieldKind::Enum(allowed), PValue::Enum(s)) => allowed.contains(&s.as_str()),
        (FieldKind::Height | FieldKind::XPos, PValue::Height(h)) => {
            meta.height_refs().contains(&h.reference.as_str())
        }
        (FieldKind::Selection, PValue::Selection(_)) => true,
        (FieldKind::Point, PValue::Point(_)) => true,
        (FieldKind::Points, PValue::Points(_)) => true,
        // Sub-fields were checked when the table was read.
        (FieldKind::Table(_), PValue::Table(_)) => true,
        _ => false,
    }
}
