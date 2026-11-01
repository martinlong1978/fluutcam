// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::cutter::synth::{Builder, cuboid};
use crate::geometry::SetupBody;
use fluut_geom::{Frame, Interval, Surface, UvBox, Vec3};
use fluut_topo::BrepBody;

fn iv(lo: f64, hi: f64) -> Interval {
    Interval { lo, hi }
}

fn frame(o: [f64; 3], z: [f64; 3], x: [f64; 3]) -> Frame {
    Frame::new(
        Point3::new(o[0], o[1], o[2]),
        Vec3::new(z[0], z[1], z[2]),
        Vec3::new(x[0], x[1], x[2]),
    )
    .unwrap()
}

fn setup(body: BrepBody) -> SetupGeometry {
    SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-1".into(),
            geom: BodyGeom::Brep(body),
        }],
    }
}

fn single(s: Surface, uv: UvBox) -> SetupGeometry {
    let mut b = Builder::new("f");
    b.patch(s, uv);
    setup(b.build())
}

fn sphere(r: f64) -> SetupGeometry {
    let s = Surface::sphere(frame([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), r).unwrap();
    single(s, UvBox::new(iv(-1.0, 1.0), iv(0.2, 1.4)))
}

fn tilted_plane(theta: f64) -> SetupGeometry {
    let (s, c) = libm::sincos(theta);
    let p = Surface::plane(frame([0.0; 3], [-s, 0.0, c], [c, 0.0, s]));
    single(p, UvBox::new(iv(-20.0, 20.0), iv(-20.0, 20.0)))
}

fn cube() -> SetupGeometry {
    let mut b = Builder::new("cube");
    cuboid(
        &mut b,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(10.0, 8.0, 5.0),
    );
    setup(b.build())
}

fn all() -> FaceSets {
    FaceSets::default()
}

#[test]
fn stock_to_leave_offsets_a_sphere() {
    // Ball R = 3 on a sphere ρ = 20: tip = sqrt((ρ + R + s)² − x²) − R.
    let g = sphere(20.0);
    for s in [0.0, 0.5, -1.0, -2.9] {
        let q = SurfaceQuery::new(&g, &all(), Cutter::ball(6.0).unwrap(), s).unwrap();
        for x in [10.0, 12.0, 14.0] {
            let h = q.drop(x, 0.0).unwrap();
            let want = ((23.0 + s) * (23.0 + s) - x * x).sqrt() - 3.0;
            assert!(
                (h.tip - want).abs() < 1e-7,
                "s {s} x {x}: {} vs {want}",
                h.tip
            );
            let slope = libm::asin(x / (23.0 + s));
            assert!((h.slope - slope).abs() < 1e-6, "{} vs {slope}", h.slope);
            assert!(h.drive);
        }
    }
}

#[test]
fn stock_to_leave_offsets_a_tilted_plane() {
    // Bull Ø6, r = 1 on a plane tilted by θ about Y:
    // tip = x tan θ + (R − r) tan θ + r (1/cos θ − 1) + s / cos θ.
    let th = 20f64.to_radians();
    let g = tilted_plane(th);
    for s in [0.0, 0.4, -0.5] {
        let q = SurfaceQuery::new(&g, &all(), Cutter::new(6.0, 1.0).unwrap(), s).unwrap();
        for x in [-4.0, 0.0, 5.0] {
            let h = q.drop(x, 1.0).unwrap();
            let (t, c) = (libm::tan(th), libm::cos(th));
            let want = x * t + 2.0 * t + 1.0 * (1.0 / c - 1.0) + s / c;
            assert!(
                (h.tip - want).abs() < 1e-7,
                "s {s} x {x}: {} vs {want}",
                h.tip
            );
            assert!((h.slope - th).abs() < 1e-6);
        }
    }
    // A flat tool touches with its rim and takes the face's slope.
    let q = SurfaceQuery::new(&g, &all(), Cutter::flat(6.0).unwrap(), 0.0).unwrap();
    let h = q.drop(0.0, 0.0).unwrap();
    assert!((h.tip - 3.0 * libm::tan(th)).abs() < 1e-7);
    assert!((h.slope - th).abs() < 1e-9, "{}", h.slope);
}

#[test]
fn slope_at_edges_and_flats() {
    let g = cube();
    let q = SurfaceQuery::new(&g, &all(), Cutter::ball(6.0).unwrap(), 0.0).unwrap();
    let h = q.drop(5.0, 4.0).unwrap();
    assert_eq!((h.tip, h.slope), (5.0, 0.0));
    // Half a radius past the top edge x = 10: the edge touches the ball
    // 1.5 from its axis.
    let h = q.drop(11.5, 4.0).unwrap();
    assert!((h.slope - libm::asin(0.5)).abs() < 1e-9, "{}", h.slope);
    // A flat tool on its flat bottom.
    let q = SurfaceQuery::new(&g, &all(), Cutter::flat(6.0).unwrap(), 0.0).unwrap();
    assert_eq!(q.drop(11.5, 4.0).unwrap().slope, 0.0);
}

#[test]
fn drive_check_and_excluded_faces() {
    let g = cube();
    // cuboid faces: 0 bottom, 1 top, 2..5 sides.
    let top = || BTreeSet::from(["f-1.1".to_string()]);
    let side = || BTreeSet::from(["f-1.5".to_string()]);
    let c = Cutter::ball(6.0).unwrap();
    let sets = FaceSets {
        drive: top(),
        ..Default::default()
    };
    let q = SurfaceQuery::new(&g, &sets, c, 0.0).unwrap();
    assert!(q.drop(5.0, 4.0).unwrap().drive);
    // The top face's edge counts as drive.
    assert!(q.drop(11.5, 4.0).unwrap().drive);
    // Only the side x = 10 drives: the top is a check face.
    let sets = FaceSets {
        drive: side(),
        ..Default::default()
    };
    let q = SurfaceQuery::new(&g, &sets, c, 0.0).unwrap();
    assert!(!q.drop(5.0, 4.0).unwrap().drive);
    // Excluded top face: the tool goes through to the bottom face.
    let sets = FaceSets {
        excluded: top(),
        ..Default::default()
    };
    let q = SurfaceQuery::new(&g, &sets, c, 0.0).unwrap();
    assert_eq!(q.drop(5.0, 4.0).unwrap().tip, 0.0);
    // Explicit check faces leave the other faces out of the query.
    let sets = FaceSets {
        drive: side(),
        check: Some(BTreeSet::new()),
        excluded: BTreeSet::new(),
    };
    let q = SurfaceQuery::new(&g, &sets, c, 0.0).unwrap();
    assert!(q.drop(5.0, 4.0).is_none());
}

#[test]
fn link_bound_clears_every_drop() {
    let g = sphere(20.0);
    for s in [0.0, 0.5] {
        let q = SurfaceQuery::new(&g, &all(), Cutter::ball(6.0).unwrap(), s).unwrap();
        let (a, b) = (Point2::new(4.0, -8.0), Point2::new(16.0, 6.0));
        let bound = q.link_bound(a, b);
        let mut top = f64::NEG_INFINITY;
        for k in 0..=200 {
            let p = a + (b - a) * (k as f64 / 200.0);
            if let Some(h) = q.drop(p.x(), p.y()) {
                top = top.max(h.tip);
            }
        }
        assert!(top.is_finite() && bound >= top, "{bound} < {top}");
        assert!(bound <= 20.0 + s + 1.0, "{bound}");
    }
    let q = SurfaceQuery::new(&g, &all(), Cutter::ball(6.0).unwrap(), 0.0).unwrap();
    let far = q.link_bound(Point2::new(100.0, 100.0), Point2::new(120.0, 100.0));
    assert_eq!(far, f64::NEG_INFINITY);
}

#[test]
fn drive_box_covers_the_drive_faces() {
    let g = cube();
    let sets = FaceSets {
        drive: BTreeSet::from(["f-1.1".to_string()]),
        ..Default::default()
    };
    let q = SurfaceQuery::new(&g, &sets, Cutter::ball(6.0).unwrap(), 0.0).unwrap();
    let (lo, hi) = q.drive_box().unwrap();
    assert!(lo.x() <= 0.0 && lo.y() <= 0.0 && hi.x() >= 10.0 && hi.y() >= 8.0);
    assert!(lo.x() > -0.1 && hi.x() < 10.1);
}

#[test]
fn rejects_a_cutter_shrunk_past_its_corner() {
    let g = cube();
    assert!(SurfaceQuery::new(&g, &all(), Cutter::new(6.0, 1.0).unwrap(), -1.5).is_none());
}
