// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Fixtures group of the Setup inspector (spec 03 §3.9.8): the chucks,
//! tailstocks and clamps of a rotary or indexed Setup, one block of rows
//! each, with Add and Remove. The list is inherited as a whole, so editing
//! any row of an inherited list writes the whole list to the Setup.

use super::{GroupVm, NO_FIELD, OriginVm, ResolvedSetup, RowVm, RowWidget, Snapshot, origin};
use crate::text::{tr, tr_args};
use fluut_model::edit::{Edit, setup_field_text};
use fluut_model::params::Num;
use fluut_model::project::{DisplayUnit, Fixture, Setup};

const AXES: [&str; 3] = ["X", "Y", "Z"];

/// Smallest width of a default fixture, mm.
const MIN_DEFAULT: f64 = 10.0;

fn row(key: String, label: String, widget: RowWidget, value: String) -> RowVm {
    RowVm {
        key,
        label,
        widget,
        value,
        origin: OriginVm::default(),
        pick: None,
    }
}

fn button(key: String, label: String, caption: String) -> RowVm {
    row(key, label, RowWidget::Action, caption)
}

fn number(key: String, label: String, mm: Option<f64>, unit: DisplayUnit) -> RowVm {
    super::length_row(key, label, mm, unit)
}

fn title(f: &Fixture, n: usize) -> String {
    tr_args(
        &format!("fixture-{}", f.kind()),
        &[("n", &(n + 1).to_string())],
    )
}

/// The group, for rotary and indexed Setups. Its first row says where the
/// list comes from and resets it.
pub(super) fn group(s: &Snapshot, r: &ResolvedSetup, unit: DisplayUnit) -> GroupVm {
    let list = &r.fixtures;
    let summary = match list.len() {
        0 => tr("fixtures-none"),
        n => tr_args("fixtures-count", &[("n", &n.to_string())]),
    };
    let mut head = row(
        "fixtures".to_owned(),
        tr("setup-fixtures"),
        RowWidget::ReadOnly,
        summary,
    );
    head.origin = origin(s, r, "fixtures");
    let inherited = head.origin.inherited.is_some();
    let mut rows = vec![head];
    if inherited && !list.is_empty() {
        rows.push(button(
            "fixtures.local".to_owned(),
            String::new(),
            tr("fixtures-local"),
        ));
    }
    for (i, f) in list.iter().enumerate() {
        rows.push(button(
            format!("fixtures.{i}.remove"),
            title(f, i),
            tr("fixtures-remove"),
        ));
        let at = |field: &str| format!("fixtures.{i}.{field}");
        match f {
            Fixture::Chuck { diameter, length } => {
                rows.push(number(
                    at("diameter"),
                    tr("fixture-diameter"),
                    Some(diameter.v),
                    unit,
                ));
                rows.push(number(
                    at("length"),
                    tr("fixture-length"),
                    Some(length.v),
                    unit,
                ));
            }
            Fixture::Tailstock {
                diameter,
                length,
                tip_diameter,
            } => {
                rows.push(number(
                    at("diameter"),
                    tr("fixture-diameter"),
                    Some(diameter.v),
                    unit,
                ));
                rows.push(number(
                    at("length"),
                    tr("fixture-length"),
                    Some(length.v),
                    unit,
                ));
                rows.push(number(
                    at("tip_diameter"),
                    tr("fixture-tip-diameter"),
                    tip_diameter.map(|t| t.v),
                    unit,
                ));
            }
            Fixture::Clamp { min, max } => {
                for (what, p) in [("min", min), ("max", max)] {
                    for (a, name) in AXES.iter().enumerate() {
                        rows.push(number(
                            format!("fixtures.{i}.{what}.{a}"),
                            tr_args(&format!("fixture-{what}"), &[("axis", name)]),
                            Some(p[a].v),
                            unit,
                        ));
                    }
                }
            }
        }
    }
    rows.push(row(
        "fixtures.add".to_owned(),
        tr("fixtures-add"),
        RowWidget::Choice(
            std::iter::once((String::new(), tr("fixtures-add-prompt")))
                .chain(Fixture::KINDS.into_iter().map(|k| {
                    (
                        k.to_owned(),
                        tr_args(&format!("fixture-{k}"), &[("n", "")])
                            .trim()
                            .to_owned(),
                    )
                }))
                .collect(),
        ),
        String::new(),
    ));
    if !list.is_empty() {
        rows.push(button(
            "fixtures.clear".to_owned(),
            String::new(),
            tr("fixtures-clear"),
        ));
    }
    GroupVm {
        id: "setup-fixtures",
        label: tr("setup-fixtures"),
        rows,
    }
}

/// A new fixture of `kind` beside the stock of the Setup: sizes follow the
/// stock's radius and length so it is visible and does not touch the part.
fn new_fixture(snap: &Snapshot, setup: &str, kind: &str) -> Option<Fixture> {
    let b = snap.scenes.get(setup).and_then(|c| c.stock);
    let (x0, x1, radius) = b.map_or((0.0, 50.0, 25.0), |b| {
        let r = [b.min[1], b.max[1], b.min[2], b.max[2]]
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()));
        (f64::from(b.min[0]), f64::from(b.max[0]), f64::from(r))
    });
    let radius = radius.max(MIN_DEFAULT * 0.5);
    let round = |v: f64| Num::float((v * 10.0).round() / 10.0);
    Some(match kind {
        "chuck" => Fixture::Chuck {
            diameter: round(3.0 * radius),
            length: round(((x1 - x0) * 0.4).max(MIN_DEFAULT)),
        },
        "tailstock" => Fixture::Tailstock {
            diameter: round(radius),
            length: round(((x1 - x0) * 0.3).max(MIN_DEFAULT)),
            tip_diameter: None,
        },
        "clamp" => {
            // A block against the +Y side of the stock, a fifth of its
            // length wide, at A = 0.
            let mid = 0.5 * (x0 + x1);
            let w = ((x1 - x0) * 0.1).max(MIN_DEFAULT * 0.5);
            Fixture::Clamp {
                min: [round(mid - w), round(radius), round(-radius * 0.5)],
                max: [
                    round(mid + w),
                    round(radius + MIN_DEFAULT * 2.0),
                    round(radius * 0.5),
                ],
            }
        }
        _ => return None,
    })
}

/// The edit of the `fixtures.…` row `key`, with `text` the entry (or the
/// option chosen). `length` reads the entry as a length in mm.
///
/// # Errors
///
/// A message for display under the row; the rules are those of the
/// project file.
pub(super) fn edit(
    snap: &Snapshot,
    r: &ResolvedSetup,
    key: &str,
    text: &str,
    length: &dyn Fn() -> Result<Num, String>,
) -> Result<Edit, String> {
    if !r.kind.is_rotary() {
        return Err(NO_FIELD.to_owned());
    }
    let rest = key.strip_prefix("fixtures.").ok_or(NO_FIELD)?;
    let mut list = r.fixtures.clone();
    match rest {
        // The same list, written to this Setup: replaces the inherited one.
        "local" => {}
        "clear" => list.clear(),
        "add" => list.push(new_fixture(snap, &r.id, text).ok_or(NO_FIELD)?),
        _ => {
            let mut parts = rest.split('.');
            let i: usize = parts
                .next()
                .and_then(|i| i.parse().ok())
                .filter(|i| *i < list.len())
                .ok_or(NO_FIELD)?;
            let field = parts.next().ok_or(NO_FIELD)?;
            let axis = parts.next();
            if field == "remove" {
                list.remove(i);
            } else {
                change(&mut list[i], field, axis, text, length)?;
            }
        }
    }
    let mut t = Setup::new(&r.id, "");
    t.fixtures = Some(list.clone());
    // An empty list on a Setup with nothing to override is no field.
    let value = (!list.is_empty() || r.based_on.is_some())
        .then(|| setup_field_text(&t, "fixtures"))
        .flatten();
    Ok(Edit::set_setup_field(&r.id, "fixtures", value.as_deref()))
}

/// Sets `field` (and `axis` of a clamp corner) of `f`, with the checks the
/// project file reader makes.
fn change(
    f: &mut Fixture,
    field: &str,
    axis: Option<&str>,
    text: &str,
    length: &dyn Fn() -> Result<Num, String>,
) -> Result<(), String> {
    let positive = || -> Result<Num, String> {
        let v = length()?;
        if v.v > 0.0 {
            Ok(v)
        } else {
            Err(tr("setup-entry-positive"))
        }
    };
    match (f, field) {
        (Fixture::Chuck { diameter, .. } | Fixture::Tailstock { diameter, .. }, "diameter") => {
            *diameter = positive()?;
        }
        (Fixture::Chuck { length: l, .. } | Fixture::Tailstock { length: l, .. }, "length") => {
            *l = positive()?;
        }
        (Fixture::Tailstock { tip_diameter, .. }, "tip_diameter") => {
            // Empty is the plain cylinder; 0 is a point.
            *tip_diameter = if text.is_empty() {
                None
            } else {
                let v = length()?;
                if v.v < 0.0 {
                    return Err(tr("setup-entry-not-negative"));
                }
                Some(v)
            };
        }
        (Fixture::Clamp { min, max }, "min" | "max") => {
            let a: usize = axis
                .and_then(|a| a.parse().ok())
                .filter(|a| *a < 3)
                .ok_or(NO_FIELD)?;
            let v = length()?;
            let (mut lo, mut hi) = (*min, *max);
            if field == "min" {
                lo[a] = v;
            } else {
                hi[a] = v;
            }
            if hi[a].v <= lo[a].v {
                return Err(tr("fixture-clamp-order"));
            }
            (*min, *max) = (lo, hi);
        }
        _ => return Err(NO_FIELD.to_owned()),
    }
    Ok(())
}
