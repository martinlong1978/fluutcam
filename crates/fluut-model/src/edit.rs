// SPDX-License-Identifier: MIT OR Apache-2.0

//! Edits (D06 §6.1): every change to a project is an [`Edit`] value.
//!
//! Payloads are in file syntax: a Setup, toolpath or geometry source as
//! its `[[…]]` text, a field value as its inline TOML value. So an edit
//! is checked by the same reader as a file, and the journal (D06 §7.3)
//! stores edits without serde on the model types.

use crate::params::value::quote;
use crate::params::{Sel, ToolpathTypeId};
use crate::project::{
    AxisPick, Cx, DisplayUnit, GeometrySource, Orientation, Project, SETUP_KEYS, Setup, StockSpec,
    TOOLPATH_KEYS, Toolpath, check_ids, read, setup_value, write_setup, write_source,
    write_toolpath,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;
use toml::{Table, Value};

/// One change to a project. `at` positions count from 0; `None` appends.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Edit {
    SetName(String),
    /// `mm` or `inch`.
    SetUnits(String),
    /// A `[[geometry]]` table.
    AddGeometry {
        at: Option<u32>,
        text: String,
    },
    /// Replaces source `id` (re-import: new hash and bodies).
    SetGeometry {
        id: String,
        text: String,
    },
    RemoveGeometry(String),
    /// A `[[setup]]` table, with its toolpaths.
    AddSetup {
        at: Option<u32>,
        text: String,
    },
    /// `value` is an inline TOML value; `None` resets the field to
    /// inherited (or clears `name`).
    SetSetupField {
        setup: String,
        key: String,
        value: Option<String>,
    },
    MoveSetup {
        id: String,
        to: u32,
    },
    RemoveSetup(String),
    /// A `[[setup.toolpath]]` table.
    AddToolpath {
        setup: String,
        at: Option<u32>,
        text: String,
    },
    /// `key` is an item key (`name`, `tool`, `enabled`, `template`) or a
    /// parameter path (`passes.stepover`, `tool_params.feed.cut`).
    SetToolpathField {
        toolpath: String,
        key: String,
        value: Option<String>,
    },
    MoveToolpath {
        id: String,
        setup: String,
        to: u32,
    },
    RemoveToolpath(String),
    /// A new toolpath of `new_type` after `toolpath`, with the item keys
    /// and every parameter the two types share (spec 03 §3.10.1).
    DuplicateAs {
        toolpath: String,
        new_id: String,
        new_type: String,
    },
    /// Renames entity ids in selections, Setup bodies, orientation picks
    /// and body stock (re-import, spec 03 §3.5).
    RemapIds {
        map: Vec<(String, String)>,
    },
    /// Several edits as one undo step, applied in order.
    Group {
        label: String,
        edits: Vec<Edit>,
    },
}

impl Edit {
    pub fn add_geometry(at: Option<usize>, g: &GeometrySource) -> Edit {
        let mut text = String::new();
        write_source(&mut text, g);
        Edit::AddGeometry {
            at: at.map(|a| a as u32),
            text,
        }
    }

    pub fn set_geometry(g: &GeometrySource) -> Edit {
        let mut text = String::new();
        write_source(&mut text, g);
        Edit::SetGeometry {
            id: g.id.clone(),
            text,
        }
    }

    pub fn add_setup(at: Option<usize>, s: &Setup) -> Edit {
        let mut text = String::new();
        write_setup(&mut text, s);
        Edit::AddSetup {
            at: at.map(|a| a as u32),
            text,
        }
    }

    pub fn add_toolpath(setup: &str, at: Option<usize>, t: &Toolpath) -> Edit {
        let mut text = String::new();
        write_toolpath(&mut text, t);
        Edit::AddToolpath {
            setup: setup.to_owned(),
            at: at.map(|a| a as u32),
            text,
        }
    }

    pub fn set_setup_field(setup: &str, key: &str, value: Option<&str>) -> Edit {
        Edit::SetSetupField {
            setup: setup.to_owned(),
            key: key.to_owned(),
            value: value.map(str::to_owned),
        }
    }

    pub fn set_toolpath_field(toolpath: &str, key: &str, value: Option<&str>) -> Edit {
        Edit::SetToolpathField {
            toolpath: toolpath.to_owned(),
            key: key.to_owned(),
            value: value.map(str::to_owned),
        }
    }

    /// Short text for the undo menu.
    pub fn label(&self) -> String {
        match self {
            Edit::SetName(_) => "Rename project".into(),
            Edit::SetUnits(_) => "Change units".into(),
            Edit::AddGeometry { .. } => "Add geometry".into(),
            Edit::SetGeometry { id, .. } => format!("Update geometry {id}"),
            Edit::RemoveGeometry(id) => format!("Remove geometry {id}"),
            Edit::AddSetup { .. } => "Add Setup".into(),
            Edit::SetSetupField { setup, key, .. } => format!("Set {key} of {setup}"),
            Edit::MoveSetup { id, .. } => format!("Move {id}"),
            Edit::RemoveSetup(id) => format!("Remove {id}"),
            Edit::AddToolpath { .. } => "Add toolpath".into(),
            Edit::SetToolpathField { toolpath, key, .. } => format!("Set {key} of {toolpath}"),
            Edit::MoveToolpath { id, .. } => format!("Move {id}"),
            Edit::RemoveToolpath(id) => format!("Remove {id}"),
            Edit::DuplicateAs { toolpath, .. } => format!("Duplicate {toolpath}"),
            Edit::RemapIds { .. } => "Match re-imported geometry".into(),
            Edit::Group { label, .. } => label.clone(),
        }
    }

    /// `(target, key)` for edits that coalesce when repeated (D06 §6.2).
    pub fn coalesce_key(&self) -> Option<(&str, &str)> {
        match self {
            Edit::SetSetupField { setup, key, .. } => Some((setup, key)),
            Edit::SetToolpathField { toolpath, key, .. } => Some((toolpath, key)),
            _ => None,
        }
    }
}

/// An edit that cannot be applied; the project is unchanged.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[error("{0}")]
pub struct EditError(pub String);

type R<T> = Result<T, EditError>;

fn e<T>(msg: impl Into<String>) -> R<T> {
    Err(EditError(msg.into()))
}

/// Ids of the nodes an edit changed: sources, Setups, toolpaths, or
/// `project` for project fields (D06 §6.1, for invalidation).
pub type Changed = BTreeSet<String>;

/// Applies `edit` to a copy of `p`. Unchanged Setups stay shared.
pub fn apply(p: &Project, edit: &Edit) -> R<(Project, Changed)> {
    let mut q = p.clone();
    let mut changed = Changed::new();
    apply_in(&mut q, edit, &mut changed)?;
    check_ids(&q).map_err(|x| EditError(x.0))?;
    if chain_errors(&q) > chain_errors(p) {
        return e("the edit breaks a `based_on` chain: a parent must exist and come earlier");
    }
    Ok((q, changed))
}

/// Setups whose `based_on` names a missing or later Setup.
fn chain_errors(p: &Project) -> usize {
    p.setups
        .iter()
        .enumerate()
        .filter(|(i, s)| {
            s.based_on
                .as_ref()
                .is_some_and(|b| p.setup_index(b).is_none_or(|j| j >= *i))
        })
        .count()
}

/// Parses `key = <value>`.
fn value(text: &str) -> R<Value> {
    let t: Table = toml::from_str(&format!("v = {text}"))
        .map_err(|x| EditError(format!("`{text}` is not a TOML value: {x}")))?;
    t.get("v")
        .cloned()
        .ok_or_else(|| EditError("no value".into()))
}

const HEADER: &str = "format = \"fluut.project/1\"\n";

/// Reads a fragment through the project reader. Unknown keys are errors
/// in an edit.
fn fragment(text: &str) -> R<Project> {
    let l = read(&format!("{HEADER}{text}")).map_err(|x| EditError(x.0))?;
    if let Some(w) = l.warnings.first() {
        return e(w.clone());
    }
    Ok(l.project)
}

fn insert<T>(v: &mut Vec<T>, at: Option<u32>, x: T) -> R<()> {
    let i = at.map_or(v.len(), |a| a as usize);
    if i > v.len() {
        return e(format!("position {i} is past the end ({})", v.len()));
    }
    v.insert(i, x);
    Ok(())
}

fn setup_of(p: &Project, toolpath: &str) -> R<usize> {
    p.setups
        .iter()
        .position(|s| s.toolpaths.iter().any(|t| t.id == toolpath))
        .ok_or_else(|| EditError(format!("toolpath `{toolpath}` is not in the project")))
}

fn setup_at(p: &Project, id: &str) -> R<usize> {
    p.setup_index(id)
        .ok_or_else(|| EditError(format!("Setup `{id}` is not in the project")))
}

fn apply_in(p: &mut Project, edit: &Edit, ch: &mut Changed) -> R<()> {
    match edit {
        Edit::SetName(n) => {
            p.name = n.clone();
            ch.insert("project".into());
        }
        Edit::SetUnits(u) => {
            p.units = DisplayUnit::parse(u)
                .ok_or_else(|| EditError(format!("`{u}` is not mm or inch")))?;
            ch.insert("project".into());
        }
        Edit::AddGeometry { at, text } => {
            let mut f = fragment(text)?;
            if f.geometry.len() != 1 || !f.setups.is_empty() {
                return e("expected one [[geometry]] table");
            }
            let g = f.geometry.remove(0);
            ch.insert(g.id.clone());
            insert(&mut p.geometry, *at, g)?;
        }
        Edit::SetGeometry { id, text } => {
            let mut f = fragment(text)?;
            if f.geometry.len() != 1 || f.geometry[0].id != *id {
                return e(format!("expected one [[geometry]] table with id `{id}`"));
            }
            let slot = p
                .geometry
                .iter_mut()
                .find(|g| g.id == *id)
                .ok_or_else(|| EditError(format!("geometry `{id}` is not in the project")))?;
            *slot = f.geometry.remove(0);
            ch.insert(id.clone());
        }
        Edit::RemoveGeometry(id) => {
            let i = p
                .geometry
                .iter()
                .position(|g| g.id == *id)
                .ok_or_else(|| EditError(format!("geometry `{id}` is not in the project")))?;
            p.geometry.remove(i);
            ch.insert(id.clone());
        }
        Edit::AddSetup { at, text } => {
            let mut f = fragment(text)?;
            if f.setups.len() != 1 || !f.geometry.is_empty() {
                return e("expected one [[setup]] table");
            }
            let s = f.setups.remove(0);
            ch.insert(s.id.clone());
            ch.extend(s.toolpaths.iter().map(|t| t.id.clone()));
            insert(&mut p.setups, *at, s)?;
        }
        Edit::SetSetupField {
            setup,
            key,
            value: v,
        } => {
            if !SETUP_KEYS[1..].contains(&key.as_str()) {
                return e(format!("`{key}` is not a Setup field"));
            }
            let i = setup_at(p, setup)?;
            let v = v.as_deref().map(value).transpose()?;
            let s = Arc::make_mut(&mut p.setups[i]);
            let mut w = Vec::new();
            Cx::new(&mut w)
                .at(key)
                .setup_key(s, key, v.as_ref())
                .map_err(|x| EditError(x.0))?;
            if let Some(w) = w.first() {
                return e(w.clone());
            }
            ch.insert(setup.clone());
        }
        Edit::MoveSetup { id, to } => {
            let i = setup_at(p, id)?;
            let s = p.setups.remove(i);
            insert(&mut p.setups, Some(*to), s)?;
            ch.insert(id.clone());
        }
        Edit::RemoveSetup(id) => {
            let i = setup_at(p, id)?;
            let s = p.setups.remove(i);
            ch.insert(id.clone());
            ch.extend(s.toolpaths.iter().map(|t| t.id.clone()));
        }
        Edit::AddToolpath { setup, at, text } => {
            let f = fragment(&format!("[[setup]]\nid = {}\n{text}", quote(setup)))?;
            let mut s = f
                .setups
                .into_iter()
                .next()
                .ok_or_else(|| EditError("no Setup".into()))?;
            let s = Arc::make_mut(&mut s);
            if s.toolpaths.len() != 1 {
                return e("expected one [[setup.toolpath]] table");
            }
            let t = s.toolpaths.remove(0);
            let i = setup_at(p, setup)?;
            ch.insert(t.id.clone());
            insert(&mut Arc::make_mut(&mut p.setups[i]).toolpaths, *at, t)?;
        }
        Edit::SetToolpathField {
            toolpath,
            key,
            value: v,
        } => {
            let si = setup_of(p, toolpath)?;
            let v = v.as_deref().map(value).transpose()?;
            let s = Arc::make_mut(&mut p.setups[si]);
            let t = s
                .toolpaths
                .iter_mut()
                .find(|t| t.id == *toolpath)
                .ok_or_else(|| EditError(format!("toolpath `{toolpath}` is not in the project")))?;
            if TOOLPATH_KEYS.contains(&key.as_str()) {
                if key == "id" || key == "type" {
                    return e(format!("`{key}` is fixed at creation"));
                }
                let mut w = Vec::new();
                Cx::new(&mut w)
                    .toolpath_key(t, key, v.as_ref())
                    .map_err(|x| EditError(x.0))?;
            } else {
                t.params.set_toml(key, v.as_ref()).map_err(EditError)?;
            }
            ch.insert(toolpath.clone());
        }
        Edit::MoveToolpath { id, setup, to } => {
            let from = setup_of(p, id)?;
            let dest = setup_at(p, setup)?;
            let s = Arc::make_mut(&mut p.setups[from]);
            let i = s.toolpaths.iter().position(|t| t.id == *id).unwrap_or(0);
            let t = s.toolpaths.remove(i);
            insert(
                &mut Arc::make_mut(&mut p.setups[dest]).toolpaths,
                Some(*to),
                t,
            )?;
            ch.insert(id.clone());
        }
        Edit::RemoveToolpath(id) => {
            let si = setup_of(p, id)?;
            Arc::make_mut(&mut p.setups[si])
                .toolpaths
                .retain(|t| t.id != *id);
            ch.insert(id.clone());
        }
        Edit::DuplicateAs {
            toolpath,
            new_id,
            new_type,
        } => {
            let kind = ToolpathTypeId::parse(new_type)
                .ok_or_else(|| EditError(format!("`{new_type}` is not a toolpath type")))?;
            let si = setup_of(p, toolpath)?;
            let s = Arc::make_mut(&mut p.setups[si]);
            let i = s
                .toolpaths
                .iter()
                .position(|t| t.id == *toolpath)
                .unwrap_or(0);
            let old = &s.toolpaths[i];
            let mut t = Toolpath::new(new_id, &old.name, kind, &old.tool);
            t.enabled = old.enabled;
            for (path, _) in crate::params::Params::leaf_fields(old.kind()) {
                if let Some(v) = old.params.get(&path) {
                    // Shared fields only; a value the new type rejects is
                    // left at its default.
                    let _ = t.params.set(&path, Some(v.clone()));
                }
            }
            s.toolpaths.insert(i + 1, t);
            ch.insert(new_id.clone());
        }
        Edit::RemapIds { map } => remap(p, map, ch),
        Edit::Group { edits, .. } => {
            for x in edits {
                apply_in(p, x, ch)?;
            }
        }
    }
    Ok(())
}

fn remap(p: &mut Project, map: &[(String, String)], ch: &mut Changed) {
    let get = |id: &str| map.iter().find(|(a, _)| a == id).map(|(_, b)| b.clone());
    for i in 0..p.setups.len() {
        // Look first, so untouched Setups stay shared.
        let s = &p.setups[i];
        let hits_setup = s
            .geometry
            .as_ref()
            .and_then(|g| g.bodies.as_ref())
            .is_some_and(|b| b.iter().any(|x| get(x).is_some()))
            || pick_ids(s).iter().any(|x| get(x).is_some())
            || matches!(&s.stock, Some(StockSpec::Body { body }) if get(body).is_some());
        let hits_tp: Vec<String> = s
            .toolpaths
            .iter()
            .filter(|t| {
                t.params
                    .selections()
                    .iter()
                    .any(|(_, x)| get(x.id()).is_some())
            })
            .map(|t| t.id.clone())
            .collect();
        if !hits_setup && hits_tp.is_empty() {
            continue;
        }
        let s = Arc::make_mut(&mut p.setups[i]);
        if hits_setup {
            if let Some(b) = s.geometry.as_mut().and_then(|g| g.bodies.as_mut()) {
                for x in b.iter_mut() {
                    if let Some(n) = get(x) {
                        *x = n;
                    }
                }
            }
            let fix = |a: &mut AxisPick| match a {
                AxisPick::Face(x) | AxisPick::Edge(x) => {
                    if let Some(n) = get(x) {
                        *x = n;
                    }
                }
                AxisPick::Axis(_) => {}
            };
            match &mut s.orientation {
                Some(Orientation::ThreeAxis { tool_axis, x_axis }) => {
                    fix(tool_axis);
                    fix(x_axis);
                }
                Some(Orientation::Rotary {
                    rotary_axis,
                    a_zero_toward,
                }) => {
                    fix(rotary_axis);
                    if let Some(a) = a_zero_toward {
                        fix(a);
                    }
                }
                None => {}
            }
            if let Some(StockSpec::Body { body }) = &mut s.stock
                && let Some(n) = get(body)
            {
                *body = n;
            }
            ch.insert(s.id.clone());
        }
        for t in s.toolpaths.iter_mut().filter(|t| hits_tp.contains(&t.id)) {
            t.params.map_selections(&mut |x: &mut Sel| {
                if let Some(n) = get(x.id()) {
                    *x = match x {
                        Sel::Body(_) => Sel::Body(n),
                        Sel::Face(_) => Sel::Face(n),
                        Sel::Edge(_) => Sel::Edge(n),
                    };
                }
            });
            ch.insert(t.id.clone());
        }
    }
}

fn pick_ids(s: &Setup) -> Vec<String> {
    let id = |a: &AxisPick| match a {
        AxisPick::Face(x) | AxisPick::Edge(x) => Some(x.clone()),
        AxisPick::Axis(_) => None,
    };
    match &s.orientation {
        Some(Orientation::ThreeAxis { tool_axis, x_axis }) => {
            [id(tool_axis), id(x_axis)].into_iter().flatten().collect()
        }
        Some(Orientation::Rotary {
            rotary_axis,
            a_zero_toward,
        }) => [id(rotary_axis), a_zero_toward.as_ref().and_then(id)]
            .into_iter()
            .flatten()
            .collect(),
        None => Vec::new(),
    }
}

/// The inline value of Setup `key` as it is now, for building edits (and
/// their inverses in tests).
pub fn setup_field_text(s: &Setup, key: &str) -> Option<String> {
    setup_value(s, key)
}
