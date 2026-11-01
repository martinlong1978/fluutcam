// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::brep::{Coedge, Edge, Face, Loop, Vertex};
use crate::pcurve;
use fluut_geom::{
    BSplineCurve3, BSplineSurface, Curve3, Frame, Interval, Point3, Surface, UnitVec3, Vec3,
};
use std::f64::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;

const TOL: f64 = 1e-6;

/// Builds one-face bodies whose loops are all inner (`outer` false), as a
/// face bounded only by `FACE_BOUND` would be before the outer loop is
/// chosen.
struct Build {
    b: BrepBody,
}

impl Build {
    fn new() -> Build {
        Build {
            b: BrepBody {
                tol_model: TOL,
                ..Default::default()
            },
        }
    }

    fn vertex(&mut self, p: Point3) -> u32 {
        match self.b.vertices.iter().position(|v| v.p.distance(p) < 1e-9) {
            Some(i) => i as u32,
            None => {
                self.b.vertices.push(Vertex { p });
                self.b.vertices.len() as u32 - 1
            }
        }
    }

    fn edge(&mut self, c: Curve3, lo: f64, hi: f64) -> u32 {
        let (v0, v1) = (self.vertex(c.eval(lo)), self.vertex(c.eval(hi)));
        self.b.curves.push(c);
        self.b.edges.push(Edge {
            curve: Some(self.b.curves.len() as u32 - 1),
            range: Interval::new(lo, hi).unwrap(),
            v0,
            v1,
        });
        self.b.edges.len() as u32 - 1
    }

    fn line(&mut self, a: [f64; 3], b: [f64; 3]) -> u32 {
        let (a, b) = (p3(a), p3(b));
        let len = a.distance(b);
        self.edge(Curve3::line(a, UnitVec3::new(b - a).unwrap()), 0.0, len)
    }

    /// A closed polygon of lines through `pts`.
    fn polygon(&mut self, pts: &[[f64; 3]]) -> Vec<(u32, bool)> {
        (0..pts.len())
            .map(|i| (self.line(pts[i], pts[(i + 1) % pts.len()]), true))
            .collect()
    }

    fn face(&mut self, s: Surface, loops: &[&[(u32, bool)]]) -> u32 {
        self.b.surfaces.push(s);
        let mut ls = Vec::new();
        for l in loops {
            let mut cs = Vec::new();
            for &(edge, forward) in *l {
                self.b.coedges.push(Coedge { edge, forward });
                cs.push(self.b.coedges.len() as u32 - 1);
            }
            self.b.loops.push(Loop {
                outer: false,
                coedges: cs,
            });
            ls.push(self.b.loops.len() as u32 - 1);
        }
        self.b.faces.push(Face {
            surface: self.b.surfaces.len() as u32 - 1,
            same_sense: true,
            loops: ls,
        });
        self.b.faces.len() as u32 - 1
    }

    /// Computes the p-curves and returns the choice for face `f`.
    fn outer(mut self, f: u32) -> (BrepBody, Outer) {
        let st = pcurve::compute(&mut self.b);
        assert!(st.failures.is_empty(), "{:?}", st.failures);
        let o = outer_loop(&self.b, f);
        (self.b, o)
    }
}

fn p3(a: [f64; 3]) -> Point3 {
    Point3::new(a[0], a[1], a[2])
}

fn frame(o: [f64; 3], z: [f64; 3], x: [f64; 3]) -> Frame {
    Frame::new(
        p3(o),
        Vec3::new(z[0], z[1], z[2]),
        Vec3::new(x[0], x[1], x[2]),
    )
    .unwrap()
}

fn up(o: [f64; 3]) -> Frame {
    frame(o, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])
}

fn square(x0: f64, y0: f64, w: f64) -> Vec<[f64; 3]> {
    vec![
        [x0, y0, 0.0],
        [x0 + w, y0, 0.0],
        [x0 + w, y0 + w, 0.0],
        [x0, y0 + w, 0.0],
    ]
}

fn reversed(mut l: Vec<(u32, bool)>) -> Vec<(u32, bool)> {
    l.reverse();
    for c in &mut l {
        c.1 = !c.1;
    }
    l
}

#[test]
fn winding_of_a_square() {
    let sq = [
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
        Point2::new(1.0, 1.0),
        Point2::new(0.0, 1.0),
    ];
    assert_eq!(winding(&sq, Point2::new(0.5, 0.5)), 1);
    assert_eq!(winding(&sq, Point2::new(1.5, 0.5)), 0);
    let rev: Vec<Point2> = sq.iter().rev().copied().collect();
    assert_eq!(winding(&rev, Point2::new(0.5, 0.5)), -1);
}

#[test]
fn plane_with_two_holes_listed_first() {
    let mut bd = Build::new();
    let h1 = bd.polygon(&square(1.0, 1.0, 2.0));
    let h2 = reversed(bd.polygon(&square(5.0, 5.0, 3.0)));
    let outer = bd.polygon(&square(0.0, 0.0, 10.0));
    let f = bd.face(Surface::plane(up([0.0; 3])), &[&h1, &h2, &outer]);
    let (b, o) = bd.outer(f);
    assert_eq!(o, Outer::Contains(2));
    let poly = loop_polyline(&b, 0, b.faces[0].loops[2]).unwrap();
    assert!(!poly.wraps);
    assert!(poly.points.len() >= 4);
}

/// Cylinder r = 5 round z, side face 0 ≤ z ≤ 10 with its seam at u = 0,
/// and a window 3 ≤ z ≤ 7, |u| ≤ 0.3 across the seam, listed first.
#[test]
fn cylinder_hole_across_the_seam() {
    let (r, h) = (5.0, 10.0);
    let mut bd = Build::new();
    let f0 = up([0.0; 3]);
    let c0 = bd.edge(Curve3::circle(f0, r).unwrap(), 0.0, TAU);
    let c1 = bd.edge(Curve3::circle(up([0.0, 0.0, h]), r).unwrap(), 0.0, TAU);
    let seam = bd.line([r, 0.0, 0.0], [r, 0.0, h]);
    let a = 0.3;
    let at = |u: f64, z: f64| [r * libm::cos(u), r * libm::sin(u), z];
    let lo = bd.edge(Curve3::circle(up([0.0, 0.0, 3.0]), r).unwrap(), -a, a);
    let hi = bd.edge(Curve3::circle(up([0.0, 0.0, 7.0]), r).unwrap(), -a, a);
    let right = bd.line(at(a, 3.0), at(a, 7.0));
    let left = bd.line(at(-a, 3.0), at(-a, 7.0));
    // Clockwise in (u, v), as a hole.
    let hole = [(lo, false), (left, true), (hi, true), (right, false)];
    let outer = [(c0, true), (seam, true), (c1, false), (seam, false)];
    let f = bd.face(Surface::cylinder(f0, r).unwrap(), &[&hole, &outer]);
    let (b, o) = bd.outer(f);
    assert_eq!(o, Outer::Contains(1));
    let poly = loop_polyline(&b, 0, b.faces[0].loops[1]).unwrap();
    assert!(!poly.wraps, "the seam closes the outer loop");
}

/// Cone band between two coaxial circles, with a small window: the circles
/// wrap round the axis, so the wider one is outer.
#[test]
fn cone_band_wraps() {
    let semi = 0.2f64;
    let f0 = up([0.0; 3]);
    let cone = Surface::cone(f0, 4.0, semi).unwrap();
    let r1 = 4.0 + 6.0 * libm::tan(semi);
    let mut bd = Build::new();
    let small = bd.edge(Curve3::circle(f0, 4.0).unwrap(), 0.0, TAU);
    let big = bd.edge(Curve3::circle(up([0.0, 0.0, 6.0]), r1).unwrap(), 0.0, TAU);
    let f = bd.face(cone, &[&[(small, true)], &[(big, false)]]);
    let (b, o) = bd.outer(f);
    assert_eq!(o, Outer::Wraps(1));
    for &l in &b.faces[0].loops {
        assert!(loop_polyline(&b, 0, l).unwrap().wraps);
    }
}

/// Sphere r = 10 below latitude 80°, with a round hole of angular radius
/// 30° on the equator at u = 0 (across the seam). The hole is far wider in
/// 3-D than the small circle round the pole, which is the outer loop.
#[test]
fn sphere_widest_loop_is_not_outer() {
    let r = 10.0;
    let lat = 80f64.to_radians();
    let sf = up([0.0; 3]);
    let mut bd = Build::new();
    let top = bd.edge(
        Curve3::circle(up([0.0, 0.0, r * libm::sin(lat)]), r * libm::cos(lat)).unwrap(),
        0.0,
        TAU,
    );
    let ang = 30f64.to_radians();
    let hf = frame(
        [r * libm::cos(ang), 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    );
    let hole = bd.edge(Curve3::circle(hf, r * libm::sin(ang)).unwrap(), 0.0, TAU);
    let f = bd.face(
        Surface::sphere(sf, r).unwrap(),
        &[&[(hole, false)], &[(top, false)]],
    );
    let (b, o) = bd.outer(f);
    assert_eq!(o, Outer::Wraps(1));
    let w = |l: u32| loop_width(&b, &b.loops[l as usize].coedges);
    assert!(w(b.faces[0].loops[0]) > 2.0 * w(b.faces[0].loops[1]));
    assert!(!loop_polyline(&b, 0, b.faces[0].loops[0]).unwrap().wraps);
}

/// A sphere lune between two meridians: the joints at the poles are free
/// in u, so the loop closes through them and does not wrap.
#[test]
fn lune_through_the_poles_is_closed() {
    let r = 10.0;
    let mut bd = Build::new();
    let south = -FRAC_PI_2;
    let m0 = frame([0.0; 3], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]);
    let m1 = frame([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let a = bd.edge(Curve3::circle(m0, r).unwrap(), south + TAU, FRAC_PI_2 + TAU);
    let c = bd.edge(Curve3::circle(m1, r).unwrap(), south + TAU, FRAC_PI_2 + TAU);
    let f = bd.face(
        Surface::sphere(up([0.0; 3]), r).unwrap(),
        &[&[(a, true), (c, false)]],
    );
    let (b, o) = bd.outer(f);
    assert_eq!(o, Outer::Contains(0));
    let poly = loop_polyline(&b, 0, b.faces[0].loops[0]).unwrap();
    assert!(!poly.wraps);
}

#[test]
fn missing_pcurve_is_undecided() {
    let mut bd = Build::new();
    let h = bd.polygon(&square(1.0, 1.0, 2.0));
    let outer = bd.polygon(&square(0.0, 0.0, 10.0));
    let f = bd.face(Surface::plane(up([0.0; 3])), &[&h, &outer]);
    let (mut b, o) = bd.outer(f);
    assert_eq!(o, Outer::Contains(1));
    b.pcurves[0] = None;
    assert_eq!(outer_loop(&b, f), Outer::Undecided);
    assert!(loop_polyline(&b, 0, b.faces[0].loops[0]).is_none());
}

#[test]
fn two_separate_loops_are_undecided() {
    let mut bd = Build::new();
    let a = bd.polygon(&square(0.0, 0.0, 2.0));
    let c = bd.polygon(&square(5.0, 0.0, 2.0));
    let f = bd.face(Surface::plane(up([0.0; 3])), &[&a, &c]);
    let (_, o) = bd.outer(f);
    assert_eq!(o, Outer::Undecided);
}

/// A square tube as a B-spline surface that is closed in v but not
/// periodic, widening from half-size 2 at z = 0 to 4 at z = 10. Its seam
/// v = 0 runs along the corner (s, s).
fn square_tube() -> Surface {
    let ring = |s: f64, z: f64| {
        [(s, s), (-s, s), (-s, -s), (s, -s), (s, s)].map(|(x, y)| Point3::new(x, y, z))
    };
    let mut poles = ring(2.0, 0.0).to_vec();
    poles.extend(ring(4.0, 10.0));
    let s = BSplineSurface::new(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0],
        poles,
        2,
        5,
        None,
    )
    .unwrap();
    Surface::BSpline(Arc::new(s))
}

/// The tube's cross-section at height `z`, starting mid-side so that it
/// crosses the seam.
fn tube_ring(z: f64) -> Curve3 {
    let s = 2.0 + 0.2 * z;
    let pts = [(s, 0.0), (s, s), (-s, s), (-s, -s), (s, -s), (s, 0.0)]
        .map(|(x, y)| Point3::new(x, y, z))
        .to_vec();
    let knots = vec![0.0, 0.0, 0.125, 0.375, 0.625, 0.875, 1.0, 1.0];
    Curve3::BSpline(Arc::new(BSplineCurve3::new(1, knots, pts, None).unwrap()))
}

/// A band of the tube between two rings that cross the seam: their
/// p-curves jump back across it inside the domain, and the polylines must
/// still be seen to wrap.
#[test]
fn closed_bspline_band_across_the_seam_wraps() {
    let mut bd = Build::new();
    let lo = bd.edge(tube_ring(2.5), 0.0, 1.0);
    let hi = bd.edge(tube_ring(7.5), 0.0, 1.0);
    let f = bd.face(square_tube(), &[&[(lo, true)], &[(hi, false)]]);
    let (b, o) = bd.outer(f);
    for &l in &b.faces[0].loops {
        let poly = loop_polyline(&b, 0, l).unwrap();
        assert!(poly.wraps, "{:?}", poly.points);
    }
    assert_eq!(o, Outer::Wraps(1));
}
