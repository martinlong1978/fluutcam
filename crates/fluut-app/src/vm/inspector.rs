// SPDX-License-Identifier: MIT OR Apache-2.0

//! The inspector (D13 §4, S 08 §8.5), generated from the D15 field metadata.
//!
//! A toolpath shows the five groups of spec 03 §3.10.2 as tabs. Each field
//! has its label, its value in the project unit, its default when unset,
//! whether `visible_if` shows it, and the validation messages that name it.
//! A Setup has its own editor, [`super::setup`].

use super::entry::{EntryUnit, dimension, evaluate, length_unit};
use super::setup::SetupVm;
use super::visible::visible;
use super::{Selection, fmt_num};
use crate::jobs::Edit;
use crate::session::Snapshot;
use crate::text::{data, diagnostic, field_label, tr, tr_args};
use fluut_i18n::{display_scale, unit_symbol};
use fluut_model::edit::Edit as ModelEdit;
use fluut_model::params::{ConstVal, DefaultRule, HeightSpec};
use fluut_model::project::{DisplayUnit, Toolpath};
use fluut_model::tools::{Tool, ToolShape, ToolStore};
use fluut_model::{
    Diagnostic, FieldKind, FieldMeta, Group, Num, PValue, Params, Sel, Severity, ToolpathTypeId,
};

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
    /// A reference and an offset. `refs` are `(value, label)`.
    Height {
        refs: Vec<(String, String)>,
        reference: String,
        offset: String,
        unit: Option<&'static str>,
    },
    /// A selection, picked on the model.
    Selection {
        items: Vec<Sel>,
    },
    /// Free text.
    Text,
    /// `x y z` in the display unit, points separated by `;`.
    Points {
        unit: Option<&'static str>,
    },
    ReadOnly,
}

#[derive(Clone, PartialEq, Debug)]
pub struct DiagVm {
    pub error: bool,
    pub code: String,
    pub text: String,
}

impl DiagVm {
    pub(super) fn of(d: &Diagnostic) -> DiagVm {
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

/// The tool dropdown of a toolpath.
#[derive(Clone, PartialEq, Debug)]
pub struct ToolChoiceVm {
    /// The reference the toolpath holds.
    pub value: String,
    /// `(reference, label)`.
    pub options: Vec<(String, String)>,
    /// Why the tool does not suit, under the dropdown.
    pub diagnostics: Vec<DiagVm>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ToolpathVm {
    pub id: String,
    pub title: String,
    /// Type, Setup and tool.
    pub subtitle: String,
    pub tool: ToolChoiceVm,
    pub tabs: Vec<TabVm>,
    /// Diagnostics that name no field.
    pub general: Vec<DiagVm>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum InspectorVm {
    Empty(String),
    Setup(SetupVm),
    Toolpath(ToolpathVm),
}

/// Display unit symbol and scale of a field kind.
pub(super) fn display(kind: FieldKind, unit: DisplayUnit) -> (Option<&'static str>, f64) {
    let (dim, unit) = (dimension(kind), length_unit(unit));
    let sym = unit_symbol(dim, unit);
    ((!sym.is_empty()).then_some(sym), display_scale(dim, unit))
}

pub(crate) fn num_text(v: f64, kind: FieldKind, unit: DisplayUnit) -> String {
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

    fn widget(&self, m: &FieldMeta, local: Option<&PValue>) -> Widget {
        match m.kind {
            FieldKind::Length
            | FieldKind::Feed
            | FieldKind::Speed
            | FieldKind::Angle
            | FieldKind::Duration
            | FieldKind::Count => Widget::Number {
                unit: display(m.kind, self.unit).0,
            },
            FieldKind::Bool => Widget::Bool,
            FieldKind::Height | FieldKind::XPos => {
                let h = match local {
                    Some(PValue::Height(h)) => Some(h.clone()),
                    _ => match const_value(m) {
                        Some(PValue::Height(h)) => Some(h),
                        _ => None,
                    },
                };
                let (reference, off) =
                    h.map_or((String::new(), 0.0), |h| (h.reference.clone(), h.offset()));
                Widget::Height {
                    refs: m
                        .height_refs()
                        .iter()
                        .map(|r| ((*r).to_owned(), tr(&format!("height-ref-{r}"))))
                        .collect(),
                    reference,
                    offset: num_text(off, FieldKind::Length, self.unit),
                    unit: display(FieldKind::Length, self.unit).0,
                }
            }
            FieldKind::Selection => Widget::Selection {
                items: match local {
                    Some(PValue::Selection(v)) => v.clone(),
                    _ => Vec::new(),
                },
            },
            FieldKind::Text => Widget::Text,
            FieldKind::Point | FieldKind::Points => Widget::Points {
                unit: display(FieldKind::Length, self.unit).0,
            },
            FieldKind::Enum(opts) => Widget::Enum {
                options: opts.iter().map(|o| ((*o).to_owned(), data(*o))).collect(),
            },
            _ => Widget::ReadOnly,
        }
    }

    fn field(&self, path: String, m: &FieldMeta, diags: &[Diagnostic]) -> FieldVm {
        let local = self.tp.params.get(&path);
        let value = match local.cloned().or_else(|| const_value(m)) {
            Some(v) => match &v {
                PValue::Point(p) => points_text(std::slice::from_ref(p), self.unit),
                PValue::Points(ps) => points_text(ps, self.unit),
                v => value_text(v, m.kind, self.unit),
            },
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
            widget: self.widget(m, local),
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
                    InspectorVm::Toolpath(toolpath_vm(
                        &cx,
                        &s.tools,
                        &setup.name,
                        s.diagnostics(id),
                    ))
                }
                None => empty(),
            },
            Selection::Setup(id) | Selection::Face { setup: id, .. } => {
                match fluut_model::setup::resolve_setup(&s.project, id) {
                    Ok(st) => InspectorVm::Setup(SetupVm::new(s, &st)),
                    Err(_) => empty(),
                }
            }
            Selection::None => empty(),
        }
    }
}

fn toolpath_vm(cx: &Cx, tools: &ToolStore, setup: &str, diags: &[Diagnostic]) -> ToolpathVm {
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
        .filter(|d| d.field.as_deref() != Some("tool"))
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
        tool: tool_choice(cx, tools, diags),
        tabs,
        general,
    }
}

/// The name of a tool shape.
fn shape_label(s: ToolShape) -> String {
    tr(match s {
        ToolShape::Flat => "tool-shape-flat",
        ToolShape::Ball => "tool-shape-ball",
        ToolShape::Bull => "tool-shape-bull",
        ToolShape::V => "tool-shape-v",
        ToolShape::Taper => "tool-shape-taper",
        ToolShape::Drill => "tool-shape-drill",
    })
}

/// "library: type diameter, name".
pub(crate) fn tool_label(library: &str, t: &Tool, unit: DisplayUnit) -> String {
    let d = num_with_unit(t.diameter, FieldKind::Length, unit);
    let name = data(if t.name.is_empty() { &t.id } else { &t.name });
    tr_args(
        "inspector-tool-option",
        &[
            ("library", library),
            ("shape", &shape_label(t.shape)),
            ("diameter", &d),
            ("name", &name),
        ],
    )
}

fn tool_choice(cx: &Cx, tools: &ToolStore, diags: &[Diagnostic]) -> ToolChoiceVm {
    let mut options = Vec::new();
    if cx.tp.kind() == ToolpathTypeId::ManualNc {
        options.push((String::new(), tr("inspector-none")));
    }
    for lib in tools.libraries() {
        let label = if lib.name.is_empty() {
            &lib.id
        } else {
            &lib.name
        };
        for t in &lib.tools {
            options.push((
                format!("{}#{}", lib.id, t.id),
                tool_label(label, t, cx.unit),
            ));
        }
    }
    let value = cx.tp.tool.clone();
    if !options.iter().any(|(v, _)| *v == value) {
        options.insert(0, (value.clone(), data(&value)));
    }
    ToolChoiceVm {
        value,
        options,
        diagnostics: diags
            .iter()
            .filter(|d| d.field.as_deref() == Some("tool"))
            .map(DiagVm::of)
            .collect(),
    }
}

/// The edit that gives a toolpath the tool `reference`; empty means none.
pub fn tool_edit(toolpath: &str, reference: &str) -> ModelEdit {
    let text = PValue::Enum(reference.to_owned()).to_toml();
    ModelEdit::set_toolpath_field(toolpath, "tool", (!reference.is_empty()).then_some(&*text))
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

fn points_text(ps: &[[Num; 3]], unit: DisplayUnit) -> String {
    let one = |p: &[Num; 3]| {
        p.iter()
            .map(|c| num_text(c.v, FieldKind::Length, unit))
            .collect::<Vec<_>>()
            .join(" ")
    };
    ps.iter().map(one).collect::<Vec<_>>().join("; ")
}

/// The edit for an entry in a height: a new reference, a new offset, or
/// both. Starts from the value in force, so changing one keeps the other.
///
/// # Errors
///
/// A message for display under the field.
pub fn height_edit(
    snap: &Snapshot,
    toolpath: &str,
    path: &str,
    reference: Option<&str>,
    offset: Option<&str>,
) -> Result<Edit, String> {
    let (_, tp) = snap.project.toolpath(toolpath).ok_or("no toolpath")?;
    let m = Params::meta(tp.kind(), path).ok_or("no field")?;
    let now = match tp.params.get(path).cloned().or_else(|| const_value(m)) {
        Some(PValue::Height(h)) => Some(h),
        _ => None,
    };
    let r = reference
        .map(str::to_owned)
        .or_else(|| now.as_ref().map(|h| h.reference.clone()))
        .ok_or("no reference")?;
    let off = match offset.map(str::trim) {
        None => now.map_or(0.0, |h| h.offset()),
        Some("") => 0.0,
        Some(text) => {
            let unit = EntryUnit::of(FieldKind::Length, snap.project.units);
            evaluate(text, unit, snap.tools.get(&tp.tool)).map_err(|e| {
                tr_args(
                    "inspector-invalid-entry",
                    &[("text", &format!("{text} ({e})"))],
                )
            })?
        }
    };
    Ok(value_edit(
        toolpath,
        path,
        Some(PValue::Height(HeightSpec::new(&r, off))),
    ))
}

/// The edit for an entry in a `Point` or `Points` field: `x y z`, points
/// separated by `;`, in the display unit. Empty clears the field.
///
/// # Errors
///
/// A message for display under the field.
pub fn points_edit(
    snap: &Snapshot,
    toolpath: &str,
    path: &str,
    text: &str,
) -> Result<Edit, String> {
    let (_, tp) = snap.project.toolpath(toolpath).ok_or("no toolpath")?;
    let m = Params::meta(tp.kind(), path).ok_or("no field")?;
    let text = text.trim();
    if text.is_empty() {
        return Ok(value_edit(toolpath, path, None));
    }
    let bad = |e: &str| {
        tr_args(
            "inspector-invalid-entry",
            &[("text", &format!("{text} ({e})"))],
        )
    };
    let unit = EntryUnit::of(FieldKind::Length, snap.project.units);
    let mut points = Vec::new();
    for part in text.split(';').filter(|p| !p.trim().is_empty()) {
        let c: Vec<&str> = part.split_whitespace().collect();
        let [x, y, z] = c[..] else {
            return Err(bad("x y z"));
        };
        let mut p = [Num::float(0.0); 3];
        for (slot, t) in p.iter_mut().zip([x, y, z]) {
            *slot = Num::float(evaluate(t, unit, None).map_err(|e| bad(&e))?);
        }
        points.push(p);
    }
    let value = match (m.kind, &points[..]) {
        (FieldKind::Point, [p]) => PValue::Point(*p),
        (FieldKind::Point, _) => return Err(bad("x y z")),
        _ => PValue::Points(points),
    };
    Ok(value_edit(toolpath, path, Some(value)))
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
