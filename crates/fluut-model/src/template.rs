// SPDX-License-Identifier: MIT OR Apache-2.0

//! Toolpath and Setup templates (D06 §4.4, spec 03 §3.10.6, OQ-18).
//!
//! A template is a layered definition (`fluut.template/1`) resolved by the
//! generic merge. Applying it creates ordinary toolpaths or a Setup: later
//! template edits do not change them, and their `template` key only
//! records where they came from.
//!
//! ```toml
//! kind = "toolpath"
//! [toolpath]
//! type = "pocket_2d"
//! [toolpath.passes]
//! stepover = "0.4*tool.diameter"   # evaluated when applied
//! ```
//!
//! A Setup template has `kind = "setup"`, a `[setup]` table of Setup
//! fields (no `geometry`), and `[[toolpath]]` entries: inline toolpaths
//! (with `type`) or `{ template = "ref" }`, each with optional `name` and
//! `tool`.

use crate::defs::{DefRef, NoModes, Resolved, resolve};
use crate::edit::Edit;
use crate::expr::{eval, tool_names};
use crate::library::{DefKind, Library};
use crate::params::{FieldKind, Group, Params, ToolpathTypeId};
use crate::project::{Cx, Setup, Toolpath};
use crate::tools::ToolStore;
use toml::{Table, Value};

/// A template that cannot be applied.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[error("template {template}: {msg}")]
pub struct TemplateError {
    pub template: String,
    pub msg: String,
}

/// What a template is applied to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Target {
    /// A new toolpath in `setup` at `at` (`None` = last).
    Toolpath {
        setup: String,
        at: Option<usize>,
        id: String,
        name: Option<String>,
        tool: String,
    },
    /// A new Setup at `at`. Toolpath entries without a `tool` take `tool`.
    Setup {
        at: Option<usize>,
        id: String,
        name: Option<String>,
        tool: String,
    },
}

/// A resolved template.
#[derive(Clone, PartialEq, Debug)]
pub struct Template {
    /// `id@major.minor` of the template itself.
    pub layer: String,
    pub resolved: Resolved,
}

impl Template {
    pub fn kind(&self) -> Option<&str> {
        self.resolved.table.get("kind").and_then(Value::as_str)
    }
}

impl Library {
    /// Resolves a template and checks that every layer agrees on its kind
    /// and, for toolpath templates, its type.
    pub fn template(&self, r: &DefRef) -> Result<Template, TemplateError> {
        let fail = |msg: String| TemplateError {
            template: r.to_string(),
            msg,
        };
        let resolved = resolve(&self.store(DefKind::Template), r, &NoModes)
            .map_err(|e| fail(e.to_string()))?;
        let mut kind: Option<String> = None;
        let mut ty: Option<String> = None;
        for l in &resolved.layers {
            let lr = DefRef::parse(&l.0).map_err(|e| fail(e.to_string()))?;
            let Some(e) = self.find(DefKind::Template, &lr) else {
                continue;
            };
            let t = &e.def.table;
            if let Some(k) = t.get("kind").and_then(Value::as_str) {
                if kind.as_deref().is_some_and(|x| x != k) {
                    return Err(fail(format!("layer {l} changes the kind to `{k}`")));
                }
                kind = Some(k.to_owned());
            }
            if let Some(y) = t
                .get("toolpath")
                .and_then(Value::as_table)
                .and_then(|x| x.get("type"))
                .and_then(Value::as_str)
            {
                if ty.as_deref().is_some_and(|x| x != y) {
                    return Err(fail(format!(
                        "layer {l} changes the toolpath type to `{y}`"
                    )));
                }
                ty = Some(y.to_owned());
            }
        }
        match kind.as_deref() {
            Some("toolpath" | "setup") => {}
            Some(k) => return Err(fail(format!("`kind` is `{k}`, not toolpath or setup"))),
            None => return Err(fail("`kind` is missing".into())),
        }
        let layer = resolved
            .layers
            .last()
            .map(|l| l.0.clone())
            .unwrap_or_default();
        Ok(Template { layer, resolved })
    }

    /// The edit that applies template `r` to `target`. `new_id` makes ids
    /// for the toolpaths of a Setup template.
    pub fn apply_template(
        &self,
        r: &DefRef,
        target: &Target,
        tools: &ToolStore,
        new_id: &mut dyn FnMut() -> String,
    ) -> Result<Edit, TemplateError> {
        let t = self.template(r)?;
        let fail = |msg: String| TemplateError {
            template: r.to_string(),
            msg,
        };
        let edit = match (t.kind(), target) {
            (
                Some("toolpath"),
                Target::Toolpath {
                    setup,
                    at,
                    id,
                    name,
                    tool,
                },
            ) => {
                let tp = instantiate(&t, id, name.as_deref(), tool, tools)?;
                Edit::add_toolpath(setup, *at, &tp)
            }
            (Some("setup"), Target::Setup { at, id, name, tool }) => {
                let s = self.setup_from(&t, id, name.as_deref(), tool, tools, new_id)?;
                Edit::add_setup(*at, &s)
            }
            (k, _) => {
                return Err(fail(format!(
                    "a {} template cannot be applied here",
                    k.unwrap_or("?")
                )));
            }
        };
        Ok(Edit::Group {
            label: format!("Apply template {r}"),
            edits: vec![edit],
        })
    }

    fn setup_from(
        &self,
        t: &Template,
        id: &str,
        name: Option<&str>,
        tool: &str,
        tools: &ToolStore,
        new_id: &mut dyn FnMut() -> String,
    ) -> Result<Setup, TemplateError> {
        let fail = |msg: String| TemplateError {
            template: t.layer.clone(),
            msg,
        };
        let table = &t.resolved.table;
        let mut s = Setup::new(id, "");
        let fields = match table.get("setup") {
            None => Table::new(),
            Some(Value::Table(x)) => x.clone(),
            Some(_) => return Err(fail("`setup` must be a table".into())),
        };
        let mut w = Vec::new();
        for (k, v) in &fields {
            if k == "geometry" || k == "id" || k == "based_on" {
                return Err(fail(format!(
                    "`setup.{k}` is chosen when the template is applied"
                )));
            }
            Cx::new(&mut w)
                .at("setup")
                .setup_key(&mut s, k, Some(v))
                .map_err(|e| fail(e.0))?;
        }
        if let Some(n) = name {
            s.name = n.to_owned();
        }
        let entries = match table.get("toolpath") {
            None => Vec::new(),
            Some(Value::Array(a)) => a.clone(),
            Some(_) => return Err(fail("`toolpath` must be [[toolpath]] entries".into())),
        };
        for (i, e) in entries.iter().enumerate() {
            let e = e
                .as_table()
                .ok_or_else(|| fail(format!("toolpath[{i}] must be a table")))?;
            let tid = new_id();
            let ename = e.get("name").and_then(Value::as_str);
            let etool = e.get("tool").and_then(Value::as_str).unwrap_or(tool);
            let tp = match e.get("template") {
                Some(Value::String(tr)) => {
                    let tr = DefRef::parse(tr).map_err(|x| fail(x.to_string()))?;
                    let sub = self.template(&tr)?;
                    if sub.kind() != Some("toolpath") {
                        return Err(fail(format!(
                            "toolpath[{i}]: {tr} is not a toolpath template"
                        )));
                    }
                    instantiate(&sub, &tid, ename, etool, tools)?
                }
                Some(_) => return Err(fail(format!("toolpath[{i}].template must be a reference"))),
                None => {
                    let mut inline = e.clone();
                    inline.remove("name");
                    inline.remove("tool");
                    let one = Template {
                        layer: t.layer.clone(),
                        resolved: Resolved {
                            table: [("toolpath".to_owned(), Value::Table(inline))]
                                .into_iter()
                                .collect(),
                            ..Resolved::default()
                        },
                    };
                    let mut tp = instantiate(&one, &tid, ename, etool, tools)?;
                    tp.template = Some(t.layer.clone());
                    tp
                }
            };
            s.toolpaths.push(tp);
        }
        Ok(s)
    }
}

/// A toolpath from a resolved toolpath template, with expressions
/// evaluated against `tool` (D15 §1.6).
pub fn instantiate(
    t: &Template,
    id: &str,
    name: Option<&str>,
    tool: &str,
    tools: &ToolStore,
) -> Result<Toolpath, TemplateError> {
    let fail = |msg: String| TemplateError {
        template: t.layer.clone(),
        msg,
    };
    let Some(Value::Table(src)) = t.resolved.table.get("toolpath") else {
        return Err(fail("`[toolpath]` is missing".into()));
    };
    let ty = src
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("`toolpath.type` is missing".into()))?;
    let kind = ToolpathTypeId::parse(ty).ok_or_else(|| {
        fail(format!(
            "toolpath type `{ty}` is not supported by this build"
        ))
    })?;
    let names = tool_names(tools.get(tool));
    let mut tp = src.clone();
    for g in Group::ALL {
        let Some(Value::Table(group)) = tp.get_mut(g.key()) else {
            continue;
        };
        for (k, v) in group.iter_mut() {
            let path = format!("{}.{k}", g.key());
            evaluate(kind, &path, v, &names).map_err(fail)?;
        }
    }
    tp.insert("id".into(), Value::String(id.to_owned()));
    let display = name
        .map(str::to_owned)
        .or_else(|| src.get("name").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| ty.to_owned());
    tp.insert("name".into(), Value::String(display));
    tp.insert("tool".into(), Value::String(tool.to_owned()));
    tp.insert("template".into(), Value::String(t.layer.clone()));
    let mut w = Vec::new();
    let out = Cx::new(&mut w)
        .at("toolpath")
        .toolpath(&tp)
        .map_err(|e| fail(e.0))?;
    if let Some(w) = w.first() {
        return Err(fail(w.clone()));
    }
    Ok(out)
}

/// Replaces an expression string in a number field (or inside an inline
/// table's number fields) by its value.
fn evaluate(
    kind: ToolpathTypeId,
    path: &str,
    v: &mut Value,
    names: &dyn Fn(&str) -> Option<f64>,
) -> Result<(), String> {
    let Some(meta) = Params::meta(kind, path) else {
        return Ok(());
    };
    match (meta.kind, &mut *v) {
        (
            FieldKind::Length | FieldKind::Feed | FieldKind::Speed | FieldKind::Angle,
            Value::String(s),
        ) => {
            *v = Value::Float(eval(s, names).map_err(|e| format!("`{path}`: {e}"))?);
        }
        (FieldKind::Count, Value::String(s)) => {
            let x = eval(s, names).map_err(|e| format!("`{path}`: {e}"))?;
            if x.fract() != 0.0 || x.abs() > 1e15 {
                return Err(format!("`{path}`: {x} is not a whole number"));
            }
            *v = Value::Integer(x as i64);
        }
        (FieldKind::Table(_), Value::Table(t)) => {
            for (k, x) in t.iter_mut() {
                evaluate(kind, &format!("{path}.{k}"), x, names)?;
            }
        }
        _ => {}
    }
    Ok(())
}
