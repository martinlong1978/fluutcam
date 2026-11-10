// SPDX-License-Identifier: MIT OR Apache-2.0

//! `Setup` (D12b §4): Setup keys as properties, resolved on read.

use super::project::{by_name, free_id};
use super::toolpath::{ToolpathH, run_calc};
use super::{Res, Rt, State, fail};
use crate::value::{inline, param_toml, to_dynamic, to_toml};
use fluut_core::Targets;
use fluut_model::DefRef;
use fluut_model::edit::Edit;
use fluut_model::params::{Group, ToolpathTypeId};
use fluut_model::project::{Project, Setup, Toolpath, write};
use fluut_model::setup::{FieldOrigin, INHERITED_FIELDS, resolve_setup};
use fluut_model::template::Target;
use rhai::{Array, Dynamic, Engine, Map};
use std::sync::Arc;

/// String keys that hold names and references, never lengths.
pub(crate) const IDENT_KEYS: [&str; 9] = [
    "id",
    "name",
    "based_on",
    "machine",
    "material",
    "work_offset",
    "tool",
    "template",
    "type",
];

/// The Setup keys a script may set, in file order.
const SETUP_KEYS: [&str; 14] = [
    "name",
    "based_on",
    "kind",
    "machine",
    "material",
    "work_offset",
    "geometry",
    "orientation",
    "index_angle",
    "stock",
    "zero",
    "region",
    "clearance",
    "fixtures",
];

#[derive(Clone, Debug)]
pub struct SetupH {
    rt: Rt,
    generation: u64,
    id: String,
}

/// The file form of Setup `s`'s keys, read through the project writer.
pub(crate) fn setup_table(s: &Setup) -> toml::Table {
    let mut p = Project::new("");
    let mut s = s.clone();
    s.toolpaths.clear();
    p.setups.push(Arc::new(s));
    let t: toml::Table = toml::from_str(&write(&p)).unwrap_or_default();
    t.get("setup")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_table())
        .cloned()
        .unwrap_or_default()
}

impl SetupH {
    pub(crate) fn new(rt: Rt, generation: u64, id: &str) -> SetupH {
        SetupH {
            rt,
            generation,
            id: id.to_owned(),
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    fn with<T>(&self, f: impl FnOnce(&mut State, &Setup) -> Res<T>) -> Res<T> {
        let mut g = self.rt.at(self.generation)?;
        let s = match g.session()?.project.setup(&self.id) {
            Some(s) => s.clone(),
            None => return fail(format!("Setup `{}` is no longer in the project", self.id)),
        };
        f(&mut g, &s)
    }

    fn get(&mut self, key: &str) -> Res<Dynamic> {
        self.with(|g, s| {
            let p = &g.session()?.project;
            let shown = if key == "name" || key == "based_on" || key == "id" {
                s.clone()
            } else {
                resolve_setup(p, &s.id)
                    .or_else(|d| fail(d.to_string()))?
                    .to_setup()
            };
            Ok(setup_table(&shown)
                .get(key)
                .map_or(Dynamic::UNIT, to_dynamic))
        })
    }

    fn set(&mut self, key: &str, v: Dynamic) -> Res<()> {
        let ident = IDENT_KEYS.contains(&key);
        let value = to_toml(&v, !ident).or_else(|e| fail(format!("`{key}`: {e}")))?;
        let text = value.as_ref().map(inline);
        let id = self.id.clone();
        self.with(|g, _| g.apply(Edit::set_setup_field(&id, key, text.as_deref())))
    }

    fn origin(&mut self, key: &str) -> Res<FieldOrigin> {
        if !INHERITED_FIELDS.contains(&key) {
            return fail(format!(
                "`{key}` is not an inheritable Setup field ({})",
                INHERITED_FIELDS.join(", ")
            ));
        }
        self.with(|g, s| {
            let r = resolve_setup(&g.session()?.project, &s.id).or_else(|d| fail(d.to_string()))?;
            Ok(r.origin(key).clone())
        })
    }

    fn toolpaths(&mut self) -> Res<Array> {
        let rt = self.rt.clone();
        let generation = self.generation;
        self.with(|_, s| {
            Ok(s.toolpaths
                .iter()
                .map(|t| Dynamic::from(ToolpathH::new(rt.clone(), generation, &t.id)))
                .collect())
        })
    }

    fn toolpath(&mut self, key: &str) -> Res<ToolpathH> {
        let id = self.with(|_, s| {
            by_name("toolpath", key, s.toolpaths.iter(), |t| &t.id, |t| &t.name)
                .map(|t| t.id.clone())
        })?;
        Ok(ToolpathH::new(self.rt.clone(), self.generation, &id))
    }

    fn add_toolpath(&mut self, ty: &str, map: Map) -> Res<ToolpathH> {
        let Some(kind) = ToolpathTypeId::parse(ty) else {
            return fail(format!("`{ty}` is not a toolpath type"));
        };
        let setup = self.id.clone();
        let id = self.with(|g, _| {
            let p = &g.session()?.project;
            let id = match map.get("id") {
                Some(v) => v
                    .clone()
                    .into_string()
                    .or_else(|_| fail("`id` must be a string"))?,
                None => free_id(p, "t"),
            };
            let text = |k: &str| -> Res<Option<String>> {
                match map.get(k) {
                    None => Ok(None),
                    Some(v) => v
                        .clone()
                        .into_string()
                        .map(Some)
                        .or_else(|_| fail(format!("`{k}` must be a string"))),
                }
            };
            let name = text("name")?.unwrap_or_else(|| ty.to_owned());
            let tool = text("tool")?.unwrap_or_default();
            let mut t = Toolpath::new(&id, &name, kind, &tool);
            for (k, v) in &map {
                match k.as_str() {
                    "id" | "name" | "tool" => {}
                    "type" => return fail("the type is the first argument of add_toolpath"),
                    "enabled" => {
                        t.enabled = Some(
                            v.as_bool()
                                .or_else(|_| fail("`enabled` must be true or false"))?,
                        );
                    }
                    k if Group::from_key(k).is_some() => {
                        let Some(m) = v.clone().try_cast::<Map>() else {
                            return fail(format!("`{k}` must be a map"));
                        };
                        for (key, x) in &m {
                            let path = format!("{k}.{key}");
                            let val = param_toml(kind, &path, x).or_else(fail)?;
                            t.params.set_toml(&path, val.as_ref()).or_else(fail)?;
                        }
                    }
                    k => return fail(format!("`{k}` is not a toolpath key")),
                }
            }
            g.apply(Edit::add_toolpath(&setup, None, &t))?;
            Ok(id)
        })?;
        Ok(ToolpathH::new(self.rt.clone(), self.generation, &id))
    }

    fn apply_template(&mut self, r: &str, map: Map) -> Res<ToolpathH> {
        let setup = self.id.clone();
        let id = self.with(|g, _| {
            let s = g.session()?;
            let def = DefRef::parse(r).or_else(|e| fail(format!("`{r}`: {e}")))?;
            let id = match map.get("id") {
                Some(v) => v.to_string(),
                None => free_id(&s.project, "t"),
            };
            let target = Target::Toolpath {
                setup: setup.clone(),
                at: None,
                id: id.clone(),
                name: map.get("name").map(|v| v.to_string()),
                tool: map.get("tool").map_or(String::new(), |v| v.to_string()),
            };
            let edit = s
                .library
                .apply_template(&def, &target, &s.tools, &mut || String::new())
                .or_else(|e| fail(e.to_string()))?;
            g.apply(edit)?;
            Ok(id)
        })?;
        Ok(ToolpathH::new(self.rt.clone(), self.generation, &id))
    }

    fn calculate(&mut self) -> Res<Array> {
        let rt = self.rt.clone();
        self.with(|g, s| {
            let ids = s.toolpaths.iter().map(|t| t.id.clone()).collect();
            run_calc(&rt, g, &Targets::Toolpaths(ids))
        })
    }

    fn remove(&mut self) -> Res<()> {
        let id = self.id.clone();
        self.with(|g, _| g.apply(Edit::RemoveSetup(id)))
    }
}

pub(super) fn register(e: &mut Engine) {
    e.register_type_with_name::<SetupH>("Setup");
    e.register_get("id", |h: &mut SetupH| h.id.clone());
    for key in SETUP_KEYS {
        e.register_get(key, move |h: &mut SetupH| h.get(key));
        e.register_set(key, move |h: &mut SetupH, v: Dynamic| h.set(key, v));
    }
    e.register_fn("is_inherited", |h: &mut SetupH, key: &str| -> Res<bool> {
        Ok(h.origin(key)? != FieldOrigin::Local)
    });
    e.register_fn("origin", |h: &mut SetupH, key: &str| -> Res<String> {
        Ok(match h.origin(key)? {
            FieldOrigin::Local => "local".into(),
            FieldOrigin::Default => "default".into(),
            FieldOrigin::Inherited(id) => id,
        })
    });
    e.register_fn("reset", |h: &mut SetupH, key: &str| -> Res<()> {
        h.set(key, Dynamic::UNIT)
    });
    e.register_get("toolpaths", SetupH::toolpaths);
    e.register_fn("toolpath", SetupH::toolpath);
    e.register_fn("add_toolpath", SetupH::add_toolpath);
    e.register_fn("add_toolpath", |h: &mut SetupH, ty: &str| {
        h.add_toolpath(ty, Map::new())
    });
    e.register_fn("apply_template", SetupH::apply_template);
    e.register_fn("apply_template", |h: &mut SetupH, r: &str| {
        h.apply_template(r, Map::new())
    });
    e.register_fn("calculate", SetupH::calculate);
    e.register_fn("remove", SetupH::remove);
    e.register_fn("to_string", |h: &mut SetupH| format!("Setup({})", h.id));
    e.register_fn("==", |a: &mut SetupH, b: SetupH| a.id == b.id);
}
