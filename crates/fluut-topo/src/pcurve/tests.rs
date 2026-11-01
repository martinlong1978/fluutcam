// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::brep::{Coedge, Edge, Face, Loop, Vertex};
use fluut_geom::{BSplineCurve3, BSplineSurface, UnitVec3};

const TOL: f64 = 1e-6;

/// Builds one-face bodies.
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

    fn vert(&mut self, p: Point3) -> u32 {
        self.b.vertices.push(Vertex { p });
        self.b.vertices.len() as u32 - 1
    }

    /// An edge over `[lo, hi]` of `c`, with vertices at its ends (shared
    /// when the curve is closed over the range).
    fn edge(&mut self, c: Curve3, lo: f64, hi: f64) -> u32 {
        let (p0, p1) = (c.eval(lo), c.eval(hi));
        let v0 = self.find_or_add(p0);
        let v1 = self.find_or_add(p1);
        self.b.curves.push(c);
        self.b.edges.push(Edge {
            curve: Some(self.b.curves.len() as u32 - 1),
            range: Interval::new(lo, hi).unwrap(),
            v0,
            v1,
        });
        self.b.edges.len() as u32 - 1
    }

    fn find_or_add(&mut self, p: Point3) -> u32 {
        match self.b.vertices.iter().position(|v| v.p.distance(p) < 1e-9) {
            Some(i) => i as u32,
            None => self.vert(p),
        }
    }

    fn face(&mut self, s: Surface, loops: &[&[(u32, bool)]]) -> u32 {
        self.b.surfaces.push(s);
        let mut ls = Vec::new();
        for (k, l) in loops.iter().enumerate() {
            let mut cs = Vec::new();
            for &(edge, forward) in *l {
                self.b.coedges.push(Coedge { edge, forward });
                cs.push(self.b.coedges.len() as u32 - 1);
            }
            self.b.loops.push(Loop {
                outer: k == 0,
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
}

fn frame(o: [f64; 3], z: [f64; 3]) -> Frame {
    let zv = Vec3::new(z[0], z[1], z[2]);
    let x = if z[0].abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    Frame::new(Point3::new(o[0], o[1], o[2]), zv, x).unwrap()
}

fn unit(x: f64, y: f64, z: f64) -> UnitVec3 {
    UnitVec3::new(Vec3::new(x, y, z)).unwrap()
}

fn start_end(b: &BrepBody, c: u32) -> (Point2, Point2) {
    let ce = &b.coedges[c as usize];
    let r = b.edges[ce.edge as usize].range;
    let pc = b.pcurves[c as usize].as_ref().unwrap();
    let (p, q) = (pc.eval(r.lo), pc.eval(r.hi));
    if ce.forward { (p, q) } else { (q, p) }
}

fn all_within(st: &Stats) {
    assert!(st.failures.is_empty(), "{:?}", st.failures);
    assert_eq!(st.within_tol, st.coedges);
}

/// Cylinder side face: bottom circle, seam up, top circle back, seam down.
fn cylinder_side(seam_as_spline: bool) -> (BrepBody, Stats) {
    let (r, h) = (5.0, 10.0);
    let mut bd = Build::new();
    let f0 = frame([0.0; 3], [0.0, 0.0, 1.0]);
    let f1 = frame([0.0, 0.0, h], [0.0, 0.0, 1.0]);
    let c0 = bd.edge(Curve3::circle(f0, r).unwrap(), 0.0, TAU);
    let c1 = bd.edge(Curve3::circle(f1, r).unwrap(), 0.0, TAU);
    let seam = if seam_as_spline {
        let bs = BSplineCurve3::new(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(r, 0.0, 0.0), Point3::new(r, 0.0, h)],
            None,
        )
        .unwrap();
        bd.edge(Curve3::BSpline(Arc::new(bs)), 0.0, 1.0)
    } else {
        bd.edge(
            Curve3::line(Point3::new(r, 0.0, 0.0), unit(0.0, 0.0, 1.0)),
            0.0,
            h,
        )
    };
    bd.face(
        Surface::cylinder(f0, r).unwrap(),
        &[&[(c0, true), (seam, true), (c1, false), (seam, false)]],
    );
    let st = compute(&mut bd.b);
    (bd.b, st)
}

#[test]
fn cylinder_seam_pcurves_are_one_period_apart() {
    for spline in [false, true] {
        let (b, st) = cylinder_side(spline);
        all_within(&st);
        assert_eq!(st.exact + st.sampled, 4);
        if !spline {
            assert_eq!(st.exact, 4);
        }
        let (s1, e1) = start_end(&b, 1);
        let (s3, e3) = start_end(&b, 3);
        assert!((s1.x() - s3.x()).abs() > 6.0, "{s1:?} {s3:?}");
        assert!(((s1.x() - s3.x()).abs() - TAU).abs() < 1e-9);
        // Loop closes in uv.
        for i in 0..4u32 {
            let (_, e) = start_end(&b, i);
            let (s, _) = start_end(&b, (i + 1) % 4);
            assert!(e.distance(s) < 1e-9, "joint {i}: {e:?} {s:?}");
        }
        assert!((e1.y() - 10.0).abs() < 1e-9 && (e3.y()).abs() < 1e-9);
        // The base loop starts in [0, 2π).
        let (s0, e0) = start_end(&b, 0);
        assert!(s0.x() >= -1e-9 && s0.x() < TAU, "{s0:?}");
        assert!((e0.x() - s0.x() - TAU).abs() < 1e-9);
    }
}

#[test]
fn sphere_cap_near_the_pole() {
    let r = 20.0;
    let lat = 89.9f64.to_radians();
    let sf = frame([1.0, 2.0, 3.0], [0.0, 0.0, 1.0]);
    let mut bd = Build::new();
    let cf = frame([1.0, 2.0, 3.0 + r * libm::sin(lat)], [0.0, 0.0, 1.0]);
    let c = bd.edge(Curve3::circle(cf, r * libm::cos(lat)).unwrap(), 0.0, TAU);
    bd.face(Surface::sphere(sf, r).unwrap(), &[&[(c, true)]]);
    let st = compute(&mut bd.b);
    all_within(&st);
    assert_eq!(st.exact, 1);
    let (s, e) = start_end(&bd.b, 0);
    assert!((s.y() - lat).abs() < 1e-12 && (e.y() - lat).abs() < 1e-12);
    assert!((e.x() - s.x() - TAU).abs() < 1e-12);
}

#[test]
fn tilted_circle_around_the_pole_is_sampled() {
    // A plane tilted 5° cuts the sphere in a circle that winds once around
    // the pole, 3° from it at the closest.
    let r = 20.0;
    let sf = frame([0.0; 3], [0.0, 0.0, 1.0]);
    let tilt = 5f64.to_radians();
    let n = Vec3::new(libm::sin(tilt), 0.0, libm::cos(tilt));
    // Distance of the plane from the centre: the circle's angular radius
    // about n is 8°, so it passes 3° from the pole.
    let ang = 8f64.to_radians();
    let d = r * libm::cos(ang);
    let centre = Point3::new(n.x() * d, 0.0, n.z() * d);
    let cf = Frame::new(centre, n, Vec3::new(0.0, 1.0, 0.0)).unwrap();
    let mut bd = Build::new();
    let c = bd.edge(Curve3::circle(cf, r * libm::sin(ang)).unwrap(), 0.0, TAU);
    bd.face(Surface::sphere(sf, r).unwrap(), &[&[(c, true)]]);
    let st = compute(&mut bd.b);
    all_within(&st);
    assert_eq!(st.sampled, 1);
    let (s, e) = start_end(&bd.b, 0);
    assert!(((e.x() - s.x()).abs() - TAU).abs() < 1e-6, "{s:?} {e:?}");
    assert!((s.y() - e.y()).abs() < 1e-6, "{s:?} {e:?}");
}

#[test]
fn sphere_lune_meridians_reach_the_poles() {
    let r = 10.0;
    let sf = frame([0.0; 3], [0.0, 0.0, 1.0]);
    let mut bd = Build::new();
    // Great circles in the planes y = 0 and x = 0; t from the south pole.
    let south = -FRAC_PI_2;
    let m0 = Frame::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
    )
    .unwrap();
    let m1 = Frame::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    )
    .unwrap();
    let a = bd.edge(Curve3::circle(m0, r).unwrap(), south + TAU, FRAC_PI_2 + TAU);
    let b = bd.edge(Curve3::circle(m1, r).unwrap(), south + TAU, FRAC_PI_2 + TAU);
    bd.face(Surface::sphere(sf, r).unwrap(), &[&[(a, true), (b, false)]]);
    let st = compute(&mut bd.b);
    all_within(&st);
    assert_eq!(st.exact, 2);
    for c in 0..2 {
        let (s, e) = start_end(&bd.b, c);
        assert!((s.x() - e.x()).abs() < 1e-12, "u constant: {s:?} {e:?}");
        assert!((s.y().abs() - FRAC_PI_2).abs() < 1e-12);
        assert!((e.y().abs() - FRAC_PI_2).abs() < 1e-12);
    }
    let (sa, _) = start_end(&bd.b, 0);
    let (sb, _) = start_end(&bd.b, 1);
    let du = (sa.x() - sb.x()).rem_euclid(TAU);
    assert!((du - FRAC_PI_2).abs() < 1e-12 || (du - 3.0 * FRAC_PI_2).abs() < 1e-12);
}

/// Cone with its apex on the loop: base circle, generator to the apex and
/// back down.
fn cone_face(generator_as_spline: bool) -> (BrepBody, Stats) {
    let (r, h) = (4.0, 8.0);
    let semi = libm::atan(r / h);
    let cf = frame([0.0; 3], [0.0, 0.0, 1.0]);
    let cone = Surface::cone(cf, r, semi).unwrap();
    let mut bd = Build::new();
    let base = bd.edge(Curve3::circle(cf, r).unwrap(), 0.0, TAU);
    // The radius grows with v, so the apex is below the base.
    let (p, apex) = (Point3::new(r, 0.0, 0.0), Point3::new(0.0, 0.0, -h));
    let g = if generator_as_spline {
        let bs = BSplineCurve3::new(1, vec![0.0, 0.0, 1.0, 1.0], vec![p, apex], None).unwrap();
        bd.edge(Curve3::BSpline(Arc::new(bs)), 0.0, 1.0)
    } else {
        let len = apex.distance(p);
        bd.edge(Curve3::line(p, UnitVec3::new(apex - p).unwrap()), 0.0, len)
    };
    bd.face(cone, &[&[(base, true), (g, true), (g, false)]]);
    let st = compute(&mut bd.b);
    (bd.b, st)
}

#[test]
fn cone_apex_generators_are_one_period_apart() {
    for spline in [false, true] {
        let (b, st) = cone_face(spline);
        all_within(&st);
        let (s0, e0) = start_end(&b, 0);
        let (s1, e1) = start_end(&b, 1);
        let (s2, e2) = start_end(&b, 2);
        assert!(e0.distance(s1) < 1e-9, "{e0:?} {s1:?}");
        assert!(e2.distance(s0) < 1e-9, "{e2:?} {s0:?}");
        assert!((e1.x() - s1.x()).abs() < 1e-9, "u constant up to the apex");
        assert!(
            (s2.x() - e2.x()).abs() < 1e-9,
            "u constant down from the apex"
        );
        assert!(
            ((e1.x() - s2.x()).abs() - TAU).abs() < 1e-9,
            "{e1:?} {s2:?}"
        );
    }
}

/// A ruled bicubic patch: `S(u, v) = (1 − v)·A(u) + v·B(u)`.
fn ruled_patch(closed: bool) -> (Surface, Vec<Point3>, Vec<Point3>, Vec<f64>) {
    let (a, b): (Vec<Point3>, Vec<Point3>) = if closed {
        let ring = |z: f64, s: f64| {
            vec![
                Point3::new(s, 0.0, z),
                Point3::new(s, s, z),
                Point3::new(-s, s, z),
                Point3::new(-s, -s, z),
                Point3::new(s, -s, z),
                Point3::new(s, 0.0, z),
            ]
        };
        (ring(0.0, 10.0), ring(8.0, 7.0))
    } else {
        (
            (0..6)
                .map(|i| Point3::new(i as f64 * 4.0, 0.0, [0.0, 3.0, -2.0, 4.0, 1.0, 0.0][i]))
                .collect(),
            (0..6)
                .map(|i| Point3::new(i as f64 * 4.0, 15.0, [1.0, -1.0, 2.0, 0.0, 3.0, 1.0][i]))
                .collect(),
        )
    };
    let ku = vec![0.0, 0.0, 0.0, 0.0, 0.3, 0.6, 1.0, 1.0, 1.0, 1.0];
    let mut poles = Vec::new();
    for i in 0..6 {
        poles.push(a[i]);
        poles.push(b[i]);
    }
    let s = BSplineSurface::new(
        3,
        1,
        ku.clone(),
        vec![0.0, 0.0, 1.0, 1.0],
        poles,
        6,
        2,
        None,
    )
    .unwrap();
    (Surface::BSpline(Arc::new(s)), a, b, ku)
}

fn iso_curve(a: &[Point3], b: &[Point3], ku: &[f64], v: f64) -> Curve3 {
    let poles = a.iter().zip(b).map(|(p, q)| p.lerp(*q, v)).collect();
    Curve3::BSpline(Arc::new(
        BSplineCurve3::new(3, ku.to_vec(), poles, None).unwrap(),
    ))
}

#[test]
fn bspline_patch_iso_curve_and_ruling() {
    let (s, a, b, ku) = ruled_patch(false);
    let mut bd = Build::new();
    let iso = bd.edge(iso_curve(&a, &b, &ku, 0.3), 0.0, 1.0);
    let p0 = s.eval(0.45, 0.0);
    let p1 = s.eval(0.45, 1.0);
    let len = p0.distance(p1);
    let rule = bd.edge(Curve3::line(p0, UnitVec3::new(p1 - p0).unwrap()), 0.0, len);
    bd.face(s, &[&[(iso, true)], &[(rule, true)]]);
    let st = compute(&mut bd.b);
    all_within(&st);
    assert_eq!(st.sampled, 2);
    let pc = bd.b.pcurves[0].as_ref().unwrap();
    for t in [0.1, 0.5, 0.77] {
        let q = pc.eval(t);
        assert!(
            (q.y() - 0.3).abs() < 1e-5 && (q.x() - t).abs() < 1e-5,
            "{t}: {q:?}"
        );
    }
    let pc = bd.b.pcurves[1].as_ref().unwrap();
    for t in [0.0, 0.5 * len, len] {
        assert!((pc.eval(t).x() - 0.45).abs() < 1e-5);
    }
}

#[test]
fn closed_bspline_seam() {
    let (s, a, b, ku) = ruled_patch(true);
    let mut bd = Build::new();
    let bottom = bd.edge(iso_curve(&a, &b, &ku, 0.0), 0.0, 1.0);
    let top = bd.edge(iso_curve(&a, &b, &ku, 1.0), 0.0, 1.0);
    let (p0, p1) = (s.eval(0.0, 0.0), s.eval(0.0, 1.0));
    let seam = bd.edge(
        Curve3::line(p0, UnitVec3::new(p1 - p0).unwrap()),
        0.0,
        p0.distance(p1),
    );
    assert_eq!(shift_periods(&s, TOL), (Some(1.0), None));
    bd.face(
        s,
        &[&[(bottom, true), (seam, true), (top, false), (seam, false)]],
    );
    let st = compute(&mut bd.b);
    all_within(&st);
    let (s1, _) = start_end(&bd.b, 1);
    let (s3, _) = start_end(&bd.b, 3);
    let mut us = [s1.x(), s3.x()];
    us.sort_by(f64::total_cmp);
    assert!(us[0].abs() < 1e-6 && (us[1] - 1.0).abs() < 1e-6, "{us:?}");
}

#[test]
fn plane_images_are_exact() {
    let pf = frame([1.0, 1.0, 1.0], [0.0, 0.0, 1.0]);
    let mut bd = Build::new();
    let bs = BSplineCurve3::new(
        2,
        vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(3.0, 4.0, 1.0),
            Point3::new(6.0, -1.0, 1.0),
            Point3::new(9.0, 2.0, 1.0),
        ],
        Some(vec![1.0, 2.0, 0.5, 1.0]),
    )
    .unwrap();
    let e = bd.edge(Curve3::BSpline(Arc::new(bs)), 0.0, 1.0);
    let tilted = Frame::new(
        Point3::new(3.0, 3.0, 1.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(1.0, 1.0, 0.0),
    )
    .unwrap();
    let c = bd.edge(Curve3::ellipse(tilted, 2.0, 1.0).unwrap(), 0.0, TAU);
    bd.face(Surface::plane(pf), &[&[(e, true)], &[(c, false)]]);
    let st = compute(&mut bd.b);
    all_within(&st);
    assert_eq!(st.exact, 2);
    assert!(st.max_dev < 1e-12, "{}", st.max_dev);
}

#[test]
fn torus_circles_are_exact() {
    let tf = frame([0.0; 3], [0.0, 0.0, 1.0]);
    let (big, small) = (10.0, 2.0);
    let mut bd = Build::new();
    let outer = bd.edge(Curve3::circle(tf, big + small).unwrap(), 0.0, TAU);
    let mf = Frame::new(
        Point3::new(big, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
    )
    .unwrap();
    let meridian = bd.edge(Curve3::circle(mf, small).unwrap(), 0.0, TAU);
    bd.face(
        Surface::torus(tf, big, small).unwrap(),
        &[&[
            (outer, true),
            (meridian, true),
            (outer, false),
            (meridian, false),
        ]],
    );
    let st = compute(&mut bd.b);
    all_within(&st);
    assert_eq!(st.exact, 4);
    let (s1, _) = start_end(&bd.b, 1);
    let (s3, _) = start_end(&bd.b, 3);
    assert!(
        ((s1.x() - s3.x()).abs() - TAU).abs() < 1e-9,
        "{s1:?} {s3:?}"
    );
}

#[test]
fn off_surface_edge_is_reported() {
    let mut bd = Build::new();
    let f0 = frame([0.0; 3], [0.0, 0.0, 1.0]);
    // A circle of radius 5.01 on a cylinder of radius 5.
    let c = bd.edge(Curve3::circle(f0, 5.01).unwrap(), 0.0, TAU);
    bd.face(Surface::cylinder(f0, 5.0).unwrap(), &[&[(c, true)]]);
    let st = compute(&mut bd.b);
    assert_eq!(st.within_tol, 0);
    assert_eq!(st.failures.len(), 1);
    let f = &st.failures[0];
    assert!(f.off_surface && (f.dev - 0.01).abs() < 1e-9, "{f:?}");
    assert_eq!(f.kind, SurfaceKind::Cylinder);
}
