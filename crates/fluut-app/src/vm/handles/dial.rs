// SPDX-License-Identifier: MIT OR Apache-2.0

//! The index angle dial (S 08 §8.4): a ring in the YZ plane of the Setup
//! frame W around the rotary axis X, with a pointer at the model's own
//! +Z direction as the index angle turns it. Dragging the pointer sets the
//! angle. Everything here is plain numbers; the window maps the pointer to
//! a ray or a screen point and calls in.

use crate::jobs::Edit;
use crate::session::Snapshot;
use fluut_model::edit::{Edit as ModelEdit, setup_field_text};
use fluut_model::params::Num;
use fluut_model::project::{AngleMode, IndexAngle, Setup, SetupKind};
use fluut_model::setup::resolve_setup;

/// The step of a drag with no modifier, degrees.
pub const COARSE_STEP: f64 = 5.0;
/// The step with Shift, degrees.
pub const FINE_STEP: f64 = 1.0;
/// Within this many degrees of 0, 90, 180 or 270 a free or fine drag takes
/// the angle exactly.
pub const MAGNET: f64 = 2.0;
/// A ray whose direction has less than this of X is edge-on to the ring's
/// plane: the intersection is too far away to mean anything.
const EDGE_ON: f32 = 0.05;

/// How a drag rounds the angle.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Step {
    /// Whole steps of [`COARSE_STEP`].
    Coarse,
    /// Whole steps of [`FINE_STEP`], and the magnet.
    Fine,
    /// No rounding, but the magnet.
    Free,
}

/// The ring, in W.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Ring {
    pub centre: [f32; 3],
    pub radius: f32,
}

impl Ring {
    /// Where the pointer is for `deg`: the model's +Z, turned about X.
    pub fn point(&self, deg: f64) -> [f32; 3] {
        let (s, c) = libm::sincos(deg.to_radians());
        [
            self.centre[0],
            self.centre[1] - self.radius * s as f32,
            self.centre[2] + self.radius * c as f32,
        ]
    }
}

/// The ring of an indexed Setup: at the stock's start face, a fifth wider
/// than the stock round the axis; `None` for other kinds or no stock.
pub fn ring(s: &Snapshot, setup: &str) -> Option<Ring> {
    let b = s.scenes.get(setup)?.stock?;
    let r = resolve_setup(&s.project, setup).ok()?;
    if r.kind != SetupKind::Indexed {
        return None;
    }
    let extent = [b.min[1], b.max[1], b.min[2], b.max[2]]
        .iter()
        .fold(0.0f32, |m, v| m.max(v.abs()));
    Some(Ring {
        centre: [b.min[0], 0.0, 0.0],
        radius: (extent * 1.2).max(5.0),
    })
}

/// The resolved index angle of the Setup, degrees.
pub fn angle(s: &Snapshot, setup: &str) -> Option<f64> {
    Some(resolve_setup(&s.project, setup).ok()?.index_angle)
}

/// `raw` (any number of degrees) moved by whole turns to within half a turn
/// of `near`, so a drag past 180 degrees goes on instead of jumping.
pub fn unwrap_near(raw: f64, near: f64) -> f64 {
    let d = (raw - near).rem_euclid(360.0);
    near + if d > 180.0 { d - 360.0 } else { d }
}

/// `deg` rounded the way `step` says, then taken to the nearest multiple of
/// 90 degrees that lies within the magnet.
pub fn snap_angle(deg: f64, step: Step) -> f64 {
    let rounded = match step {
        Step::Coarse => (deg / COARSE_STEP).round() * COARSE_STEP,
        Step::Fine => (deg / FINE_STEP).round() * FINE_STEP,
        Step::Free => deg,
    };
    let quarter = (rounded / 90.0).round() * 90.0;
    let v = if (rounded - quarter).abs() <= MAGNET {
        quarter
    } else {
        rounded
    };
    // No negative zero in the entry.
    v + 0.0
}

/// The angle of the point where the ray from `origin` along `dir` meets the
/// ring's plane, near `near` degrees; `None` when the ray is edge-on to the
/// plane or points away from it.
pub fn angle_on_ray(ring: &Ring, origin: [f32; 3], dir: [f32; 3], near: f64) -> Option<f64> {
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if len < 1e-9 || (dir[0] / len).abs() < EDGE_ON {
        return None;
    }
    let t = (ring.centre[0] - origin[0]) / dir[0];
    if t <= 0.0 {
        return None;
    }
    let y = f64::from(origin[1] + t * dir[1] - ring.centre[1]);
    let z = f64::from(origin[2] + t * dir[2] - ring.centre[2]);
    if y.hypot(z) < 1e-6 {
        return None;
    }
    // Pointer at (-sin a, cos a).
    Some(unwrap_near(libm::atan2(-y, z).to_degrees(), near))
}

/// The angle whose ring point lies nearest `pointer` on the screen, near
/// `near` degrees. For a ring seen edge-on, where a ray gives no answer:
/// the two sides of a line squeezed flat are equally near, so the one
/// nearer `near` wins.
pub fn angle_on_screen(
    ring: &Ring,
    project: &dyn Fn([f32; 3]) -> Option<[f32; 2]>,
    pointer: [f32; 2],
    near: f64,
) -> Option<f64> {
    const SAMPLES: usize = 720;
    let mut best: Option<(f64, f64)> = None;
    for k in 0..SAMPLES {
        let a = k as f64 * 360.0 / SAMPLES as f64;
        let Some(p) = project(ring.point(a)) else {
            continue;
        };
        let d = f64::from(((p[0] - pointer[0]).powi(2) + (p[1] - pointer[1]).powi(2)).sqrt());
        let a = unwrap_near(a, near);
        let closer_in_time = |b: f64| (a - near).abs() < (b - near).abs();
        match best {
            Some((bd, ba)) if d > bd + 0.5 || (d > bd - 0.5 && !closer_in_time(ba)) => {}
            _ => best = Some((d, a)),
        }
    }
    best.map(|(_, a)| a)
}

/// The edit that makes the Setup's resolved index angle `deg`; `None` when
/// it is already. A relative angle is written as the difference from the
/// angle it is relative to; the angle is kept within 0..360 (absolute) or
/// -180..180 (relative).
pub fn angle_edit(s: &Snapshot, setup: &str, deg: f64) -> Option<Edit> {
    let r = resolve_setup(&s.project, setup).ok()?;
    if r.kind != SetupKind::Indexed {
        return None;
    }
    let local = s.project.setup(setup).and_then(|x| x.index_angle.as_ref());
    let (mode, value) = match local {
        Some(IndexAngle {
            mode: AngleMode::Relative,
            value,
        }) => {
            let base = r.index_angle - value.v;
            let d = (deg - base + 180.0).rem_euclid(360.0) - 180.0;
            (AngleMode::Relative, if d == -180.0 { 180.0 } else { d })
        }
        _ => (AngleMode::Absolute, deg.rem_euclid(360.0)),
    };
    let same = (deg - r.index_angle).rem_euclid(360.0);
    if same.min(360.0 - same) < 1e-9 {
        return None;
    }
    let mut t = Setup::new(setup, "");
    t.index_angle = Some(IndexAngle {
        mode,
        value: Num::float(value + 0.0),
    });
    let text = setup_field_text(&t, "index_angle");
    Some(Edit::Model(ModelEdit::set_setup_field(
        setup,
        "index_angle",
        text.as_deref(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::Jobs;
    use crate::session::tests::opened;
    use crate::viewport::camera::Camera;
    use crate::vm::handles::{HandleKind, handles};
    use crate::vm::setup::setup_edit;
    use std::sync::Arc;

    fn run(j: &mut Jobs, e: Edit) -> Arc<Snapshot> {
        j.edit(e);
        j.wait_snapshot().unwrap()
    }

    fn model(s: &Snapshot, id: &str, key: &str, text: &str) -> Edit {
        Edit::Model(setup_edit(s, id, key, text).unwrap())
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn angles_round_to_steps_and_take_the_quarters() {
        assert!(near(snap_angle(47.0, Step::Coarse), 45.0));
        assert!(near(snap_angle(88.0, Step::Coarse), 90.0));
        assert!(near(snap_angle(2.4, Step::Coarse), 0.0));
        assert!(near(snap_angle(273.0, Step::Coarse), 275.0));
        // The fine step is a whole degree; the magnet takes the quarters.
        assert!(near(snap_angle(46.4, Step::Fine), 46.0));
        assert!(near(snap_angle(91.4, Step::Fine), 90.0));
        assert!(near(snap_angle(93.0, Step::Fine), 93.0));
        // Free keeps the fraction, except near a quarter.
        assert!(near(snap_angle(46.37, Step::Free), 46.37));
        assert!(near(snap_angle(178.2, Step::Free), 180.0));
        assert!(near(snap_angle(-1.5, Step::Free), 0.0));
        assert!(snap_angle(-0.4, Step::Free).is_sign_positive());
        assert!(near(snap_angle(-91.0, Step::Free), -90.0));
    }

    #[test]
    fn a_turn_past_half_way_goes_on() {
        assert!(near(unwrap_near(350.0, 10.0), -10.0));
        assert!(near(unwrap_near(10.0, 350.0), 370.0));
        assert!(near(unwrap_near(-170.0, 170.0), 190.0));
        assert!(near(unwrap_near(45.0, 45.0), 45.0));
    }

    #[test]
    fn the_pointer_is_the_model_z_turned_about_x() {
        let ring = Ring {
            centre: [5.0, 0.0, 0.0],
            radius: 20.0,
        };
        let close = |p: [f32; 3], q: [f32; 3]| p.iter().zip(q).all(|(a, b)| (a - b).abs() < 1e-4);
        assert!(close(ring.point(0.0), [5.0, 0.0, 20.0]));
        assert!(close(ring.point(90.0), [5.0, -20.0, 0.0]));
        assert!(close(ring.point(180.0), [5.0, 0.0, -20.0]));
        assert!(close(ring.point(270.0), [5.0, 20.0, 0.0]));
    }

    /// The model turns the way the pointer does: a point of the part goes
    /// where the frame's turn about X puts it.
    #[test]
    fn the_pointer_turns_the_way_the_part_does() {
        let mut j = opened("flipside");
        let s = j.snapshot.clone().unwrap();
        let before = s.scenes["s1"].snap.clone();
        let e = model(&s, "s1", "index_angle", "90");
        let s = run(&mut j, e);
        let after = &s.scenes["s1"].snap;
        assert_eq!(before.len(), after.len());
        let k = before
            .iter()
            .position(|p| p.at[1].abs() > 1.0 && p.at[2].abs() > 1.0)
            .expect("a vertex away from the axis");
        let (a, b) = (before[k].at, after[k].at);
        // 90 degrees: (y, z) goes to (-z, y), as the ring pointer does.
        assert!(
            (b[1] + a[2]).abs() < 1e-3 && (b[2] - a[1]).abs() < 1e-3,
            "{a:?} {b:?}"
        );
        assert!((a[0] - b[0]).abs() < 1e-3);
    }

    #[test]
    fn a_ray_gives_the_angle_of_its_point_on_the_ring_plane() {
        let ring = Ring {
            centre: [0.0, 0.0, 0.0],
            radius: 10.0,
        };
        let from_x = |y: f32, z: f32| angle_on_ray(&ring, [-100.0, y, z], [1.0, 0.0, 0.0], 0.0);
        assert!(near(from_x(0.0, 10.0).unwrap(), 0.0));
        assert!(near(from_x(-10.0, 0.0).unwrap(), 90.0));
        assert!(near(from_x(0.0, -10.0).unwrap().abs(), 180.0));
        // From the other side of the plane the same point.
        let back = angle_on_ray(&ring, [100.0, 0.0, 10.0], [-1.0, 0.0, 0.0], 0.0);
        assert!(near(back.unwrap(), 0.0));
        // Edge-on, and pointing away, give nothing.
        assert!(angle_on_ray(&ring, [0.0, -100.0, 0.0], [0.0, 1.0, 0.0], 0.0).is_none());
        assert!(angle_on_ray(&ring, [-100.0, 0.0, 0.0], [-1.0, 0.0, 0.0], 0.0).is_none());
        // At the centre there is no angle.
        assert!(from_x(0.0, 0.0).is_none());
        // It goes on past 180 degrees from where the drag was.
        let on = angle_on_ray(&ring, [-100.0, 10.0, 0.0], [1.0, 0.0, 0.0], 200.0);
        assert!(near(on.unwrap(), 270.0));
    }

    #[test]
    fn the_camera_ray_finds_the_pointer_from_any_oblique_view() {
        let ring = Ring {
            centre: [10.0, 0.0, 0.0],
            radius: 30.0,
        };
        let size = glam::Vec2::new(800.0, 600.0);
        let mut cam = Camera::framing(glam::Vec3::splat(-40.0), glam::Vec3::splat(40.0));
        for (yaw, pitch) in [(-0.6, 0.75), (0.4, 0.2), (2.5, -0.5), (0.1, 1.2)] {
            cam.yaw = yaw;
            cam.pitch = pitch;
            for deg in [0.0, 37.0, 90.0, 133.0, 180.0, 215.0, 270.0, 341.0] {
                let p = cam.project(ring.point(deg).into(), size).unwrap();
                let (o, d) = cam.ray(p, size);
                let got = angle_on_ray(&ring, o.into(), d.into(), deg).unwrap();
                assert!((got - deg).abs() < 0.05, "{yaw} {pitch}: {deg} -> {got}");
            }
        }
    }

    #[test]
    fn a_ring_seen_edge_on_falls_back_to_the_nearest_point_on_the_screen() {
        let ring = Ring {
            centre: [0.0, 0.0, 0.0],
            radius: 30.0,
        };
        let size = glam::Vec2::new(800.0, 600.0);
        let mut cam = Camera::framing(glam::Vec3::splat(-40.0), glam::Vec3::splat(40.0));
        // From +Y with the camera level, the plane x = 0 is a line.
        cam.target = glam::Vec3::ZERO;
        cam.yaw = std::f32::consts::FRAC_PI_2;
        cam.pitch = 0.0;
        let (o, d) = cam.ray(size / 2.0, size);
        assert!(angle_on_ray(&ring, o.into(), d.into(), 0.0).is_none());
        let project = |v: [f32; 3]| cam.project(v.into(), size).map(|q| [q.x, q.y]);
        let at = project(ring.point(30.0)).unwrap();
        // Both sides of the line look the same on the screen: the side
        // nearer the angle the drag was at wins.
        let up = angle_on_screen(&ring, &project, at, 20.0).unwrap();
        assert!((up - 30.0).abs() < 1.0, "{up}");
        let down = angle_on_screen(&ring, &project, at, -20.0).unwrap();
        // (Perspective makes the far and near sides differ a little.)
        assert!(down < 0.0 && (down + 30.0).abs() < 20.0, "{down}");
    }

    #[test]
    fn only_an_indexed_setup_has_a_dial() {
        let j = opened("flipside");
        let s = j.snapshot.clone().unwrap();
        let h = handles(&s, "s2");
        let dial = h.iter().find(|h| h.kind == HandleKind::Dial).unwrap();
        assert_eq!(dial.value_mm, 180.0);
        let r = ring(&s, "s2").unwrap();
        assert_eq!(dial.at, r.point(180.0));
        let b = s.scenes["s2"].stock.unwrap();
        assert_eq!(r.centre, [b.min[0], 0.0, 0.0]);
        // The ring clears the stock all round.
        let extent = [b.min[1], b.max[1], b.min[2], b.max[2]]
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(r.radius > extent);
        for name in ["block", "shaft", "plate"] {
            let j = opened(name);
            let s = j.snapshot.clone().unwrap();
            let id = s.project.setups[0].id.clone();
            assert!(
                handles(&s, &id).iter().all(|h| h.kind != HandleKind::Dial),
                "{name}"
            );
            assert!(ring(&s, &id).is_none());
        }
        assert!(angle(&s, "nope").is_none());
    }

    #[test]
    fn a_dial_edit_sets_the_angle_in_one_undo_step() {
        let mut j = opened("flipside");
        let s = j.snapshot.clone().unwrap();
        let local = |s: &Snapshot, id: &str| s.project.setup(id).unwrap().index_angle.clone();
        // Unchanged, or a whole turn away, is no edit.
        assert!(angle_edit(&s, "s1", 0.0).is_none());
        assert!(angle_edit(&s, "s1", 360.0).is_none());
        assert!(angle_edit(&s, "s2", -180.0).is_none());
        let e = angle_edit(&s, "s1", 90.0).unwrap();
        let after = run(&mut j, e);
        let a = local(&after, "s1").unwrap();
        assert_eq!((a.mode, a.value.v), (AngleMode::Absolute, 90.0));
        // The child has its own absolute angle, which does not follow.
        assert_eq!(local(&after, "s2").unwrap().value.v, 180.0);
        // Negative and large angles are brought into 0..360.
        let e = angle_edit(&after, "s1", -45.0).unwrap();
        let after = run(&mut j, e);
        assert_eq!(local(&after, "s1").unwrap().value.v, 315.0);
        let e = angle_edit(&after, "s1", 725.0).unwrap();
        let after = run(&mut j, e);
        assert_eq!(local(&after, "s1").unwrap().value.v, 5.0);
        for _ in 0..3 {
            j.undo();
            j.wait();
        }
        assert_eq!(j.snapshot.clone().unwrap().project, s.project);
        // A three_axis Setup has no angle.
        let j = opened("block");
        let s = j.snapshot.clone().unwrap();
        assert!(angle_edit(&s, "s1", 30.0).is_none());
    }

    #[test]
    fn a_relative_angle_is_written_relative_to_its_base() {
        let mut j = opened("flipside");
        let s = j.snapshot.clone().unwrap();
        let e = model(&s, "s1", "index_angle", "30");
        run(&mut j, e);
        let e = Edit::Model(fluut_model::edit::Edit::set_setup_field(
            "s2",
            "index_angle",
            Some("{ mode = \"relative\", value = 60 }"),
        ));
        let s2 = run(&mut j, e);
        assert_eq!(angle(&s2, "s2"), Some(90.0));
        // 200 degrees resolved is 170 from the parent's 30.
        let e = angle_edit(&s2, "s2", 200.0).unwrap();
        let s3 = run(&mut j, e);
        let a = s3.project.setup("s2").unwrap().index_angle.clone().unwrap();
        assert_eq!((a.mode, a.value.v), (AngleMode::Relative, 170.0));
        assert_eq!(angle(&s3, "s2"), Some(200.0));
        // 300 resolved is 270 from 30: written as -90.
        let e = angle_edit(&s3, "s2", 300.0).unwrap();
        let s4 = run(&mut j, e);
        let a = s4.project.setup("s2").unwrap().index_angle.clone().unwrap();
        assert_eq!(a.value.v, -90.0);
        assert_eq!(angle(&s4, "s2").map(|v| v.rem_euclid(360.0)), Some(300.0));
    }
}
