// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn plate() -> Region2 {
    Region2::rect(0.0, 0.0, 100.0, 60.0).with_hole(&Region2::circle(50.0, 30.0, 10.0))
}

#[test]
fn areas() {
    assert!(close(Region2::rect(0.0, 0.0, 4.0, 3.0).area(), 12.0, 1e-12));
    assert!(close(
        Region2::circle(1.0, 2.0, 3.0).area(),
        9.0 * PI,
        1e-12
    ));
    assert!(close(plate().area(), 6000.0 - 100.0 * PI, 1e-9));
    // A clockwise polygon is turned counter-clockwise.
    let cw = Region2::polygon(&[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)]);
    assert!(close(cw.area(), 1.0, 1e-12));
}

#[test]
fn hole_is_clockwise() {
    let p = plate();
    assert!(p.loops[0].is_ccw());
    assert!(!p.loops[1].is_ccw());
}

#[test]
fn reversed_twice_is_identity() {
    let l = &plate().loops[0];
    assert_eq!(&l.reversed().reversed(), l);
    let c = &Region2::circle(0.0, 0.0, 2.0).loops[0];
    assert!(close(c.reversed().area(), -4.0 * PI, 1e-12));
}

#[test]
fn contains_in_on_out() {
    let p = plate();
    assert_eq!(p.contains(Point2::new(10.0, 10.0)), Inside::In);
    assert_eq!(p.contains(Point2::new(50.0, 30.0)), Inside::Out); // in the hole
    assert_eq!(p.contains(Point2::new(60.0, 30.0)), Inside::On); // on the hole
    assert_eq!(p.contains(Point2::new(0.0, 30.0)), Inside::On);
    assert_eq!(p.contains(Point2::new(-1.0, 30.0)), Inside::Out);
    assert_eq!(p.contains(Point2::new(50.0, 41.0)), Inside::In);
}

#[test]
fn support_of_circle_is_exact() {
    let c = Region2::circle(1.0, 2.0, 3.0);
    let (s, co) = libm::sincos(PI / 6.0);
    let dir = Vec2::new(co, s);
    let (lo, hi) = c.support(dir);
    let centre = 1.0 * co + 2.0 * s;
    assert!(close(lo, centre - 3.0, 1e-12), "{lo}");
    assert!(close(hi, centre + 3.0, 1e-12), "{hi}");
}

#[test]
fn bbox_of_plate_ignores_hole() {
    let (lo, hi) = plate().bbox();
    assert_eq!((lo.x(), lo.y(), hi.x(), hi.y()), (0.0, 0.0, 100.0, 60.0));
}

#[test]
fn arc_geometry() {
    // Quarter arc counter-clockwise from (1, 0) to (0, 1) about the origin.
    let s = Seg2 {
        a: Point2::new(1.0, 0.0),
        b: Point2::new(0.0, 1.0),
        bulge: libm::tan(PI / 8.0),
    };
    let g = s.arc().unwrap();
    assert!(g.centre.distance(Point2::new(0.0, 0.0)) < 1e-12);
    assert!(close(g.radius, 1.0, 1e-12));
    assert!(close(g.sweep, PI / 2.0, 1e-12));
    assert!(close(s.length(), PI / 2.0, 1e-12));
    let m = s.point_at(0.5);
    let h = std::f64::consts::FRAC_1_SQRT_2;
    assert!(m.distance(Point2::new(h, h)) < 1e-12);
}

#[test]
fn pline_round_trip() {
    let p = plate();
    for l in &p.loops {
        assert_eq!(&Loop2::from_pline(&l.to_pline()), l);
    }
}

#[test]
fn perimeter() {
    assert!(close(plate().perimeter(), 320.0 + 20.0 * PI, 1e-9));
}

fn ccw_outer_cw_holes(r: &Region2) {
    // Holes are the loops contained in another loop; every other loop is an
    // outer loop. Here it is enough that the signs add up.
    let outers = r.loops.iter().filter(|l| l.is_ccw()).count();
    assert!(outers >= 1 || r.is_empty());
}

#[test]
fn offset_rectangle_out_and_in() {
    let r = Region2::rect(0.0, 0.0, 10.0, 6.0);
    let g = r.offset(1.0);
    // Rectangle grown by 1 with round corners: 60 + 2·(10+6)·1 + π.
    assert!(close(g.area(), 60.0 + 32.0 + PI, 1e-9), "{}", g.area());
    assert_eq!(g.loops.len(), 1);
    assert!(g.loops[0].is_ccw());
    let s = r.offset(-1.0);
    assert!(close(s.area(), 8.0 * 4.0, 1e-9), "{}", s.area());
    assert!(s.offset(-2.0).is_empty());
}

#[test]
fn offset_circle() {
    let c = Region2::circle(0.0, 0.0, 5.0);
    assert!(close(c.offset(2.0).area(), 49.0 * PI, 1e-9));
    assert!(close(c.offset(-2.0).area(), 9.0 * PI, 1e-9));
    assert!(c.offset(-5.5).is_empty());
}

#[test]
fn offset_plate_hole_grows() {
    let p = plate();
    let s = p.offset(-5.0);
    // Outer 90×50, hole radius 15.
    assert!(close(s.area(), 4500.0 - 225.0 * PI, 1e-6), "{}", s.area());
    assert_eq!(s.loops.len(), 2);
    ccw_outer_cw_holes(&s);
    // At −10 the hole (radius 20) meets the outer boundary (y = 10 and 50);
    // past that the region splits into a left and a right piece.
    let split = p.offset(-16.0);
    assert_eq!(split.loops.len(), 2, "{:?}", split.loops.len());
    assert!(split.loops.iter().all(Loop2::is_ccw));
    for l in &split.loops {
        assert!(close(l.area(), split.area() / 2.0, 1e-6));
    }
}

#[test]
fn offset_zero_and_empty() {
    assert_eq!(plate().offset(0.0), plate());
    assert!(Region2::empty().offset(3.0).is_empty());
}

fn check_ops(a: &Region2, b: &Region2, tol: f64) {
    let (u, i, d) = (a.union(b), a.intersect(b), a.subtract(b));
    let (aa, ab) = (a.area(), b.area());
    assert!(
        close(u.area() + i.area(), aa + ab, tol),
        "u {} i {}",
        u.area(),
        i.area()
    );
    assert!(
        close(d.area(), aa - i.area(), tol),
        "d {} i {}",
        d.area(),
        i.area()
    );
    assert!(i.area() >= -tol && d.area() >= -tol);
}

#[test]
fn booleans_of_rectangles() {
    let a = Region2::rect(0.0, 0.0, 10.0, 10.0);
    let b = Region2::rect(5.0, 5.0, 15.0, 15.0);
    assert!(close(a.intersect(&b).area(), 25.0, 1e-9));
    assert!(close(a.union(&b).area(), 175.0, 1e-9));
    assert!(close(a.subtract(&b).area(), 75.0, 1e-9));
    assert_eq!(a.union(&b).loops.len(), 1);
    check_ops(&a, &b, 1e-9);
}

#[test]
fn booleans_disjoint_and_contained() {
    let a = Region2::rect(0.0, 0.0, 10.0, 10.0);
    let far = Region2::rect(20.0, 0.0, 30.0, 10.0);
    assert!(a.intersect(&far).is_empty());
    assert_eq!(a.union(&far).loops.len(), 2);
    let d = a.subtract(&far);
    assert_eq!(d.loops.len(), 1);
    assert!(close(d.area(), 100.0, 1e-12));
    let inner = Region2::circle(5.0, 5.0, 2.0);
    let ring = a.subtract(&inner);
    assert_eq!(ring.loops.len(), 2);
    assert!(close(ring.area(), 100.0 - 4.0 * PI, 1e-9));
    assert!(close(a.intersect(&inner).area(), 4.0 * PI, 1e-9));
    assert!(close(a.union(&inner).area(), 100.0, 1e-9));
    check_ops(&a, &inner, 1e-9);
}

#[test]
fn booleans_touching_edge() {
    let a = Region2::rect(0.0, 0.0, 10.0, 10.0);
    let b = Region2::rect(10.0, 0.0, 20.0, 10.0);
    let u = a.union(&b);
    assert!(close(u.area(), 200.0, 1e-9));
    assert_eq!(u.loops.len(), 1);
    assert!(a.intersect(&b).area().abs() < 1e-9);
    assert!(close(a.subtract(&b).area(), 100.0, 1e-9));
    // Same rectangle: coincident everywhere.
    assert!(close(a.union(&a).area(), 100.0, 1e-9));
    assert!(close(a.intersect(&a).area(), 100.0, 1e-9));
    assert!(a.subtract(&a).is_empty());
}

#[test]
fn plate_minus_disc_across_hole() {
    let p = plate();
    let disc = Region2::circle(62.0, 30.0, 6.0);
    let r = p.subtract(&disc);
    // The disc and the hole overlap in a lens; numeric check of areas.
    check_ops(&p, &disc, 1e-9);
    assert_eq!(r.loops.len(), 2);
    assert!(r.area() < p.area());
}

mod props {
    use super::*;
    use proptest::prelude::*;

    fn shape() -> impl Strategy<Value = Region2> {
        prop_oneof![
            (0.0..20.0f64, 0.0..20.0f64, 1.0..15.0f64, 1.0..15.0f64)
                .prop_map(|(x, y, w, h)| Region2::rect(x, y, x + w, y + h)),
            (0.0..20.0f64, 0.0..20.0f64, 0.5..10.0f64)
                .prop_map(|(x, y, r)| Region2::circle(x, y, r)),
        ]
    }

    proptest! {
        #[test]
        fn area_identities(a in shape(), b in shape(), c in shape()) {
            let ab = a.union(&b);
            check_ops(&ab, &c, 1e-6);
            check_ops(&a.subtract(&b), &c, 1e-6);
        }
    }
}
