// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Setup inspector (S 08 §8.5, spec 03 §3.9): every field the Setup
//! owns, and the edits that change them. A field a `based_on` Setup takes
//! from its parent shows where it comes from and can be reset.

use super::entry::{EntryUnit, evaluate};
use super::fmt_num;
use super::inspector::{DiagVm, display, num_text};
use super::pick::SetupPick;
use crate::session::Snapshot;
use crate::text::{data, diagnostic, tr, tr_args};
use fluut_core::ops::DEFAULT_STOCK_MARGIN;
use fluut_model::edit::{Edit, setup_field_text};
use fluut_model::params::Num;
use fluut_model::project::{
    AngleMode, AxisDir, AxisPick, AxisX, ClearanceSpec, Corner, DisplayUnit, GeometrySel,
    IndexAngle, Orientation, Setup, SetupKind, StockSpec, ZeroSpec,
};
use fluut_model::setup::{
    FieldOrigin, ResolvedSetup, default_orientation, default_zero, resolve_setup,
};
use fluut_model::{FieldKind, Sel};

/// Work offsets of the dropdown.
const WORK_OFFSETS: [&str; 6] = ["G54", "G55", "G56", "G57", "G58", "G59"];

/// How a Setup field is edited.
#[derive(Clone, PartialEq, Debug)]
pub enum RowWidget {
    Text,
    /// `(value, label)`.
    Choice(Vec<(String, String)>),
    Number {
        unit: Option<&'static str>,
    },
    /// A check box; the value is `true` or `false`.
    Check,
    ReadOnly,
}

/// Where an inheritable field comes from.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct OriginVm {
    /// "Inherited from …", when it comes from a parent.
    pub inherited: Option<String>,
    /// The Setup field that "reset to inherited" clears.
    pub reset: Option<&'static str>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct RowVm {
    /// What [`setup_edit`] takes: `name`, `stock.margin.0`, `zero.choice`, …
    pub key: String,
    pub label: String,
    pub widget: RowWidget,
    /// The text shown, or the value of the option chosen.
    pub value: String,
    pub origin: OriginVm,
    /// A Pick button beside the row starts this pick on the model.
    pub pick: Option<SetupPick>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct GroupVm {
    /// The message id of its heading; what `UiState::inspector_focus` holds.
    pub id: &'static str,
    pub label: String,
    pub rows: Vec<RowVm>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct SetupVm {
    pub id: String,
    pub title: String,
    pub groups: Vec<GroupVm>,
    /// Read-only `(label, value)`.
    pub info: Vec<(String, String)>,
    pub diagnostics: Vec<DiagVm>,
}

impl SetupVm {
    /// The row with key `key`.
    pub fn row(&self, key: &str) -> Option<&RowVm> {
        self.groups
            .iter()
            .flat_map(|g| &g.rows)
            .find(|r| r.key == key)
    }
}

fn origin(s: &Snapshot, r: &ResolvedSetup, field: &'static str) -> OriginVm {
    let inherited = match r.origin(field) {
        FieldOrigin::Inherited(id) => {
            let name = s.project.setup(id).map_or(id.as_str(), |x| x.name.as_str());
            Some(tr_args("setup-inherited", &[("setup", name)]))
        }
        _ => None,
    };
    let local = *r.origin(field) == FieldOrigin::Local;
    OriginVm {
        inherited,
        reset: (local && r.based_on.is_some()).then_some(field),
    }
}

fn choices(items: &[(&str, &str)]) -> RowWidget {
    RowWidget::Choice(
        items
            .iter()
            .map(|(v, id)| ((*v).to_owned(), tr(id)))
            .collect(),
    )
}

fn length_row(key: String, label: String, mm: Option<f64>, unit: DisplayUnit) -> RowVm {
    RowVm {
        key,
        label,
        widget: RowWidget::Number {
            unit: display(FieldKind::Length, unit).0,
        },
        value: mm.map_or_else(String::new, |v| num_text(v, FieldKind::Length, unit)),
        origin: OriginVm::default(),
        pick: None,
    }
}

const STOCK_CHOICES: [(&str, &str); 4] = [
    ("margin", "stock-margin"),
    ("size", "stock-size"),
    ("cylinder", "stock-cylinder"),
    ("from_previous", "stock-from-previous"),
];

fn stock_key(st: &StockSpec) -> &'static str {
    match st {
        StockSpec::Block { size: Some(_), .. } => "size",
        StockSpec::Block { .. } => "margin",
        StockSpec::Cylinder { .. } => "cylinder",
        StockSpec::Body { .. } => "body",
        StockSpec::FromPrevious => "from_previous",
    }
}

fn stock_group(s: &Snapshot, r: &ResolvedSetup, unit: DisplayUnit) -> GroupVm {
    let st = r.stock.clone().unwrap_or_default();
    let offered: Vec<(&str, &str)> = STOCK_CHOICES
        .into_iter()
        .filter(|(k, _)| match *k {
            "cylinder" => r.kind.is_rotary(),
            "from_previous" => r.based_on.is_some(),
            _ => !r.kind.is_rotary(),
        })
        .collect();
    let mut widget = choices(&offered);
    let has_bodies = project_bodies(s).next().is_some();
    if let RowWidget::Choice(o) = &mut widget
        && (matches!(st, StockSpec::Body { .. }) || has_bodies)
    {
        o.push(("body".to_owned(), tr("stock-body")));
    }
    let mut rows = vec![RowVm {
        key: "stock.kind".to_owned(),
        label: tr("setup-stock-kind"),
        widget,
        value: stock_key(&st).to_owned(),
        origin: origin(s, r, "stock"),
        pick: None,
    }];
    let triple = |rows: &mut Vec<RowVm>, what: &str, ids: [&str; 3], v: Option<[Num; 3]>| {
        for (i, id) in ids.into_iter().enumerate() {
            rows.push(length_row(
                format!("stock.{what}.{i}"),
                tr(id),
                Some(v.map_or(0.0, |x| x[i].v)),
                unit,
            ));
        }
    };
    match &st {
        StockSpec::Block { size: Some(v), .. } => triple(
            &mut rows,
            "size",
            ["stock-size-x", "stock-size-y", "stock-size-z"],
            Some(*v),
        ),
        StockSpec::Block { margin, .. } => triple(
            &mut rows,
            "margin",
            ["stock-margin-x", "stock-margin-y", "stock-margin-top"],
            *margin,
        ),
        StockSpec::Cylinder { radius, length } => {
            rows.push(length_row(
                "stock.radius".to_owned(),
                tr("stock-radius"),
                Some(radius.v),
                unit,
            ));
            rows.push(length_row(
                "stock.length".to_owned(),
                tr("stock-length"),
                Some(length.v),
                unit,
            ));
        }
        StockSpec::Body { body } => rows.push(RowVm {
            key: "stock.body".to_owned(),
            label: tr("stock-body"),
            widget: options(project_bodies(s), Some(body), false),
            value: body.clone(),
            origin: OriginVm::default(),
            pick: Some(SetupPick::StockBody),
        }),
        StockSpec::FromPrevious => {}
    }
    GroupVm {
        id: "setup-stock",
        label: tr("setup-stock"),
        rows,
    }
}

const ZERO_THREE_AXIS: [(&str, &str); 10] = [
    ("top.front_left", "zero-top-front-left"),
    ("top.front_right", "zero-top-front-right"),
    ("top.back_left", "zero-top-back-left"),
    ("top.back_right", "zero-top-back-right"),
    ("top_centre", "zero-top-centre"),
    ("bottom.front_left", "zero-bottom-front-left"),
    ("bottom.front_right", "zero-bottom-front-right"),
    ("bottom.back_left", "zero-bottom-back-left"),
    ("bottom.back_right", "zero-bottom-back-right"),
    ("point", "zero-point"),
];

const ZERO_ROTARY: [(&str, &str); 3] = [
    ("stock_left", "zero-stock-left"),
    ("stock_right", "zero-stock-right"),
    ("at", "zero-at"),
];

pub(super) fn zero_key(z: &ZeroSpec) -> String {
    match z {
        ZeroSpec::StockTopCorner(c) => format!("top.{}", c.as_str()),
        ZeroSpec::StockBottomCorner(c) => format!("bottom.{}", c.as_str()),
        ZeroSpec::StockTopCentre => "top_centre".to_owned(),
        ZeroSpec::Point(_) => "point".to_owned(),
        ZeroSpec::OnAxis {
            x: AxisX::StockLeft,
        } => "stock_left".to_owned(),
        ZeroSpec::OnAxis {
            x: AxisX::StockRight,
        } => "stock_right".to_owned(),
        ZeroSpec::OnAxis { x: AxisX::At(_) } => "at".to_owned(),
    }
}

fn zero_group(s: &Snapshot, r: &ResolvedSetup, unit: DisplayUnit) -> GroupVm {
    let widget = if r.kind.is_rotary() {
        choices(&ZERO_ROTARY)
    } else {
        choices(&ZERO_THREE_AXIS)
    };
    let mut rows = vec![RowVm {
        key: "zero.choice".to_owned(),
        label: tr("setup-zero-at"),
        widget,
        value: zero_key(&r.zero),
        origin: origin(s, r, "zero"),
        pick: None,
    }];
    match &r.zero {
        ZeroSpec::Point(p) => {
            for (i, id) in ["zero-x", "zero-y", "zero-z"].into_iter().enumerate() {
                rows.push(length_row(
                    format!("zero.point.{i}"),
                    tr(id),
                    Some(p[i].v),
                    unit,
                ));
            }
        }
        ZeroSpec::OnAxis { x: AxisX::At(v) } => {
            rows.push(length_row(
                "zero.at".to_owned(),
                tr("zero-x"),
                Some(v.v),
                unit,
            ));
        }
        _ => {}
    }
    GroupVm {
        id: "setup-zero",
        label: tr("setup-zero"),
        rows,
    }
}

fn clearance_group(s: &Snapshot, r: &ResolvedSetup, unit: DisplayUnit) -> GroupVm {
    let row = if r.kind.is_rotary() {
        let radius = match &r.clearance {
            Some(ClearanceSpec::Cylinder { radius }) => Some(radius.v),
            _ => None,
        };
        length_row(
            "clearance.radius".to_owned(),
            tr("clearance-radius"),
            radius,
            unit,
        )
    } else {
        let offset = r
            .clearance
            .as_ref()
            .and_then(ClearanceSpec::plane_offset)
            .unwrap_or(ClearanceSpec::DEFAULT_OFFSET);
        length_row(
            "clearance.offset".to_owned(),
            tr("clearance-offset"),
            Some(offset),
            unit,
        )
    };
    GroupVm {
        id: "setup-clearance",
        label: tr("setup-clearance"),
        rows: vec![RowVm {
            origin: origin(s, r, "clearance"),
            ..row
        }],
    }
}

/// The options of a dropdown that always shows the current value, even one
/// that is not on the list. `none` adds an empty choice.
fn options(
    list: impl Iterator<Item = (String, String)>,
    current: Option<&str>,
    none: bool,
) -> RowWidget {
    let mut o: Vec<(String, String)> = Vec::new();
    if none {
        o.push((String::new(), tr("inspector-none")));
    }
    o.extend(list);
    if let Some(c) = current
        && !o.iter().any(|(v, _)| v == c)
    {
        o.push((c.to_owned(), data(c)));
    }
    RowWidget::Choice(o)
}

fn general_group(s: &Snapshot, r: &ResolvedSetup) -> GroupVm {
    let machines = s
        .machines
        .iter()
        .filter(|m| !r.kind.is_rotary() || m.rotary)
        .map(|m| (m.reference.clone(), data(&m.name)));
    let materials = s.tools.materials().into_iter().map(|m| (m.clone(), m));
    let offsets = WORK_OFFSETS.iter().map(|o| ((*o).to_owned(), data(*o)));
    let plain = |key: &str, id: &str, widget, value: String, field: Option<&'static str>| RowVm {
        key: key.to_owned(),
        label: tr(id),
        widget,
        value,
        origin: field.map_or_else(OriginVm::default, |f| origin(s, r, f)),
        pick: None,
    };
    let mut rows = vec![
        plain(
            "name",
            "setup-name-label",
            RowWidget::Text,
            data(&r.name),
            None,
        ),
        plain(
            "kind",
            "setup-kind",
            RowWidget::Choice(
                SetupKind::ALL
                    .into_iter()
                    .map(|k| {
                        (
                            k.as_str().to_owned(),
                            tr(&format!("setup-kind-{}", k.as_str())),
                        )
                    })
                    .collect(),
            ),
            r.kind.as_str().to_owned(),
            None,
        ),
        plain(
            "material",
            "setup-material",
            options(materials, r.material.as_deref(), true),
            r.material.clone().unwrap_or_default(),
            Some("material"),
        ),
        plain(
            "machine",
            "setup-machine",
            options(machines, r.machine.as_deref(), true),
            r.machine.clone().unwrap_or_default(),
            Some("machine"),
        ),
        plain(
            "work_offset",
            "setup-work-offset",
            options(offsets, Some(&r.work_offset), false),
            data(&r.work_offset),
            Some("work_offset"),
        ),
    ];
    if r.kind == SetupKind::Indexed {
        rows.push(plain(
            "index_angle",
            "setup-index-angle",
            RowWidget::Number {
                unit: display(FieldKind::Angle, s.project.units).0,
            },
            fmt_num(r.index_angle),
            Some("index_angle"),
        ));
    }
    GroupVm {
        id: "setup-general",
        label: tr("setup-general"),
        rows,
    }
}

impl SetupVm {
    pub fn new(s: &Snapshot, r: &ResolvedSetup) -> SetupVm {
        let unit = s.project.units;
        let toolpaths = s.project.setup(&r.id).map_or(0, |x| x.toolpaths.len());
        SetupVm {
            id: r.id.clone(),
            title: data(&r.name),
            groups: vec![
                general_group(s, r),
                bodies_group(s, r),
                geometry_group(s, r),
                orientation_group(s, r),
                stock_group(s, r, unit),
                zero_group(s, r, unit),
                clearance_group(s, r, unit),
            ],
            info: vec![(tr("setup-toolpaths"), toolpaths.to_string())],
            diagnostics: r.diagnostics.iter().map(DiagVm::of).collect(),
        }
    }
}

/// `(id, label)` of every body of the project's sources, in order.
fn project_bodies(s: &Snapshot) -> impl Iterator<Item = (String, String)> + '_ {
    s.project.geometry.iter().flat_map(|g| {
        g.bodies.iter().map(|b| {
            let label = match &b.name {
                Some(n) => format!("{n} ({})", b.id),
                None => b.id.clone(),
            };
            (b.id.clone(), data(&label))
        })
    })
}

fn all_body_ids(s: &Snapshot) -> Vec<String> {
    project_bodies(s).map(|(id, _)| id).collect()
}

fn check_row(key: String, label: String, on: bool, pick: Option<SetupPick>) -> RowVm {
    RowVm {
        key,
        label,
        widget: RowWidget::Check,
        value: on.to_string(),
        origin: OriginVm::default(),
        pick,
    }
}

/// The bodies the Setup machines: All bodies, or one check box per body.
fn bodies_group(s: &Snapshot, r: &ResolvedSetup) -> GroupVm {
    let mut rows = vec![check_row(
        "bodies.all".to_owned(),
        tr("setup-bodies-all"),
        r.geometry.bodies.is_none(),
        Some(SetupPick::Bodies),
    )];
    for (id, label) in project_bodies(s) {
        let on = r.geometry.bodies.as_ref().is_none_or(|b| b.contains(&id));
        rows.push(check_row(format!("bodies.{id}"), label, on, None));
    }
    GroupVm {
        id: "setup-bodies",
        label: tr("setup-bodies"),
        rows,
    }
}

const AXIS_NAMES: [&str; 3] = ["X", "Y", "Z"];

/// Scale and mirror of the model about its origin, per axis.
fn geometry_group(_s: &Snapshot, r: &ResolvedSetup) -> GroupVm {
    let k = r.geometry.scale.map_or([1.0; 3], |k| k.map(|n| n.v));
    // One factor for every axis comes first: STEP solids take no other.
    let same = k[0] == k[1] && k[1] == k[2];
    let mut rows = vec![RowVm {
        key: "scale.all".to_owned(),
        label: tr("setup-scale-all"),
        widget: RowWidget::Number { unit: None },
        value: if same { fmt_num(k[0]) } else { String::new() },
        origin: OriginVm::default(),
        pick: None,
    }];
    // A file with a factor per axis shows them; the bodies decide whether
    // the frame accepts them.
    for (i, n) in AXIS_NAMES.into_iter().enumerate().filter(|_| !same) {
        let v = k[i];
        rows.push(RowVm {
            key: format!("scale.{i}"),
            label: tr_args("setup-scale-axis", &[("axis", n)]),
            widget: RowWidget::Number { unit: None },
            value: fmt_num(v),
            origin: OriginVm::default(),
            pick: None,
        });
    }
    for (i, n) in AXIS_NAMES.into_iter().enumerate() {
        let on = r.geometry.mirror.is_some_and(|m| m[i]);
        rows.push(check_row(
            format!("mirror.{i}"),
            tr_args("setup-mirror-axis", &[("axis", n)]),
            on,
            None,
        ));
    }
    GroupVm {
        id: "setup-geometry",
        label: tr("setup-geometry"),
        rows,
    }
}

/// The text of a pick as a dropdown value: `+Z`, `face:<id>`, `edge:<id>`.
fn pick_value(p: &AxisPick) -> String {
    match p {
        AxisPick::Axis(a) => a.as_str().to_owned(),
        AxisPick::Face(id) => format!("face:{id}"),
        AxisPick::Edge(id) => format!("edge:{id}"),
    }
}

fn parse_pick(text: &str) -> Option<AxisPick> {
    if let Some(id) = text.strip_prefix("face:") {
        return Some(AxisPick::Face(id.to_owned()));
    }
    if let Some(id) = text.strip_prefix("edge:") {
        return Some(AxisPick::Edge(id.to_owned()));
    }
    AxisDir::ALL
        .into_iter()
        .find(|a| a.as_str() == text)
        .map(AxisPick::Axis)
}

/// One orientation direction: the six model axes, the current pick if it
/// is a face or edge, and `none` as the empty choice when given.
fn axis_row(
    s: &Snapshot,
    r: &ResolvedSetup,
    (key, label, role): (&str, String, SetupPick),
    current: Option<&AxisPick>,
    none: Option<String>,
) -> RowVm {
    let mut o: Vec<(String, String)> = Vec::new();
    if let Some(n) = none {
        o.push((String::new(), n));
    }
    for a in AxisDir::ALL {
        let name = tr_args("orientation-axis", &[("axis", a.as_str())]);
        o.push((a.as_str().to_owned(), name));
    }
    let value = current.map(pick_value).unwrap_or_default();
    match current {
        Some(AxisPick::Face(id)) => {
            o.push((value.clone(), tr_args("orientation-face", &[("id", id)])));
        }
        Some(AxisPick::Edge(id)) => {
            o.push((value.clone(), tr_args("orientation-edge", &[("id", id)])));
        }
        _ => {}
    }
    RowVm {
        key: key.to_owned(),
        label,
        widget: RowWidget::Choice(o),
        value,
        origin: origin(s, r, "orientation"),
        pick: Some(role),
    }
}

/// Which model direction is up (and along X), or the rotary axis (spec 03
/// §3.9.3). The frame, and so the viewport, follows.
fn orientation_group(s: &Snapshot, r: &ResolvedSetup) -> GroupVm {
    let rows = match &r.orientation {
        Orientation::ThreeAxis { tool_axis, x_axis } => vec![
            axis_row(
                s,
                r,
                (
                    "orientation.tool_axis",
                    tr("orientation-tool-axis"),
                    SetupPick::ToolAxis,
                ),
                Some(tool_axis),
                None,
            ),
            axis_row(
                s,
                r,
                (
                    "orientation.x_axis",
                    tr("orientation-x-axis"),
                    SetupPick::XAxis,
                ),
                Some(x_axis),
                None,
            ),
        ],
        Orientation::Rotary {
            rotary_axis,
            a_zero_toward,
        } => vec![
            axis_row(
                s,
                r,
                (
                    "orientation.rotary_axis",
                    tr("orientation-rotary-axis"),
                    SetupPick::RotaryAxis,
                ),
                Some(rotary_axis),
                None,
            ),
            axis_row(
                s,
                r,
                (
                    "orientation.a_zero_toward",
                    tr("orientation-a-zero"),
                    SetupPick::AZero,
                ),
                a_zero_toward.as_ref(),
                Some(tr("orientation-default-z")),
            ),
        ],
    };
    GroupVm {
        id: "setup-orientation",
        label: tr("setup-orientation"),
        rows,
    }
}

/// The orientation with direction `role` replaced by `pick` (`None` clears
/// the optional A = 0 direction).
fn orientation_with(
    o: &Orientation,
    role: SetupPick,
    pick: Option<AxisPick>,
) -> Option<Orientation> {
    Some(match (o.clone(), role) {
        (Orientation::ThreeAxis { x_axis, .. }, SetupPick::ToolAxis) => Orientation::ThreeAxis {
            tool_axis: pick?,
            x_axis,
        },
        (Orientation::ThreeAxis { tool_axis, .. }, SetupPick::XAxis) => Orientation::ThreeAxis {
            tool_axis,
            x_axis: pick?,
        },
        (Orientation::Rotary { a_zero_toward, .. }, SetupPick::RotaryAxis) => Orientation::Rotary {
            rotary_axis: pick?,
            a_zero_toward,
        },
        (Orientation::Rotary { rotary_axis, .. }, SetupPick::AZero) => Orientation::Rotary {
            rotary_axis,
            a_zero_toward: pick,
        },
        _ => return None,
    })
}

fn orientation_edit(setup: &str, o: Orientation) -> Edit {
    let mut t = Setup::new(setup, "");
    t.orientation = Some(o);
    let text = setup_field_text(&t, "orientation");
    Edit::set_setup_field(setup, "orientation", text.as_deref())
}

/// The edit that sets the Setup's bodies: `Err(true)` is "all bodies",
/// `Err(false)` the same list written out, `Ok((id, on))` adds or removes
/// one body.
fn bodies_edit(
    snap: &Snapshot,
    r: &ResolvedSetup,
    change: Result<(&str, bool), bool>,
) -> Result<Edit, String> {
    let all = all_body_ids(snap);
    let mut list = r.geometry.bodies.clone().unwrap_or_else(|| all.clone());
    let bodies = match change {
        Err(true) => None,
        Err(false) => Some(all.clone()),
        Ok((id, on)) => {
            if on && !list.iter().any(|b| b == id) {
                list.push(id.to_owned());
            } else if !on {
                list.retain(|b| b != id);
            }
            if list.is_empty() {
                return Err(tr("setup-bodies-one"));
            }
            // In the order of the model; every body is "all".
            list.sort_by_key(|b| all.iter().position(|a| a == b));
            (list != all).then_some(list)
        }
    };
    Ok(geometry_edit(
        r,
        GeometrySel {
            bodies,
            scale: r.geometry.scale,
            mirror: r.geometry.mirror,
        },
    ))
}

/// The edit that writes the Setup's `geometry` field as `g`.
fn geometry_edit(r: &ResolvedSetup, g: GeometrySel) -> Edit {
    let plain = g.bodies.is_none() && g.scale.is_none() && g.mirror.is_none();
    let mut t = Setup::new(&r.id, "");
    t.geometry = Some(g);
    // A child writes `{}` to override its parent's bodies.
    let text = (!plain || r.based_on.is_some())
        .then(|| setup_field_text(&t, "geometry"))
        .flatten();
    Edit::set_setup_field(&r.id, "geometry", text.as_deref())
}

/// The edit of a scale or mirror row: `scale.<i>` takes a non-zero number
/// (all ones is no scale), `mirror.<i>` takes `true` or `false`.
fn scale_mirror_edit(
    r: &ResolvedSetup,
    key: &str,
    text: &str,
    number: &dyn Fn() -> Result<Num, String>,
) -> Result<Edit, String> {
    let (what, i) = key.split_once('.').ok_or(NO_FIELD)?;
    let mut g = r.geometry.clone();
    if key == "scale.all" {
        let v = number()?;
        if v.v == 0.0 {
            return Err(tr("setup-entry-nonzero"));
        }
        g.scale = (v.v != 1.0).then_some([v; 3]);
        return Ok(geometry_edit(r, g));
    }
    let i = axis(i).ok_or(NO_FIELD)?;
    match what {
        "scale" => {
            let v = number()?;
            if v.v == 0.0 {
                return Err(tr("setup-entry-nonzero"));
            }
            let mut k = g.scale.unwrap_or([Num::int(1); 3]);
            k[i] = v;
            g.scale = k.iter().any(|n| n.v != 1.0).then_some(k);
        }
        "mirror" => {
            let mut m = g.mirror.unwrap_or([false; 3]);
            m[i] = text == "true";
            g.mirror = m.contains(&true).then_some(m);
        }
        _ => return Err(NO_FIELD.to_owned()),
    }
    Ok(geometry_edit(r, g))
}

/// The body a picked face, edge or body belongs to.
pub fn body_of(sel: &Sel) -> Option<String> {
    let id = sel.id();
    let stem = id
        .strip_prefix("f-")
        .or_else(|| id.strip_prefix("e-"))
        .or_else(|| id.strip_prefix("b-"))?;
    let stem = stem.rsplit_once('.').map_or(stem, |(a, _)| a);
    Some(format!("b-{stem}"))
}

/// The edit a pick on the model makes for `role` of Setup `setup`: a body
/// pick toggles the body; the others take a face or an edge.
///
/// # Errors
///
/// A message when the edit is refused.
pub fn setup_pick_edit(
    snap: &Snapshot,
    setup: &str,
    role: SetupPick,
    sel: &Sel,
) -> Result<Edit, String> {
    let r = resolve_setup(&snap.project, setup).map_err(|d| diagnostic(&d))?;
    match role {
        SetupPick::Bodies => {
            let body = body_of(sel).ok_or(NO_FIELD)?;
            let included = r.geometry.bodies.as_ref().is_none_or(|b| b.contains(&body));
            bodies_edit(snap, &r, Ok((&body, !included)))
        }
        SetupPick::StockBody => {
            let body = body_of(sel).ok_or(NO_FIELD)?;
            let mut t = Setup::new(setup, "");
            t.stock = Some(StockSpec::Body { body });
            let text = setup_field_text(&t, "stock");
            Ok(Edit::set_setup_field(setup, "stock", text.as_deref()))
        }
        role => {
            let pick = match sel {
                Sel::Face(id) => AxisPick::Face(id.clone()),
                Sel::Edge(id) => AxisPick::Edge(id.clone()),
                Sel::Body(_) => return Err(NO_FIELD.to_owned()),
            };
            let o = orientation_with(&r.orientation, role, Some(pick)).ok_or(NO_FIELD)?;
            Ok(orientation_edit(setup, o))
        }
    }
}

fn zero_from(choice: &str, r: &ResolvedSetup) -> Option<ZeroSpec> {
    Some(match choice {
        "top_centre" => ZeroSpec::StockTopCentre,
        "point" => match &r.zero {
            ZeroSpec::Point(p) => ZeroSpec::Point(*p),
            _ => ZeroSpec::Point([Num::float(0.0); 3]),
        },
        "stock_left" => ZeroSpec::OnAxis {
            x: AxisX::StockLeft,
        },
        "stock_right" => ZeroSpec::OnAxis {
            x: AxisX::StockRight,
        },
        "at" => match &r.zero {
            ZeroSpec::OnAxis { x: AxisX::At(v) } => ZeroSpec::OnAxis { x: AxisX::At(*v) },
            _ => ZeroSpec::OnAxis {
                x: AxisX::At(Num::float(0.0)),
            },
        },
        c => {
            let (place, name) = c.split_once('.')?;
            let at = Corner::ALL.into_iter().find(|x| x.as_str() == name)?;
            match place {
                "top" => ZeroSpec::StockTopCorner(at),
                "bottom" => ZeroSpec::StockBottomCorner(at),
                _ => return None,
            }
        }
    })
}

/// The stock box of the Setup's scene, as `(x, y, z)` sizes in mm, rounded
/// to a thousandth: what a size or a cylinder starts from.
fn stock_box(s: &Snapshot, setup: &str) -> Option<[f64; 3]> {
    let b = s.scenes.get(setup)?.stock?;
    Some(std::array::from_fn(|i| {
        (f64::from(b.max[i] - b.min[i]) * 1000.0).round() / 1000.0
    }))
}

fn stock_from(kind: &str, s: &Snapshot, r: &ResolvedSetup) -> Option<StockSpec> {
    let own = r.stock.clone().filter(|x| x.kind() != "from_previous");
    let current = own.or_else(|| r.carried_stock.clone());
    Some(match kind {
        "margin" => StockSpec::Block {
            margin: Some(match current {
                Some(StockSpec::Block {
                    margin: Some(m), ..
                }) => m,
                _ => DEFAULT_STOCK_MARGIN.map(Num::int),
            }),
            size: None,
        },
        "size" => StockSpec::Block {
            margin: None,
            size: Some(match current {
                Some(StockSpec::Block { size: Some(v), .. }) => v,
                _ => stock_box(s, &r.id)
                    .unwrap_or([100.0, 100.0, 20.0])
                    .map(Num::float),
            }),
        },
        "cylinder" => match current {
            Some(c @ StockSpec::Cylinder { .. }) => c,
            _ => {
                let b = stock_box(s, &r.id).unwrap_or([100.0, 50.0, 50.0]);
                StockSpec::Cylinder {
                    radius: Num::float(b[1].max(b[2]) / 2.0),
                    length: Num::float(b[0]),
                }
            }
        },
        "from_previous" => StockSpec::FromPrevious,
        "body" => {
            // One the Setup does not machine; else the first.
            let all = all_body_ids(s);
            let machined = r.geometry.bodies.clone().unwrap_or_else(|| all.clone());
            let body = all
                .iter()
                .find(|b| !machined.contains(b))
                .or_else(|| all.first())?
                .clone();
            StockSpec::Body { body }
        }
        _ => return None,
    })
}

const NO_FIELD: &str = "no such field";

/// The stock after an entry in row `key`.
fn stock_entry(
    r: &ResolvedSetup,
    key: &str,
    length: &dyn Fn() -> Result<Num, String>,
) -> Result<StockSpec, String> {
    let positive = || {
        length().and_then(|v| {
            if v.v > 0.0 {
                Ok(v)
            } else {
                Err(tr("setup-entry-positive"))
            }
        })
    };
    let mut st = r.stock.clone().unwrap_or_default();
    match (key, &mut st) {
        ("stock.radius", StockSpec::Cylinder { radius, .. }) => *radius = positive()?,
        ("stock.length", StockSpec::Cylinder { length: l, .. }) => *l = positive()?,
        (k, StockSpec::Block { margin, size }) => {
            let (what, i) = k
                .strip_prefix("stock.")
                .and_then(|k| k.split_once('.'))
                .ok_or(NO_FIELD)?;
            let i = axis(i).ok_or(NO_FIELD)?;
            match what {
                "margin" => {
                    let v = length()?;
                    if v.v < 0.0 {
                        return Err(tr("setup-entry-not-negative"));
                    }
                    margin.get_or_insert([Num::float(0.0); 3])[i] = v;
                }
                "size" => size.get_or_insert([Num::float(0.0); 3])[i] = positive()?,
                _ => return Err(NO_FIELD.to_owned()),
            }
        }
        _ => return Err(NO_FIELD.to_owned()),
    }
    Ok(st)
}

fn axis(k: &str) -> Option<usize> {
    k.parse::<usize>().ok().filter(|i| *i < 3)
}

/// The edit for a new value of row `key`, or why it is not one. `text` is
/// the entry, or the value of the option chosen.
///
/// # Errors
///
/// A message for display under the row.
pub fn setup_edit(snap: &Snapshot, setup: &str, key: &str, text: &str) -> Result<Edit, String> {
    let r = resolve_setup(&snap.project, setup).map_err(|d| diagnostic(&d))?;
    let unit = snap.project.units;
    let text = text.trim();
    let eval = |kind: FieldKind| {
        evaluate(text, EntryUnit::of(kind, unit), None)
            .map(Num::float)
            .map_err(|e| {
                tr_args(
                    "inspector-invalid-entry",
                    &[("text", &format!("{text} ({e})"))],
                )
            })
    };
    let length = || eval(FieldKind::Length);
    let mut t = Setup::new(setup, "");
    let field = match key.split_once('.').map_or(key, |(a, _)| a) {
        "kind" => return kind_edit(snap, &r, text),
        "bodies" => {
            let on = text == "true";
            return match key.strip_prefix("bodies.") {
                Some("all") => bodies_edit(snap, &r, Err(on)),
                Some(id) => bodies_edit(snap, &r, Ok((id, on))),
                None => Err(NO_FIELD.to_owned()),
            };
        }
        "scale" | "mirror" => return scale_mirror_edit(&r, key, text, &|| eval(FieldKind::Count)),
        "orientation" => {
            let role = match key {
                "orientation.tool_axis" => SetupPick::ToolAxis,
                "orientation.x_axis" => SetupPick::XAxis,
                "orientation.rotary_axis" => SetupPick::RotaryAxis,
                "orientation.a_zero_toward" => SetupPick::AZero,
                _ => return Err(NO_FIELD.to_owned()),
            };
            let pick = if text.is_empty() {
                None
            } else {
                Some(parse_pick(text).ok_or(NO_FIELD)?)
            };
            let o = orientation_with(&r.orientation, role, pick).ok_or(NO_FIELD)?;
            return Ok(orientation_edit(setup, o));
        }
        "name" => {
            if text.is_empty() {
                return Err(tr("setup-name-empty"));
            }
            t.name = text.to_owned();
            "name"
        }
        // An empty choice goes back to inherited or unset.
        k @ ("material" | "machine") if text.is_empty() => {
            return Ok(Edit::set_setup_field(setup, k, None));
        }
        "material" => {
            t.material = Some(text.to_owned());
            "material"
        }
        "machine" => {
            t.machine = Some(text.to_owned());
            "machine"
        }
        "work_offset" => {
            t.work_offset = Some(text.to_owned());
            "work_offset"
        }
        "index_angle" => {
            let mode = snap
                .project
                .setup(setup)
                .and_then(|x| x.index_angle.as_ref())
                .map_or(AngleMode::Absolute, |a| a.mode);
            t.index_angle = Some(IndexAngle {
                mode,
                value: eval(FieldKind::Angle)?,
            });
            "index_angle"
        }
        "stock" => {
            t.stock = Some(if key == "stock.kind" {
                stock_from(text, snap, &r).ok_or(NO_FIELD)?
            } else if key == "stock.body" {
                StockSpec::Body {
                    body: text.to_owned(),
                }
            } else {
                stock_entry(&r, key, &length)?
            });
            "stock"
        }
        "zero" => {
            t.zero = Some(match key {
                "zero.choice" => zero_from(text, &r).ok_or(NO_FIELD)?,
                "zero.at" => ZeroSpec::OnAxis {
                    x: AxisX::At(length()?),
                },
                k => {
                    let i = k
                        .strip_prefix("zero.point.")
                        .and_then(axis)
                        .ok_or(NO_FIELD)?;
                    let ZeroSpec::Point(mut p) = r.zero.clone() else {
                        return Err(NO_FIELD.to_owned());
                    };
                    p[i] = length()?;
                    ZeroSpec::Point(p)
                }
            });
            "zero"
        }
        "clearance" => {
            let v = length()?;
            t.clearance = Some(if key == "clearance.radius" {
                if v.v <= 0.0 {
                    return Err(tr("setup-entry-positive"));
                }
                ClearanceSpec::Cylinder { radius: v }
            } else {
                ClearanceSpec::Plane { offset: Some(v) }
            });
            "clearance"
        }
        _ => return Err(NO_FIELD.to_owned()),
    };
    Ok(Edit::set_setup_field(
        setup,
        field,
        setup_field_text(&t, field).as_deref(),
    ))
}

/// The edit that changes the kind of a Setup. Rotary and indexed share what
/// a Setup holds, so only `kind` is written. Crossing to or from three_axis
/// also writes what the other class needs (orientation, zero, stock and
/// clearance), as one undo step. Toolpaths the new kind does not allow keep
/// their place and show their validation.
fn kind_edit(snap: &Snapshot, r: &ResolvedSetup, text: &str) -> Result<Edit, String> {
    let kind = SetupKind::ALL
        .into_iter()
        .find(|k| k.as_str() == text)
        .ok_or(NO_FIELD)?;
    if kind.is_rotary()
        && let Some(m) = r
            .machine
            .as_deref()
            .and_then(|m| snap.machines.iter().find(|c| c.reference == m))
        && !m.rotary
    {
        return Err(tr_args("setup-kind-needs-rotary", &[("machine", &m.name)]));
    }
    let id = r.id.as_str();
    let mut edits = vec![Edit::set_setup_field(
        id,
        "kind",
        Some(&format!("\"{}\"", kind.as_str())),
    )];
    if kind.is_rotary() != r.kind.is_rotary() {
        let mut t = Setup::new(id, "");
        t.orientation = Some(default_orientation(kind));
        t.zero = Some(default_zero(kind));
        let stock = if r.stock == Some(StockSpec::FromPrevious) {
            None
        } else {
            let want = if kind.is_rotary() {
                "cylinder"
            } else {
                "margin"
            };
            stock_from(want, snap, r)
        };
        t.stock = stock;
        for field in ["orientation", "zero", "stock"] {
            if let Some(v) = setup_field_text(&t, field) {
                edits.push(Edit::set_setup_field(id, field, Some(&v)));
            }
        }
        edits.push(Edit::set_setup_field(id, "clearance", None));
    }
    if edits.len() == 1 {
        return Ok(edits.remove(0));
    }
    Ok(Edit::Group {
        label: format!("Change kind of {id}"),
        edits,
    })
}

/// The edit that resets `field` to inherited.
pub fn reset_edit(setup: &str, field: &str) -> Edit {
    Edit::set_setup_field(setup, field, None)
}

#[cfg(test)]
mod tests;
