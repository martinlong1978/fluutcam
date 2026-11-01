// SPDX-License-Identifier: MIT OR Apache-2.0

//! Drop solvers against the brute-force reference (D07 §9).

use super::drop::{drop, drop_all};
use super::reference::{Density, reference};
use super::solve::{Axis, SolveStats};
use super::synth::{Builder, Lcg, bspline_square, cuboid, tiled};
use super::*;
use fluut_geom::{Aabb3, Frame, Interval, Point3, Surface, UvBox, Vec3};
use fluut_topo::BrepBody;

fn iv(lo: f64, hi: f64) -> Interval {
    Interval { lo, hi }
}

fn cutters() -> [Cutter; 3] {
    [
        Cutter::flat(6.0).unwrap(),
        Cutter::ball(6.0).unwrap(),
        Cutter::new(6.0, 1.0).unwrap(),
    ]
}

fn body_box(b: &BrepBody) -> Aabb3 {
    let mut bx = Aabb3::empty();
    for e in 0..b.edges.len() as u32 {
        for p in b.edge_points(e, 0.01) {
            bx = bx.with_point(p);
        }
    }
    bx
}

/// The result is a real point of the geometry in the footprint.
fn assert_real(g: &QueryGeometry, c: &Cutter, ax: Axis, r: &Contact) {
    let d = ax.lateral(r.point);
    assert!(d <= c.radius() + 1e-9, "outside footprint: {r:?}");
    assert!(
        (r.tau - (r.point.z() - c.h(d.min(c.radius())))).abs() < 1e-9,
        "{r:?}"
    );
    match r.on {
        ContactOn::Face { face, u, v } => {
            let s = &g.body.surfaces[g.body.faces[face as usize].surface as usize];
            assert!(s.eval(u, v).distance(r.point) < 1e-9);
            assert!(g.faces[face as usize].as_ref().unwrap().trim.contains(u, v));
        }
        ContactOn::Edge { edge, t } => {
            let e = &g.body.edges[edge as usize];
            assert!(e.range.lo - 1e-12 <= t && t <= e.range.hi + 1e-12);
        }
        ContactOn::Vertex(_) => {}
    }
}

/// Drops on an `n × n` grid over the body (grown by the tool radius) agree
/// with the reference: no misses beyond 1e-7. The solver may be higher (the
/// reference is a lower bound, weakest on the footprint rim), but its
/// contact must be a real point.
fn check_against_reference(body: &BrepBody, n: usize, density: Density) {
    let g = QueryGeometry::new(body, &BuildOptions::default());
    assert!(g.issues.is_empty(), "{:?}", g.issues);
    let bx = body_box(body);
    for c in cutters() {
        let rad = c.radius();
        for i in 0..n {
            for j in 0..n {
                let x = bx.min.x() - rad
                    + (bx.max.x() - bx.min.x() + 2.0 * rad) * (i as f64 + 0.37) / n as f64;
                let y = bx.min.y() - rad
                    + (bx.max.y() - bx.min.y() + 2.0 * rad) * (j as f64 + 0.61) / n as f64;
                let ax = Axis { x, y };
                let mut st = SolveStats::default();
                let got = drop_all(&g, &c, x, y, &Seeding::default(), &mut st);
                let fast = drop(&g, &c, x, y, &DropOptions::default(), &mut st);
                let want = reference(&g, &c, ax, density);
                match (got, want) {
                    (None, None) => {}
                    (Some(r), Some(w)) => {
                        assert_real(&g, &c, ax, &r);
                        assert!(
                            r.tau >= w - 1e-7,
                            "{} miss at ({x}, {y}) {c:?}: {} < {w}, {r:?}",
                            body.name,
                            r.tau
                        );
                        assert!(
                            r.tau <= w + 1e-5,
                            "{} reference low at ({x}, {y}) {c:?}: {} > {w}, {r:?}",
                            body.name,
                            r.tau
                        );
                        let f = fast.expect("branch and bound found nothing");
                        assert!(
                            (f.tau - r.tau).abs() < 1e-6,
                            "pruned the contact: {f:?} vs {r:?}"
                        );
                    }
                    other => panic!("{} at ({x}, {y}) {c:?}: {other:?}", body.name),
                }
            }
        }
    }
}

fn frame(o: (f64, f64, f64), z: (f64, f64, f64), x: (f64, f64, f64)) -> Frame {
    Frame::new(
        Point3::new(o.0, o.1, o.2),
        Vec3::new(z.0, z.1, z.2),
        Vec3::new(x.0, x.1, x.2),
    )
    .unwrap()
}

fn single(name: &str, s: Surface, uv: UvBox) -> BrepBody {
    let mut b = Builder::new(name);
    b.patch(s, uv);
    b.build()
}

const DENSE: Density = Density {
    face: 80,
    edge: 300,
};

#[test]
fn tilted_plane() {
    let s = Surface::plane(frame((0.0, 0.0, 1.0), (0.3, -0.2, 1.0), (1.0, 0.0, 0.0)));
    check_against_reference(
        &single("plane", s, UvBox::new(iv(0.0, 10.0), iv(0.0, 6.0))),
        4,
        DENSE,
    );
}

#[test]
fn horizontal_cylinder() {
    let s = Surface::cylinder(
        frame((0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)),
        4.0,
    )
    .unwrap();
    check_against_reference(
        &single("cylinder", s, UvBox::new(iv(0.2, 3.0), iv(0.0, 10.0))),
        4,
        DENSE,
    );
}

#[test]
fn tilted_cylinder() {
    let s = Surface::cylinder(
        frame((0.0, 0.0, 0.0), (1.0, 0.3, 0.4), (0.0, 1.0, 0.0)),
        2.0,
    )
    .unwrap();
    check_against_reference(
        &single("cylinder", s, UvBox::new(iv(-1.0, 2.5), iv(0.0, 8.0))),
        4,
        DENSE,
    );
}

#[test]
fn cone() {
    let s = Surface::cone(
        frame((0.0, 0.0, 5.0), (0.0, 0.2, -1.0), (1.0, 0.0, 0.0)),
        1.0,
        0.5,
    )
    .unwrap();
    check_against_reference(
        &single("cone", s, UvBox::new(iv(0.0, 5.0), iv(0.0, 4.0))),
        4,
        DENSE,
    );
}

#[test]
fn sphere() {
    let s = Surface::sphere(
        frame((0.0, 0.0, 0.0), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0)),
        5.0,
    )
    .unwrap();
    check_against_reference(
        &single("sphere", s, UvBox::new(iv(0.5, 4.0), iv(-0.5, 1.3))),
        4,
        DENSE,
    );
}

#[test]
fn concave_sphere() {
    // The inside of a bowl: radius of curvature 5 against tool radius 3.
    let s = Surface::sphere(
        frame((0.0, 0.0, 5.0), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0)),
        5.0,
    )
    .unwrap();
    check_against_reference(
        &single("bowl", s, UvBox::new(iv(0.0, 6.0), iv(-1.4, -0.3))),
        4,
        DENSE,
    );
}

#[test]
fn torus() {
    let s = Surface::torus(
        frame((0.0, 0.0, 0.0), (0.1, 0.0, 1.0), (1.0, 0.0, 0.0)),
        6.0,
        2.0,
    )
    .unwrap();
    check_against_reference(
        &single("torus", s, UvBox::new(iv(0.0, 2.0), iv(-1.0, 2.5))),
        4,
        DENSE,
    );
}

#[test]
fn bspline_bump() {
    let mut b = Builder::new("bump");
    let mut rng = Lcg(7);
    let hs: Vec<f64> = (0..36).map(|_| 4.0 * rng.uniform() - 2.0).collect();
    let (s, bd) = bspline_square(Point3::ORIGIN, 12.0, 6, |i, j| hs[i * 6 + j]);
    b.bspline_patch(s, bd);
    check_against_reference(&b.build(), 4, DENSE);
}

#[test]
fn cube_top_edges_and_vertices() {
    let mut b = Builder::new("cube");
    cuboid(
        &mut b,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(10.0, 8.0, 5.0),
    );
    let body = b.build();
    let g = QueryGeometry::new(&body, &BuildOptions::default());
    let mut st = SolveStats::default();
    for c in cutters() {
        let o = DropOptions::default();
        // Over the top face.
        let r = drop(&g, &c, 5.0, 4.0, &o, &mut st).unwrap();
        assert!((r.tau - 5.0).abs() < 1e-12);
        // Beside the top edge x = 10, half a radius out.
        let r = drop(&g, &c, 11.5, 4.0, &o, &mut st).unwrap();
        let want = 5.0 - c.h(1.5);
        assert!((r.tau - want).abs() < 1e-9, "{c:?} {r:?}");
        // Past the corner (10, 8) diagonally.
        let r = drop(&g, &c, 11.0, 9.0, &o, &mut st).unwrap();
        let want = 5.0 - c.h(libm::hypot(1.0, 1.0));
        assert!((r.tau - want).abs() < 1e-9, "{c:?} {r:?}");
        // Out of reach.
        assert!(drop(&g, &c, 14.0, 4.0, &o, &mut st).is_none());
    }
}

#[test]
fn tile_model_branch_and_bound_matches_full_scan() {
    let body = tiled(2, false);
    let g = QueryGeometry::new(&body, &BuildOptions::default());
    let mut rng = Lcg(11);
    for c in cutters() {
        for _ in 0..60 {
            let (x, y) = (22.0 * rng.uniform() - 1.0, 22.0 * rng.uniform() - 1.0);
            let mut st = SolveStats::default();
            let a = drop_all(&g, &c, x, y, &Seeding::default(), &mut st);
            let b = drop(&g, &c, x, y, &DropOptions::default(), &mut st);
            match (a, b) {
                (None, None) => {}
                (Some(a), Some(b)) => assert!((a.tau - b.tau).abs() < 1e-6, "{a:?} {b:?}"),
                other => panic!("{other:?}"),
            }
        }
    }
}

#[test]
fn tile_model_against_reference() {
    check_against_reference(
        &tiled(1, false),
        3,
        Density {
            face: 40,
            edge: 200,
        },
    );
}

#[test]
fn freeform_tile_against_reference() {
    check_against_reference(
        &tiled(1, true),
        3,
        Density {
            face: 40,
            edge: 200,
        },
    );
}

/// Points where earlier seeding missed free-form contacts (S2 report §3).
#[test]
fn freeform_tiles_hard_points() {
    let body = tiled(6, true);
    let g = QueryGeometry::new(&body, &BuildOptions::default());
    for (x, y, corner) in [
        (21.049039427582667, 51.53632464093423, 0.0),
        (21.049039427582667, 51.53632464093423, 1.0),
        (10.645306821387255, 19.145782717923947, 3.0),
        (15.596900133816447, 9.802168317858154, 3.0),
        (16.799139513553097, 9.88248325492302, 3.0),
        (10.794356826497541, 17.828900485874012, 3.0),
        (0.47305581221192505, 5.21424032411868, 0.0),
    ] {
        let c = Cutter::new(6.0, corner).unwrap();
        let mut st = SolveStats::default();
        let got = drop(&g, &c, x, y, &DropOptions::default(), &mut st).unwrap();
        let want = reference(&g, &c, Axis { x, y }, DENSE).unwrap();
        assert!(
            got.tau >= want - 1e-7,
            "({x}, {y}) {c:?}: {} < {want}",
            got.tau
        );
    }
}
