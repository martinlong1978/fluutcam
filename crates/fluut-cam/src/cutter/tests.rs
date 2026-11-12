// SPDX-License-Identifier: MIT OR Apache-2.0

//! Drop solvers against the brute-force reference (D07 §9).

use super::drop::{drop, drop_all};
use super::geom::Prim;
use super::reference::{Density, reference};
use super::solve::{Axis, SolveStats};
use super::synth::{Builder, Lcg, bspline_square, cuboid, sculpted, tiled};
use super::*;
use fluut_geom::{Aabb3, Curve3, Frame, Interval, Point3, Surface, UvBox, Vec3};
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

/// A drop limited to a window [floor, ceiling] decides the same side of
/// every height in it as the full drop, and is exact inside it.
#[test]
fn window_drop_classifies_like_the_full_drop() {
    let body = tiled(2, false);
    let g = QueryGeometry::new(&body, &BuildOptions::default());
    let mut rng = Lcg(5);
    for c in cutters() {
        for _ in 0..80 {
            let (x, y) = (22.0 * rng.uniform() - 1.0, 22.0 * rng.uniform() - 1.0);
            let z = 6.0 * rng.uniform() - 1.0;
            let mut st = SolveStats::default();
            let full = drop(&g, &c, x, y, &DropOptions::default(), &mut st).map(|r| r.tau);
            let o = DropOptions {
                floor: z - 0.5,
                ceiling: z + 0.5,
                ..Default::default()
            };
            let win = drop(&g, &c, x, y, &o, &mut st).map(|r| r.tau);
            let f = full.unwrap_or(f64::NEG_INFINITY);
            let w = win.unwrap_or(f64::NEG_INFINITY);
            assert_eq!(f > z, w > z, "{c:?} ({x}, {y}) z {z}: {full:?} {win:?}");
            if f > z - 0.5 && f < z + 0.5 {
                assert!((f - w).abs() < 1e-9, "{full:?} {win:?}");
            }
            if f >= z + 0.5 {
                assert!(w >= z + 0.5 && w <= f);
            }
        }
    }
}

/// A band of a drafted (tilted) cylinder, r = 25, whose top edge is a
/// circle `gap` mm outside the surface: a small model gap between a face
/// and its edge, as on the NIST corpus. Ball tools touch such a near-
/// vertical wall at the rim of their footprint.
fn drafted_band(gap: f64) -> BrepBody {
    let tilt = 2f64.to_radians();
    let (st, ct) = libm::sincos(tilt);
    let frame = Frame::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, st, ct),
        Vec3::new(1.0, 0.0, 0.0),
    )
    .unwrap();
    let r = 25.0;
    let s = Surface::Cylinder { frame, r };
    let (u0, u1) = (0.0, std::f64::consts::PI);
    let mut b = Builder::new("drafted");
    let top = Curve3::circle(frame, r + gap).unwrap();
    let (bc, br) = super::synth::iso_v(&s, -40.0, iv(u0, u1)).unwrap();
    let bottom = b.edge(Some(bc), br, Point3::ORIGIN);
    let (rc, rr) = super::synth::line(s.eval(u1, -40.0), top.eval(u1)).unwrap();
    let right = b.edge(Some(rc), rr, Point3::ORIGIN);
    let t = b.edge(Some(top), iv(u0, u1), Point3::ORIGIN);
    let (lc, lr) = super::synth::line(
        s.eval(u0, -40.0),
        Curve3::circle(frame, r + gap).unwrap().eval(u0),
    )
    .unwrap();
    let left = b.edge(Some(lc), lr, Point3::ORIGIN);
    b.face(
        s,
        false,
        &[vec![
            (bottom, true),
            (right, true),
            (t, false),
            (left, false),
        ]],
    );
    b.build()
}

/// Ball drops across the top of a drafted wall whose edge is a little off
/// the surface. The face's tangent point is just above its trimmed top and
/// the edge is just out of reach, so neither found the top of the wall:
/// the drop fell about 38 mm over a band 3 µm wide (the lost waterline
/// traces on `nist_ctc_02`). The face's own boundary covers it now.
#[test]
fn drafted_wall_with_a_model_gap_has_no_cracks() {
    let body = drafted_band(3e-3);
    let g = QueryGeometry::new(&body, &BuildOptions::default());
    let c = Cutter::ball(6.0).unwrap();
    let o = DropOptions::default();
    let mut st = SolveStats::default();
    let tilt = 2f64.to_radians();
    for k in [1, 4, 8, 13] {
        let u = std::f64::consts::PI * k as f64 / 16.0;
        let (su, cu) = libm::sincos(u);
        // A point of the top boundary, and the inward horizontal direction.
        let p = Point3::new(25.0 * cu, 25.0 * su * libm::cos(tilt), 0.0);
        let n = 2000;
        let mut prev: Option<f64> = None;
        for i in 0..=n {
            let s = 3.0 - 0.002 + 0.008 * i as f64 / n as f64;
            let (x, y) = (p.x() - cu * s, p.y() - su * s);
            let tau = drop(&g, &c, x, y, &o, &mut st).map_or(f64::NEG_INFINITY, |r| r.tau);
            if let Some(t) = prev {
                assert!((tau - t).abs() < 0.1, "u {u} s {s}: {t} -> {tau}");
            }
            prev = Some(tau);
            if i % 250 == 0 {
                let want = reference(
                    &g,
                    &c,
                    Axis { x, y },
                    Density {
                        face: 300,
                        edge: 2000,
                    },
                )
                .unwrap_or(f64::NEG_INFINITY);
                assert!(tau >= want - 1e-3, "u {u} s {s}: {tau} < {want}");
            }
        }
    }
}

/// The face boundary search finds contacts inside a boundary segment whose
/// two ends are both out of the footprint.
#[test]
fn rim_finds_contacts_between_its_points() {
    let body = drafted_band(3e-3);
    let g = QueryGeometry::new(&body, &BuildOptions::default());
    let c = Cutter::ball(6.0).unwrap();
    let lp = g.faces[0]
        .as_ref()
        .unwrap()
        .rim
        .iter()
        .max_by_key(|l| l.pts.len())
        .unwrap();
    // A segment of the top boundary (z near 0, away from the corners).
    let k = (0..lp.pts.len() - 1)
        .filter(|&k| lp.pts[k].2.z().abs() < 0.5 && lp.pts[k + 1].2.z().abs() < 0.5)
        .nth(5)
        .unwrap();
    let (a, b) = (lp.pts[k].2, lp.pts[k + 1].2);
    let m = Point3::new(0.5 * (a.x() + b.x()), 0.5 * (a.y() + b.y()), 0.0);
    // Inward (towards the cylinder axis) from the segment's middle.
    let r = libm::hypot(m.x(), m.y());
    let s = 3.0 - 1e-4;
    let ax = Axis {
        x: m.x() * (1.0 - s / r),
        y: m.y() * (1.0 - s / r),
    };
    assert!(ax.lateral(a) > 3.0 && ax.lateral(b) > 3.0);
    let got = super::solve::rim(&g, 0, ax, &c, f64::NEG_INFINITY).unwrap();
    assert!(ax.lateral(got.point) <= 3.0 + 1e-9);
    let mut st = SolveStats::default();
    let d = drop(&g, &c, ax.x, ax.y, &DropOptions::default(), &mut st).unwrap();
    assert!(d.tau >= got.tau - 1e-3, "{d:?} {got:?}");
}

/// A cylinder wall of radius 2 and height 1 between two full circles.
fn wall() -> BrepBody {
    use super::synth::iso_v;
    use std::f64::consts::TAU;
    let mut b = Builder::new("wall");
    let s = Surface::cylinder(Frame::world_xy(), 2.0).unwrap();
    let (c0, r) = iso_v(&s, 0.0, iv(0.0, TAU)).unwrap();
    let (c1, _) = iso_v(&s, 1.0, iv(0.0, TAU)).unwrap();
    let (p0, p1) = (c0.eval(0.0), c1.eval(0.0));
    let e0 = b.edge(Some(c0), r, p0);
    let e1 = b.edge(Some(c1), r, p1);
    b.face(s, true, &[vec![(e0, true)], vec![(e1, false)]]);
    b.build()
}

#[test]
fn a_small_ball_finds_the_same_contacts_with_split_edges_and_without_rims() {
    let body = wall();
    let g = QueryGeometry::new(&body, &BuildOptions::default());
    let ball = Cutter::ball(0.02).unwrap();
    let plain = DropOptions::default();
    let fast = DropOptions {
        rims: false,
        seeding: Seeding {
            split_edges: true,
            ..Seeding::default()
        },
        ..DropOptions::default()
    };
    let mut seen = 0;
    for i in 0..40 {
        for j in 0..40 {
            let (x, y) = (-2.1 + 0.105 * i as f64, -2.1 + 0.105 * j as f64);
            let mut st = SolveStats::default();
            let a = drop(&g, &ball, x, y, &plain, &mut st).map(|c| c.tau);
            let b = drop(&g, &ball, x, y, &fast, &mut st).map(|c| c.tau);
            match (a, b) {
                (None, None) => {}
                (Some(a), Some(b)) => {
                    seen += 1;
                    assert!((a - b).abs() < 1e-7, "{x} {y}: {a} {b}");
                }
                other => panic!("{x} {y}: {other:?}"),
            }
        }
    }
    assert!(seen > 0);
}

/// The pruned drop on smooth free-form faces (sub-box bounds, patches
/// queued again under them) equals the drop that solves every primitive, in
/// whatever order the points come.
#[test]
fn smooth_bspline_drops_match_the_full_scan_in_any_order() {
    let body = sculpted(8, 3.0, 5);
    let g = QueryGeometry::new(&body, &BuildOptions::default());
    let mut rng = Lcg(21);
    let mut pts: Vec<(f64, f64)> = (0..50)
        .map(|_| (-3.0 + 30.0 * rng.uniform(), -3.0 + 30.0 * rng.uniform()))
        .collect();
    let heights = |pts: &[(f64, f64)]| -> Vec<(f64, f64, f64, f64)> {
        let mut out = Vec::new();
        for c in cutters() {
            for &(x, y) in pts {
                let mut st = SolveStats::default();
                let all = drop_all(&g, &c, x, y, &Seeding::default(), &mut st);
                let got = drop(&g, &c, x, y, &DropOptions::default(), &mut st);
                if let Some(r) = &got {
                    assert_real(&g, &c, Axis { x, y }, r);
                }
                let h = |r: Option<Contact>| r.map_or(f64::NEG_INFINITY, |r| r.tau);
                out.push((
                    x + 1000.0 * c.radius() + 10.0 * c.corner(),
                    y,
                    h(all),
                    h(got),
                ));
            }
        }
        out
    };
    let first = heights(&pts);
    for (x, y, all, got) in &first {
        assert!(
            *got >= all - 1e-7,
            "({x}, {y}): {got} below the full scan {all}"
        );
        assert!(
            *got <= all + 1e-6,
            "({x}, {y}): {got} above the full scan {all}"
        );
    }
    // The same points shuffled (Fisher-Yates on a fixed LCG) give the same
    // height at each point.
    for i in (1..pts.len()).rev() {
        pts.swap(i, (rng.uniform() * (i + 1) as f64) as usize);
    }
    let second = heights(&pts);
    let mut a: Vec<_> = first
        .iter()
        .map(|r| (r.0.to_bits(), r.1.to_bits(), r.3))
        .collect();
    let mut b: Vec<_> = second
        .iter()
        .map(|r| (r.0.to_bits(), r.1.to_bits(), r.3))
        .collect();
    a.sort_by_key(|p| (p.0, p.1));
    b.sort_by_key(|p| (p.0, p.1));
    assert_eq!(a, b);
}

/// Contacts on the rim of a flat tool's footprint. Newton stops when the
/// residual is below 1e-8, which can leave the point just outside the
/// footprint and turn a true contact away; the last step is taken anyway.
/// These points lost their contact (0.4 to 24 µm too low) before.
#[test]
fn flat_tool_rim_contacts_are_not_turned_away() {
    let body = sculpted(8, 3.0, 5);
    let g = QueryGeometry::new(&body, &BuildOptions::default());
    let c = Cutter::flat(6.0).unwrap();
    for (x, y) in [
        (1.8801872184682518, 14.772201211271422),
        (4.521782188777992, 12.6835457223462),
        (6.858063336360371, 6.341528395062466),
        (23.75588457508617, 1.235704689291329),
    ] {
        let mut st = SolveStats::default();
        let all = drop_all(&g, &c, x, y, &Seeding::default(), &mut st).unwrap();
        let got = drop(&g, &c, x, y, &DropOptions::default(), &mut st).unwrap();
        assert_real(&g, &c, Axis { x, y }, &got);
        assert!(
            got.tau >= all.tau - 1e-7,
            "({x}, {y}): {} < {}",
            got.tau,
            all.tau
        );
    }
}

/// Sub-boxes of a patch hold the patch, and lie inside its pole box.
#[test]
fn patch_sub_boxes_contain_the_patch() {
    let body = tiled(2, true);
    let g = QueryGeometry::new(&body, &BuildOptions::default());
    let mut rng = Lcg(5);
    let mut patches = 0;
    for i in 0..g.prims.len() as u32 {
        let Prim::Face { face, uv } = g.prims[i as usize] else {
            continue;
        };
        let s = &g.body.surfaces[g.body.faces[face as usize].surface as usize];
        if !matches!(s, Surface::BSpline(_)) {
            assert!(g.sub_boxes(i).is_empty());
            continue;
        }
        patches += 1;
        let subs = g.sub_boxes(i);
        assert!(!subs.is_empty());
        let outer = g.prim_box(i).expand(1e-4); // f32 rounding
        for b in subs {
            assert!(
                outer.contains(Point3::new(
                    f64::from(b.min_x),
                    f64::from(b.min_y),
                    outer.min.z()
                )) && outer.contains(Point3::new(
                    f64::from(b.max_x),
                    f64::from(b.max_y),
                    f64::from(b.max_z)
                )),
                "{b:?} {outer:?}"
            );
        }
        for _ in 0..200 {
            let p = s.eval(uv.u.at(rng.uniform()), uv.v.at(rng.uniform()));
            assert!(subs.iter().any(|b| b.holds(p, 1e-9)), "{p:?} in no sub-box");
        }
    }
    assert!(patches > 10);
}

/// Sub-boxes of a B-spline edge curve hold the curve.
#[test]
fn edge_sub_boxes_contain_the_curve() {
    let body = sculpted(2, 12.0, 9);
    let g = QueryGeometry::new(&body, &BuildOptions::default());
    let mut curves = 0;
    for i in 0..g.prims.len() as u32 {
        let Prim::Edge(e) = g.prims[i as usize] else {
            continue;
        };
        let edge = &g.body.edges[e as usize];
        let curve = &g.body.curves[edge.curve.unwrap() as usize];
        let subs = g.sub_boxes(i);
        if !matches!(curve, Curve3::BSpline(_)) {
            assert!(subs.is_empty());
            continue;
        }
        curves += 1;
        assert!(!subs.is_empty());
        for k in 0..=400 {
            let p = curve.eval(edge.range.at(k as f64 / 400.0));
            assert!(subs.iter().any(|b| b.holds(p, 1e-9)), "{p:?} in no sub-box");
        }
    }
    assert!(curves > 10);
}

/// Contacts a flat tool's patch solves miss beside a face boundary: the
/// contact is within 0.05 mm of the boundary, the patch holding it finds
/// nothing, and neighbouring patches, which the sub-box bounds rule out, used
/// to find it by chance. The patch is solved again with richer seeds.
#[test]
fn contacts_beside_a_face_boundary_are_not_missed() {
    let c = Cutter::flat(6.0).unwrap();
    let cases = [
        (sculpted(4, 12.0, 9), 14.998465258227126, 47.47672328041821),
        (tiled(4, true), 16.310242393116383, 42.75744430749068),
    ];
    for (body, x, y) in cases {
        let g = QueryGeometry::new(&body, &BuildOptions::default());
        let mut st = SolveStats::default();
        let all = drop_all(&g, &c, x, y, &Seeding::default(), &mut st).unwrap();
        let got = drop(&g, &c, x, y, &DropOptions::default(), &mut st).unwrap();
        assert_real(&g, &c, Axis { x, y }, &got);
        assert!(
            got.tau >= all.tau - 1e-7,
            "({x}, {y}): {} < {}",
            got.tau,
            all.tau
        );
    }
}

/// Drops on the rough tiles that missed a contact before the climbs of
/// the drop-misses plan: a flat tool's rim on a nearly level spot beside a
/// bump's top just outside the footprint (four chord ends of the finishing
/// bench), the rim on a face beside its corner, and a bull-nose's corner
/// just outside its flat beside a bump's top. Each drop must reach the
/// brute-force reference.
#[test]
fn tiles_rim_and_corner_contacts() {
    let body = tiled(8, true);
    let g = QueryGeometry::new(&body, &BuildOptions::default());
    let flat = Cutter::flat(6.0).unwrap();
    let bull = Cutter::new(6.0, 1.0).unwrap();
    let cases = [
        (flat, 32.26786365234263, 59.42595777017104),
        (flat, 69.90231388034083, 50.066774875282064),
        (flat, 12.213812205111996, 79.19874875353938),
        (flat, 33.90060941171048, 59.13640862192875),
        (flat, 49.7146763373046, 68.75708905731656),
        (bull, 43.285517305406515, 8.040800317618928),
        (bull, 68.42528399473194, 50.44182495342554),
    ];
    for (c, x, y) in cases {
        let mut st = SolveStats::default();
        let got = drop(&g, &c, x, y, &DropOptions::default(), &mut st).expect("contact");
        assert_real(&g, &c, Axis { x, y }, &got);
        let want = reference(&g, &c, Axis { x, y }, DENSE).expect("reference");
        assert!(
            got.tau >= want - 1e-7,
            "miss at ({x}, {y}) {c:?}: {} < {want}",
            got.tau
        );
    }
}
