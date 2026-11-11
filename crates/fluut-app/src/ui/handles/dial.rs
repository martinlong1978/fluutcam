// SPDX-License-Identifier: MIT OR Apache-2.0

//! The index angle dial in the viewport: the ring, its quarter marks and
//! the pointer. Dragging the pointer turns it with the cursor, found as the
//! point of the ring plane under the cursor, or, when the ring is seen
//! edge-on, the nearest point of the ring on the screen. Shift rounds to 1
//! degree and Alt does not round; all of them take 0, 90, 180 and 270
//! exactly when near. Nothing is sent until the release, which is one edit.

use super::{HandleDrag, id_of};
use crate::brand::{self, c};
use crate::jobs::Jobs;
use crate::session::Snapshot;
use crate::ui::viewport::ViewportState;
use crate::vm::handles::dial::{self, Step};
use crate::vm::handles::{Handle, HandleKind};
use eframe::egui::{self, Pos2, Rect, Sense, Stroke, Vec2};

/// How many segments the ring is drawn with.
const RING_SEGMENTS: usize = 96;

pub(super) fn overlay(
    ui: &mut egui::Ui,
    jobs: &mut Jobs,
    vs: &mut ViewportState,
    snap: &Snapshot,
    setup: &str,
    rect: Rect,
    h: &Handle,
) {
    let Some(ring) = dial::ring(snap, setup) else {
        return;
    };
    let size = rect.size();
    let cam = vs.camera;
    let to_screen = |v: [f32; 3]| {
        cam.project(glam::Vec3::from(v), glam::Vec2::new(size.x, size.y))
            .map(|q| rect.min + Vec2::new(q.x, q.y))
    };
    let painter = ui.painter_at(rect);
    let orange = c(brand::CUTTER_ORANGE);
    let ink = c(brand::INK);
    let mine = vs
        .handle_drag
        .as_ref()
        .filter(|d| d.kind == HandleKind::Dial && d.setup == setup)
        .map(|d| d.mm);
    let angle = mine.unwrap_or(h.value_mm);
    // The ring and its quarter marks.
    let pts: Vec<Pos2> = (0..=RING_SEGMENTS)
        .filter_map(|k| to_screen(ring.point(k as f64 * 360.0 / RING_SEGMENTS as f64)))
        .collect();
    painter.add(egui::Shape::line(
        pts,
        Stroke::new(1.0, ink.gamma_multiply(0.6)),
    ));
    let centre = to_screen(ring.centre);
    for q in 0..4 {
        let deg = f64::from(q) * 90.0;
        let rim = ring.point(deg);
        let inner = [rim[0], rim[1] * 0.92, rim[2] * 0.92];
        if let (Some(a), Some(b)) = (to_screen(rim), to_screen(inner)) {
            painter.line_segment([a, b], Stroke::new(1.5, ink));
            painter.text(
                a + (a - b) * 0.9,
                egui::Align2::CENTER_CENTER,
                format!("{deg:.0}"),
                egui::FontId::proportional(10.0),
                ink,
            );
        }
    }
    let Some(shown) = to_screen(ring.point(angle)) else {
        return;
    };
    if let Some(from) = centre {
        painter.line_segment([from, shown], Stroke::new(1.0, orange.gamma_multiply(0.7)));
    }
    let r = ui.interact(
        Rect::from_center_size(shown, Vec2::splat(20.0)),
        egui::Id::new(("handle", setup, id_of(HandleKind::Dial))),
        Sense::drag(),
    );
    let hot = r.hovered() || r.dragged();
    let radius = if hot { 8.0 } else { 6.5 };
    painter.circle_filled(shown, radius, orange);
    painter.circle_stroke(shown, radius, Stroke::new(1.0, ink));
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if r.drag_started() {
        vs.handle_drag = Some(HandleDrag {
            setup: setup.to_owned(),
            kind: HandleKind::Dial,
            mm: h.value_mm,
            target: None,
        });
    }
    if r.dragged()
        && let Some(p) = r.interact_pointer_pos()
        && let Some(d) = vs.handle_drag.as_mut()
    {
        let step = ui.input(|i| {
            if i.modifiers.alt {
                Step::Free
            } else if i.modifiers.shift {
                Step::Fine
            } else {
                Step::Coarse
            }
        });
        let local = p - rect.min;
        let (origin, dir) = cam.ray(
            glam::Vec2::new(local.x, local.y),
            glam::Vec2::new(size.x, size.y),
        );
        let raw = dial::angle_on_ray(&ring, origin.into(), dir.into(), d.mm).or_else(|| {
            let project = |v: [f32; 3]| to_screen(v).map(|q| [q.x, q.y]);
            dial::angle_on_screen(&ring, &project, [p.x, p.y], d.mm)
        });
        if let Some(raw) = raw {
            d.mm = dial::snap_angle(raw, step);
        }
        painter.text(
            shown + Vec2::new(12.0, -12.0),
            egui::Align2::LEFT_BOTTOM,
            format!("{}\u{b0}", crate::vm::fmt_num(d.mm.rem_euclid(360.0))),
            egui::FontId::proportional(13.0),
            orange,
        );
    }
    if r.drag_stopped()
        && let Some(d) = vs.handle_drag.take()
        && let Some(e) = dial::angle_edit(snap, setup, d.mm)
    {
        jobs.edit(e);
    }
}
