// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::path2::seg_closest;
use crate::region::{Loop2, Vertex};
use proptest::prelude::*;
use std::f64::consts::PI;

fn close(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}

fn pt(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn line(ax: f64, ay: f64, bx: f64, by: f64) -> Geom {
    let d = pt(bx, by) - pt(ax, ay);
    let len = d.length();
    Geom::Line {
        a: pt(ax, ay),
        t: d * (1.0 / len),
        len,
    }
}

fn region_of(loops: Vec<Vec<(f64, f64, f64)>>) -> Region2 {
    let mut r = Region2::empty();
    for (i, l) in loops.into_iter().enumerate() {
        let lp = Loop2 {
            verts: l
                .into_iter()
                .map(|(x, y, b)| Vertex::new(x, y, b))
                .collect(),
        };
        // First loop outer (CCW), others holes (CW).
        let ccw = lp.is_ccw();
        let lp = if (i == 0) == ccw { lp } else { lp.reversed() };
        r.loops.push(lp);
    }
    r
}

// ── sites and bisectors ─────────────────────────────────────────────────

#[test]
fn sites_of_a_polygon_with_a_reflex_vertex() {
    // L-shape: one reflex vertex at (10, 10).
    let r = Region2::polygon(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 10.0),
        (10.0, 10.0),
        (10.0, 20.0),
        (0.0, 20.0),
    ]);
    let s = build_sites(&r, 1e-8);
    assert_eq!(s.len(), 7);
    let points: Vec<&Site> = s
        .iter()
        .filter(|x| matches!(x.geom, Geom::Circle { r, .. } if r == 0.0))
        .collect();
    assert_eq!(points.len(), 1);
    let Geom::Circle { c, sweep, .. } = points[0].geom else {
        unreachable!()
    };
    assert_eq!(c, pt(10.0, 10.0));
    assert!(close(sweep, -PI / 2.0, 1e-12));
    // The point is linked between its two segments; convex corners are not.
    let pi = s.iter().position(|x| x == points[0]).unwrap();
    let (a, b) = (points[0].prev.unwrap(), points[0].next.unwrap());
    assert_eq!(s[a].next, Some(pi));
    assert_eq!(s[b].prev, Some(pi));
    assert_eq!(s.iter().filter(|x| x.next.is_none()).count(), 5);
}

#[test]
fn tangent_joints_link_sites() {
    // Stadium: two lines and two half circles, all joints tangent.
    let r = region_of(vec![vec![
        (0.0, 0.0, 0.0),
        (10.0, 0.0, 1.0),
        (10.0, 4.0, 0.0),
        (0.0, 4.0, 1.0),
    ]]);
    let s = build_sites(&r, 1e-8);
    assert_eq!(s.len(), 4);
    assert!(s.iter().all(|x| x.next.is_some() && x.prev.is_some()));
    assert!(s[1].geom.is_concave());
}

#[test]
fn site_parameters_feet_and_distances() {
    let l = line(0.0, 0.0, 10.0, 0.0);
    assert_eq!(l.param(pt(3.0, 2.0)), 3.0);
    assert_eq!(l.dist(pt(3.0, 2.0)), 2.0);
    assert_eq!(l.cone_dist(pt(-1.0, 2.0), 1e-9), None);
    // A concave quarter arc of radius 5 about the origin.
    let a = Geom::Circle {
        c: pt(0.0, 0.0),
        r: 5.0,
        start: 0.0,
        sweep: PI / 2.0,
    };
    assert!(close(a.param(pt(1.0, 1.0)), PI / 4.0, 1e-15));
    assert!(close(a.dist(pt(1.0, 1.0)), 5.0 - 2f64.sqrt(), 1e-15));
    assert_eq!(a.cone_dist(pt(0.0, 0.0), 1e-9), Some(5.0));
    assert!(a.param(pt(1.0, -0.01)) < 0.0);
    // A reflex point with its cone from +y clockwise to +x.
    let p = Geom::Circle {
        c: pt(0.0, 0.0),
        r: 0.0,
        start: PI / 2.0,
        sweep: -PI / 2.0,
    };
    assert!(close(
        p.cone_dist(pt(1.0, 1.0), 1e-9).unwrap(),
        2f64.sqrt(),
        1e-15
    ));
    assert_eq!(p.cone_dist(pt(-1.0, 1.0), 1e-9), None);
    assert_eq!(p.normal(0.0).y(), 1.0);
}

#[test]
fn bisector_points_are_equidistant_for_every_pair_of_kinds() {
    let sites = [
        line(0.0, 0.0, 10.0, 0.0),
        line(10.0, 8.0, 0.0, 8.0),
        line(0.0, 9.0, 0.0, 0.0),
        Geom::Circle {
            c: pt(5.0, 3.0),
            r: 6.0,
            start: -0.4,
            sweep: 2.0,
        },
        Geom::Circle {
            c: pt(4.0, 4.0),
            r: 1.0,
            start: 1.0,
            sweep: -2.5,
        },
        Geom::Circle {
            c: pt(6.0, 5.0),
            r: 0.0,
            start: 0.0,
            sweep: -1.5,
        },
    ];
    let mut n = 0;
    for (i, a) in sites.iter().enumerate() {
        for (j, b) in sites.iter().enumerate() {
            if i == j {
                continue;
            }
            for k in 0..=8 {
                let u = a.len() * k as f64 / 8.0;
                if let Some((p, rho)) = a.bisector_at(b, u, 1e-9) {
                    assert!(close(a.dist(p), rho, 1e-9), "{i} {j} {u}");
                    assert!(close(b.dist(p), rho, 1e-9), "{i} {j} {u}");
                    assert!(close(a.param(p), u, 1e-9), "{i} {j} {u}");
                    n += 1;
                }
            }
        }
    }
    assert!(n > 100, "{n}");
}

#[test]
fn conic_kinds() {
    let ax = |l: Geom, r: Geom| MedialAxis {
        sites: [l, r]
            .iter()
            .map(|g| Site {
                geom: *g,
                loop_ix: 0,
                next: None,
                prev: None,
                bbox: (pt(0.0, 0.0), pt(0.0, 0.0)),
            })
            .collect(),
        ..MedialAxis::default()
    };
    let e = Edge {
        left: 0,
        right: 1,
        u0: 0.0,
        u1: 1.0,
        a: 0,
        b: 0,
    };
    let c = |x: f64, r: f64, sweep: f64| Geom::Circle {
        c: pt(x, 0.0),
        r,
        start: 0.0,
        sweep,
    };
    let l = line(0.0, 0.0, 1.0, 0.0);
    assert_eq!(ax(l, l).conic(&e), Conic::Line);
    assert_eq!(ax(l, c(0.0, 1.0, 1.0)).conic(&e), Conic::Parabola);
    assert_eq!(ax(l, c(0.0, 0.0, -1.0)).conic(&e), Conic::Parabola);
    assert_eq!(
        ax(c(0.0, 0.0, -1.0), c(3.0, 0.0, -1.0)).conic(&e),
        Conic::Line
    );
    assert_eq!(
        ax(c(0.0, 1.0, -1.0), c(5.0, 2.0, -1.0)).conic(&e),
        Conic::Hyperbola
    );
    assert_eq!(
        ax(c(0.0, 9.0, 1.0), c(1.0, 2.0, -1.0)).conic(&e),
        Conic::Ellipse
    );
    assert_eq!(
        ax(c(0.0, 9.0, 1.0), c(0.0, 2.0, -1.0)).conic(&e),
        Conic::Ellipse
    );
}

#[test]
fn three_site_vertices() {
    // Incircle of the 3-4-5 triangle: centre (1, 1), radius 1.
    let a = line(0.0, 0.0, 4.0, 0.0);
    let b = line(4.0, 0.0, 0.0, 3.0);
    let c = line(0.0, 3.0, 0.0, 0.0);
    let v = equidistant([&a, &b, &c], pt(2.0, 2.0), 1e-9);
    assert_eq!(v.len(), 1);
    assert!(v[0].0.distance(pt(1.0, 1.0)) < 1e-14 && close(v[0].1, 1.0, 1e-14));
    // Two lines and a concave arc between them: the arc centre, a double
    // root.
    let top = line(10.0, 1.0, 0.0, 1.0);
    let bot = line(0.0, -1.0, 10.0, -1.0);
    let arc = Geom::Circle {
        c: pt(10.0, 0.0),
        r: 1.0,
        start: -PI / 2.0,
        sweep: PI,
    };
    let v = equidistant([&top, &bot, &arc], pt(3.0, 0.0), 1e-8);
    assert!(
        v.iter()
            .any(|q| q.0.distance(pt(10.0, 0.0)) < 1e-7 && close(q.1, 1.0, 1e-8)),
        "{v:?}"
    );
    // A line, a convex circle and a point: check the equations.
    let p = Geom::Circle {
        c: pt(2.0, 5.0),
        r: 0.0,
        start: 0.0,
        sweep: -1.0,
    };
    let hole = Geom::Circle {
        c: pt(7.0, 4.0),
        r: 1.5,
        start: 0.0,
        sweep: -1.0,
    };
    let v = equidistant([&a, &p, &hole], pt(0.0, 0.0), 1e-9);
    assert!(!v.is_empty());
    for (q, rho) in v {
        for g in [&a, &p, &hole] {
            assert!(close(g.dist(q), rho, 1e-12));
        }
    }
}

// ── whole axes ──────────────────────────────────────────────────────────

/// Nearest boundary point of `r` to `p`.
fn nearest(r: &Region2, p: Point2) -> Point2 {
    r.segs()
        .map(|s| s.point_at(seg_closest(&s, p)))
        .min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))
        .unwrap()
}

/// Points of the axis: every edge sampled finely, and every node.
fn axis_points(m: &MedialAxis) -> Vec<Point2> {
    let mut out: Vec<Point2> = m.nodes.iter().map(|n| n.p).collect();
    for e in &m.edges {
        for u in m.sample(e, 1e-4, 1.0) {
            out.push(m.eval(e, u).0);
        }
    }
    out
}

fn dist_to_axis(pts: &[Point2], m: &MedialAxis, q: Point2) -> f64 {
    // Points plus the chords between consecutive edge samples.
    let mut best = pts
        .iter()
        .map(|p| p.distance(q))
        .fold(f64::INFINITY, f64::min);
    for e in &m.edges {
        let us = m.sample(e, 1e-4, 1.0);
        for w in us.windows(2) {
            let (a, b) = (m.eval(e, w[0]).0, m.eval(e, w[1]).0);
            let s = Seg2 { a, b, bulge: 0.0 };
            best = best.min(crate::region::seg_distance(&s, q));
        }
    }
    best
}

/// Checks the axis properties on `r` and returns the axis.
fn check(r: &Region2, holes: usize, areas: usize) -> MedialAxis {
    let m = medial_axis(r).unwrap();
    let (lo, hi) = r.bbox();
    let scale = (hi.x() - lo.x()).max(hi.y() - lo.y());
    let eps = 1e-7 * scale.max(1.0);
    // 1. Nodes: clearance is the boundary distance, at least two feet
    //    unless a leaf.
    for (i, n) in m.nodes.iter().enumerate() {
        let d = r.boundary_distance(n.p);
        assert!(
            close(d, n.radius, eps),
            "node {i} {:?}: {d} vs {}",
            n.p,
            n.radius
        );
        assert!(
            r.contains(n.p) != crate::region::Inside::Out,
            "node {i} outside"
        );
    }
    // 2. Edge points: equidistant from two distinct feet, no boundary
    //    point nearer.
    for (k, e) in m.edges.iter().enumerate() {
        assert!(e.u1 > e.u0, "edge {k}: {e:?}");
        for j in 1..8 {
            let u = e.u0 + (e.u1 - e.u0) * j as f64 / 8.0;
            let (p, rho) = m.eval(e, u);
            let d = r.boundary_distance(p);
            assert!(close(d, rho, eps), "edge {k} at {u}: {:?} {d} vs {rho}", p);
            let (fl, fr) = m.feet(e, u);
            assert!(close(fl.distance(p), rho, eps), "edge {k}: left foot");
            assert!(close(fr.distance(p), rho, eps), "edge {k}: right foot");
            assert!(
                fl.distance(fr) > 1e-9 * scale || rho < 1e3 * eps,
                "edge {k}: one foot"
            );
        }
    }
    // 3. Structure: E − V + components = holes.
    let mut parent: Vec<usize> = (0..m.nodes.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for e in &m.edges {
        let (a, b) = (find(&mut parent, e.a), find(&mut parent, e.b));
        parent[a] = b;
    }
    let comps = (0..m.nodes.len())
        .filter(|&i| find(&mut parent, i) == i)
        .count();
    assert_eq!(comps, areas, "connected components");
    assert_eq!(
        m.edges.len() as i64 - m.nodes.len() as i64 + comps as i64,
        holes as i64,
        "cycles: {} edges, {} nodes",
        m.edges.len(),
        m.nodes.len()
    );
    // 4. Completeness: the centre of the largest empty disc along the ray
    //    from the foot of an interior point lies on the axis.
    let pts = axis_points(&m);
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut tested = 0;
    for _ in 0..400 {
        let q = pt(
            lo.x() + (hi.x() - lo.x()) * rnd(),
            lo.y() + (hi.y() - lo.y()) * rnd(),
        );
        if r.contains(q) != crate::region::Inside::In || r.boundary_distance(q) < 1e-3 * scale {
            continue;
        }
        let f = nearest(r, q);
        let d = (q - f) * (1.0 / q.distance(f));
        let (mut a, mut b) = (q.distance(f), 2.0 * scale);
        for _ in 0..80 {
            let t = 0.5 * (a + b);
            if r.boundary_distance(f + d * t) >= t - 1e-9 * scale {
                a = t;
            } else {
                b = t;
            }
        }
        let c = f + d * a;
        let gap = dist_to_axis(&pts, &m, c);
        assert!(gap < 1e-4 * scale.max(1.0), "{c:?} is {gap} from the axis");
        tested += 1;
        if tested == 60 {
            break;
        }
    }
    m
}

fn leaves(m: &MedialAxis) -> usize {
    m.degrees().iter().filter(|&&d| d == 1).count()
}

#[test]
fn rectangle() {
    let r = Region2::rect(0.0, 0.0, 20.0, 10.0);
    let m = check(&r, 0, 1);
    assert_eq!(leaves(&m), 4);
    let max = m.nodes.iter().map(|n| n.radius).fold(0.0, f64::max);
    assert!(close(max, 5.0, 1e-12));
    let d = m.degrees();
    let branch: Vec<Point2> = (0..m.nodes.len())
        .filter(|&i| d[i] == 3)
        .map(|i| m.nodes[i].p)
        .collect();
    assert_eq!(branch.len(), 2);
    assert!(branch.iter().any(|p| p.distance(pt(5.0, 5.0)) < 1e-12));
    assert!(branch.iter().any(|p| p.distance(pt(15.0, 5.0)) < 1e-12));
    assert!(m.edges.iter().all(|e| m.conic(e) == Conic::Line));
}

#[test]
fn square_has_one_degree_four_node() {
    let m = check(&Region2::rect(0.0, 0.0, 10.0, 10.0), 0, 1);
    let d = m.degrees();
    assert_eq!(d.iter().filter(|&&x| x == 4).count(), 1);
    assert_eq!(leaves(&m), 4);
    assert_eq!(m.edges.len(), 4);
}

#[test]
fn nearly_square_rectangles() {
    for h in [10.0 + 1e-9, 10.0 + 1e-6, 10.0 - 1e-7] {
        let m = check(&Region2::rect(0.0, 0.0, 10.0, h), 0, 1);
        assert_eq!(leaves(&m), 4);
    }
}

#[test]
fn triangle_and_l_shape() {
    let m = check(
        &Region2::polygon(&[(0.0, 0.0), (4.0, 0.0), (0.0, 3.0)]),
        0,
        1,
    );
    assert_eq!(leaves(&m), 3);
    assert!(m.nodes.iter().any(|n| n.p.distance(pt(1.0, 1.0)) < 1e-12));
    let l = Region2::polygon(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 10.0),
        (10.0, 10.0),
        (10.0, 20.0),
        (0.0, 20.0),
    ]);
    let m = check(&l, 0, 1);
    assert!(m.edges.iter().any(|e| m.conic(e) == Conic::Parabola));
}

#[test]
fn disc_is_one_point() {
    let m = check(&Region2::circle(3.0, 4.0, 5.0), 0, 1);
    assert_eq!(m.nodes.len(), 1);
    assert!(m.edges.is_empty());
    assert!(m.nodes[0].p.distance(pt(3.0, 4.0)) < 1e-12);
    assert!(close(m.nodes[0].radius, 5.0, 1e-12));
}

#[test]
fn annuli() {
    let r = Region2::circle(0.0, 0.0, 10.0).with_hole(&Region2::circle(0.0, 0.0, 4.0));
    let m = check(&r, 1, 1);
    assert!(m.edges.iter().all(|e| m.conic(e) == Conic::Ellipse));
    assert!(m.nodes.iter().all(|n| close(n.radius, 3.0, 1e-9)));
    let r = Region2::circle(0.0, 0.0, 10.0).with_hole(&Region2::circle(2.0, 1.0, 3.0));
    check(&r, 1, 1);
}

#[test]
fn rectangle_with_round_hole() {
    let r = Region2::rect(0.0, 0.0, 30.0, 20.0).with_hole(&Region2::circle(12.0, 9.0, 4.0));
    let m = check(&r, 1, 1);
    assert!(m.edges.iter().any(|e| m.conic(e) == Conic::Parabola));
    let r = Region2::rect(0.0, 0.0, 30.0, 20.0)
        .with_hole(&Region2::rect(5.0, 5.0, 10.0, 15.0))
        .with_hole(&Region2::circle(20.0, 10.0, 3.0));
    check(&r, 2, 1);
}

#[test]
fn stadium_and_rounded_rectangle() {
    let st = region_of(vec![vec![
        (0.0, 0.0, 0.0),
        (10.0, 0.0, 1.0),
        (10.0, 4.0, 0.0),
        (0.0, 4.0, 1.0),
    ]]);
    let m = check(&st, 0, 1);
    // One segment between the two arc centres.
    assert_eq!(
        m.edges.iter().filter(|e| m.conic(e) == Conic::Line).count(),
        m.edges.len()
    );
    let ends: Vec<Point2> = (0..m.nodes.len())
        .filter(|&i| m.degrees()[i] == 1)
        .map(|i| m.nodes[i].p)
        .collect();
    assert_eq!(ends.len(), 2);
    assert!(ends.iter().any(|p| p.distance(pt(0.0, 2.0)) < 1e-6));
    assert!(ends.iter().any(|p| p.distance(pt(10.0, 2.0)) < 1e-6));
    // Rounded rectangle 20 × 10, corner radius 2.
    let b = libm::tan(PI / 8.0);
    let rr = region_of(vec![vec![
        (2.0, 0.0, 0.0),
        (18.0, 0.0, b),
        (20.0, 2.0, 0.0),
        (20.0, 8.0, b),
        (18.0, 10.0, 0.0),
        (2.0, 10.0, b),
        (0.0, 8.0, 0.0),
        (0.0, 2.0, b),
    ]]);
    let m = check(&rr, 0, 1);
    assert_eq!(leaves(&m), 4);
}

#[test]
fn touching_loops() {
    // Two squares sharing a corner: two areas.
    let mut r = Region2::rect(0.0, 0.0, 10.0, 10.0);
    r.loops.extend(Region2::rect(10.0, 10.0, 20.0, 20.0).loops);
    check(&r, 0, 2);
    // A triangular hole whose apex touches the outer bottom edge.
    let outer = Region2::rect(0.0, 0.0, 20.0, 12.0);
    let hole = Region2::polygon(&[(10.0, 0.0), (14.0, 6.0), (6.0, 6.0)]);
    let r = outer.with_hole(&hole);
    // The hole touches the boundary, so no loop goes round it: the axis is
    // a tree with two leaves at the touching point.
    let m = check(&r, 0, 1);
    let at = m
        .nodes
        .iter()
        .filter(|n| n.p.distance(pt(10.0, 0.0)) < 1e-9)
        .count();
    assert_eq!(at, 2);
}

#[test]
fn nearly_tangent_joints() {
    // A polygon approximating a circle with tiny corners, and a joint that
    // turns by 1e-10.
    let n = 64;
    let pts: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let a = TAU * i as f64 / n as f64;
            (10.0 * libm::cos(a), 10.0 * libm::sin(a))
        })
        .collect();
    let m = check(&Region2::polygon(&pts), 0, 1);
    assert_eq!(leaves(&m), n);
    let r = Region2::polygon(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (20.0, 1e-9),
        (20.0, 10.0),
        (0.0, 10.0),
    ]);
    check(&r, 0, 1);
}

/// A lettering-like outline: a curved stroke with round ends, a bowl with
/// a hole, and a serifed stem.
pub(crate) fn letters() -> Region2 {
    let b = libm::tan(PI / 8.0);
    // ")" stroke: two concentric half circles joined by round ends.
    let s = region_of(vec![vec![
        (0.0, -6.0, 1.0),
        (0.0, 6.0, 1.0),
        (0.0, 4.0, -1.0),
        (0.0, -4.0, 1.0),
    ]]);
    // "O": ellipse-like bowl from four arcs, with a hole.
    let o = region_of(vec![
        vec![
            (14.0, 0.0, b),
            (18.0, 6.0, b),
            (14.0, 12.0, b),
            (10.0, 6.0, b),
        ],
        vec![
            (14.0, 2.0, b),
            (16.0, 6.0, b),
            (14.0, 10.0, b),
            (12.0, 6.0, b),
        ],
    ]);
    // "I" with serifs.
    let i = Region2::polygon(&[
        (21.0, 0.0),
        (27.0, 0.0),
        (27.0, 1.0),
        (25.0, 1.5),
        (25.0, 10.5),
        (27.0, 11.0),
        (27.0, 12.0),
        (21.0, 12.0),
        (21.0, 11.0),
        (23.0, 10.5),
        (23.0, 1.5),
        (21.0, 1.0),
    ]);
    let mut r = s;
    r.loops.extend(o.loops);
    r.loops.extend(i.loops);
    r
}

#[test]
fn regular_polygons_have_one_centre() {
    for n in 3..=12 {
        let pts: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let a = TAU * i as f64 / n as f64 + 0.3;
                (7.0 + 5.0 * libm::cos(a), -2.0 + 5.0 * libm::sin(a))
            })
            .collect();
        let m = check(&Region2::polygon(&pts), 0, 1);
        assert_eq!(leaves(&m), n, "{n}");
        assert_eq!(m.degrees().iter().filter(|&&d| d == n).count(), 1, "{n}");
    }
}

fn shifted(r: &Region2, dx: f64, dy: f64) -> Region2 {
    Region2 {
        loops: r
            .loops
            .iter()
            .map(|l| Loop2 {
                verts: l
                    .verts
                    .iter()
                    .map(|v| Vertex::new(v.p.x() + dx, v.p.y() + dy, v.bulge))
                    .collect(),
            })
            .collect(),
    }
}

#[test]
fn far_from_the_origin() {
    let r = shifted(&letters(), 1234.5678, -987.654);
    let m = check(&r, 1, 3);
    let m0 = medial_axis(&letters()).unwrap();
    assert_eq!(m.nodes.len(), m0.nodes.len());
    assert_eq!(m.edges.len(), m0.edges.len());
    for (a, b) in m.nodes.iter().zip(&m0.nodes) {
        assert!(a.p.distance(b.p + Vec2::new(1234.5678, -987.654)) < 1e-9);
        assert!(close(a.radius, b.radius, 1e-9));
    }
}

#[test]
fn lettering() {
    let r = letters();
    check(&r, 1, 3);
}

#[test]
fn axis_is_bit_identical() {
    let m = medial_axis(&letters()).unwrap();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |v: f64| {
        for b in v.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    };
    for n in &m.nodes {
        eat(n.p.x());
        eat(n.p.y());
        eat(n.radius);
    }
    for e in &m.edges {
        eat(e.u0);
        eat(e.u1);
        eat(e.left as f64);
        eat(e.right as f64);
    }
    assert_eq!(
        (m.nodes.len(), m.edges.len(), h),
        (41, 39, 10518139422119060752)
    );
}

/// Whether no loop of `r` meets itself and no joint folds back.
fn simple(r: &Region2) -> bool {
    r.loops.iter().all(|l| {
        let path = crate::path2::Path2::from_loop(l);
        let n = path.segs.len();
        let folds = (0..n).any(|i| {
            let t_in = seg_tangent(&path.segs[(i + n - 1) % n], 1.0);
            let t_out = seg_tangent(&path.segs[i], 0.0);
            libm::atan2(t_in.perp_dot(t_out), t_in.dot(t_out)).abs() > PI - 0.05
        });
        let chain = crate::chain::Chain {
            path,
            z: fluut_geom::Interval { lo: 0.0, hi: 0.0 },
        };
        !folds && !chain.self_intersects()
    })
}

fn star(radii: &[f64], bulges: &[f64]) -> Region2 {
    let n = radii.len();
    let verts: Vec<(f64, f64, f64)> = (0..n)
        .map(|i| {
            let a = TAU * i as f64 / n as f64;
            (radii[i] * libm::cos(a), radii[i] * libm::sin(a), bulges[i])
        })
        .collect();
    region_of(vec![verts])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn random_polygons(radii in prop::collection::vec(4.0f64..10.0, 3..12)) {
        let b = vec![0.0; radii.len()];
        check(&star(&radii, &b), 0, 1);
    }

    #[test]
    fn random_arc_shapes(
        radii in prop::collection::vec(5.0f64..10.0, 3..10),
        bulges in prop::collection::vec(-0.25f64..0.25, 10),
    ) {
        let b: Vec<f64> = bulges[..radii.len()].to_vec();
        let r = star(&radii, &b);
        prop_assume!(simple(&r));
        check(&r, 0, 1);
    }

    #[test]
    fn random_arc_shapes_with_holes(
        radii in prop::collection::vec(7.0f64..10.0, 4..9),
        bulges in prop::collection::vec(-0.2f64..0.2, 9),
        hole in prop::collection::vec(1.0f64..2.5, 3..7),
        hb in prop::collection::vec(-0.3f64..0.3, 7),
        hx in -1.0f64..1.0,
    ) {
        let outer = star(&radii, &bulges[..radii.len()]);
        let inner = shifted(&star(&hole, &hb[..hole.len()]), hx, 0.5 * hx);
        prop_assume!(simple(&outer) && simple(&inner));
        let r = outer.with_hole(&inner);
        check(&r, 1, 1);
    }

    #[test]
    fn random_polygons_with_a_hole(
        radii in prop::collection::vec(8.0f64..10.0, 5..10),
        hx in -1.5f64..1.5,
        hy in -1.5f64..1.5,
        hr in 0.5f64..3.0,
    ) {
        let b = vec![0.0; radii.len()];
        let r = star(&radii, &b).with_hole(&Region2::circle(hx, hy, hr));
        check(&r, 1, 1);
    }
}

#[test]
fn s3_shapes() {
    for (name, r) in crate::spike::shapes::all() {
        let holes = r.loops.iter().filter(|l| !l.is_ccw()).count();
        let areas = r.loops.len() - holes;
        let m = check(&r, holes, areas);
        assert!(!m.edges.is_empty(), "{name}");
    }
}
