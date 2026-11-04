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
fn offset_to_exactly_half_the_width() {
    // A 50 × 30 slot with corner radius 5 offset inward by 15 collapses to
    // its centre line. The offset used to hit a debug assertion in
    // `cavalier_contours` (a zero-length segment); now it must just be
    // empty or a sliver, at every nearby distance.
    let k = core::f64::consts::SQRT_2 - 1.0;
    let slot = Region2 {
        loops: vec![Loop2 {
            verts: vec![
                Vertex::new(20.0, 15.0, 0.0),
                Vertex::new(60.0, 15.0, k),
                Vertex::new(65.0, 20.0, 0.0),
                Vertex::new(65.0, 40.0, k),
                Vertex::new(60.0, 45.0, 0.0),
                Vertex::new(20.0, 45.0, k),
                Vertex::new(15.0, 40.0, 0.0),
                Vertex::new(15.0, 20.0, k),
            ],
        }],
    };
    assert!(close(slot.area(), 1500.0 - (4.0 - PI) * 25.0, 1e-9));
    for d in [-14.999_999, -15.0, -15.000_001, -(3.0 + 2.4 * 5.0)] {
        let s = slot.offset(d);
        assert!(s.area() < 1e-3, "d={d} area={}", s.area());
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
    for r in [&u, &i, &d] {
        assert_simple(r);
    }
}

#[test]
fn booleans_of_rectangles() {
    let a = Region2::rect(0.0, 0.0, 10.0, 10.0);
    let b = Region2::rect(5.0, 5.0, 15.0, 15.0);
    assert!(close(a.intersect(&b).area(), 25.0, 1e-9));
    assert!(close(a.union(&b).area(), 175.0, 1e-9));
    assert!(close(a.subtract(&b).area(), 75.0, 1e-9));
    assert_eq!(a.union(&b).loops.len(), 1);
    check_ops(&a, &b, 1e-6);
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

/// No two vertices of a loop at the same point, so a loop can neither touch
/// nor cross itself at a vertex.
fn assert_simple(r: &Region2) {
    for l in &r.loops {
        for (i, u) in l.verts.iter().enumerate() {
            for v in &l.verts[i + 1..] {
                assert!(
                    u.p.distance(v.p) > 1e-7,
                    "loop passes {:?} twice: {:?}",
                    u.p,
                    l.verts
                );
            }
        }
    }
}

/// Areas of the loops, sorted.
fn loop_areas(r: &Region2) -> Vec<f64> {
    let mut a: Vec<f64> = r.loops.iter().map(Loop2::area).collect();
    a.sort_by(f64::total_cmp);
    a
}

#[test]
fn booleans_squares_sharing_a_corner() {
    let a = Region2::rect(0.0, 0.0, 1.0, 1.0);
    let b = Region2::rect(1.0, 1.0, 2.0, 2.0);
    for u in [a.union(&b), b.union(&a)] {
        assert_simple(&u);
        assert_eq!(loop_areas(&u).len(), 2, "{u:?}");
        for x in loop_areas(&u) {
            assert!(close(x, 1.0, 1e-12), "{u:?}");
        }
    }
    assert!(a.intersect(&b).area().abs() < 1e-12);
    assert!(close(a.subtract(&b).area(), 1.0, 1e-12));
    // The big square minus the diagonal pair leaves the other diagonal pair,
    // which also touch at the centre.
    let big = Region2::rect(0.0, 0.0, 2.0, 2.0);
    let d = big.subtract(&a.union(&b));
    assert_simple(&d);
    assert_eq!(loop_areas(&d).len(), 2, "{d:?}");
    assert!(close(d.area(), 2.0, 1e-12));
    let i = big.intersect(&a.union(&b));
    assert_simple(&i);
    assert_eq!(i.loops.len(), 2);
}

#[test]
fn square_minus_tangent_disc() {
    let sq = Region2::rect(0.0, 0.0, 4.0, 4.0);
    // Touches the bottom edge at (2, 0) from inside.
    let inside = Region2::circle(2.0, 1.0, 1.0);
    let d = sq.subtract(&inside);
    assert_simple(&d);
    let areas = loop_areas(&d);
    assert_eq!(areas.len(), 2, "{d:?}");
    assert!(
        close(areas[0], -PI, 1e-9) && close(areas[1], 16.0, 1e-12),
        "{areas:?}"
    );
    assert!(close(sq.union(&inside).area(), 16.0, 1e-9));
    assert!(close(sq.intersect(&inside).area(), PI, 1e-9));
    // Touches the bottom edge at (2, 0) from outside.
    let outside = Region2::circle(2.0, -1.0, 1.0);
    for u in [sq.union(&outside), outside.union(&sq)] {
        assert_simple(&u);
        let areas = loop_areas(&u);
        assert_eq!(areas.len(), 2, "{u:?}");
        assert!(
            close(areas[0], PI, 1e-9) && close(areas[1], 16.0, 1e-12),
            "{areas:?}"
        );
    }
    let d = sq.subtract(&outside);
    assert_simple(&d);
    assert!(close(d.area(), 16.0, 1e-12));
}

#[test]
fn disc_tangent_to_hole() {
    // A disc subtracted from a ring touches the ring's hole at one point:
    // the result has one outer loop and two holes that touch there.
    let ring = Region2::rect(0.0, 0.0, 10.0, 10.0).with_hole(&Region2::circle(5.0, 5.0, 2.0));
    let disc = Region2::circle(8.0, 5.0, 1.0);
    let d = ring.subtract(&disc);
    assert_simple(&d);
    assert!(close(d.area(), 100.0 - 5.0 * PI, 1e-9));
    assert_eq!(d.loops.len(), 3, "{d:?}");
    assert_eq!(d.loops.iter().filter(|l| l.is_ccw()).count(), 1);
    check_ops(&ring, &disc, 1e-9);
}

#[test]
fn union_of_nearly_collinear_stadiums() {
    // Two consecutive steps of an optimised-load pass, swept by R = 3: their
    // straight sides are almost collinear and overlap.
    let a = Region2 {
        loops: vec![Loop2 {
            verts: vec![
                Vertex::new(85.79290117169626, 51.50368067174173, 0.0),
                Vertex::new(86.54289987192651, 51.50507697147379, 1.0),
                Vertex::new(86.53172947407005, 57.50506657331575, 0.0),
                Vertex::new(85.7817307738398, 57.503670273583694, 1.0),
            ],
        }],
    };
    let b = Region2 {
        loops: vec![Loop2 {
            verts: vec![
                Vertex::new(86.54222287068346, 51.505075787464875, 0.0),
                Vertex::new(87.29222186691594, 51.50630283688617, 1.0),
                Vertex::new(87.28240547154557, 57.50629480674596, 0.0),
                Vertex::new(86.5324064753131, 57.50506775732467, 1.0),
            ],
        }],
    };
    let u = a.union(&b);
    // Scanline integration of the two inputs gives 37.2743.
    assert!(close(u.area(), 37.2743, 1e-3), "{u:?}");
    assert_eq!(u.loops.len(), 1);
    check_ops(&a, &b, 1e-6);
    // Here a corner of `b` is 4e-4 along the edge from the crossing, close
    // enough for the piece between them to pass near `a`'s edge at its
    // middle but not at its end.
    let a = Region2 {
        loops: vec![Loop2 {
            verts: vec![
                Vertex::new(37.25114451005001, 45.379930205364545, 0.0),
                Vertex::new(36.67505541474776, 44.89969968335055, 1.0),
                Vertex::new(40.51689959085974, 40.29098692093255, 0.0),
                Vertex::new(41.09298868616199, 40.77121744294655, 1.0),
            ],
        }],
    };
    let b = Region2 {
        loops: vec![Loop2 {
            verts: vec![
                Vertex::new(36.67569122130502, 44.900229545991294, 0.0),
                Vertex::new(36.099469660342585, 44.42015797561661, 1.0),
                Vertex::new(39.940042223340036, 39.810385487917124, 0.0),
                Vertex::new(40.516263784302474, 40.29045705829181, 1.0),
            ],
        }],
    };
    // The two caps of radius 3 overlap almost fully: the lens is 9π.
    assert!(close(a.intersect(&b).area(), 9.0 * PI, 1e-3));
    check_ops(&a, &b, 1e-6);
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

    /// Unions of rectangles and discs on a coarse grid, so that edges
    /// coincide and corners and tangent points touch often.
    fn grid_shape() -> impl Strategy<Value = Region2> {
        let rect = (0..5i32, 0..5i32, 1..3i32, 1..3i32).prop_map(|(x, y, w, h)| {
            let (x, y) = (f64::from(x), f64::from(y));
            Region2::rect(x, y, x + f64::from(w), y + f64::from(h))
        });
        let disc =
            (1..5i32, 1..5i32).prop_map(|(x, y)| Region2::circle(f64::from(x), f64::from(y), 1.0));
        prop::collection::vec(prop_oneof![rect, disc], 1..4)
            .prop_map(|v| v.iter().fold(Region2::empty(), |acc, r| acc.union(r)))
    }

    proptest! {
        #[test]
        fn area_identities(a in shape(), b in shape(), c in shape()) {
            let ab = a.union(&b);
            check_ops(&ab, &c, 1e-6);
            check_ops(&a.subtract(&b), &c, 1e-6);
        }

        #[test]
        fn grid_area_identities(a in grid_shape(), b in grid_shape()) {
            check_ops(&a, &b, 1e-6);
        }
    }
}

#[test]
fn clip_line_through_plate() {
    let iv = plate().clip_line(Point2::new(-5.0, 30.0), Vec2::new(1.0, 0.0));
    assert_eq!(iv.len(), 2, "{iv:?}");
    let want = [(5.0, 45.0), (65.0, 105.0)];
    for (got, want) in iv.iter().zip(want) {
        assert!(
            close(got.0, want.0, 1e-12) && close(got.1, want.1, 1e-12),
            "{iv:?}"
        );
    }
    // Through the hole off-centre: chord ends at x = 50 ± √(100 − 36).
    let iv = plate().clip_line(Point2::new(0.0, 36.0), Vec2::new(2.0, 0.0));
    assert!(close(iv[0].1, (50.0 - 8.0) / 2.0, 1e-12), "{iv:?}");
    assert!(close(iv[1].0, (50.0 + 8.0) / 2.0, 1e-12), "{iv:?}");
}

#[test]
fn clip_line_tangent_and_edge() {
    let c = Region2::circle(0.0, 0.0, 5.0);
    assert!(
        c.clip_line(Point2::new(-10.0, 5.0), Vec2::new(1.0, 0.0))
            .is_empty()
    );
    let r = Region2::rect(0.0, 0.0, 10.0, 6.0);
    let iv = r.clip_line(Point2::new(-1.0, 0.0), Vec2::new(1.0, 0.0));
    assert_eq!(iv, vec![(1.0, 11.0)]);
    let iv = r.clip_line(Point2::new(0.0, 0.0), Vec2::new(1.0, 1.0));
    assert_eq!(iv, vec![(0.0, 6.0)]);
}

#[test]
fn steep_lines_meeting_end_to_end_do_not_overlap() {
    // Two almost collinear, almost vertical segments that only share an
    // end. `line_line_intr` divided by the 3e-5 X extent of the second one
    // (more than `eps`) and reported an overlap 0.11 mm along it.
    use cavalier_contours::core::math::{LineLineIntr, Vector2, line_line_intr};
    let a = Vector2::new(-5.25394371263927, 42.21079446367146);
    let b = Vector2::new(-5.253939252133978, 42.10113256533973);
    let c = Vector2::new(-5.253970535472758, 41.35137972498433);
    match line_line_intr(a, b, b, c, 1e-5_f64) {
        LineLineIntr::TrueIntersect { seg2_t, .. } => assert!(seg2_t.abs() < 1e-12, "{seg2_t}"),
        r => panic!("{r:?}"),
    }
}

#[test]
fn offset_of_a_sliver_triangle() {
    // A triangle 0.86 mm long and 3e-5 mm wide, from a section of the dome
    // (`optimised_load_3d_on_the_dome` in `tests/rough3d.rs`). Its raw
    // offset has the two segments of the test above, and the false overlap
    // made an offset slice start off its segment, a debug assertion in
    // `cavalier_contours`.
    let hole = Loop2 {
        verts: vec![
            Vertex::new(-2.253944715120961, 42.21091648883799, 0.0),
            Vertex::new(-2.2539402546156686, 42.10125459050626, 0.0),
            Vertex::new(-2.2539715380825056, 41.35125459115869, 0.0),
        ],
    };
    assert!(!hole.is_ccw());
    let len = (hole.verts[0].p - hole.verts[2].p).length();
    for d in [3.0, 2.999_999] {
        // Clockwise, it is a hole that a growth fills.
        let r = Region2 {
            loops: vec![hole.clone()],
        }
        .offset_raw(d);
        assert!(r.is_empty(), "{r:?}");
        // Counter-clockwise, it grows to a stadium.
        let r = Region2 {
            loops: vec![hole.reversed()],
        }
        .offset_raw(d);
        assert!(
            close(r.area(), 2.0 * d * len + PI * d * d, 1e-3),
            "{}",
            r.area()
        );
    }
}

#[test]
fn forced_failures_are_typed() {
    let square = Region2::rect(0.0, 0.0, 10.0, 10.0);
    fault::set(true);
    let offset = square.try_offset(1.0);
    let boolean = square.try_union(&Region2::rect(5.0, 5.0, 15.0, 15.0));
    let raised = std::panic::catch_unwind(|| square.offset(-1.0));
    let open = std::panic::catch_unwind(|| {
        let line = crate::path2::Path2::open(vec![Seg2 {
            a: Point2::new(0.0, 0.0),
            b: Point2::new(5.0, 0.0),
            bulge: 0.0,
        }]);
        crate::strategy::contour_2d::offset_open(&line, 1.0)
    });
    fault::set(false);
    let forced = |op| RegionFailure {
        op,
        message: "forced region failure".to_owned(),
    };
    assert_eq!(offset, Err(forced(RegionOp::Offset)));
    assert_eq!(boolean, Err(forced(RegionOp::Boolean)));
    for r in [raised.map(|_| ()), open.map(|_| ())] {
        let e = r.unwrap_err();
        assert_eq!(
            e.downcast_ref::<RegionFailure>(),
            Some(&forced(RegionOp::Offset))
        );
    }
    // Off again.
    assert!(close(
        square.try_offset(1.0).unwrap().area(),
        140.0 + PI,
        1e-9
    ));
}

#[test]
fn offsets_at_near_tangent_contacts() {
    // Found by the `region` fuzz target. A circle 1.5 µm past the edge of
    // a strip, and one 1 µm short of a rectangle's edge, leave micrometre
    // bumps where the offset slices meet almost tangentially. The offset
    // of the closed loop came back open, which the shape offset took as a
    // loop (a debug assertion in `cavalier_contours`).
    let a = Region2::rect(13.75, 13.75, 13.750001, 21.250002)
        .union(&Region2::circle(15.75, 15.750003, 2.0000015));
    let r = a.try_offset(-1.46875).unwrap();
    assert!(
        close(r.area(), PI * 0.5312515f64.powi(2), 1e-5),
        "{}",
        r.area()
    );
    let b = Region2::rect(10.500001, 4.0, 26.250004, 19.750003)
        .subtract(&Region2::circle(15.75, 10.500003, 5.2500005))
        .offset(3.1875);
    // The same area as a release build without the fix.
    let r = b.try_offset(-4.0).unwrap();
    assert!(close(r.area(), 94.32710442594538, 1e-9), "{}", r.area());
}

#[test]
fn offset_of_loops_touching_at_a_vertex() {
    // Found by the `region` fuzz target. A triangle with a corner on a
    // strip's corner, grown by 3.97: the shape offset closed its result
    // with a second vertex on top of the first, and the zero-length arc
    // made a later boolean fail a debug assertion in `cavalier_contours`.
    let a = Region2::polygon(&[
        (15.500002, 2.500001),
        (15.750003, 15.750003),
        (10.500001, 10.500001),
    ])
    .union(&Region2::rect(15.750003, 0.0, 16.250003, 15.750003));
    assert_eq!(a.loops.len(), 2);
    let g = a.try_offset(3.96875).unwrap();
    for l in &g.loops {
        let n = l.verts.len();
        for i in 0..n {
            let d = (l.verts[(i + 1) % n].p - l.verts[i].p).length();
            assert!(d > 1e-6, "{l:?}");
        }
    }
    let b = Region2::rect(0.0, 12.250003, 15.750003, 27.500003);
    let u = g.try_union(&b).unwrap();
    assert!(u.area() > g.area().max(b.area()), "{}", u.area());
}

/// The ring of fuzz class 3 (docs/plan/2026-11-03-region-micro-loops.md):
/// a disc minus a disc that a third disc overlaps by 2e-6 mm, which leaves
/// a sliver bump 2e-6 mm high on the hole.
fn ring_with_a_sliver_bump() -> Region2 {
    Region2::circle(5.250001, 5.250001, 7.500001)
        .subtract(&Region2::circle(1e-6, 5.250001, 2.6250015))
        .union(&Region2::circle(5.250001, 5.250001, 2.6250005))
}

fn vertex_count(r: &Region2) -> usize {
    r.loops.iter().map(|l| l.verts.len()).sum()
}

#[test]
fn chained_offsets_of_a_ring_with_a_sliver_bump() {
    // Found by the `region` fuzz target (class 3). The fifth offset failed
    // a debug assertion in `parametric_from_point` (from
    // `line_circle_intr`); release builds gave a region larger than before
    // the shrink.
    let mut r = ring_with_a_sliver_bump();
    for d in [-1.34375, -1.34375, 3.5625, -1.375] {
        r = r.try_offset(d).unwrap();
    }
    let before = r.area();
    let r = r.try_offset(-1.34375).unwrap();
    assert!(r.area() < before, "{} after {before}", r.area());
    assert!(r.area() > 0.0);
}

#[test]
fn vertex_counts_stay_bounded_under_chained_offsets() {
    // The same ring, offset in and out again and again. Without the clean-up
    // the loops multiplied: 27 vertices became 110, 1 168 and then
    // 20 million in three more offsets, with the area out of all bounds.
    let mut r = ring_with_a_sliver_bump();
    let start = r.area();
    for _ in 0..6 {
        for d in [-1.34375, -1.34375, 3.5625, -1.375] {
            let before = r.area();
            r = r.try_offset(d).unwrap();
            // A growth holds the region, a shrink lies inside it.
            assert!((r.area() - before) * d >= 0.0, "{before} to {}", r.area());
            assert!(r.loops.len() <= 1, "{} loops", r.loops.len());
            assert!(vertex_count(&r) <= 20, "{} vertices", vertex_count(&r));
            assert!(r.area() >= 0.0 && r.area() < 2.0 * start, "{}", r.area());
        }
    }
}

#[test]
fn tidied_removes_only_tiny_features() {
    let l = |pts: &[(f64, f64)]| Loop2 {
        verts: pts.iter().map(|&(x, y)| Vertex::new(x, y, 0.0)).collect(),
    };
    // A spike 5 mm long and 2e-6 mm wide, a vertex 3e-6 mm from the next
    // and a notch 4e-6 mm deep in 2e-4 mm segments go. One vertex on the
    // top edge stays, between two long lines.
    let t = l(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (5.0, 10.0),
        (5.000001, 15.0),
        (5.000002, 10.0),
        (4.9999, 10.0),
        (4.9998, 9.999996),
        (4.9997, 10.0),
        (0.0, 10.0),
        (0.0, 3e-6),
    ])
    .tidied()
    .unwrap();
    assert_eq!(t.verts.len(), 5, "{t:?}");
    assert!(close(t.area(), 100.0, 1e-4), "{}", t.area());
    // A vertex where two long lines meet in the same direction stays (it
    // is a point of the G-code), however straight.
    let t = l(&[
        (0.0, 0.0),
        (5.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
    ])
    .tidied()
    .unwrap();
    assert_eq!(t.verts.len(), 5);
    // A loop 5e-6 mm wide goes altogether.
    assert!(
        l(&[(0.0, 0.0), (1.0, 0.0), (1.0, 5e-6), (0.0, 5e-6)])
            .tidied()
            .is_none()
    );
    // A short arc with a sagitta of 5e-7 mm becomes a line; a longer one
    // stays an arc.
    let arc = |chord: f64, sagitta: f64| Loop2 {
        verts: vec![
            Vertex::new(0.0, 0.0, 2.0 * sagitta / chord),
            Vertex::new(chord, 0.0, 0.0),
            Vertex::new(chord, 1.0, 0.0),
            Vertex::new(0.0, 1.0, 0.0),
        ],
    };
    assert_eq!(arc(5e-4, 5e-7).tidied().unwrap().verts[0].bulge, 0.0);
    assert_ne!(arc(1e-2, 5e-7).tidied().unwrap().verts[0].bulge, 0.0);
}

#[test]
fn polygons_without_area_are_empty() {
    // Found by the `region` fuzz target: a triangle 2e-6 mm across
    // intersected with a "triangle" with two equal points failed a debug
    // assertion in `parametric_from_point` (from `line_line_intr`, a
    // zero-length edge) on every retry.
    let a = Region2::polygon(&[(0.750002, 0.750002), (0.75, 0.750002), (0.750002, 0.75)]);
    let b = Region2::polygon(&[(0.750002, 0.750002), (0.750002, 0.750002), (7.75, 3.75)]);
    assert!(a.is_empty() && b.is_empty());
    let c = Region2::rect(0.0, 0.0, 1.0, 1.0);
    assert!(c.try_intersect(&b).unwrap().is_empty());
    // Repeated points are left out of a polygon with area.
    let d = Region2::polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)]);
    assert_eq!(d.loops[0].verts.len(), 3);
    assert!(close(d.area(), 0.5, 1e-12));
}

#[test]
fn intersection_with_a_nearly_vertical_line() {
    // Found by the `region` fuzz target. The shrunk union holds a line
    // 1.07e-3 mm long with an X extent of 6e-10 mm. `line_circle_intr`
    // took it as vertical, and the intersection points with the circle,
    // 2 700 line lengths away, came out 1.6e-6 mm off it: a debug assertion
    // in `parametric_from_point` on every retry.
    let c = Region2::circle(4.000001, 5.750002, 2.875001);
    let a = c
        .union(&Region2::circle(5.750002, 5.750001, 4.625))
        .offset(0.125)
        .offset(-2.71875);
    let r = a.try_intersect(&c).unwrap();
    assert!(r.area() > 0.0 && r.area() < a.area(), "{}", r.area());
}

/// The loop of the collapsing-arc growth: a concave arc of radius 3 (from
/// the last vertex to the first) in a 0.26 mm² loop.
fn loop_with_a_concave_arc_of_radius_3() -> Region2 {
    Region2 {
        loops: vec![Loop2 {
            verts: vec![
                Vertex::new(20.412821428802026, 19.30420795048225, -6.621615729048858e-6),
                Vertex::new(20.412899018905485, 19.30422508437571, 0.0),
                Vertex::new(20.46410677843245, 19.69318663440609, 0.0),
                Vertex::new(20.5415041911791, 20.38041847575817, 0.0),
                Vertex::new(19.991516959012024, 20.379028816162474, -0.09711078463494548),
            ],
        }],
    }
}

#[test]
fn growth_where_a_concave_arc_collapses_to_a_point() {
    // `optimised_load::shrink` grows the holes of a cleared region by
    // R − 1e-6. With a concave arc of radius R the arc's offset radius is
    // 1e-6 mm, and `cavalier_contours` gave no loop at all (2.9999 and 3.0
    // give 36.9 mm²; docs/plan/2026-11-04-region-empty-growth.md).
    let a = loop_with_a_concave_arc_of_radius_3();
    let retries = containment_retries();
    let mut last = 0.0;
    // Around the collapse, the area grows smoothly with the distance.
    for d in [
        2.9999, 2.99999, 2.999999, 2.9999999, 3.0, 3.0000001, 3.000001, 3.00001,
    ] {
        let g = a.try_offset(d).unwrap();
        assert_eq!(g.loops.len(), 1);
        assert!(
            g.area() > last && close(g.area(), 36.916, 3e-3),
            "{d}: {}",
            g.area()
        );
        last = g.area();
    }
    assert_eq!(containment_retries(), retries);
}

/// The loop of the short-collinear-lines growth: three lines 0.014 to
/// 0.042 mm long, collinear within 4.3e-5 mm, next to a nearly tangent arc.
fn loop_with_short_nearly_collinear_lines() -> Region2 {
    Region2 {
        loops: vec![Loop2 {
            verts: vec![
                Vertex::new(6.964288267857258, 7.479660399608717, -0.07626804134349185),
                Vertex::new(7.312397383992374, 5.2892095561762975, 0.0),
                Vertex::new(7.312455771167016, 5.267258283738914, 0.0),
                Vertex::new(7.312444408467266, 5.224778705253155, 0.0),
                Vertex::new(7.312397383992378, 5.2107924438222035, -0.07626804134344047),
                Vertex::new(6.964288267857265, 3.020341600391289, 0.23291380525219202),
                Vertex::new(8.062502000000002, 5.250001000000001, 0.23291380525219368),
            ],
        }],
    }
}

#[test]
fn growth_of_short_nearly_collinear_lines() {
    // Grown by 3.5625, `cavalier_contours` took the offsets of the two
    // concave corners between the lines for overlaps, failed the assertion
    // in `parametric_from_point` (from `line_line_join`) and, in release
    // builds, dropped the whole growth.
    let a = loop_with_short_nearly_collinear_lines();
    let g = a.try_offset(3.5625).unwrap();
    assert_eq!(g.loops.len(), 1);
    assert!(close(g.area(), 76.735, 1e-3), "{}", g.area());
    // The lines keep their (trimmed) offsets, 3.5625 mm out.
    let x = g.loops[0]
        .verts
        .iter()
        .filter(|v| close(v.p.x(), 7.3124 - 3.5625, 1e-4));
    assert_eq!(x.count(), 4, "{g:?}");
}

#[test]
fn wrong_offsets_are_retried_then_raised() {
    // A growth must hold its input and a shrink must lie inside it. A result
    // that does not is retried like a panic and, if every retry is wrong
    // too, is a `RegionFailure`, never a silently wrong region.
    let square = Region2::rect(0.0, 0.0, 10.0, 10.0);
    for (wrong, d) in [
        (fault::Wrong::Empty, 1.0),
        (fault::Wrong::Moved, 1.0),
        (fault::Wrong::Moved, -1.0),
    ] {
        fault::set_wrong(Some(wrong));
        let r = square.try_offset(d);
        fault::set_wrong(None);
        let e = r.unwrap_err();
        assert_eq!(e.op, RegionOp::Offset);
        assert!(e.message.contains("input"), "{}", e.message);
        assert_eq!(e.code(), "cam.region.offset_failed");
    }
    // Only the first attempt wrong: the retry gives the right answer.
    let retries = containment_retries();
    fault::set_wrong_once(fault::Wrong::Empty);
    let g = square.try_offset(1.0).unwrap();
    assert!(close(g.area(), 140.0 + PI, 1e-5), "{}", g.area());
    assert_eq!(containment_retries(), retries + 1);
    // An empty shrink is right.
    assert!(square.try_offset(-6.0).unwrap().is_empty());
}

#[test]
fn offsets_hold_their_input() {
    // The guard accepts ordinary growths and shrinks, with holes, arcs and
    // offsets below the clean-up tolerance.
    let retries = containment_retries();
    let p = plate();
    for d in [
        -20.0, -5.0, -1e-6, 1e-7, 1e-4, 0.5, 3.0, 9.999999, 10.0, 25.0,
    ] {
        let o = p.try_offset(d).unwrap();
        assert!((o.area() - p.area()) * d >= 0.0, "{d}");
    }
    let ring = ring_with_a_sliver_bump();
    for d in [-1.34375, 1.34375, 3.5625] {
        ring.try_offset(d).unwrap();
    }
    assert_eq!(containment_retries(), retries);
}

/// From `rest_after_roughing_cuts_little`: a frame round 10–70 × 10–50 and a
/// ring round a 35–45 × 25–35 hole, both about 0.01 mm wide.
fn thin_frame_and_ring() -> Region2 {
    Region2 {
        loops: vec![
            Loop2 {
                verts: vec![
                    Vertex::new(10.01, 47.0, -0.41421356237309503),
                    Vertex::new(13.0, 49.99, 0.0),
                    Vertex::new(33.783450803936034, 49.99, 0.0),
                    Vertex::new(67.0, 49.99, -0.41421356237309503),
                    Vertex::new(69.99, 47.0, 0.0),
                    Vertex::new(69.99, 13.0, -0.41421356237309503),
                    Vertex::new(67.0, 10.01, 0.0),
                    Vertex::new(13.0, 10.01, -0.41421356237309503),
                    Vertex::new(10.01, 13.0, 0.0),
                ],
            },
            Loop2 {
                verts: vec![
                    Vertex::new(35.07553891311464, 35.01, 0.0),
                    Vertex::new(35.04602562873344, 35.00850780917998, 0.0),
                    Vertex::new(34.99999878830466, 35.00000447656944, 0.0),
                    Vertex::new(34.999997416109444, 35.00000385115426, 0.0),
                    Vertex::new(34.999996317113286, 35.00000281855016, 0.0),
                    Vertex::new(34.99999560751484, 35.00000148793603, 0.0),
                    Vertex::new(34.99999542404506, 35.000000753999956, 0.0),
                    Vertex::new(34.99, 34.877860196654964, 0.0),
                    Vertex::new(34.99, 25.117477419952895, 0.0),
                    Vertex::new(34.99925496047504, 24.999881997331833, 0.0),
                    Vertex::new(34.99928259283219, 24.99976690028097, 0.0),
                    Vertex::new(34.99932789013475, 24.999657542918783, 0.0),
                    Vertex::new(34.99938973701279, 24.999556617986507, 0.0),
                    Vertex::new(34.99946661059091, 24.999466610590925, 0.0),
                    Vertex::new(34.99955661798651, 24.999389737012798, 0.0),
                    Vertex::new(34.99965754291881, 24.999327890134737, 0.0),
                    Vertex::new(34.99976690028096, 24.99928259283219, 0.0),
                    Vertex::new(34.99988199733182, 24.999254960475046, 0.0),
                    Vertex::new(35.11747741995293, 24.990000000000002, 0.0),
                    Vertex::new(44.88252258004709, 24.990000000000002, 0.0),
                    Vertex::new(45.000118002668174, 24.999254960475042, 0.0),
                    Vertex::new(45.00023309971903, 24.999282592832188, 0.0),
                    Vertex::new(45.00034245708118, 24.999327890134737, 0.0),
                    Vertex::new(45.000443382013486, 24.999389737012795, 0.0),
                    Vertex::new(45.000533389409114, 24.999466610590947, 0.0),
                    Vertex::new(45.00061026298722, 24.999556617986524, 0.0),
                    Vertex::new(45.00067210986524, 24.999657542918765, 0.0),
                    Vertex::new(45.000717407167805, 24.99976690028095, 0.0),
                    Vertex::new(45.00074503952496, 24.999881997331848, 0.0),
                    Vertex::new(45.01, 25.1174774199529, 0.0),
                    Vertex::new(45.01, 34.88252258004711, 0.0),
                    Vertex::new(45.00074503952496, 35.000118002668174, 0.0),
                    Vertex::new(45.00071740716781, 35.000233099719026, 0.0),
                    Vertex::new(45.000672109865256, 35.0003424570812, 0.0),
                    Vertex::new(45.000610262987195, 35.00044338201351, 0.0),
                    Vertex::new(45.000533389409085, 35.000533389409085, 0.0),
                    Vertex::new(45.00044338201351, 35.000610262987195, 0.0),
                    Vertex::new(45.0003424570812, 35.000672109865256, 0.0),
                    Vertex::new(45.000233099719026, 35.00071740716781, 0.0),
                    Vertex::new(45.000118002668174, 35.00074503952496, 0.0),
                    Vertex::new(44.88252258004711, 35.01, 0.0),
                ],
            },
            Loop2 {
                verts: vec![
                    Vertex::new(10.0, 47.0, 0.0),
                    Vertex::new(10.0, 13.0, 0.4142135623730951),
                    Vertex::new(13.0, 10.0, 0.0),
                    Vertex::new(67.0, 10.0, 0.4142135623730951),
                    Vertex::new(70.0, 13.0, 0.0),
                    Vertex::new(70.0, 47.0, 0.4142135623730951),
                    Vertex::new(67.0, 50.0, 0.0),
                    Vertex::new(13.0, 50.0, 0.4142135623730951),
                ],
            },
            Loop2 {
                verts: vec![
                    Vertex::new(35.0, 25.0, 0.0),
                    Vertex::new(35.0, 35.0, 0.0),
                    Vertex::new(45.0, 35.0, 0.0),
                    Vertex::new(45.0, 25.0, 0.0),
                ],
            },
        ],
    }
}

#[test]
fn shrinking_a_thin_ring_around_a_hole_leaves_nothing() {
    // Shrunk by 0.02, `cavalier_contours` returned the grown hole alone (a
    // region of area −100.8), which the containment guard rejected on
    // every retry.
    let r = thin_frame_and_ring();
    assert!(close(r.area(), 2.344, 1e-3), "{}", r.area());
    let o = r.try_offset(-0.02).unwrap();
    assert!(o.is_empty(), "{o:?}");
    // A hole that does lie in the shrink stays.
    let thick =
        Region2::rect(30.0, 20.0, 50.0, 40.0).with_hole(&Region2::rect(35.0, 25.0, 45.0, 35.0));
    let o = thick.try_offset(-0.5).unwrap();
    assert_eq!(o.loops.len(), 2);
}

#[test]
fn shrink_of_a_thin_triangle_to_near_its_inradius() {
    // Found by the `region` fuzz target: a disc minus a thin triangle,
    // grown by 0.6875, 4.9e-4 mm less than the triangle's inradius. The
    // shrunk triangle has sides 1.1e-3 and 6.6e-3 mm long, 0.16 rad apart at
    // its tip; `line_line_intr` took the two long ones for overlapping
    // (perp product under its epsilon), and slicing there failed the debug
    // assertion in `PlineViewData::create` on every retry.
    let tri = Region2::polygon(&[(5.250001, 5.000001), (5.250001, 3.5), (14.500003, 5.250001)]);
    let s = tri.try_offset(-0.6875).unwrap();
    assert!(s.area() > 0.0 && s.area() < 1e-5, "{}", s.area());
    let mut r = Region2::circle(3.750003, 7.000001, 7.0);
    for d in [3.0, 3.96875] {
        r = r.try_offset(d).unwrap();
    }
    r = r.try_subtract(&tri).unwrap();
    for d in [0.6875, -1.3125, -3.21875] {
        let before = r.area();
        r = r.try_offset(d).unwrap();
        assert!(
            (r.area() - before) * d > 0.0,
            "{d}: {before} to {}",
            r.area()
        );
    }
}

#[test]
fn growths_of_a_shape_with_spike_tips() {
    // Found by the `region` fuzz target through the containment guard. A
    // disc and a bar 3.5 mm high, shrunk by 1.75: the bar collapses to its
    // middle line, and the shape ends in two tips where concave arcs of
    // radius 1.75 meet back to back. Rounding leaves each tip turning back a
    // micro-radian on the inner side, `cavalier_contours` took them as inner
    // joins whose offsets do not cross, and every growth came back empty.
    let s = Region2::circle(9.250001, 5.5, 5.0)
        .union(&Region2::rect(
            2.500002,
            3.750003,
            18.250004999999998,
            7.250005,
        ))
        .try_offset(-1.75)
        .unwrap();
    assert_eq!(s.loops.len(), 1);
    let mut last = s.area();
    for d in [0.5, 1.0, 1.75, 3.0, 3.78125] {
        let g = s.try_offset(d).unwrap();
        assert_eq!(g.loops.len(), 1, "{d}");
        assert!(g.area() > last, "{d}: {}", g.area());
        last = g.area();
    }
    // The tips grow round: the growth reaches d left of the left tip.
    let g = s.try_offset(1.0).unwrap();
    let (lo, _) = g.bbox();
    assert!(close(lo.x(), s.bbox().0.x() - 1.0, 1e-6), "{lo:?}");
}
