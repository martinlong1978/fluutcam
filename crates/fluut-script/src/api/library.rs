// SPDX-License-Identifier: MIT OR Apache-2.0

//! `Library` (D12b §7): read-only views of the definition library.

use super::{LibraryH, Res, fail};
use fluut_model::DefRef;
use fluut_model::library::DefKind;
use fluut_model::tools::Tool;
use rhai::{Array, Dynamic, Engine, FLOAT, INT, Map};
use std::collections::BTreeSet;

fn tool_map(lib: &str, t: &Tool) -> Map {
    let mut m = Map::new();
    let opt = |x: Option<f64>| x.map_or(Dynamic::UNIT, |v| (v as FLOAT).into());
    m.insert("id".into(), t.id.clone().into());
    m.insert("ref".into(), format!("{lib}#{}", t.id).into());
    m.insert("name".into(), t.name.clone().into());
    m.insert("number".into(), (t.number as INT).into());
    let shape = format!("{:?}", t.shape).to_ascii_lowercase();
    m.insert("shape".into(), shape.into());
    m.insert("diameter".into(), (t.diameter as FLOAT).into());
    m.insert("corner_radius".into(), opt(t.corner_radius));
    m.insert("flute_length".into(), opt(t.flute_length));
    m
}

impl LibraryH {
    fn refs(&mut self, kind: DefKind) -> Res<Array> {
        let g = self.rt.at(self.generation)?;
        let set: BTreeSet<String> = g
            .session()?
            .library
            .entries()
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| format!("{}@{}", e.def.id, e.def.version))
            .collect();
        Ok(set.into_iter().map(Into::into).collect())
    }

    fn tools(&mut self, lib: &str) -> Res<Array> {
        let g = self.rt.at(self.generation)?;
        let s = g.session()?;
        let r = DefRef::parse(lib).or_else(|e| fail(format!("`{lib}`: {e}")))?;
        let Some(mut l) = s.tools.library(&r.id, r.version) else {
            return fail(format!("tool library `{lib}` is not in the library"));
        };
        // The library's own tools, then those it inherits.
        let mut seen = BTreeSet::new();
        let mut out = Array::new();
        for _ in 0..fluut_model::MAX_DEPTH {
            for t in &l.tools {
                if seen.insert(t.id.clone()) {
                    out.push(Dynamic::from_map(tool_map(lib, t)));
                }
            }
            let Some(parent) = l.extends.as_deref().and_then(|p| DefRef::parse(p).ok()) else {
                break;
            };
            match s.tools.library(&parent.id, parent.version) {
                Some(p) => l = p,
                None => break,
            }
        }
        Ok(out)
    }

    fn tool(&mut self, r: &str) -> Res<Dynamic> {
        let g = self.rt.at(self.generation)?;
        let s = g.session()?;
        let lib = r.split_once('#').map_or("", |(l, _)| l);
        Ok(s.tools
            .get(r)
            .map_or(Dynamic::UNIT, |t| Dynamic::from_map(tool_map(lib, t))))
    }
}

pub(super) fn register(e: &mut Engine) {
    e.register_fn("tools", LibraryH::tools);
    e.register_fn("tool", LibraryH::tool);
    e.register_get("machines", |h: &mut LibraryH| h.refs(DefKind::Machine));
    e.register_get("posts", |h: &mut LibraryH| h.refs(DefKind::Post));
    e.register_get("templates", |h: &mut LibraryH| h.refs(DefKind::Template));
}
