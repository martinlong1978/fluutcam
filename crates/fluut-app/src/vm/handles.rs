// SPDX-License-Identifier: MIT OR Apache-2.0

//! Handles on the model (S 08 §8.4): the work zero, which snaps to the
//! stock's corners and top centre, the model's vertices and the centres of
//! its arcs and circles; the margins and the size of block stock; and the
//! radius and length of cylinder stock. A drag changes nothing until it
//! ends; then it is one edit, so one undo step. Numeric entry stays in the
//! inspector.

use super::inspector::num_text;
use super::setup::{setup_edit, zero_key};
use crate::jobs::Edit;
use crate::session::Snapshot;
use fluut_model::FieldKind;
use fluut_model::edit::setup_field_text;
use fluut_model::params::Num;
use fluut_model::project::{AxisX, Corner, Setup, StockSpec, ZeroSpec};
use fluut_model::setup::resolve_setup;

/// What a handle changes.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum HandleKind {
    Zero,
    /// The margin of stock by axis: 0 each side in X, 1 each side in Y,
    /// 2 on top.
    Margin(usize),
    /// The size of block stock given by size: 0 in X, 1 in Y, 2 in Z.
    Size(usize),
    /// The radius of cylinder stock.
    Radius,
    /// The length of cylinder stock.
    Length,
}

impl HandleKind {
    /// The `setup_edit` key of the value the handle drags; `None` for the
    /// zero.
    pub fn key(self) -> Option<String> {
        match self {
            HandleKind::Zero => None,
            HandleKind::Margin(i) => Some(format!("stock.margin.{i}")),
            HandleKind::Size(i) => Some(format!("stock.size.{i}")),
            HandleKind::Radius => Some("stock.radius".to_owned()),
            HandleKind::Length => Some("stock.length".to_owned()),
        }
    }
}

/// A handle, in the work frame of its Setup.
#[derive(Clone, PartialEq, Debug)]
pub struct Handle {
    pub kind: HandleKind,
    pub at: [f32; 3],
    /// For a stock value: the unit direction it grows in.
    pub dir: [f32; 3],
    /// For a stock value: it, mm.
    pub value_mm: f64,
    /// How far the value moves per mm the handle moves: 2 where the stock
    /// grows on both sides of the model (a length, a size in X and Y).
    pub gain: f64,
}

/// Smallest step of a drag, mm.
const MARGIN_STEP: f64 = 0.1;

/// The handles of a Setup: the work zero at the origin of W, and for stock
/// one at the middle of each face its margin or size grows (+X, +Y and the
/// top), or for a cylinder its radius and its right end.
pub fn handles(s: &Snapshot, setup: &str) -> Vec<Handle> {
    let Some(b) = s.scenes.get(setup).and_then(|c| c.stock) else {
        return Vec::new();
    };
    let mut out = vec![Handle {
        kind: HandleKind::Zero,
        at: [0.0; 3],
        dir: [0.0; 3],
        value_mm: 0.0,
        gain: 1.0,
    }];
    let Ok(r) = resolve_setup(&s.project, setup) else {
        return out;
    };
    let mid = |i: usize| 0.5 * (b.min[i] + b.max[i]);
    let faces = [
        ([b.max[0], mid(1), mid(2)], [1.0, 0.0, 0.0]),
        ([mid(0), b.max[1], mid(2)], [0.0, 1.0, 0.0]),
        ([mid(0), mid(1), b.max[2]], [0.0, 0.0, 1.0]),
    ];
    match &r.stock {
        Some(StockSpec::Block { size: Some(v), .. }) => {
            // Centred on the model in X and Y: both sides move.
            for (i, (at, dir)) in faces.into_iter().enumerate() {
                out.push(Handle {
                    kind: HandleKind::Size(i),
                    at,
                    dir,
                    value_mm: v[i].v,
                    gain: if i == 2 { 1.0 } else { 2.0 },
                });
            }
        }
        Some(StockSpec::Block {
            margin: Some(m), ..
        }) => {
            for (i, (at, dir)) in faces.into_iter().enumerate() {
                out.push(Handle {
                    kind: HandleKind::Margin(i),
                    at,
                    dir,
                    value_mm: m[i].v,
                    gain: 1.0,
                });
            }
        }
        Some(StockSpec::Cylinder { radius, length }) => {
            out.push(Handle {
                kind: HandleKind::Radius,
                at: [mid(0), 0.0, radius.v as f32],
                dir: [0.0, 0.0, 1.0],
                value_mm: radius.v,
                gain: 1.0,
            });
            // The cylinder is centred on the model: both ends move.
            out.push(Handle {
                kind: HandleKind::Length,
                at: [b.max[0], 0.0, 0.0],
                dir: [1.0, 0.0, 0.0],
                value_mm: length.v,
                gain: 2.0,
            });
        }
        _ => {}
    }
    out
}

/// Positions closer than this, mm, are one place.
const SAME_PLACE: f32 = 1e-3;

/// The places the work zero snaps to, as keys with their position in the
/// current W: the stock corners and top centre, or the two ends of the stock
/// on the rotary axis (`zero.choice` keys), and then the model's vertices
/// and arc or circle centres (`point:x,y,z` in the model frame, or
/// `axis:x` in a rotary Setup, where only X counts).
pub fn zero_targets(s: &Snapshot, setup: &str) -> Vec<(String, [f32; 3])> {
    let Some(scene) = s.scenes.get(setup) else {
        return Vec::new();
    };
    let Some(b) = scene.stock else {
        return Vec::new();
    };
    let Ok(r) = resolve_setup(&s.project, setup) else {
        return Vec::new();
    };
    let rotary = r.kind.is_rotary();
    let with_model_points = |mut out: Vec<(String, [f32; 3])>| {
        for p in &scene.snap {
            // A stock target at the same place keeps its name.
            if out
                .iter()
                .any(|(_, q)| q.iter().zip(&p.at).all(|(a, b)| (a - b).abs() < SAME_PLACE))
            {
                continue;
            }
            let key = if rotary {
                format!("axis:{:.6}", p.axis_x)
            } else {
                format!(
                    "point:{:.6},{:.6},{:.6}",
                    p.model[0], p.model[1], p.model[2]
                )
            };
            // Two points on one X are one target on the axis.
            if rotary && out.iter().any(|(k, _)| *k == key) {
                continue;
            }
            out.push((key, p.at));
        }
        out
    };
    if rotary {
        return with_model_points(vec![
            ("stock_left".to_owned(), [b.min[0], 0.0, 0.0]),
            ("stock_right".to_owned(), [b.max[0], 0.0, 0.0]),
        ]);
    }
    let corner = |c: Corner, z: f32| {
        let x = match c {
            Corner::FrontLeft | Corner::BackLeft => b.min[0],
            _ => b.max[0],
        };
        let y = match c {
            Corner::FrontLeft | Corner::FrontRight => b.min[1],
            _ => b.max[1],
        };
        [x, y, z]
    };
    let mut out = Vec::new();
    for c in Corner::ALL {
        out.push((format!("top.{}", c.as_str()), corner(c, b.max[2])));
    }
    out.push((
        "top_centre".to_owned(),
        [
            0.5 * (b.min[0] + b.max[0]),
            0.5 * (b.min[1] + b.max[1]),
            b.max[2],
        ],
    ));
    for c in Corner::ALL {
        out.push((format!("bottom.{}", c.as_str()), corner(c, b.min[2])));
    }
    with_model_points(out)
}

/// The nearest target to `at` on the screen, within `radius`; its index.
pub fn nearest_target(targets: &[[f32; 2]], at: [f32; 2], radius: f32) -> Option<usize> {
    let d = |p: &[f32; 2]| ((p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2)).sqrt();
    targets
        .iter()
        .enumerate()
        .map(|(i, p)| (i, d(p)))
        .filter(|(_, d)| *d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The edit of dropping the zero on target `key`; `None` when the zero is
/// there already.
pub fn zero_edit(s: &Snapshot, setup: &str, key: &str) -> Option<Edit> {
    let r = resolve_setup(&s.project, setup).ok()?;
    let numbers =
        |text: &str| -> Option<Vec<f64>> { text.split(',').map(|v| v.parse().ok()).collect() };
    let zero = if let Some(p) = key.strip_prefix("point:") {
        let p = numbers(p).filter(|p| p.len() == 3)?;
        ZeroSpec::Point([Num::float(p[0]), Num::float(p[1]), Num::float(p[2])])
    } else if let Some(x) = key.strip_prefix("axis:") {
        let x = numbers(x).filter(|p| p.len() == 1)?;
        ZeroSpec::OnAxis {
            x: AxisX::At(Num::float(x[0])),
        }
    } else {
        if zero_key(&r.zero) == key {
            return None;
        }
        return setup_edit(s, setup, "zero.choice", key)
            .ok()
            .map(Edit::Model);
    };
    let same = match (&zero, &r.zero) {
        (ZeroSpec::Point(a), ZeroSpec::Point(b)) => {
            a.iter().zip(b).all(|(a, b)| (a.v - b.v).abs() < 1e-6)
        }
        (ZeroSpec::OnAxis { x: AxisX::At(a) }, ZeroSpec::OnAxis { x: AxisX::At(b) }) => {
            (a.v - b.v).abs() < 1e-6
        }
        _ => false,
    };
    if same {
        return None;
    }
    let mut t = Setup::new(setup, "");
    t.zero = Some(zero);
    let text = setup_field_text(&t, "zero");
    Some(Edit::Model(fluut_model::edit::Edit::set_setup_field(
        setup,
        "zero",
        text.as_deref(),
    )))
}

/// A stock value after a drag of `delta_mm` from `base_mm`: not below
/// `min_mm`, in steps of a tenth of a mm.
fn stepped(base_mm: f64, delta_mm: f64, min_mm: f64) -> f64 {
    let v = (base_mm + delta_mm).max(min_mm);
    (v / MARGIN_STEP).round() * MARGIN_STEP
}

/// A margin after a drag of `delta_mm` from `base_mm`: not below nothing, in
/// steps of a tenth of a mm.
pub fn dragged_margin(base_mm: f64, delta_mm: f64) -> f64 {
    stepped(base_mm, delta_mm, 0.0)
}

/// The value of a handle after its drag moved it `travel_mm` along its
/// direction: a margin is not below nothing, a size, radius or length not
/// below a step.
pub fn dragged_value(h: &Handle, travel_mm: f64) -> f64 {
    let min = if matches!(h.kind, HandleKind::Margin(_)) {
        0.0
    } else {
        MARGIN_STEP
    };
    stepped(h.value_mm, h.gain * travel_mm, min)
}

/// The edit of setting the value `kind` drags to `mm`; `None` when it is as
/// it was.
///
/// # Errors
///
/// The message of a refused entry.
pub fn value_edit(
    s: &Snapshot,
    setup: &str,
    kind: HandleKind,
    mm: f64,
) -> Result<Option<Edit>, String> {
    let key = kind.key().ok_or("the zero has no value")?;
    let current = handles(s, setup)
        .into_iter()
        .find(|h| h.kind == kind)
        .map(|h| h.value_mm);
    if current.is_some_and(|c| (c - mm).abs() < 1e-9) {
        return Ok(None);
    }
    let text = num_text(mm, FieldKind::Length, s.project.units);
    setup_edit(s, setup, &key, &text).map(|e| Some(Edit::Model(e)))
}

/// The edit of setting margin `axis` to `mm`; `None` when it is as it was.
///
/// # Errors
///
/// The message of a refused entry.
pub fn margin_edit(
    s: &Snapshot,
    setup: &str,
    axis: usize,
    mm: f64,
) -> Result<Option<Edit>, String> {
    value_edit(s, setup, HandleKind::Margin(axis), mm)
}

#[cfg(test)]
mod tests;
