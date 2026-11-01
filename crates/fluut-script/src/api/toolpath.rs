// SPDX-License-Identifier: MIT OR Apache-2.0

//! `Toolpath` and its parameter groups (D12b §5).

use super::setup::SetupH;
use super::{Calc, Res, Rt, State, diag_array, fail};
use crate::value::{inline, param_toml, to_dynamic};
use fluut_core::{JobReport, JobState, NodeId, Targets};
use fluut_model::edit::Edit;
use fluut_model::params::Group;
use fluut_model::project::{Project, Setup, Toolpath, write};
use rhai::{Array, Dynamic, Engine, FLOAT, Map};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ToolpathH {
    rt: Rt,
    generation: u64,
    id: String,
}

/// `toolpath.passes` and the other groups.
#[derive(Clone, Debug)]
pub struct GroupH {
    tp: ToolpathH,
    group: Group,
}

/// The file form of a toolpath's group tables.
fn toolpath_table(setup: &Setup, t: &Toolpath) -> toml::Table {
    let mut p = Project::new("");
    let mut s = Setup::new(&setup.id, "");
    s.toolpaths.push(t.clone());
    p.setups.push(Arc::new(s));
    let all: toml::Table = toml::from_str(&write(&p)).unwrap_or_default();
    all.get("setup")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.get("toolpath"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_table())
        .cloned()
        .unwrap_or_default()
}

/// A job report as a script map (D12b §5.3).
pub(crate) fn report_map(r: &JobReport) -> Map {
    let mut m = Map::new();
    m.insert("toolpath".into(), r.toolpath.clone().into());
    let state = match r.state {
        JobState::Done { cached: false } => "calculated",
        JobState::Done { cached: true } => "up_to_date",
        JobState::Failed => "failed",
        JobState::Skipped => "disabled",
    };
    m.insert("state".into(), state.into());
    m.insert("diagnostics".into(), diag_array(&r.diagnostics).into());
    if let Some(st) = &r.stats {
        m.insert("cut_length".into(), (st.cut_length as FLOAT).into());
        m.insert("rapid_length".into(), (st.rapid_length as FLOAT).into());
    }
    m
}

/// Calculates `targets` as a session job and records each outcome.
pub(crate) fn run_calc(rt: &Rt, g: &mut State, targets: &Targets) -> Res<Array> {
    let s = g.session_mut()?;
    let reports = rt.ctl.host_call(|| s.calculate(targets));
    let graph = s.graph();
    for r in &reports {
        if r.state == JobState::Skipped {
            continue;
        }
        let hash = graph
            .hash(&NodeId::Result(r.toolpath.clone()))
            .unwrap_or_default()
            .to_owned();
        g.calc.insert(
            r.toolpath.clone(),
            Calc {
                failed: r.state == JobState::Failed,
                hash,
                diagnostics: r.diagnostics.clone(),
            },
        );
    }
    Ok(reports
        .iter()
        .map(|r| Dynamic::from_map(report_map(r)))
        .collect())
}

impl ToolpathH {
    pub(crate) fn new(rt: Rt, generation: u64, id: &str) -> ToolpathH {
        ToolpathH {
            rt,
            generation,
            id: id.to_owned(),
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut State, &Setup, &Toolpath) -> Res<T>) -> Res<T> {
        let mut g = self.rt.at(self.generation)?;
        let (s, t) = match g.session()?.project.toolpath(&self.id) {
            Some((s, t)) => (s.clone(), t.clone()),
            None => {
                return fail(format!(
                    "toolpath `{}` is no longer in the project",
                    self.id
                ));
            }
        };
        f(&mut g, &s, &t)
    }

    fn set_item(&mut self, key: &str, v: Option<toml::Value>) -> Res<()> {
        let id = self.id.clone();
        let text = v.as_ref().map(inline);
        self.with(|g, _, _| g.apply(Edit::set_toolpath_field(&id, key, text.as_deref())))
    }

    fn group(&mut self, group: Group) -> GroupH {
        GroupH {
            tp: self.clone(),
            group,
        }
    }

    /// Replaces a whole group: keys absent from `m` are cleared.
    fn set_group(&mut self, group: Group, m: Map) -> Res<()> {
        let id = self.id.clone();
        self.with(|g, _, t| {
            let kind = t.kind();
            let mut edits = Vec::new();
            for (k, _) in t.params.group(group).iter() {
                edits.push(Edit::set_toolpath_field(
                    &id,
                    &format!("{}.{k}", group.key()),
                    None,
                ));
            }
            for (k, v) in &m {
                let path = format!("{}.{k}", group.key());
                let val = param_toml(kind, &path, v).or_else(fail)?;
                if let Some(val) = val {
                    edits.push(Edit::set_toolpath_field(&id, &path, Some(&inline(&val))));
                }
            }
            g.apply(Edit::Group {
                label: format!("Set {} of {id}", group.key()),
                edits,
            })
        })
    }

    fn state(&mut self) -> Res<String> {
        self.with(|g, _, t| {
            let s = g.session()?;
            if s.result(&t.id).is_some() {
                return Ok("done".into());
            }
            let hash = s
                .graph()
                .hash(&NodeId::Result(t.id.clone()))
                .unwrap_or_default()
                .to_owned();
            Ok(match g.calc.get(&t.id) {
                Some(c) if c.failed && c.hash == hash => "error",
                Some(c) if !c.failed => "stale",
                _ => "not_calculated",
            }
            .into())
        })
    }

    fn summary(&mut self) -> Res<Dynamic> {
        self.with(|g, _, t| {
            let Some(r) = g.session()?.result(&t.id) else {
                return Ok(Dynamic::UNIT);
            };
            let st = r.stats();
            let b = st.bounds;
            let mut m = Map::new();
            m.insert("cut_length".into(), (st.cut_length as FLOAT).into());
            m.insert("rapid_length".into(), (st.rapid_length as FLOAT).into());
            let v = |x: f64, y: f64, z: f64| -> Dynamic {
                Dynamic::from_array(vec![x.into(), y.into(), z.into()])
            };
            m.insert("min".into(), v(b.min.x(), b.min.y(), b.min.z()));
            m.insert("max".into(), v(b.max.x(), b.max.y(), b.max.z()));
            m.insert("min_z".into(), (b.min.z() as FLOAT).into());
            m.insert("max_z".into(), (b.max.z() as FLOAT).into());
            Ok(Dynamic::from_map(m))
        })
    }
}

impl GroupH {
    fn check_key(&self, t: &Toolpath, key: &str) -> Res<()> {
        if t.kind().group_fields(self.group).any(|m| m.key == key) {
            Ok(())
        } else {
            fail(format!(
                "`{}.{key}` is not a field of {}",
                self.group.key(),
                t.kind()
            ))
        }
    }

    fn get(&mut self, key: &str) -> Res<Dynamic> {
        let group = self.group;
        self.tp.clone().with(|_, s, t| {
            self.check_key(t, key)?;
            Ok(toolpath_table(s, t)
                .get(group.key())
                .and_then(|g| g.get(key))
                .map_or(Dynamic::UNIT, to_dynamic))
        })
    }

    fn set(&mut self, key: &str, v: Dynamic) -> Res<()> {
        let path = format!("{}.{key}", self.group.key());
        let id = self.tp.id.clone();
        self.tp.clone().with(|g, _, t| {
            self.check_key(t, key)?;
            let val = param_toml(t.kind(), &path, &v).or_else(fail)?;
            let text = val.as_ref().map(inline);
            g.apply(Edit::set_toolpath_field(&id, &path, text.as_deref()))
        })
    }

    fn local_map(&mut self) -> Res<Map> {
        let group = self.group;
        self.tp.clone().with(|_, s, t| {
            let tt = toolpath_table(s, t);
            Ok(tt
                .get(group.key())
                .and_then(|g| g.as_table())
                .map(|g| {
                    g.iter()
                        .map(|(k, v)| (k.as_str().into(), to_dynamic(v)))
                        .collect()
                })
                .unwrap_or_default())
        })
    }
}

pub(super) fn register(e: &mut Engine) {
    e.register_type_with_name::<ToolpathH>("Toolpath");
    e.register_type_with_name::<GroupH>("ParamGroup");
    e.register_get("id", |h: &mut ToolpathH| h.id.clone());
    e.register_get("type", |h: &mut ToolpathH| -> Res<String> {
        h.with(|_, _, t| Ok(t.kind().as_str().to_owned()))
    });
    e.register_get("name", |h: &mut ToolpathH| -> Res<String> {
        h.with(|_, _, t| Ok(t.name.clone()))
    });
    e.register_set("name", |h: &mut ToolpathH, v: &str| {
        h.set_item("name", Some(v.into()))
    });
    e.register_get("tool", |h: &mut ToolpathH| -> Res<String> {
        h.with(|_, _, t| Ok(t.tool.clone()))
    });
    e.register_set("tool", |h: &mut ToolpathH, v: &str| {
        h.set_item("tool", Some(v.into()))
    });
    e.register_get("enabled", |h: &mut ToolpathH| -> Res<bool> {
        h.with(|_, _, t| Ok(t.is_enabled()))
    });
    e.register_set("enabled", |h: &mut ToolpathH, v: bool| {
        h.set_item("enabled", Some(v.into()))
    });
    e.register_get("template", |h: &mut ToolpathH| -> Res<Dynamic> {
        h.with(|_, _, t| Ok(t.template.clone().map_or(Dynamic::UNIT, Into::into)))
    });
    e.register_get("setup", |h: &mut ToolpathH| -> Res<SetupH> {
        let generation = h.generation;
        let rt = h.rt.clone();
        h.with(|_, s, _| Ok(SetupH::new(rt, generation, &s.id)))
    });
    for g in Group::ALL {
        e.register_get(g.key(), move |h: &mut ToolpathH| h.group(g));
        e.register_set(g.key(), move |h: &mut ToolpathH, m: Map| h.set_group(g, m));
    }
    e.register_indexer_get(GroupH::get);
    e.register_indexer_set(GroupH::set);
    e.register_fn("keys", |h: &mut GroupH| -> Res<Array> {
        Ok(h.local_map()?.keys().map(|k| k.to_string().into()).collect())
    });
    e.register_fn("fields", |h: &mut GroupH| -> Res<Array> {
        let group = h.group;
        h.tp.with(|_, _, t| {
            Ok(t.kind()
                .group_fields(group)
                .map(|m| m.key.to_owned().into())
                .collect())
        })
    });
    e.register_fn("to_map", GroupH::local_map);
    e.register_fn("calculate", |h: &mut ToolpathH| -> Res<Map> {
        let id = h.id.clone();
        let rt = h.rt.clone();
        let mut g = h.rt.at(h.generation)?;
        let r = run_calc(&rt, &mut g, &Targets::Toolpaths(vec![id.clone()]))?;
        match r.into_iter().next() {
            Some(m) => Ok(m.cast::<Map>()),
            None => fail(format!("toolpath `{id}` is no longer in the project")),
        }
    });
    e.register_get("state", ToolpathH::state);
    e.register_get("result_summary", ToolpathH::summary);
    e.register_get("diagnostics", |h: &mut ToolpathH| -> Res<Array> {
        let id = h.id.clone();
        let g = h.rt.at(h.generation)?;
        Ok(g.calc
            .get(&id)
            .map(|c| diag_array(&c.diagnostics))
            .unwrap_or_default())
    });
    e.register_fn("remove", |h: &mut ToolpathH| -> Res<()> {
        let id = h.id.clone();
        h.with(|g, _, _| g.apply(Edit::RemoveToolpath(id)))
    });
    e.register_fn("to_string", |h: &mut ToolpathH| {
        format!("Toolpath({})", h.id)
    });
    e.register_fn("to_string", |h: &mut GroupH| {
        format!("ParamGroup({}.{})", h.tp.id, h.group.key())
    });
    e.register_fn("==", |a: &mut ToolpathH, b: ToolpathH| a.id == b.id);
}
