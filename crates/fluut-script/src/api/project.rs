// SPDX-License-Identifier: MIT OR Apache-2.0

//! `Project` (D12b §3).

use super::model::ModelH;
use super::setup::SetupH;
use super::toolpath::{ToolpathH, report_map, run_calc};
use super::{Res, Rt, State, errors_text, fail};
use crate::perm::Access;
use crate::value::{inline, to_toml};
use fluut_core::{PostOptions, Targets};
use fluut_model::DefRef;
use fluut_model::edit::Edit;
use fluut_model::library::DefKind;
use fluut_model::project::{GeometrySource, Project};
use fluut_model::setup::resolve_setup;
use fluut_model::template::Target;
use rhai::{Array, Dynamic, Engine, Map};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ProjectH {
    rt: Rt,
    generation: u64,
}

/// The first id `prefix<n>` not used in the project.
pub(crate) fn free_id(p: &Project, prefix: &str) -> String {
    let mut n = 1;
    loop {
        let id = format!("{prefix}{n}");
        if !p.ids().any(|x| x == id) {
            return id;
        }
        n += 1;
    }
}

/// Looks an object up by id, then by name; a name used twice is an error.
pub(crate) fn by_name<'a, T>(
    what: &str,
    key: &str,
    items: impl Iterator<Item = &'a T> + Clone,
    id: impl Fn(&T) -> &str,
    name: impl Fn(&T) -> &str,
) -> Res<&'a T> {
    if let Some(x) = items.clone().find(|x| id(x) == key) {
        return Ok(x);
    }
    let mut hits = items.filter(|x| name(x) == key);
    match (hits.next(), hits.next()) {
        (Some(x), None) => Ok(x),
        (Some(_), Some(_)) => fail(format!("two {what}s are named `{key}`; use the id")),
        (None, _) => fail(format!("no {what} has the id or name `{key}`")),
    }
}

/// `to` relative to the folder `from`, with `/` separators and `..` where
/// needed; `to` as it is when the two share no root (another drive).
pub(crate) fn relative(from: &Path, to: &Path) -> String {
    let real = |p: &Path| crate::perm::real_path(&std::path::absolute(p).unwrap_or(p.into()));
    let (Some(from), Some(to)) = (real(from), real(to)) else {
        return to.to_string_lossy().into_owned();
    };
    let a: Vec<Component> = from.components().collect();
    let b: Vec<Component> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if common == 0 {
        return to.to_string_lossy().into_owned();
    }
    let mut parts: Vec<String> = vec!["..".into(); a.len() - common];
    parts.extend(
        b[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

impl ProjectH {
    pub(crate) fn new(rt: Rt, generation: u64) -> ProjectH {
        ProjectH { rt, generation }
    }

    fn with<T>(&self, f: impl FnOnce(&mut State) -> Res<T>) -> Res<T> {
        let mut g = self.rt.at(self.generation)?;
        f(&mut g)
    }

    fn setup_h(&self, id: &str) -> SetupH {
        SetupH::new(self.rt.clone(), self.generation, id)
    }

    fn setups(&mut self) -> Res<Array> {
        let ids: Vec<String> = self.with(|g| {
            Ok(g.session()?
                .project
                .setups
                .iter()
                .map(|s| s.id.clone())
                .collect())
        })?;
        Ok(ids
            .iter()
            .map(|id| Dynamic::from(self.setup_h(id)))
            .collect())
    }

    fn setup(&mut self, key: &str) -> Res<SetupH> {
        let id = self.with(|g| {
            let p = &g.session()?.project;
            by_name("Setup", key, p.setups.iter(), |s| &s.id, |s| &s.name).map(|s| s.id.clone())
        })?;
        Ok(self.setup_h(&id))
    }

    fn toolpaths(&mut self) -> Res<Array> {
        let ids: Vec<String> = self.with(|g| {
            Ok(g.session()?
                .project
                .setups
                .iter()
                .flat_map(|s| s.toolpaths.iter().map(|t| t.id.clone()))
                .collect())
        })?;
        Ok(ids
            .iter()
            .map(|id| Dynamic::from(ToolpathH::new(self.rt.clone(), self.generation, id)))
            .collect())
    }

    fn toolpath(&mut self, key: &str) -> Res<ToolpathH> {
        let id = self.with(|g| {
            let p = &g.session()?.project;
            by_name(
                "toolpath",
                key,
                p.setups.iter().flat_map(|s| s.toolpaths.iter()),
                |t| &t.id,
                |t| &t.name,
            )
            .map(|t| t.id.clone())
        })?;
        Ok(ToolpathH::new(self.rt.clone(), self.generation, &id))
    }

    fn add_setup(&mut self, map: Map) -> Res<SetupH> {
        let id = self.with(|g| {
            let p = &g.session()?.project;
            let id = match map.get("id") {
                Some(v) => v
                    .clone()
                    .into_string()
                    .or_else(|_| fail("`id` must be a string"))?,
                None => free_id(p, "s"),
            };
            let name = format!("Setup {}", p.setups.len() + 1);
            let mut text = format!("[[setup]]\nid = {}\n", inline(&id.clone().into()));
            if !map.contains_key("name") {
                text.push_str(&format!("name = {}\n", inline(&name.into())));
            }
            for (k, v) in &map {
                if k.as_str() == "id" {
                    continue;
                }
                if k.as_str() == "toolpath" || k.as_str() == "toolpaths" {
                    return fail("add toolpaths with setup.add_toolpath");
                }
                let ident = super::setup::IDENT_KEYS.contains(&k.as_str());
                match to_toml(v, !ident).or_else(|e| fail(format!("`{k}`: {e}")))? {
                    Some(x) => text.push_str(&format!("{k} = {}\n", inline(&x))),
                    None => return fail(format!("`{k}` is `()`; leave the key out instead")),
                }
            }
            g.apply(Edit::AddSetup { at: None, text })?;
            Ok(id)
        })?;
        Ok(self.setup_h(&id))
    }

    fn apply_template(&mut self, r: &str, map: Map) -> Res<SetupH> {
        let id = self.with(|g| {
            let s = g.session()?;
            let def = DefRef::parse(r).or_else(|e| fail(format!("`{r}`: {e}")))?;
            let p = &s.project;
            let id = match map.get("id") {
                Some(v) => v.to_string(),
                None => free_id(p, "s"),
            };
            let name = map.get("name").map(|v| v.to_string());
            let tool = map.get("tool").map_or(String::new(), |v| v.to_string());
            let mut taken: Vec<String> = p.ids().map(str::to_owned).collect();
            taken.push(id.clone());
            let mut next = || {
                let mut n = 1;
                loop {
                    let t = format!("t{n}");
                    if !taken.contains(&t) {
                        taken.push(t.clone());
                        return t;
                    }
                    n += 1;
                }
            };
            let edit = s
                .library
                .apply_template(
                    &def,
                    &Target::Setup {
                        at: None,
                        id: id.clone(),
                        name,
                        tool,
                    },
                    &s.tools,
                    &mut next,
                )
                .or_else(|e| fail(e.to_string()))?;
            g.apply(edit)?;
            Ok(id)
        })?;
        Ok(self.setup_h(&id))
    }

    fn save(&mut self) -> Res<()> {
        self.with(|g| {
            let path = match &g.session()?.path {
                Some(p) => p.to_string_lossy().into_owned(),
                None => return fail("the project has no file yet; use save_as"),
            };
            g.path(&path, Access::Write)?;
            g.session_mut()?.save().or_else(|d| fail(d.to_string()))
        })
    }

    fn save_as(&mut self, path: &str) -> Res<()> {
        self.with(|g| {
            let p = g.path(path, Access::Write)?;
            let new_dir = p.parent().map_or_else(PathBuf::new, Path::to_path_buf);
            let s = g.session()?;
            let old_dir = s.dir.clone();
            let had_file = s.path.is_some();
            let moved: Vec<GeometrySource> = if had_file && old_dir != new_dir {
                s.project
                    .geometry
                    .iter()
                    .map(|src| {
                        let abs = old_dir.join(&src.path);
                        let mut n = src.clone();
                        n.path = relative(&new_dir, &abs);
                        n
                    })
                    .filter(|n| s.project.source(&n.id).is_some_and(|o| o.path != n.path))
                    .collect()
            } else {
                Vec::new()
            };
            for n in &moved {
                g.apply(Edit::set_geometry(n))?;
            }
            std::fs::create_dir_all(&new_dir)
                .or_else(|e| fail(format!("cannot create {}: {e}", new_dir.display())))?;
            let s = g.session_mut()?;
            s.path = Some(p);
            s.dir = new_dir;
            s.save().or_else(|d| fail(d.to_string()))
        })
    }

    fn calculate_all(&mut self) -> Res<Array> {
        let rt = self.rt.clone();
        self.with(|g| run_calc(&rt, g, &Targets::All))
    }

    fn post(&mut self, target: Dynamic, path: &str, opts: Map) -> Res<Map> {
        let rt = self.rt.clone();
        self.with(|g| {
            let out = g.path(path, Access::Write)?;
            let setups: Option<Vec<String>> = if let Some(s) = target.clone().try_cast::<SetupH>() {
                Some(vec![s.id().to_owned()])
            } else if target.clone().into_string().is_ok_and(|t| t == "all") {
                None
            } else {
                return fail("the post target is `\"all\"` or a Setup");
            };
            let text = |k: &str, d: &str| opts.get(k).map_or(d.to_owned(), |v| v.to_string());
            for k in opts.keys() {
                if !["post", "date", "time", "notes"].contains(&k.as_str()) {
                    return fail(format!(
                        "`{k}` is not a post option (post, date, time, notes)"
                    ));
                }
            }
            let po = PostOptions {
                date: text("date", &g.date),
                time: text("time", &g.time),
                notes: text("notes", ""),
            };
            let post_id = match opts.get("post") {
                Some(v) => v.to_string(),
                None => machine_post(g, setups.as_deref())?,
            };
            let s = g.session_mut()?;
            let store = s.library.mem_store(DefKind::Post);
            let r = rt
                .ctl
                .host_call(|| s.post_setups(&post_id, &store, &po, setups.as_deref()));
            let (output, reports) = match r {
                Ok(x) => x,
                Err(d) => return fail(format!("post failed: {}", errors_text(&d))),
            };
            let mut files = Array::new();
            for (i, text) in output.files.iter().enumerate() {
                let f = if i == 0 {
                    out.clone()
                } else {
                    numbered(&out, i + 1)
                };
                std::fs::write(&f, text)
                    .or_else(|e| fail(format!("cannot write {}: {e}", f.display())))?;
                files.push(f.to_string_lossy().into_owned().into());
            }
            let mut m = Map::new();
            m.insert("files".into(), files.into());
            m.insert(
                "warnings".into(),
                output
                    .warnings
                    .iter()
                    .map(|w| Dynamic::from(w.clone()))
                    .collect::<Array>()
                    .into(),
            );
            m.insert(
                "reports".into(),
                reports
                    .iter()
                    .map(|r| Dynamic::from_map(report_map(r)))
                    .collect::<Array>()
                    .into(),
            );
            Ok(m)
        })
    }
}

/// The post of the machine of the first chosen Setup that names one.
fn machine_post(g: &State, setups: Option<&[String]>) -> Res<String> {
    let s = g.session()?;
    for st in &s.project.setups {
        if setups.is_some_and(|ids| !ids.contains(&st.id)) {
            continue;
        }
        let Ok(rs) = resolve_setup(&s.project, &st.id) else {
            continue;
        };
        if let Some(m) = rs.machine
            && let Ok(r) = DefRef::parse(&m)
            && let Ok(mach) = s.library.machine(&r)
            && let Some(p) = mach.fields.post
        {
            return Ok(p);
        }
    }
    fail("no post: give `#{ post: \"…\" }` or a Setup machine with a post")
}

/// `out.ngc` → `out.2.ngc` for the files after the first.
fn numbered(p: &Path, n: usize) -> PathBuf {
    let stem = p
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let name = match p.extension() {
        Some(e) => format!("{stem}.{n}.{}", e.to_string_lossy()),
        None => format!("{stem}.{n}"),
    };
    p.with_file_name(name)
}

pub(super) fn register(e: &mut Engine) {
    e.register_type_with_name::<ProjectH>("Project");
    e.register_get("name", |h: &mut ProjectH| -> Res<String> {
        h.with(|g| Ok(g.session()?.project.name.clone()))
    });
    e.register_set("name", |h: &mut ProjectH, v: &str| -> Res<()> {
        h.with(|g| g.apply(Edit::SetName(v.into())))
    });
    e.register_get("units", |h: &mut ProjectH| -> Res<String> {
        h.with(|g| Ok(g.session()?.project.units.as_str().to_owned()))
    });
    e.register_set("units", |h: &mut ProjectH, v: &str| -> Res<()> {
        h.with(|g| g.apply(Edit::SetUnits(v.into())))
    });
    e.register_get("path", |h: &mut ProjectH| -> Res<Dynamic> {
        h.with(|g| {
            Ok(g.session()?
                .path
                .as_ref()
                .map_or(Dynamic::UNIT, |p| p.to_string_lossy().into_owned().into()))
        })
    });
    e.register_get("model", |h: &mut ProjectH| {
        ModelH::new(h.rt.clone(), h.generation)
    });
    e.register_get("setups", ProjectH::setups);
    e.register_fn("setup", ProjectH::setup);
    e.register_get("toolpaths", ProjectH::toolpaths);
    e.register_fn("toolpath", ProjectH::toolpath);
    e.register_fn("add_setup", ProjectH::add_setup);
    e.register_fn("apply_template", |h: &mut ProjectH, r: &str| {
        h.apply_template(r, Map::new())
    });
    e.register_fn("apply_template", ProjectH::apply_template);
    e.register_fn("save", ProjectH::save);
    e.register_fn("save_as", ProjectH::save_as);
    e.register_fn("calculate_all", ProjectH::calculate_all);
    e.register_fn("post", |h: &mut ProjectH, target: Dynamic, path: &str| {
        h.post(target, path, Map::new())
    });
    e.register_fn("post", ProjectH::post);
}
