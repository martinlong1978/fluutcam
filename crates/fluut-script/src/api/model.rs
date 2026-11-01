// SPDX-License-Identifier: MIT OR Apache-2.0

//! `Model`, `Body` and `Face` (D12b §6): import, re-import, and read-only
//! geometry queries in the model frame M.

use super::project::{by_name, free_id, relative};
use super::{Res, Rt, State, diag_array, errors_text, fail};
use crate::perm::Access;
use fluut_core::entities::{BodyInfo, Bounds, FaceInfo, Param, bodies};
use fluut_model::edit::Edit;
use fluut_model::project::{BodyEntry, GeometrySource, SourceKind};
use fluut_model::{Diagnostic, Severity};
use rhai::{Array, Dynamic, Engine, FLOAT, INT, Map};

#[derive(Clone, Debug)]
pub struct ModelH {
    rt: Rt,
    generation: u64,
}

#[derive(Clone, Debug)]
pub struct BodyH {
    rt: Rt,
    generation: u64,
    pub(crate) id: String,
}

#[derive(Clone, Debug)]
pub struct FaceH {
    rt: Rt,
    generation: u64,
    body: String,
    pub(crate) id: String,
}

fn bounds_map(b: &Bounds) -> Dynamic {
    let v = |x: &[f64]| -> Dynamic {
        x.iter()
            .map(|x| Dynamic::from(*x as FLOAT))
            .collect::<Array>()
            .into()
    };
    let mut m = Map::new();
    m.insert("min".into(), v(&b[..3]));
    m.insert("max".into(), v(&b[3..]));
    Dynamic::from_map(m)
}

fn param(p: &Param) -> Dynamic {
    match p {
        Param::Num(x) => (*x as FLOAT).into(),
        Param::Bool(b) => (*b).into(),
        Param::Vec(v) => v
            .iter()
            .map(|x| Dynamic::from(*x as FLOAT))
            .collect::<Array>()
            .into(),
    }
}

/// The loaded bodies, named as the project names them.
fn all_bodies(g: &State) -> Res<Vec<BodyInfo>> {
    let s = g.session()?;
    let mut b = bodies(&s.store);
    for x in &mut b {
        if let Some(n) = s
            .project
            .source(&x.source)
            .and_then(|src| src.bodies.iter().find(|e| e.id == x.id))
            .and_then(|e| e.name.clone())
        {
            x.name = n;
        }
    }
    Ok(b)
}

fn source_kind(path: &str) -> Res<SourceKind> {
    let ext = std::path::Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    Ok(match ext.as_str() {
        "step" | "stp" => SourceKind::Step,
        "stl" => SourceKind::Stl,
        "dxf" => SourceKind::Dxf,
        "svg" => SourceKind::Svg,
        _ => return fail(format!("`{path}`: not a STEP, STL, DXF or SVG file")),
    })
}

impl ModelH {
    pub(crate) fn new(rt: Rt, generation: u64) -> ModelH {
        ModelH { rt, generation }
    }

    fn body_h(&self, id: &str) -> BodyH {
        BodyH {
            rt: self.rt.clone(),
            generation: self.generation,
            id: id.to_owned(),
        }
    }

    fn import(&mut self, path: &str, unit: Option<String>) -> Res<Array> {
        let mut g = self.rt.at(self.generation)?;
        let kind = source_kind(path)?;
        let file = g.path(path, Access::Read)?;
        let s = g.session()?;
        if s.path.is_none() {
            return fail(
                "save the project first (save_as): geometry paths are stored relative to its folder",
            );
        }
        let mut src = GeometrySource {
            id: free_id(&s.project, "g"),
            path: relative(&s.dir, &file),
            kind,
            hash: None,
            unit,
            bodies: Vec::new(),
        };
        let s = g.session_mut()?;
        let dir = s.dir.clone();
        let diags: Vec<Diagnostic> = self.rt.ctl.host_call(|| s.store.load(&src, &dir));
        if diags.iter().any(|d| d.severity == Severity::Error) {
            return fail(format!("import failed: {}", errors_text(&diags)));
        }
        for d in &diags {
            self.rt.log(crate::host::Level::Warn, &d.to_string());
        }
        let Some(loaded) = s.store.source(&src.id).cloned() else {
            return fail(format!("import failed: `{path}` gave no geometry"));
        };
        src.hash = Some(loaded.hash.clone());
        src.bodies = bodies(&s.store)
            .into_iter()
            .filter(|b| b.source == src.id)
            .map(|b| BodyEntry {
                name: (!b.name.is_empty()).then_some(b.name),
                id: b.id,
            })
            .collect();
        let ids: Vec<String> = src.bodies.iter().map(|b| b.id.clone()).collect();
        g.apply(Edit::add_geometry(None, &src))?;
        Ok(ids
            .iter()
            .map(|id| Dynamic::from(self.body_h(id)))
            .collect())
    }

    fn reimport(&mut self, id: &str) -> Res<Map> {
        let mut g = self.rt.at(self.generation)?;
        let s = g.session()?;
        let Some(src) = s.project.source(id) else {
            return fail(format!("geometry `{id}` is not in the project"));
        };
        let file = s.dir.join(&src.path).to_string_lossy().into_owned();
        g.path(&file, Access::Read)?;
        g.ensure_group()?;
        let s = g.session_mut()?;
        let r = self
            .rt
            .ctl
            .host_call(|| s.reimport(id))
            .or_else(|d| fail(format!("re-import failed: {}", errors_text(&d))))?;
        let mut m = Map::new();
        m.insert("matched".into(), (r.matching.ids.len() as INT).into());
        m.insert(
            "unmatched".into(),
            r.unmatched
                .iter()
                .map(|u| Dynamic::from(u.id.clone()))
                .collect::<Array>()
                .into(),
        );
        m.insert("diagnostics".into(), diag_array(&r.diagnostics).into());
        Ok(m)
    }

    fn sources(&mut self) -> Res<Array> {
        let g = self.rt.at(self.generation)?;
        Ok(g.session()?
            .project
            .geometry
            .iter()
            .map(|s| {
                let mut m = Map::new();
                m.insert("id".into(), s.id.clone().into());
                m.insert("path".into(), s.path.clone().into());
                m.insert("kind".into(), s.kind.as_str().into());
                m.insert(
                    "unit".into(),
                    s.unit.clone().map_or(Dynamic::UNIT, Into::into),
                );
                Dynamic::from_map(m)
            })
            .collect())
    }

    fn bodies(&mut self) -> Res<Array> {
        let g = self.rt.at(self.generation)?;
        Ok(all_bodies(&g)?
            .iter()
            .map(|b| Dynamic::from(self.body_h(&b.id)))
            .collect())
    }

    fn body(&mut self, key: &str) -> Res<BodyH> {
        let g = self.rt.at(self.generation)?;
        let all = all_bodies(&g)?;
        let b = by_name("body", key, all.iter(), |b| &b.id, |b| &b.name)?;
        Ok(self.body_h(&b.id))
    }
}

impl BodyH {
    fn info<T>(&self, f: impl FnOnce(&BodyInfo) -> T) -> Res<T> {
        let g = self.rt.at(self.generation)?;
        let all = all_bodies(&g)?;
        match all.iter().find(|b| b.id == self.id) {
            Some(b) => Ok(f(b)),
            None => fail(format!("body `{}` is no longer in the model", self.id)),
        }
    }

    fn face_h(&self, id: &str) -> FaceH {
        FaceH {
            rt: self.rt.clone(),
            generation: self.generation,
            body: self.id.clone(),
            id: id.to_owned(),
        }
    }
}

impl FaceH {
    fn info<T>(&self, f: impl FnOnce(&FaceInfo) -> T) -> Res<T> {
        let g = self.rt.at(self.generation)?;
        let all = all_bodies(&g)?;
        match all
            .iter()
            .find(|b| b.id == self.body)
            .and_then(|b| b.faces.iter().find(|x| x.id == self.id))
        {
            Some(x) => Ok(f(x)),
            None => fail(format!("face `{}` is no longer in the model", self.id)),
        }
    }
}

pub(super) fn register(e: &mut Engine) {
    e.register_type_with_name::<ModelH>("Model");
    e.register_type_with_name::<BodyH>("Body");
    e.register_type_with_name::<FaceH>("Face");
    e.register_fn("import", |h: &mut ModelH, path: &str| h.import(path, None));
    e.register_fn("import", |h: &mut ModelH, path: &str, unit: &str| {
        h.import(path, Some(unit.to_owned()))
    });
    e.register_fn("reimport", ModelH::reimport);
    e.register_get("sources", ModelH::sources);
    e.register_get("bodies", ModelH::bodies);
    e.register_fn("body", ModelH::body);

    e.register_get("id", |h: &mut BodyH| h.id.clone());
    e.register_get("name", |h: &mut BodyH| h.info(|b| b.name.clone()));
    e.register_get("source", |h: &mut BodyH| h.info(|b| b.source.clone()));
    e.register_get("kind", |h: &mut BodyH| h.info(|b| b.kind.to_owned()));
    e.register_get("bounds", |h: &mut BodyH| h.info(|b| bounds_map(&b.bounds)));
    e.register_get("faces", |h: &mut BodyH| -> Res<Array> {
        let ids = h.info(|b| b.faces.iter().map(|f| f.id.clone()).collect::<Vec<_>>())?;
        Ok(ids.iter().map(|id| Dynamic::from(h.face_h(id))).collect())
    });
    e.register_fn("face", |h: &mut BodyH, id: &str| -> Res<FaceH> {
        if h.info(|b| b.faces.iter().any(|f| f.id == id))? {
            Ok(h.face_h(id))
        } else {
            fail(format!("body `{}` has no face `{id}`", h.id))
        }
    });
    e.register_fn("to_string", |h: &mut BodyH| format!("Body({})", h.id));

    e.register_get("id", |h: &mut FaceH| h.id.clone());
    e.register_get("body", |h: &mut FaceH| h.body.clone());
    e.register_get("kind", |h: &mut FaceH| h.info(|f| f.kind.to_owned()));
    e.register_get("bounds", |h: &mut FaceH| h.info(|f| bounds_map(&f.bounds)));
    e.register_get("params", |h: &mut FaceH| -> Res<Map> {
        h.info(|f| {
            f.params
                .iter()
                .map(|(k, v)| ((*k).into(), param(v)))
                .collect()
        })
    });
    e.register_fn("to_string", |h: &mut FaceH| format!("Face({})", h.id));
}
