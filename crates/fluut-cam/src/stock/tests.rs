// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::region::Inside;
use fluut_geom::{Point3, UnitVec3};
use fluut_path::{AxisPos, CycleKind, CycleParams, FeedRef, MotionInfo, Pass, PassKind};

fn block() -> LayeredStock {
    LayeredStock::planar(&Aabb3 {
        min: Point3::new(0.0, 0.0, 0.0),
        max: Point3::new(100.0, 50.0, 20.0),
    })
}

fn m(motion: Motion) -> MotionInfo {
    MotionInfo::new(motion, FeedRef::Cut, 0.01)
}

fn p(x: f64, y: f64, z: f64) -> AxisPos {
    AxisPos::xyz(Point3::new(x, y, z))
}

fn line(a: AxisPos, b: AxisPos, motion: Motion) -> Segment {
    Segment::Line { a, b, m: m(motion) }
}

fn result(segs: Vec<Segment>) -> ToolpathResult {
    ToolpathResult::new(vec![Pass::new(PassKind::Cut, segs)]).unwrap()
}

const FLAT: Sweep = Sweep {
    radius: 3.0,
    lift: 0.0,
};

fn inside(r: &Region2, x: f64, y: f64) -> bool {
    r.contains(Point2::new(x, y)) == Inside::In
}

#[test]
fn initial_planar_and_cylindrical() {
    let s = block();
    assert_eq!(s.top(), 20.0);
    assert!((s.at(10.0).area() - 5000.0).abs() < 1e-9);
    assert!(s.at(0.0).is_empty(), "at the bottom");
    assert!(s.at(21.0).is_empty(), "above the top");
    let c = LayeredStock::cylindrical(25.0, -5.0, 105.0);
    assert_eq!(c.frame, StockFrame::Cylindrical);
    assert!((c.at(10.0).area() - 110.0 * TAU).abs() < 1e-9);
}

#[test]
fn ensure_level_copies_the_slab() {
    let mut s = block();
    s.ensure_level(12.0);
    assert_eq!(s.keys(), vec![12.0, 20.0]);
    assert_eq!(s.at(5.0), s.at(15.0));
    s.ensure_level(25.0);
    s.ensure_level(-1.0);
    assert_eq!(s.keys(), vec![12.0, 20.0]);
}

#[test]
fn a_cut_clears_only_the_slabs_above_its_tip() {
    let mut s = block();
    let hole = Region2::rect(10.0, 10.0, 30.0, 30.0);
    s.remove_swept(12.0, &hole);
    assert!(!inside(&s.at(15.0), 20.0, 20.0));
    assert!(inside(&s.at(11.0), 20.0, 20.0));
    assert!(!inside(&s.above(12.0), 20.0, 20.0));
    assert!(inside(&s.above(11.0), 20.0, 20.0));
    // A deeper cut elsewhere leaves the first one.
    s.remove_swept(5.0, &Region2::rect(60.0, 10.0, 70.0, 20.0));
    assert!(!inside(&s.at(8.0), 65.0, 15.0));
    assert!(inside(&s.at(8.0), 20.0, 20.0));
    assert!(inside(&s.at(4.0), 65.0, 15.0));
}

#[test]
fn between_takes_the_slabs_in_a_window() {
    let mut s = block();
    s.remove_swept(15.0, &Region2::rect(10.0, 10.0, 30.0, 30.0));
    s.remove_swept(5.0, &Region2::rect(60.0, 10.0, 70.0, 20.0));
    // (15, 20]: both holes.
    let w = s.between(15.0, 18.0);
    assert!(!inside(&w, 20.0, 20.0) && !inside(&w, 65.0, 15.0));
    // (10, 15]: only the deep one.
    let w = s.between(10.0, 15.0);
    assert!(inside(&w, 20.0, 20.0) && !inside(&w, 65.0, 15.0));
    // (4, ∞): material everywhere below 5.
    let w = s.between(4.0, f64::INFINITY);
    assert!(inside(&w, 20.0, 20.0) && inside(&w, 65.0, 15.0));
    assert!(s.between(20.0, 30.0).is_empty());
}

#[test]
fn a_level_run_sweeps_its_capsule() {
    let mut s = block();
    let r = result(vec![
        line(p(10.0, 25.0, 30.0), p(10.0, 25.0, 15.0), Motion::Plunge),
        line(p(10.0, 25.0, 15.0), p(50.0, 25.0, 15.0), Motion::Cut),
        line(p(50.0, 25.0, 15.0), p(50.0, 25.0, 30.0), Motion::Retract),
    ]);
    s.apply(&r, FLAT, 0.01);
    let a = s.at(16.0);
    assert!(!inside(&a, 30.0, 27.9));
    assert!(inside(&a, 30.0, 28.1));
    assert!(inside(&s.at(14.0), 30.0, 25.0));
    // Removed area: 40 × 5.98 plus a disc of radius 2.99.
    let removed = 5000.0 - a.area();
    let want = 40.0 * 5.98 + std::f64::consts::PI * 2.99 * 2.99;
    assert!((removed - want).abs() < 0.05, "{removed} {want}");
}

#[test]
fn rapids_do_not_cut() {
    let mut s = block();
    let r = result(vec![line(
        p(10.0, 25.0, 15.0),
        p(50.0, 25.0, 15.0),
        Motion::Rapid,
    )]);
    s.apply(&r, FLAT, 0.01);
    assert_eq!(s, block());
}

#[test]
fn arcs_keep_material_outside_their_chords() {
    let mut s = block();
    let a = p(40.0, 25.0, 10.0);
    let r = result(vec![Segment::Arc {
        a,
        b: a,
        centre: Point3::new(30.0, 25.0, 10.0),
        normal: UnitVec3::Z,
        sweep: TAU,
        m: m(Motion::Cut),
    }]);
    s.apply(&r, FLAT, 0.01);
    let at = s.at(12.0);
    // The ring from 7 to 13 is cut, less the tolerance on each side.
    assert!(!inside(&at, 30.0 + 7.02, 25.0));
    assert!(!inside(&at, 30.0 - 12.97, 25.0));
    assert!(inside(&at, 30.0 + 13.01, 25.0));
    assert!(inside(&at, 30.0, 25.0));
}

#[test]
fn a_ramp_counts_at_the_level_above_it() {
    let mut s = block();
    let r = result(vec![
        line(p(10.0, 25.0, 20.0), p(30.0, 25.0, 12.0), Motion::Ramp),
        line(p(30.0, 25.0, 12.0), p(60.0, 25.0, 12.0), Motion::Cut),
        line(p(60.0, 25.0, 12.0), p(80.0, 25.0, 8.0), Motion::Ramp),
    ]);
    s.apply(&r, FLAT, 0.01);
    // The first ramp's top (20) has no level at or above it: not counted.
    assert!(inside(&s.at(19.0), 15.0, 25.0));
    // The second ramp's top (12) counts at level 12.
    assert!(!inside(&s.at(13.0), 70.0, 25.0));
    assert!(inside(&s.at(11.0), 70.0, 25.0));
}

#[test]
fn ball_and_drill_cuts_count_above_the_tip() {
    let ball = Sweep {
        radius: 3.0,
        lift: 3.0,
    };
    let mut s = block();
    let r = result(vec![line(
        p(10.0, 25.0, 10.0),
        p(50.0, 25.0, 10.0),
        Motion::Cut,
    )]);
    s.apply(&r, ball, 0.01);
    assert!(inside(&s.at(12.0), 30.0, 25.0));
    assert!(!inside(&s.at(14.0), 30.0, 25.0));
    // A drill cycle: a disc at the depth plus the point height.
    let mut s = block();
    let c = CycleParams {
        kind: CycleKind::Drill,
        top_z: 20.0,
        depth_z: 5.0,
        retract_z: 22.0,
        peck: None,
        dwell_s: None,
    };
    let r = result(vec![Segment::Cycle {
        at: p(20.0, 20.0, 22.0),
        cycle: c,
        m: m(Motion::Plunge),
    }]);
    s.apply(
        &r,
        Sweep {
            radius: 2.5,
            lift: 1.0,
        },
        0.01,
    );
    assert!(!inside(&s.at(6.5), 20.0, 20.0));
    assert!(inside(&s.at(5.5), 20.0, 20.0));
}

#[test]
fn sweep_of_tools() {
    use fluut_model::tools::ToolLibrary;
    let lib = ToolLibrary::parse(
        r#"
format = "fluut.tools/1"
id = "l"
[[tool]]
id = "f"
shape = "flat"
diameter = 6
[[tool]]
id = "b"
shape = "ball"
diameter = 6
[[tool]]
id = "d"
shape = "drill"
diameter = 5
point_angle_deg = 90
"#,
    )
    .unwrap()
    .0;
    let lift = |id: &str| Sweep::of(lib.tool(id).unwrap()).lift;
    assert_eq!(lift("f"), 0.0);
    assert_eq!(lift("b"), 3.0);
    assert!((lift("d") - 2.5).abs() < 1e-12);
}

#[test]
fn rotary_sweeps_clear_strips() {
    let mut c = LayeredStock::cylindrical(25.0, 0.0, 100.0);
    let pos = |x: f64, r: f64, a: f64| AxisPos {
        a: Some(a),
        ..p(x, 0.0, r)
    };
    let r = ToolpathResult::new(vec![
        Pass::new(
            PassKind::Cut,
            vec![Segment::Rotary {
                a: pos(50.0, 20.0, 0.0),
                b: pos(50.0, 20.0, 360.0),
                radius: 20.0,
                m: m(Motion::Cut),
            }],
        ),
        Pass::new(
            PassKind::Link,
            vec![line(
                pos(50.0, 20.0, 360.0),
                pos(70.0, 22.0, 350.0),
                Motion::Rapid,
            )],
        ),
        Pass::new(
            PassKind::Cut,
            vec![Segment::Rotary {
                a: pos(70.0, 22.0, 350.0),
                b: pos(70.0, 22.0, 380.0),
                radius: 22.0,
                m: m(Motion::Cut),
            }],
        ),
    ])
    .unwrap();
    c.apply(&r, FLAT, 0.01);
    assert!(!inside(&c.at(21.0), 50.0, 1.0));
    assert!(inside(&c.at(19.0), 50.0, 1.0));
    // The second wraps through 0: cut at 355° and 10°, not at 30°.
    let at = c.at(23.0);
    assert!(!inside(&at, 70.0, 355f64.to_radians()));
    assert!(!inside(&at, 70.0, 10f64.to_radians()));
    assert!(inside(&at, 70.0, 30f64.to_radians()));
}

#[test]
fn wrap_splits_at_the_seam() {
    assert_eq!(wrap(0.0, 1.0), vec![(0.0, 1.0)]);
    assert_eq!(wrap(1.0, 0.0), vec![(0.0, 1.0)]);
    assert_eq!(wrap(-1.0, 7.0), vec![(0.0, TAU)]);
    let w = wrap(-0.5, 0.5);
    assert_eq!(w.len(), 2);
    assert!((w[0].0 - (TAU - 0.5)).abs() < 1e-12 && (w[1].1 - 0.5).abs() < 1e-12);
}
