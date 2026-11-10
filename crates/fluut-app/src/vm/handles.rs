// SPDX-License-Identifier: MIT OR Apache-2.0

//! Handles on the model (S 08 §8.4): the work zero, which snaps to the
//! stock's corners and top centre, and the margins of block stock. A drag
//! changes nothing until it ends; then it is one edit, so one undo step.
//! Numeric entry stays in the inspector.

use super::inspector::num_text;
use super::setup::{setup_edit, zero_key};
use crate::jobs::Edit;
use crate::session::Snapshot;
use fluut_model::FieldKind;
use fluut_model::project::{Corner, StockSpec};
use fluut_model::setup::resolve_setup;

/// What a handle changes.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum HandleKind {
    Zero,
    /// The margin of stock by axis: 0 each side in X, 1 each side in Y,
    /// 2 on top.
    Margin(usize),
}

/// A handle, in the work frame of its Setup.
#[derive(Clone, PartialEq, Debug)]
pub struct Handle {
    pub kind: HandleKind,
    pub at: [f32; 3],
    /// For a margin: the unit direction it grows in.
    pub dir: [f32; 3],
    /// For a margin: its value, mm.
    pub value_mm: f64,
}

/// Smallest step of a margin drag, mm.
const MARGIN_STEP: f64 = 0.1;

/// The handles of a Setup: the work zero at the origin of W, and for block
/// stock given by margin, one at the middle of each face the margin grows
/// (+X, +Y and the top).
pub fn handles(s: &Snapshot, setup: &str) -> Vec<Handle> {
    let Some(b) = s.scenes.get(setup).and_then(|c| c.stock) else {
        return Vec::new();
    };
    let mut out = vec![Handle {
        kind: HandleKind::Zero,
        at: [0.0; 3],
        dir: [0.0; 3],
        value_mm: 0.0,
    }];
    let Ok(r) = resolve_setup(&s.project, setup) else {
        return out;
    };
    let mid = |i: usize| 0.5 * (b.min[i] + b.max[i]);
    if let Some(StockSpec::Block {
        margin: Some(m), ..
    }) = &r.stock
    {
        let faces = [
            ([b.max[0], mid(1), mid(2)], [1.0, 0.0, 0.0]),
            ([mid(0), b.max[1], mid(2)], [0.0, 1.0, 0.0]),
            ([mid(0), mid(1), b.max[2]], [0.0, 0.0, 1.0]),
        ];
        for (i, (at, dir)) in faces.into_iter().enumerate() {
            out.push(Handle {
                kind: HandleKind::Margin(i),
                at,
                dir,
                value_mm: m[i].v,
            });
        }
    }
    out
}

/// The places the work zero snaps to, as `zero.choice` keys with their
/// position in the current W: the stock corners and top centre, or the two
/// ends of the stock on the rotary axis.
pub fn zero_targets(s: &Snapshot, setup: &str) -> Vec<(String, [f32; 3])> {
    let Some(b) = s.scenes.get(setup).and_then(|c| c.stock) else {
        return Vec::new();
    };
    let Ok(r) = resolve_setup(&s.project, setup) else {
        return Vec::new();
    };
    if r.kind.is_rotary() {
        return vec![
            ("stock_left".to_owned(), [b.min[0], 0.0, 0.0]),
            ("stock_right".to_owned(), [b.max[0], 0.0, 0.0]),
        ];
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
    out
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
    if zero_key(&r.zero) == key {
        return None;
    }
    setup_edit(s, setup, "zero.choice", key)
        .ok()
        .map(Edit::Model)
}

/// A margin after a drag of `delta_mm` from `base_mm`: not below nothing, in
/// steps of a tenth of a mm.
pub fn dragged_margin(base_mm: f64, delta_mm: f64) -> f64 {
    let v = (base_mm + delta_mm).max(0.0);
    (v / MARGIN_STEP).round() * MARGIN_STEP
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
    let current = handles(s, setup)
        .into_iter()
        .find(|h| h.kind == HandleKind::Margin(axis))
        .map(|h| h.value_mm);
    if current.is_some_and(|c| (c - mm).abs() < 1e-9) {
        return Ok(None);
    }
    let text = num_text(mm, FieldKind::Length, s.project.units);
    setup_edit(s, setup, &format!("stock.margin.{axis}"), &text).map(|e| Some(Edit::Model(e)))
}

#[cfg(test)]
mod tests;
