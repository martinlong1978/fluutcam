// SPDX-License-Identifier: MIT OR Apache-2.0

//! The inspector (D13 §4, S 08 §8.5), generated from the D15 field metadata.
//!
//! A toolpath shows the five groups of spec 03 §3.10.2 as tabs. Each field
//! has its label, its value in the project unit, its default when unset,
//! whether `visible_if` shows it, and the validation messages that name it.
//! Number, enum and bool fields are edited here; heights, selections and
//! points are shown read-only until their pickers exist.

use super::entry::{EntryUnit, evaluate};
use super::visible::visible;
use super::{Selection, fmt_num};
use crate::jobs::Edit;
use crate::session::Snapshot;
use crate::text::{data, diagnostic, field_label, tr, tr_args};
use fluut_model::params::{ConstVal, DefaultRule};
use fluut_model::project::{DisplayUnit, StockSpec, Toolpath, ZeroSpec};
use fluut_model::tools::Tool;
use fluut_model::{Diagnostic, FieldKind, FieldMeta, Group, Num, PValue, Params, Severity};

/// How a field is edited.
#[derive(Clone, PartialEq, Debug)]
pub enum Widget {
    /// Text entry with expressions; `unit` is the suffix shown.
    Number {
        unit: Option<&'static str>,
    },
    /// One of the options `(value, label)`.
    Enum {
        options: Vec<(String, String)>,
    },
    Bool,
    ReadOnly,
}

#[derive(Clone, PartialEq, Debug)]
pub struct DiagVm {
    pub error: bool,
    pub code: String,
    pub text: String,
}

impl DiagVm {
    fn of(d: &Diagnostic) -> DiagVm {
        DiagVm {
            error: d.severity == Severity::Error,
            code: d.code.clone(),
            text: diagnostic(d),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct FieldVm {
    /// `group.key` or `group.key.sub`.
    pub path: String,
    pub label: String,
    pub widget: Widget,
    /// Effective value: the local value, else the default. Numbers in the
    /// display unit, without the unit; empty when there is none.
    pub value: String,
    /// The field has a local value (it is written to the file).
    pub set: bool,
    /// "Default: …", shown when the field is unset.
    pub default_text: String,
    pub visible: bool,
    pub diagnostics: Vec<DiagVm>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct TabVm {
    pub group: Group,
    pub label: String,
    pub fields: Vec<FieldVm>,
    pub errors: usize,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ToolpathVm {
    pub id: String,
    pub title: String,
    /// Type, Setup and tool.
    pub subtitle: String,
    pub tabs: Vec<TabVm>,
    /// Diagnostics that name no field.
    pub general: Vec<DiagVm>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct SetupVm {
    pub title: String,
    /// `(label, value)`, read-only in M5.
    pub rows: Vec<(String, String)>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum InspectorVm {
    Empty(String),
    Setup(SetupVm),
    Toolpath(ToolpathVm),
}

/// Display unit symbol and scale of a field kind.
fn display(kind: FieldKind, unit: DisplayUnit) -> (Option<&'static str>, f64) {
    let inch = unit == DisplayUnit::Inch;
    match kind {
        FieldKind::Length => (Some(if inch { "in" } else { "mm" }), unit.mm()),
        FieldKind::Feed => (Some(if inch { "in/min" } else { "mm/min" }), unit.mm()),
        FieldKind::Speed => (Some("rpm"), 1.0),
        FieldKind::Angle => (Some("°"), 1.0),
        _ => (None, 1.0),
    }
}

fn num_text(v: f64, kind: FieldKind, unit: DisplayUnit) -> String {
    fmt_num(v / display(kind, unit).1)
}

fn num_with_unit(v: f64, kind: FieldKind, unit: DisplayUnit) -> String {
    match display(kind, unit).0 {
        Some("°") => format!("{}°", num_text(v, kind, unit)),
        Some(u) => format!("{} {u}", num_text(v, kind, unit)),
        None => num_text(v, kind, unit),
    }
}

fn point_text(p: &[Num; 3]) -> String {
    format!(
        "({}, {}, {})",
        fmt_num(p[0].v),
        fmt_num(p[1].v),
        fmt_num(p[2].v)
    )
}

/// Text of a stored value, in the display unit for numbers.
fn value_text(v: &PValue, kind: FieldKind, unit: DisplayUnit) -> String {
    match v {
        PValue::Num(n) => num_text(n.v, kind, unit),
        PValue::Bool(b) => tr(if *b { "inspector-yes" } else { "inspector-no" }),
        PValue::Enum(s) => data(s),
        PValue::Height(h) => {
            let off = h.offset();
            if off == 0.0 {
                data(&h.reference)
            } else {
                let sign = if off < 0.0 { "−" } else { "+" };
                format!(
                    "{} {sign} {}",
                    h.reference,
                    num_with_unit(off.abs(), FieldKind::Length, unit)
                )
            }
        }
        PValue::Selection(s) => tr_args("inspector-items", &[("count", &s.len().to_string())]),
        PValue::Point(p) => point_text(p),
        PValue::Points(ps) => ps.iter().map(point_text).collect::<Vec<_>>().join(" "),
        PValue::Table(_) => String::new(),
    }
}

/// The default as a value, when it is a constant.
fn const_value(m: &FieldMeta) -> Option<PValue> {
    match m.default {
        DefaultRule::Const(ConstVal::Num(v)) => Some(PValue::Num(Num::float(v))),
        DefaultRule::Const(ConstVal::Bool(b)) => Some(PValue::Bool(b)),
        DefaultRule::Const(ConstVal::Str(s)) => Some(PValue::Enum(s.to_owned())),
        DefaultRule::Const(ConstVal::Height(r, o)) => {
            Some(PValue::Height(fluut_model::HeightSpec::new(r, o)))
        }
        _ => None,
    }
}

struct Cx<'a> {
    tp: &'a Toolpath,
    tool: Option<&'a Tool>,
    material: Option<&'a str>,
    unit: DisplayUnit,
}

impl Cx<'_> {
    fn meta(&self, path: &str) -> Option<&'static FieldMeta> {
        Params::meta(self.tp.kind(), path)
    }

    /// Value of a preset key for this tool and material.
    fn preset(&self, key: &str) -> Option<f64> {
        self.tool?.preset(self.material)?.get(key)
    }

    /// The effective value as text: local value, else constant default,
    /// else the tool preset.
    fn effective(&self, path: &str) -> Option<String> {
        let m = self.meta(path)?;
        if let Some(v) = self.tp.params.get(path) {
            return Some(match v {
                PValue::Num(n) => n.v.to_string(),
                PValue::Enum(s) => s.clone(),
                PValue::Bool(b) => b.to_string(),
                other => value_text(other, m.kind, self.unit),
            });
        }
        match m.default {
            DefaultRule::Const(ConstVal::Num(v)) => Some(v.to_string()),
            DefaultRule::Const(ConstVal::Str(s)) => Some(s.to_owned()),
            DefaultRule::Const(ConstVal::Bool(b)) => Some(b.to_string()),
            DefaultRule::FromTool(k) => self.preset(k).map(|v| v.to_string()),
            _ => None,
        }
    }

    fn default_text(&self, m: &FieldMeta) -> String {
        let value = match m.default {
            DefaultRule::None => return tr("inspector-default-none"),
            DefaultRule::Const(_) => {
                let v = const_value(m).expect("constant");
                match (&v, display(m.kind, self.unit).0) {
                    (PValue::Num(n), Some(_)) => num_with_unit(n.v, m.kind, self.unit),
                    _ => value_text(&v, m.kind, self.unit),
                }
            }
            DefaultRule::FromTool(k) => {
                let src = tr_args("inspector-default-preset", &[("key", k)]);
                match self.preset(k) {
                    Some(v) => format!("{} ({src})", num_with_unit(v, m.kind, self.unit)),
                    None => src,
                }
            }
            DefaultRule::Computed(f) => data(f),
        };
        tr_args("inspector-default", &[("value", &value)])
    }

    fn widget(&self, m: &FieldMeta) -> Widget {
        match m.kind {
            FieldKind::Length
            | FieldKind::Feed
            | FieldKind::Speed
            | FieldKind::Angle
            | FieldKind::Count => Widget::Number {
                unit: display(m.kind, self.unit).0,
            },
            FieldKind::Bool => Widget::Bool,
            FieldKind::Enum(opts) => Widget::Enum {
                options: opts.iter().map(|o| ((*o).to_owned(), data(*o))).collect(),
            },
            _ => Widget::ReadOnly,
        }
    }

    fn field(&self, path: String, m: &FieldMeta, diags: &[Diagnostic]) -> FieldVm {
        let local = self.tp.params.get(&path);
        let value = match local.cloned().or_else(|| const_value(m)) {
            Some(v) => value_text(&v, m.kind, self.unit),
            None => match m.default {
                DefaultRule::FromTool(k) => self
                    .preset(k)
                    .map_or_else(String::new, |v| num_text(v, m.kind, self.unit)),
                _ => String::new(),
            },
        };
        let kind = self.tp.kind();
        FieldVm {
            label: field_label(m),
            widget: self.widget(m),
            value,
            set: local.is_some(),
            default_text: self.default_text(m),
            visible: visible(kind, &path, |p| self.effective(p)),
            diagnostics: diags
                .iter()
                .filter(|d| d.field.as_deref() == Some(path.as_str()))
                .map(DiagVm::of)
                .collect(),
            path,
        }
    }
}

impl InspectorVm {
    pub fn new(snap: Option<&Snapshot>, sel: &Selection) -> InspectorVm {
        let empty = || InspectorVm::Empty(tr("inspector-empty"));
        let Some(s) = snap else {
            return empty();
        };
        let unit = s.project.units;
        match sel {
            Selection::Toolpath(id) => match s.project.toolpath(id) {
                Some((setup, tp)) => {
                    let cx = Cx {
                        tp,
                        tool: s.tools.get(&tp.tool),
                        material: setup.material.as_deref(),
                        unit,
                    };
                    InspectorVm::Toolpath(toolpath_vm(&cx, &setup.name, s.diagnostics(id)))
                }
                None => empty(),
            },
            Selection::Setup(id) | Selection::Face { setup: id, .. } => match s.project.setup(id) {
                Some(st) => InspectorVm::Setup(setup_vm(st, unit)),
                None => empty(),
            },
            Selection::None => empty(),
        }
    }
}

fn toolpath_vm(cx: &Cx, setup: &str, diags: &[Diagnostic]) -> ToolpathVm {
    let leaves = Params::leaf_fields(cx.tp.kind());
    let tabs = Group::ALL
        .into_iter()
        .map(|g| {
            let prefix = format!("{}.", g.key());
            let fields: Vec<FieldVm> = leaves
                .iter()
                .filter(|(p, _)| p.starts_with(&prefix))
                .map(|(p, m)| cx.field(p.clone(), m, diags))
                .collect();
            let errors = fields
                .iter()
                .flat_map(|f| f.diagnostics.iter())
                .filter(|d| d.error)
                .count();
            TabVm {
                group: g,
                label: tr(&format!("group-{}", g.key())),
                fields,
                errors,
            }
        })
        .collect();
    let paths: Vec<&str> = leaves.iter().map(|(p, _)| p.as_str()).collect();
    let general = diags
        .iter()
        .filter(|d| d.field.as_deref().is_none_or(|f| !paths.contains(&f)))
        .map(DiagVm::of)
        .collect();
    let tool = cx.tool.map_or_else(
        || data(&cx.tp.tool),
        |t| data(if t.name.is_empty() { &t.id } else { &t.name }),
    );
    ToolpathVm {
        id: cx.tp.id.clone(),
        title: data(&cx.tp.name),
        subtitle: format!("{} · {setup} · {tool}", cx.tp.kind().as_str()),
        tabs,
        general,
    }
}

fn setup_vm(s: &fluut_model::project::Setup, unit: DisplayUnit) -> SetupVm {
    let len = |v: f64| num_with_unit(v, FieldKind::Length, unit);
    let opt = |v: &Option<String>| v.clone().unwrap_or_else(|| tr("inspector-none"));
    let StockSpec::Block { margin } = s.stock.clone().unwrap_or_default();
    let stock = format!(
        "block, margin {} × {} × {}",
        len(margin[0].v),
        len(margin[1].v),
        len(margin[2].v)
    );
    let zero = match &s.zero {
        None => tr("setup-default"),
        Some(z) => match z {
            ZeroSpec::StockTopCorner(c) => format!("stock_top_corner {}", c.as_str()),
            ZeroSpec::StockTopCentre => "stock_top_centre".into(),
            ZeroSpec::StockBottomCorner(c) => format!("stock_bottom_corner {}", c.as_str()),
            ZeroSpec::Point(p) => point_text(p),
        },
    };
    let bodies = match &s.bodies {
        None => tr("setup-bodies-all"),
        Some(b) => b.join(", "),
    };
    SetupVm {
        title: data(&s.name),
        rows: vec![
            (tr("setup-kind"), data(s.kind.as_str())),
            (tr("setup-material"), opt(&s.material)),
            (tr("setup-machine"), opt(&s.machine)),
            (tr("setup-work-offset"), data(s.work_offset())),
            (tr("setup-stock"), stock),
            (tr("setup-zero"), zero),
            (tr("setup-bodies"), bodies),
            (tr("setup-toolpaths"), s.toolpaths.len().to_string()),
        ],
    }
}

/// The edit for an entry in a number field, or why it is not a value.
///
/// # Errors
///
/// A message for display under the field.
pub fn number_edit(
    snap: &Snapshot,
    toolpath: &str,
    path: &str,
    text: &str,
) -> Result<Edit, String> {
    let (_, tp) = snap.project.toolpath(toolpath).ok_or("no toolpath")?;
    let m = Params::meta(tp.kind(), path).ok_or("no field")?;
    let tool = snap.tools.get(&tp.tool);
    let text = text.trim();
    let value = if text.is_empty() {
        None
    } else {
        let unit = EntryUnit::of(m.kind, snap.project.units);
        let v = evaluate(text, unit, tool).map_err(|e| {
            tr_args(
                "inspector-invalid-entry",
                &[("text", &format!("{text} ({e})"))],
            )
        })?;
        Some(PValue::Num(if m.kind == FieldKind::Count {
            Num::int(v.round() as i64)
        } else {
            Num::float(v)
        }))
    };
    Ok(Edit::SetParam {
        toolpath: toolpath.to_owned(),
        path: path.to_owned(),
        value,
    })
}

/// The edit that sets an enum or bool field, or clears it (`None`).
pub fn value_edit(toolpath: &str, path: &str, value: Option<PValue>) -> Edit {
    Edit::SetParam {
        toolpath: toolpath.to_owned(),
        path: path.to_owned(),
        value,
    }
}

#[cfg(test)]
mod tests;
