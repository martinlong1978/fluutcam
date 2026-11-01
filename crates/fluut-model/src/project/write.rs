// SPDX-License-Identifier: MIT OR Apache-2.0

//! Writing the project file in canonical form (D06 §7.2): fixed key order,
//! `=` aligned within each table, absent (inherited or default) fields
//! omitted, numbers in the form they were read or entered.

use super::read::SETUP_KEYS;
use super::*;
use crate::params::Num;
use crate::params::value::quote;
use std::fmt::Write;
use toml::Value;

/// Lines of one table: `(key, value)` written with aligned `=`.
struct Block(Vec<(String, String)>);

impl Block {
    fn new() -> Block {
        Block(Vec::new())
    }

    fn put(&mut self, k: &str, v: String) {
        self.0.push((k.to_owned(), v));
    }

    fn opt(&mut self, k: &str, v: Option<String>) {
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

/// `{ a = x, b = y }` from the parts that are present.
fn inline(parts: &[(&str, Option<String>)]) -> String {
    let p: Vec<String> = parts
        .iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k} = {v}")))
        .collect();
    if p.is_empty() {
        "{}".to_owned()
    } else {
        format!("{{ {} }}", p.join(", "))
    }
}

fn pick(p: &AxisPick) -> String {
    match p {
        AxisPick::Axis(a) => quote(a.as_str()),
        AxisPick::Face(id) => format!("{{ face = {} }}", quote(id)),
        AxisPick::Edge(id) => format!("{{ edge = {} }}", quote(id)),
    }
}

/// A TOML value in inline form; for saved views, whose keys this build
/// does not interpret.
pub(crate) fn toml_value(v: &Value) -> String {
    match v {
        Value::String(s) => quote(s),
        Value::Integer(i) => i.to_string(),
        Value::Float(x) => format!("{x:?}"),
        Value::Boolean(b) => b.to_string(),
        Value::Datetime(d) => d.to_string(),
        Value::Array(a) => {
            let items: Vec<String> = a.iter().map(toml_value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Table(t) => {
            let parts: Vec<String> = t
                .iter()
                .map(|(k, v)| format!("{} = {}", key(k), toml_value(v)))
                .collect();
            if parts.is_empty() {
                "{}".to_owned()
            } else {
                format!("{{ {} }}", parts.join(", "))
            }
        }
    }
}

/// A bare key when TOML allows one, else quoted.
fn key(k: &str) -> String {
    if !k.is_empty()
        && k.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        k.to_owned()
    } else {
        quote(k)
    }
}

/// The file value of Setup key `k`, or `None` when absent.
pub(crate) fn setup_value(s: &Setup, k: &str) -> Option<String> {
    match k {
        "id" => Some(quote(&s.id)),
        "name" => Some(quote(&s.name)),
        "based_on" => s.based_on.as_deref().map(quote),
        "kind" => s.kind.map(|k| quote(k.as_str())),
        "machine" => s.machine.as_deref().map(quote),
        "material" => s.material.as_deref().map(quote),
        "work_offset" => s.work_offset.as_deref().map(quote),
        "geometry" => s.geometry.as_ref().map(|g| {
            inline(&[
                (
                    "bodies",
                    g.bodies.as_ref().map(|ids| {
                        let ids: Vec<String> = ids.iter().map(|i| quote(i)).collect();
                        format!("[{}]", ids.join(", "))
                    }),
                ),
                ("scale", g.scale.as_ref().map(num3)),
                (
                    "mirror",
                    g.mirror.map(|m| format!("[{}, {}, {}]", m[0], m[1], m[2])),
                ),
            ])
        }),
        "orientation" => s.orientation.as_ref().map(|o| match o {
            Orientation::ThreeAxis { tool_axis, x_axis } => inline(&[
                ("tool_axis", Some(pick(tool_axis))),
                ("x_axis", Some(pick(x_axis))),
            ]),
            Orientation::Rotary {
                rotary_axis,
                a_zero_toward,
            } => inline(&[
                ("rotary_axis", Some(pick(rotary_axis))),
                ("a_zero_toward", a_zero_toward.as_ref().map(pick)),
            ]),
        }),
        "index_angle" => s.index_angle.as_ref().map(|a| {
            inline(&[
                ("mode", Some(quote(a.mode.as_str()))),
                ("value", Some(a.value.to_toml())),
            ])
        }),
        "stock" => s.stock.as_ref().map(|st| {
            let kind = Some(quote(st.kind()));
            match st {
                StockSpec::Block { margin, size } => inline(&[
                    ("kind", kind),
                    ("margin", margin.as_ref().map(num3)),
                    ("size", size.as_ref().map(num3)),
                ]),
                StockSpec::Cylinder { radius, length } => inline(&[
                    ("kind", kind),
                    ("radius", Some(radius.to_toml())),
                    ("length", Some(length.to_toml())),
                ]),
                StockSpec::Body { body } => inline(&[("kind", kind), ("body", Some(quote(body)))]),
                StockSpec::FromPrevious => inline(&[("kind", kind)]),
            }
        }),
        "zero" => s.zero.as_ref().map(|z| match z {
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
            ZeroSpec::OnAxis { x } => format!(
                "{{ x = {} }}",
                match x {
                    AxisX::StockLeft => quote("stock_left"),
                    AxisX::StockRight => quote("stock_right"),
                    AxisX::At(n) => n.to_toml(),
                }
            ),
        }),
        "region" => s
            .region
            .as_ref()
            .map(|r| format!("{{ floor_z = {} }}", r.floor_z.to_toml())),
        "clearance" => s.clearance.as_ref().map(|c| match c {
            ClearanceSpec::Plane { offset } => inline(&[
                ("kind", Some(quote("plane"))),
                ("offset", offset.map(Num::to_toml)),
            ]),
            ClearanceSpec::Cylinder { radius } => inline(&[
                ("kind", Some(quote("cylinder"))),
                ("radius", Some(radius.to_toml())),
            ]),
        }),
        _ => None,
    }
}

/// The project file text.
pub fn write(p: &Project) -> String {
    let mut out = String::new();
    let mut b = Block::new();
    b.put(
        "format",
        quote(&format!("{PROJECT_FORMAT}/{PROJECT_MAJOR}")),
    );
    b.put("name", quote(&p.name));
    b.put("units", quote(p.units.as_str()));
    b.write(&mut out, "");
    for g in &p.geometry {
        source(&mut out, g);
    }
    for s in &p.setups {
        setup(&mut out, s);
    }
    for v in &p.views {
        out.push_str("\n[[view]]\n");
        let mut b = Block::new();
        for (k, x) in v {
            b.put(&key(k), toml_value(x));
        }
        b.write(&mut out, "");
    }
    out
}

pub(crate) fn source(out: &mut String, g: &GeometrySource) {
    out.push_str("\n[[geometry]]\n");
    let mut b = Block::new();
    b.put("id", quote(&g.id));
    b.put("path", quote(&g.path));
    b.put("kind", quote(g.kind.as_str()));
    b.opt("hash", g.hash.as_deref().map(quote));
    b.opt("unit", g.unit.as_deref().map(quote));
    b.write(out, "");
    for body in &g.bodies {
        out.push_str("\n  [[geometry.body]]\n");
        let mut b = Block::new();
        b.put("id", quote(&body.id));
        b.opt("name", body.name.as_deref().map(quote));
        b.write(out, "  ");
    }
}

pub(crate) fn setup(out: &mut String, s: &Setup) {
    out.push_str("\n[[setup]]\n");
    let mut b = Block::new();
    for k in SETUP_KEYS {
        b.opt(k, setup_value(s, k));
    }
    b.write(out, "");
    for t in &s.toolpaths {
        toolpath(out, t);
    }
}

pub(crate) fn toolpath(out: &mut String, t: &Toolpath) {
    out.push_str("\n  [[setup.toolpath]]\n");
    let mut b = Block::new();
    b.put("id", quote(&t.id));
    b.put("name", quote(&t.name));
    b.put("type", quote(t.kind().as_str()));
    b.opt("tool", (!t.tool.is_empty()).then(|| quote(&t.tool)));
    b.opt("enabled", t.enabled.map(|e| e.to_string()));
    b.opt("template", t.template.as_deref().map(quote));
    b.write(out, "  ");
    t.params.write(out, "    ");
}
