// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use fluut_geom::{Point3, UnitVec3};
use fluut_path::{AxisPos, MotionInfo};

fn p(x: f64, y: f64, z: f64) -> AxisPos {
    AxisPos::xyz(Point3::new(x, y, z))
}

fn cut(a: AxisPos, b: AxisPos) -> Segment {
    Segment::Line {
        a,
        b,
        m: MotionInfo::new(Motion::Cut, FeedRef::Cut, 0.01),
    }
}

fn rapid(a: AxisPos, b: AxisPos) -> Segment {
    Segment::Line {
        a,
        b,
        m: MotionInfo::new(Motion::Rapid, FeedRef::Rapid, 0.01),
    }
}

/// 600 mm/min = 10 mm/s, 100 mm/s².
fn machine() -> Kinematics {
    Kinematics::uniform(6000.0, 6000.0, Some(100.0))
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} vs {b}");
}

#[test]
fn straight_line_without_and_with_acceleration() {
    let segs = [cut(p(0.0, 0.0, 0.0), p(100.0, 0.0, 0.0))];
    let f = Feeds::all(600.0);
    // 100 mm at 10 mm/s.
    close(
        estimate(&segs, &f, &Kinematics::uniform(6000.0, 6000.0, None)),
        10.0,
    );
    // 0.1 s up to speed over 0.5 mm, the same down, 99 mm cruising.
    close(estimate(&segs, &f, &machine()), 10.1);
}

#[test]
fn split_straight_line_runs_through_the_junction() {
    let segs = [
        cut(p(0.0, 0.0, 0.0), p(50.0, 0.0, 0.0)),
        cut(p(50.0, 0.0, 0.0), p(100.0, 0.0, 0.0)),
    ];
    let t = segment_times(&segs, &Feeds::all(600.0), &machine());
    close(t[0], 0.1 + 49.5 / 10.0);
    close(t[0] + t[1], 10.1);
}

#[test]
fn right_angle_corner_by_hand() {
    let segs = [
        cut(p(0.0, 0.0, 0.0), p(50.0, 0.0, 0.0)),
        cut(p(50.0, 0.0, 0.0), p(50.0, 50.0, 0.0)),
    ];
    let t = estimate(&segs, &Feeds::all(600.0), &machine());
    // θ = 90°: v² = 100·0.01·sin 45° / (1 − sin 45°) = 2.41421…
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let vj = (100.0 * 0.01 * s / (1.0 - s)).sqrt();
    // Each line: 0.1 s from rest to 10 mm/s over 0.5 mm, (10 − vj)/100 s
    // down to vj over (100 − vj²)/200 mm, the rest at 10 mm/s.
    let d2 = (100.0 - vj * vj) / 200.0;
    let line = 0.1 + (10.0 - vj) / 100.0 + (50.0 - 0.5 - d2) / 10.0;
    close(t, 2.0 * line);
    // A reversal stops the machine: two separate 50 mm moves.
    let back = [
        cut(p(0.0, 0.0, 0.0), p(50.0, 0.0, 0.0)),
        cut(p(50.0, 0.0, 0.0), p(0.0, 0.0, 0.0)),
    ];
    close(
        estimate(&back, &Feeds::all(600.0), &machine()),
        2.0 * (0.2 + 49.0 / 10.0),
    );
}

#[test]
fn short_move_is_triangular() {
    // 0.4 mm never reaches 10 mm/s: peak √(100·0.4) = 6.32 mm/s.
    let segs = [cut(p(0.0, 0.0, 0.0), p(0.4, 0.0, 0.0))];
    let vp = (100.0_f64 * 0.4).sqrt();
    close(
        estimate(&segs, &Feeds::all(600.0), &machine()),
        2.0 * vp / 100.0,
    );
}

#[test]
fn rapids_are_axis_limited() {
    // Each axis rapids at 6000 mm/min, so a diagonal in XY runs √2 faster.
    let segs = [rapid(p(0.0, 0.0, 0.0), p(100.0, 100.0, 0.0))];
    let k = Kinematics::uniform(3000.0, 6000.0, None);
    close(estimate(&segs, &Feeds::all(600.0), &k), 1.0);
    // The cutting feed is capped by the machine: 3000 mm/min along X.
    let segs = [cut(p(0.0, 0.0, 0.0), p(100.0, 0.0, 0.0))];
    close(estimate(&segs, &Feeds::all(9000.0), &k), 2.0);
}

#[test]
fn arc_is_limited_by_centripetal_acceleration() {
    let a = p(1.0, 0.0, 0.0);
    let segs = [Segment::Arc {
        a,
        b: a,
        centre: Point3::ORIGIN,
        normal: UnitVec3::Z,
        sweep: std::f64::consts::TAU,
        m: MotionInfo::new(Motion::Cut, FeedRef::Cut, 0.01),
    }];
    // r = 1, a = 100: at most 10 mm/s anyway; at a = 25, 5 mm/s.
    let k = Kinematics::uniform(6000.0, 6000.0, Some(25.0));
    let len = std::f64::consts::TAU;
    let t = estimate(&segs, &Feeds::all(600.0), &k);
    close(t, 5.0 / 25.0 * 2.0 + (len - 2.0 * 25.0 / 50.0) / 5.0);
    // The tangent at the start of a counter-clockwise arc is +Y.
    let d = arc_tangent(&segs[0], 0.0).unwrap();
    assert!((d - DVec3::Y).length() < 1e-12);
}

#[test]
fn dwell_stop_and_factor() {
    let segs = [
        cut(p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0)),
        Segment::Marker {
            kind: MarkerKind::Comment("x".into()),
        },
        Segment::Marker {
            kind: MarkerKind::Dwell(2.5),
        },
        cut(p(10.0, 0.0, 0.0), p(20.0, 0.0, 0.0)),
    ];
    let mut k = Kinematics::uniform(6000.0, 6000.0, None);
    k.factor = 2.0;
    let t = segment_times(&segs, &Feeds::all(600.0), &k);
    assert_eq!(t.len(), 4);
    close(t[1], 0.0);
    close(t[2], 5.0);
    close(t.iter().sum(), 2.0 * (1.0 + 2.5 + 1.0));
    // With acceleration the dwell stops the machine between the lines.
    let k = machine();
    close(
        estimate(&segs, &Feeds::all(600.0), &k),
        2.0 * (0.2 + 9.0 / 10.0) + 2.5,
    );
}
