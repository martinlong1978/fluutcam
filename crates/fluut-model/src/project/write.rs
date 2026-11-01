// SPDX-License-Identifier: MIT OR Apache-2.0

//! Writing the project file in canonical form (D06 §7.2): fixed key order,
//! `=` aligned within each table, absent fields omitted, numbers in the
//! form they were read or entered.

use super::*;
use crate::params::Num;
use crate::params::value::quote;
use std::fmt::Write;

/// Lines of one table: `(key, value)` written with aligned `=`.
struct Block(Vec<(&'static str, String)>);

impl Block {
    fn new() -> Block {
        Block(Vec::new())
    }

    fn put(&mut self, k: &'static str, v: String) {
        self.0.push((k, v));
    }

    fn opt(&mut self, k: &'static str, v: Option<String>) {
        if let Some(v) = v {
            self.put(k, v);
        }
    }

    fn write(&self, out: &mut String, indent: &str) {
        let w = self.0.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        for (k, v) in &self.0 {
            let _ = writeln!(out, "{indent}{k:<w$} = {v}");
        }
    }
}

fn num3(n: &[Num; 3]) -> String {
    format!(
        "[{}, {}, {}]",
        n[0].to_toml(),
        n[1].to_toml(),
        n[2].to_toml()
    )
}

/// The project file text.
pub fn write(p: &Project) -> String {
    let mut out = String::new();
    let mut b = Block::new();
    b.put("format", quote(&p.format));
    b.put("name", quote(&p.name));
    b.put("units", quote(p.units.as_str()));
    b.write(&mut out, "");
    for g in &p.geometry {
        out.push_str("\n[[geometry]]\n");
        let mut b = Block::new();
        b.put("id", quote(&g.id));
        b.put("path", quote(&g.path));
        b.put("kind", quote(g.kind.as_str()));
        b.opt("hash", g.hash.as_deref().map(quote));
        b.opt("unit", g.unit.as_deref().map(quote));
        b.write(&mut out, "");
        for body in &g.bodies {
            out.push_str("\n  [[geometry.body]]\n");
            let mut b = Block::new();
            b.put("id", quote(&body.id));
            b.opt("name", body.name.as_deref().map(quote));
            b.write(&mut out, "  ");
        }
    }
    for s in &p.setups {
        setup(&mut out, s);
    }
    out
}

fn setup(out: &mut String, s: &Setup) {
    out.push_str("\n[[setup]]\n");
    let mut b = Block::new();
    b.put("id", quote(&s.id));
    b.put("name", quote(&s.name));
    b.put("kind", quote(s.kind.as_str()));
    b.opt("machine", s.machine.as_deref().map(quote));
    b.opt("material", s.material.as_deref().map(quote));
    b.opt("work_offset", s.work_offset.as_deref().map(quote));
    b.opt(
        "geometry",
        s.bodies.as_ref().map(|ids| {
            let ids: Vec<String> = ids.iter().map(|i| quote(i)).collect();
            format!("{{ bodies = [{}] }}", ids.join(", "))
        }),
    );
    b.opt(
        "orientation",
        s.orientation.map(|o| {
            format!(
                "{{ tool_axis = {}, x_axis = {} }}",
                quote(o.tool_axis.as_str()),
                quote(o.x_axis.as_str())
            )
        }),
    );
    b.opt(
        "stock",
        s.stock.as_ref().map(|st| match st {
            StockSpec::Block { margin } => {
                format!("{{ kind = \"block\", margin = {} }}", num3(margin))
            }
        }),
    );
    b.opt(
        "zero",
        s.zero.as_ref().map(|z| match z {
            ZeroSpec::StockTopCorner(c) => format!(
                "{{ method = \"stock_top_corner\", corner = {} }}",
                quote(c.as_str())
            ),
            ZeroSpec::StockBottomCorner(c) => format!(
                "{{ method = \"stock_bottom_corner\", corner = {} }}",
                quote(c.as_str())
            ),
            ZeroSpec::StockTopCentre => "{ method = \"stock_top_centre\" }".to_owned(),
            ZeroSpec::Point(at) => format!("{{ method = \"point\", at = {} }}", num3(at)),
        }),
    );
    b.opt(
        "clearance",
        s.clearance
            .as_ref()
            .map(|c| format!("{{ kind = \"plane\", offset = {} }}", c.offset.to_toml())),
    );
    b.write(out, "");
    for t in &s.toolpaths {
        toolpath(out, t);
    }
}

fn toolpath(out: &mut String, t: &Toolpath) {
    out.push_str("\n  [[setup.toolpath]]\n");
    let mut b = Block::new();
    b.put("id", quote(&t.id));
    b.put("name", quote(&t.name));
    b.put("type", quote(t.kind().as_str()));
    b.put("tool", quote(&t.tool));
    b.opt("enabled", t.enabled.map(|e| e.to_string()));
    b.opt("template", t.template.as_deref().map(quote));
    b.write(out, "  ");
    t.params.write(out, "    ");
}
