// SPDX-License-Identifier: MIT OR Apache-2.0

//! Post block merging (D10 §2, spec 05 §5.2.2) and `flatten`.
//!
//! A block in a layer is `{ mode?, lines?, replace_line?, remove_line?,
//! before?, after? }`. Each line is a string or `{ id?, when?, line }`. The
//! merged block is `{ lines = [...] }`, every line a table that also records
//! the layer that supplied or last changed it (`layer`), for `explain`.

use fluut_model::{DefError, DefRef, DefStore, LayerId, ModeMerger, Resolved, resolve};
use std::collections::BTreeSet;
use toml::{Table, Value};

#[cfg(test)]
mod tests;

/// The schema name and major version of native post files (spec 02 §2).
pub const POST_FORMAT: &str = "fluut.post/1";

const LINE_KEYS: [&str; 4] = ["id", "when", "line", "layer"];

/// The [`ModeMerger`] for posts: handles every `blocks.<name>` table.
#[derive(Debug, Default)]
pub struct PostMerger;

fn is_block_path(path: &str) -> bool {
    path.strip_prefix("blocks.")
        .is_some_and(|name| !name.is_empty() && !name.contains('.'))
}

impl ModeMerger for PostMerger {
    fn handles(&self, path: &str) -> bool {
        is_block_path(path)
    }

    fn merge_mode(
        &self,
        path: &str,
        parent: Option<&Value>,
        child: &Table,
        layer: &LayerId,
    ) -> Result<Value, DefError> {
        let err = |msg: String| DefError::Merge {
            layer: layer.clone(),
            path: path.to_owned(),
            msg,
        };
        if !is_block_path(path) {
            return Err(err("`mode` is only allowed on `blocks.<name>`".into()));
        }
        let parent_lines = match parent {
            None => Vec::new(),
            Some(p) => p
                .get("lines")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|l| line_table(&l, None).map_err(err))
                .collect::<Result<Vec<_>, _>>()?,
        };
        let mode = match child.get("mode") {
            None => "replace",
            Some(Value::String(m)) => m.as_str(),
            Some(v) => return Err(err(format!("`mode` must be a string, not {v}"))),
        };
        let allowed: &[&str] = if mode == "patch" {
            &["mode", "replace_line", "remove_line", "before", "after"]
        } else {
            &["mode", "lines"]
        };
        if let Some(k) = child.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(err(format!("`{k}` is not allowed with mode `{mode}`")));
        }
        let own = || -> Result<Vec<Table>, DefError> {
            match child.get("lines") {
                None => Ok(Vec::new()),
                Some(v) => lines_of(v, layer).map_err(err),
            }
        };
        let lines = match mode {
            "replace" => own()?,
            "append" => [parent_lines, own()?].concat(),
            "prepend" => [own()?, parent_lines].concat(),
            "remove" => Vec::new(),
            "patch" => patch(parent_lines, child, layer).map_err(err)?,
            m => {
                return Err(err(format!(
                    "unknown mode `{m}` (replace, append, prepend, remove, patch)"
                )));
            }
        };
        let mut ids = BTreeSet::new();
        for l in &lines {
            if let Some(id) = l.get("id").and_then(Value::as_str)
                && !ids.insert(id)
            {
                return Err(err(format!("duplicate line id `{id}`")));
            }
        }
        let mut out = Table::new();
        out.insert(
            "lines".into(),
            Value::Array(lines.into_iter().map(Value::Table).collect()),
        );
        Ok(Value::Table(out))
    }
}

/// Normalises one line to a table. `layer`, if given, replaces any recorded
/// layer.
fn line_table(v: &Value, layer: Option<&LayerId>) -> Result<Table, String> {
    let mut t = match v {
        Value::String(s) => {
            let mut t = Table::new();
            t.insert("line".into(), Value::String(s.clone()));
            t
        }
        Value::Table(t) => {
            if let Some(k) = t.keys().find(|k| !LINE_KEYS.contains(&k.as_str())) {
                return Err(format!("unknown line key `{k}`"));
            }
            for k in LINE_KEYS {
                if let Some(x) = t.get(k)
                    && !x.is_str()
                {
                    return Err(format!("line `{k}` must be a string"));
                }
            }
            if !t.contains_key("line") {
                return Err("line table without `line`".into());
            }
            t.clone()
        }
        _ => return Err(format!("a line must be a string or table, not {v}")),
    };
    if let Some(l) = layer {
        t.insert("layer".into(), Value::String(l.0.clone()));
    }
    Ok(t)
}

fn lines_of(v: &Value, layer: &LayerId) -> Result<Vec<Table>, String> {
    v.as_array()
        .ok_or_else(|| "`lines` must be an array".to_owned())?
        .iter()
        .map(|l| line_table(l, Some(layer)))
        .collect()
}

fn position(lines: &[Table], id: &str) -> Result<usize, String> {
    lines
        .iter()
        .position(|l| l.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| format!("no line with id `{id}`"))
}

fn id_table<'a>(child: &'a Table, key: &str) -> Result<Option<&'a Table>, String> {
    match child.get(key) {
        None => Ok(None),
        Some(Value::Table(t)) => Ok(Some(t)),
        Some(_) => Err(format!("`{key}` must be a table of line id = value")),
    }
}

/// Applies `replace_line`, `remove_line`, `before`, `after` in that order
/// (D10 §2).
fn patch(mut lines: Vec<Table>, child: &Table, layer: &LayerId) -> Result<Vec<Table>, String> {
    if let Some(t) = id_table(child, "replace_line")? {
        for (id, text) in t {
            let i = position(&lines, id)?;
            let text = text
                .as_str()
                .ok_or_else(|| format!("replace_line.{id} must be a string"))?;
            lines[i].insert("line".into(), Value::String(text.to_owned()));
            lines[i].insert("layer".into(), Value::String(layer.0.clone()));
        }
    }
    if let Some(v) = child.get("remove_line") {
        let ids = v
            .as_array()
            .ok_or_else(|| "`remove_line` must be an array of ids".to_owned())?;
        for id in ids {
            let id = id
                .as_str()
                .ok_or_else(|| "`remove_line` must be an array of ids".to_owned())?;
            let i = position(&lines, id)?;
            lines.remove(i);
        }
    }
    for (key, offset) in [("before", 0), ("after", 1)] {
        if let Some(t) = id_table(child, key)? {
            for (id, new) in t {
                let i = position(&lines, id)? + offset;
                let new = lines_of(new, layer)?;
                lines.splice(i..i, new);
            }
        }
    }
    Ok(lines)
}

/// Resolves a post and applies machine overrides, if any (spec 05 §5.2.2,
/// §5.2.3).
pub fn resolve_post(
    store: &dyn DefStore,
    r: &DefRef,
    overrides: Option<(&Table, &LayerId)>,
) -> Result<Resolved, DefError> {
    let mut out = resolve(store, r, &PostMerger)?;
    if let Some((ov, layer)) = overrides {
        fluut_model::apply_overrides(&mut out, ov, layer)?;
    }
    Ok(out)
}

/// The resolved post as a standalone post file: no `extends`, no mixins, no
/// `mode` keys; line ids and conditions kept (spec 05 §5.2.4).
pub fn flatten(resolved: &Resolved, id: &str, version: &str) -> String {
    let mut t = Table::new();
    t.insert("format".into(), Value::String(POST_FORMAT.into()));
    t.insert("id".into(), Value::String(id.into()));
    t.insert("version".into(), Value::String(version.into()));
    let mut body = resolved.table.clone();
    if let Some(Value::Table(blocks)) = body.get_mut("blocks") {
        for (_, b) in blocks.iter_mut() {
            let Some(Value::Array(lines)) = b.get_mut("lines") else {
                continue;
            };
            for l in lines.iter_mut() {
                if let Value::Table(lt) = l {
                    lt.remove("layer");
                    if lt.len() == 1
                        && let Some(Value::String(s)) = lt.get("line")
                    {
                        *l = Value::String(s.clone());
                    }
                }
            }
        }
    }
    t.extend(body);
    toml::to_string(&t).expect("a TOML table always serialises")
}
