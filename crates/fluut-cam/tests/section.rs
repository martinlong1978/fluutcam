// SPDX-License-Identifier: MIT OR Apache-2.0

//! Plane sections of solids (spec 04 §4.4) on the test parts.

mod support;

use fluut_cam::region::Inside;
use fluut_cam::section::Sectioner;
use fluut_geom::Point2;
use support::cam::load;
use support::step;

fn area_at(text: &str, z: f64) -> f64 {
    let g = load(text);
    let s = Sectioner::new(&g, 0.01).unwrap();
    s.section(z).unwrap().region.area()
}

#[test]
fn block_with_pocket_and_island() {
    let text = step::pocket_block([0.0; 3]);
    // Below the floor: the whole block; above: the walls and the island.
    assert!((area_at(&text, 5.0) - 4800.0).abs() < 1e-6);
    assert!((area_at(&text, 16.0) - 2500.0).abs() < 1e-6);
    let g = load(&text);
    let s = Sectioner::new(&g, 0.01).unwrap();
    let r = s.section(16.0).unwrap().region;
    assert_eq!(r.contains(Point2::new(40.0, 30.0)), Inside::In);
    assert_eq!(r.contains(Point2::new(20.0, 30.0)), Inside::Out);
    assert_eq!(r.contains(Point2::new(5.0, 30.0)), Inside::In);
    // Above the part: nothing.
    assert!(s.section(25.0).unwrap().region.is_empty());
}

#[test]
fn rounded_boss_and_holes() {
    // 40 × 30 with 5 mm corners.
    let want = 1200.0 - (4.0 - std::f64::consts::PI) * 25.0;
    assert!((area_at(&step::boss([0.0; 3]), 5.0) - want).abs() < 1e-6);
    // 80 × 50 block, two Ø5 through holes, a Ø10 counterbore and a Ø22
    // blind hole at Z 13.
    let r = |d: f64| std::f64::consts::PI * d * d / 4.0;
    let want = 4000.0 - 2.0 * r(5.0) - r(10.0) - r(22.0);
    let got = area_at(&step::holes([0.0; 3]), 13.0);
    assert!((got - want).abs() < 1e-3, "{got} {want}");
}

#[test]
fn curved_top() {
    // The ridge: a 60 × 40 block whose top is a convex cylinder along X.
    // At Z 12 the cylinder of radius 40 is cut where |y − 20| ≤ w.
    let zc = step::ridge_axis_z();
    let w = (40.0f64 * 40.0 - (12.0 - zc) * (12.0 - zc)).sqrt();
    let got = area_at(&step::ridge([0.0; 3]), 12.0);
    assert!((got - 60.0 * 2.0 * w).abs() < 1e-6, "{got} {}", 120.0 * w);
}

#[test]
fn sections_move_with_the_part() {
    let a = area_at(&step::pocket_block([0.0; 3]), 16.0);
    let b = area_at(&step::pocket_block([0.123, -4.5, 0.0]), 16.0);
    assert!((a - b).abs() < 1e-6);
}
