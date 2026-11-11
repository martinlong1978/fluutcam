// SPDX-License-Identifier: MIT OR Apache-2.0

//! Drag handles on the model (S 08 §8.4), drawn by `egui` over the
//! viewport: the work zero, which snaps to the stock's corners and top
//! centre, and the margins of block stock. Nothing is sent while dragging;
//! the release is one edit, so one undo step.

use super::viewport::ViewportState;
use crate::brand::{self, c};
use crate::jobs::Jobs;
use crate::session::Snapshot;
use crate::vm::handles::{
    HandleKind, dragged_value, handles, nearest_target, value_edit, zero_edit, zero_targets,
};

mod dial;
use crate::vm::inspector::num_text;
use eframe::egui::{self, Pos2, Rect, Sense, Stroke, Vec2};
use fluut_model::FieldKind;

/// How near the pointer must be to a target for the zero to snap, points.
const SNAP_RADIUS: f32 = 48.0;

/// A handle being dragged.
#[derive(Clone, PartialEq, Debug)]
pub struct HandleDrag {
    pub setup: String,
    pub kind: HandleKind,
    /// A stock value: how far the handle has been dragged along its
    /// direction, mm.
    pub mm: f64,
    /// The zero: the target it would drop on (a `zero.choice` key).
    pub target: Option<String>,
}

fn id_of(kind: HandleKind) -> usize {
    match kind {
        HandleKind::Zero => 0,
        HandleKind::Margin(i) => i + 1,
        HandleKind::Size(i) => i + 4,
        HandleKind::Radius => 7,
        HandleKind::Length => 8,
        HandleKind::Dial => 9,
    }
}

/// Draws and runs the handles of `setup`. `vs.handle_marks` gets the screen
/// position of each.
pub fn overlay(
    ui: &mut egui::Ui,
    jobs: &mut Jobs,
    vs: &mut ViewportState,
    snap: &Snapshot,
    setup: &str,
    rect: Rect,
) {
    let size = rect.size();
    let cam = vs.camera;
    let px = |v: [f32; 3]| {
        cam.project(glam::Vec3::from(v), glam::Vec2::new(size.x, size.y))
            .map(|q| rect.min + Vec2::new(q.x, q.y))
    };
    let painter = ui.painter_at(rect);
    let orange = c(brand::CUTTER_ORANGE);
    vs.handle_marks.clear();
    let targets = zero_targets(snap, setup);
    let screen: Vec<(usize, Pos2)> = targets
        .iter()
        .enumerate()
        .filter_map(|(i, (_, p))| px(*p).map(|q| (i, q)))
        .collect();
    for h in handles(snap, setup) {
        let Some(at) = px(h.at) else {
            continue;
        };
        vs.handle_marks.push((h.kind, at));
        if h.kind == HandleKind::Dial {
            dial::overlay(ui, jobs, vs, snap, setup, rect, &h);
            continue;
        }
        let mine = vs
            .handle_drag
            .as_ref()
            .filter(|d| d.kind == h.kind && d.setup == setup)
            .cloned();
        // A stock handle follows the drag.
        let shown = match (h.kind, &mine) {
            (HandleKind::Zero, _) | (_, None) => at,
            (_, Some(d)) => {
                let grown = ((dragged_value(&h, d.mm) - h.value_mm) / h.gain) as f32;
                px([
                    h.at[0] + h.dir[0] * grown,
                    h.at[1] + h.dir[1] * grown,
                    h.at[2] + h.dir[2] * grown,
                ])
                .unwrap_or(at)
            }
        };
        let id = egui::Id::new(("handle", setup, id_of(h.kind)));
        let r = ui.interact(
            Rect::from_center_size(shown, Vec2::splat(18.0)),
            id,
            Sense::drag(),
        );
        let hot = r.hovered() || r.dragged();
        match h.kind {
            HandleKind::Zero => {
                let w = if hot { 2.5 } else { 1.5 };
                painter.circle_stroke(at, 9.0, Stroke::new(w, orange));
            }
            _ => {
                let half = if hot { 6.0 } else { 4.5 };
                let sq = Rect::from_center_size(shown, Vec2::splat(half * 2.0));
                painter.rect_filled(sq, 1.0, orange);
                painter.rect_stroke(
                    sq,
                    1.0,
                    Stroke::new(1.0, c(brand::INK)),
                    egui::StrokeKind::Outside,
                );
            }
        }
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if r.drag_started() {
            vs.handle_drag = Some(HandleDrag {
                setup: setup.to_owned(),
                kind: h.kind,
                mm: 0.0,
                target: None,
            });
        }
        if r.dragged()
            && let Some(d) = vs.handle_drag.as_mut()
        {
            match h.kind {
                HandleKind::Zero => {
                    let to = r.interact_pointer_pos().and_then(|p| {
                        let pts: Vec<[f32; 2]> = screen.iter().map(|(_, q)| [q.x, q.y]).collect();
                        nearest_target(&pts, [p.x, p.y], SNAP_RADIUS).map(|k| screen[k].0)
                    });
                    d.target = to.map(|i| targets[i].0.clone());
                    // Every place it can go, and the one it would take.
                    // Model points show only near the pointer, so a busy
                    // model does not bury the stock's places.
                    let pointer = r.interact_pointer_pos();
                    for (i, q) in &screen {
                        let on = to == Some(*i);
                        let model_point = targets[*i].0.starts_with("point:")
                            || targets[*i].0.starts_with("axis:");
                        if model_point
                            && !on
                            && pointer.is_none_or(|p| p.distance(*q) > 3.0 * SNAP_RADIUS)
                        {
                            continue;
                        }
                        let col = if on { orange } else { c(brand::INK) };
                        painter.circle_stroke(
                            *q,
                            if on { 8.0 } else { 4.0 },
                            Stroke::new(2.0, col),
                        );
                    }
                }
                _ => {
                    // Screen points per mm along the handle's direction.
                    let tip = px([
                        h.at[0] + h.dir[0] * 10.0,
                        h.at[1] + h.dir[1] * 10.0,
                        h.at[2] + h.dir[2] * 10.0,
                    ]);
                    if let Some(tip) = tip {
                        let v = (tip - at) / 10.0;
                        if v.length_sq() > 1e-6 {
                            d.mm += f64::from(r.drag_delta().dot(v) / v.length_sq());
                        }
                    }
                    let now = dragged_value(&h, d.mm);
                    let text = num_text(now, FieldKind::Length, snap.project.units);
                    painter.text(
                        shown + Vec2::new(12.0, -12.0),
                        egui::Align2::LEFT_BOTTOM,
                        text,
                        egui::FontId::proportional(13.0),
                        orange,
                    );
                }
            }
        }
        if r.drag_stopped()
            && let Some(d) = vs.handle_drag.take()
        {
            let edit = match d.kind {
                HandleKind::Zero => d.target.and_then(|t| zero_edit(snap, setup, &t)),
                kind => value_edit(snap, setup, kind, dragged_value(&h, d.mm))
                    .ok()
                    .flatten(),
            };
            if let Some(e) = edit {
                jobs.edit(e);
            }
        }
    }
}
