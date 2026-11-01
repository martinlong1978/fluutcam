// SPDX-License-Identifier: MIT OR Apache-2.0

//! Setup inheritance (D06 §2, spec 03 §3.9.4).
//!
//! A field set in a Setup is local; an absent field comes from `based_on`,
//! or from the default for the Setup kind. [`resolve_setup`] gives every
//! field a value and records where it came from, for the "inherited from"
//! chips of the inspector (spec 08 §8.5).

use crate::diag::Diagnostic;
use crate::params::Num;
use crate::project::{
    AngleMode, AxisDir, AxisPick, AxisX, ClearanceSpec, GeometrySel, Orientation, Project, Region,
    Setup, SetupKind, StockSpec, ZeroSpec,
};
use std::collections::BTreeMap;

/// Where a resolved field came from.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FieldOrigin {
    Local,
    /// From this Setup (the nearest ancestor that sets it).
    Inherited(String),
    Default,
}

/// The inheritable fields, in file order.
pub const INHERITED_FIELDS: [&str; 11] = [
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
];

/// A Setup with every field concrete.
#[derive(Clone, PartialEq, Debug)]
pub struct ResolvedSetup {
    pub id: String,
    pub name: String,
    pub based_on: Option<String>,
    pub kind: SetupKind,
    pub machine: Option<String>,
    pub material: Option<String>,
    pub work_offset: String,
    pub geometry: GeometrySel,
    pub orientation: Orientation,
    /// Resolved rotary position in degrees; 0 unless indexed.
    pub index_angle: f64,
    /// `None` only for a rotary or indexed root without stock.
    pub stock: Option<StockSpec>,
    /// For `stock = from_previous`: the nearest ancestor's own stock. Until
    /// in-process stock exists (D08 §7) frames use its outline.
    pub carried_stock: Option<StockSpec>,
    pub zero: ZeroSpec,
    pub region: Option<Region>,
    /// `None` only for a rotary or indexed root without a clearance.
    pub clearance: Option<ClearanceSpec>,
    pub origins: BTreeMap<&'static str, FieldOrigin>,
    /// Rule findings that do not stop resolution (`from_previous`, kinds).
    pub diagnostics: Vec<Diagnostic>,
}

impl ResolvedSetup {
    pub fn origin(&self, field: &str) -> &FieldOrigin {
        self.origins.get(field).unwrap_or(&FieldOrigin::Default)
    }

    /// The resolved values as a Setup whose every field is local, without
    /// toolpaths: the canonical text of this form is the Setup's hash input.
    pub fn to_setup(&self) -> Setup {
        let mut s = Setup::new(&self.id, &self.name);
        s.kind = Some(self.kind);
        s.machine = self.machine.clone();
        s.material = self.material.clone();
        s.work_offset = Some(self.work_offset.clone());
        s.geometry = Some(self.geometry.clone());
        s.orientation = Some(self.orientation.clone());
        s.index_angle = Some(crate::project::IndexAngle {
            mode: AngleMode::Absolute,
            value: Num::float(self.index_angle),
        });
        s.stock = match (&self.stock, &self.carried_stock) {
            (Some(StockSpec::FromPrevious), Some(c)) => Some(c.clone()),
            (st, _) => st.clone(),
        };
        s.zero = Some(self.zero.clone());
        s.region = self.region.clone();
        s.clearance = self.clearance.clone();
        s
    }
}

fn default_orientation(kind: SetupKind) -> Orientation {
    match kind {
        SetupKind::ThreeAxis => Orientation::default(),
        _ => Orientation::Rotary {
            rotary_axis: AxisPick::Axis(AxisDir::PosX),
            a_zero_toward: None,
        },
    }
}

fn default_zero(kind: SetupKind) -> ZeroSpec {
    match kind {
        SetupKind::ThreeAxis => ZeroSpec::default(),
        _ => ZeroSpec::OnAxis {
            x: AxisX::StockLeft,
        },
    }
}

/// Resolves Setup `id` and its `based_on` chain. Fails when the chain is
/// broken: a missing parent, or a parent that is not earlier in machining
/// order (which also rules out cycles).
pub fn resolve_setup(p: &Project, id: &str) -> Result<ResolvedSetup, Diagnostic> {
    let mut memo = BTreeMap::new();
    resolve_memo(p, id, &mut memo)
}

/// Resolves every Setup, in order; each chain is resolved once.
pub fn resolve_all(p: &Project) -> Vec<Result<ResolvedSetup, Diagnostic>> {
    let mut memo = BTreeMap::new();
    p.setups
        .iter()
        .map(|s| resolve_memo(p, &s.id, &mut memo))
        .collect()
}

type Memo = BTreeMap<String, Result<ResolvedSetup, Diagnostic>>;

fn resolve_memo(p: &Project, id: &str, memo: &mut Memo) -> Result<ResolvedSetup, Diagnostic> {
    if let Some(r) = memo.get(id) {
        return r.clone();
    }
    let r = resolve_one(p, id, memo);
    memo.insert(id.to_owned(), r.clone());
    r
}

fn resolve_one(p: &Project, id: &str, memo: &mut Memo) -> Result<ResolvedSetup, Diagnostic> {
    let Some(at) = p.setup_index(id) else {
        return Err(Diagnostic::error(
            "setup.missing",
            format!("Setup `{id}` is not in the project"),
        ));
    };
    let s = &p.setups[at];
    let parent = match &s.based_on {
        None => None,
        Some(pid) => {
            let Some(pi) = p.setup_index(pid) else {
                return Err(Diagnostic::error(
                    "setup.based_on.missing",
                    format!("Setup `{id}`: `based_on` names `{pid}`, which is not in the project"),
                )
                .at("based_on"));
            };
            if pi >= at {
                return Err(Diagnostic::error(
                    "setup.based_on.order",
                    format!(
                        "Setup `{id}`: its parent `{pid}` must come earlier in machining order"
                    ),
                )
                .at("based_on"));
            }
            Some(resolve_memo(p, pid, memo).map_err(|e| {
                Diagnostic::error(
                    &e.code,
                    format!("Setup `{id}`: parent `{pid}`: {}", e.message),
                )
                .at("based_on")
            })?)
        }
    };
    Ok(merge(s, parent.as_ref()))
}

/// Picks the local value, else the parent's, recording the origin.
struct Pick<'a> {
    parent: Option<&'a ResolvedSetup>,
    origins: BTreeMap<&'static str, FieldOrigin>,
}

impl Pick<'_> {
    fn take<T: Clone>(
        &mut self,
        field: &'static str,
        local: &Option<T>,
        inherited: impl Fn(&ResolvedSetup) -> Option<T>,
    ) -> Option<T> {
        if let Some(v) = local {
            self.origins.insert(field, FieldOrigin::Local);
            return Some(v.clone());
        }
        let p = self.parent?;
        let v = inherited(p)?;
        let from = match p.origin(field) {
            FieldOrigin::Inherited(a) => FieldOrigin::Inherited(a.clone()),
            FieldOrigin::Local => FieldOrigin::Inherited(p.id.clone()),
            FieldOrigin::Default => return Some(v),
        };
        self.origins.insert(field, from);
        Some(v)
    }
}

fn merge(s: &Setup, parent: Option<&ResolvedSetup>) -> ResolvedSetup {
    let mut pk = Pick {
        parent,
        origins: BTreeMap::new(),
    };
    let kind = pk
        .take("kind", &s.kind, |p| Some(p.kind))
        .unwrap_or(SetupKind::ThreeAxis);
    let machine = pk.take("machine", &s.machine, |p| p.machine.clone());
    let material = pk.take("material", &s.material, |p| p.material.clone());
    let work_offset = pk
        .take("work_offset", &s.work_offset, |p| {
            Some(p.work_offset.clone())
        })
        .unwrap_or_else(|| "G54".to_owned());
    let geometry = pk
        .take("geometry", &s.geometry, |p| Some(p.geometry.clone()))
        .unwrap_or_default();
    let orientation = pk
        .take("orientation", &s.orientation, |p| {
            Some(p.orientation.clone())
        })
        .unwrap_or_else(|| default_orientation(kind));
    let stock = pk
        .take("stock", &s.stock, |p| p.stock.clone())
        .or_else(|| (kind == SetupKind::ThreeAxis).then(StockSpec::default));
    let zero = pk
        .take("zero", &s.zero, |p| Some(p.zero.clone()))
        .unwrap_or_else(|| default_zero(kind));
    let region = pk.take("region", &s.region, |p| p.region.clone());
    let clearance = pk
        .take("clearance", &s.clearance, |p| p.clearance.clone())
        .or_else(|| {
            (kind == SetupKind::ThreeAxis).then_some(ClearanceSpec::Plane { offset: None })
        });

    // Index angle (spec 03 §3.9.5): an inherited angle is the parent's
    // resolved angle; a local relative angle adds to it (0 under a rotary
    // parent).
    let parent_angle = parent.map_or(0.0, |p| {
        if p.kind == SetupKind::Rotary {
            0.0
        } else {
            p.index_angle
        }
    });
    let index_angle = match &s.index_angle {
        Some(a) => {
            pk.origins.insert("index_angle", FieldOrigin::Local);
            match a.mode {
                AngleMode::Absolute => a.value.v,
                AngleMode::Relative => parent_angle + a.value.v,
            }
        }
        None => pk
            .take("index_angle", &None, |p| Some(p.index_angle))
            .unwrap_or(0.0),
    };

    let carried_stock = match (&stock, parent) {
        (Some(StockSpec::FromPrevious), Some(p)) => match &p.stock {
            Some(StockSpec::FromPrevious) => p.carried_stock.clone(),
            st => st.clone(),
        },
        _ => None,
    };

    let mut r = ResolvedSetup {
        id: s.id.clone(),
        name: s.name.clone(),
        based_on: s.based_on.clone(),
        kind,
        machine,
        material,
        work_offset,
        geometry,
        orientation,
        index_angle,
        stock,
        carried_stock,
        zero,
        region,
        clearance,
        origins: pk.origins,
        diagnostics: Vec::new(),
    };
    r.diagnostics = check(&r, parent);
    r
}

/// The rules of spec 03 §3.9.2–§3.9.4 that need the resolved values.
fn check(r: &ResolvedSetup, parent: Option<&ResolvedSetup>) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let rotary_or = matches!(r.orientation, Orientation::Rotary { .. });
    if r.kind.is_rotary() != rotary_or {
        out.push(
            Diagnostic::error(
                "setup.orientation.kind",
                format!(
                    "Setup `{}`: a {} Setup needs {} orientation",
                    r.id,
                    r.kind,
                    if r.kind.is_rotary() {
                        "a rotary_axis"
                    } else {
                        "a tool_axis and x_axis"
                    }
                ),
            )
            .at("orientation"),
        );
    }
    if r.kind.is_rotary() && !matches!(r.zero, ZeroSpec::OnAxis { .. }) {
        out.push(
            Diagnostic::error(
                "setup.zero.kind",
                format!(
                    "Setup `{}`: the zero of a {} Setup lies on the rotary axis; give only `x`",
                    r.id, r.kind
                ),
            )
            .at("zero"),
        );
    }
    if !r.kind.is_rotary() && matches!(r.zero, ZeroSpec::OnAxis { .. }) {
        out.push(
            Diagnostic::error(
                "setup.zero.kind",
                format!("Setup `{}`: a three_axis zero needs a `method`", r.id),
            )
            .at("zero"),
        );
    }
    if r.stock.is_none() {
        out.push(
            Diagnostic::error(
                "setup.stock.missing",
                format!("Setup `{}`: a {} Setup needs a stock", r.id, r.kind),
            )
            .at("stock"),
        );
    }
    if r.stock == Some(StockSpec::FromPrevious) {
        match parent {
            None => out.push(
                Diagnostic::error(
                    "setup.stock.from_previous.parent",
                    format!("Setup `{}`: stock = from_previous needs `based_on`", r.id),
                )
                .at("stock"),
            ),
            Some(p) => {
                let mut changed = Vec::new();
                if r.machine != p.machine {
                    changed.push("machine");
                }
                if r.geometry != p.geometry {
                    changed.push("geometry");
                }
                if r.orientation != p.orientation {
                    changed.push("orientation");
                }
                if r.zero != p.zero {
                    changed.push("zero");
                }
                if !changed.is_empty() {
                    out.push(
                        Diagnostic::error(
                            "setup.stock.from_previous.changed",
                            format!(
                                "Setup `{}`: with stock = from_previous, {} must stay as in `{}`",
                                r.id,
                                changed.join(", "),
                                p.id
                            ),
                        )
                        .at(changed[0]),
                    );
                }
            }
        }
    }
    out
}
