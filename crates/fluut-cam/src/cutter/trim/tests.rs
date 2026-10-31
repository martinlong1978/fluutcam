// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::cutter::synth::{Builder, iso_v};
use fluut_geom::{Curve3, Frame, UnitVec3, Vec3};
use std::f64::consts::PI;

fn iv(lo: f64, hi: f64) -> Interval {
    Interval { lo, hi }
}

fn polygon(b: &mut Builder, pts: &[(f64, f64)]) -> Vec<(u32, bool)> {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, c) = (pts[i], pts[(i + 1) % n]);
            let (pa, pc) = (Point3::new(a.0, a.1, 0.0), Point3::new(c.0, c.1, 0.0));
            let d = pc - pa;
            let e = b.edge(
                Some(Curve3::line(pa, UnitVec3::new(d).unwrap())),
                iv(0.0, d.length()),
                pa,
            );
            (e, true)
        })
        .collect()
}

#[test]
fn plane_with_hole() {
    let mut b = Builder::new("plate");
    let outer = polygon(
        &mut b,
        &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
    );
    let hole = polygon(&mut b, &[(4.0, 4.0), (4.0, 6.0), (6.0, 6.0), (6.0, 4.0)]);
    b.face(Surface::plane(Frame::world_xy()), true, &[outer, hole]);
    let body = b.build();
    let t = Trim::build(&body, 0, 0.01).unwrap();
    assert!(t.contains(1.0, 1.0));
    assert!(t.contains(9.0, 5.0));
    assert!(!t.contains(5.0, 5.0));
    assert!(!t.contains(11.0, 5.0));
    assert!(!t.contains(5.0, -1.0));
    let uv = t.uv_box();
    assert!((uv.u.lo - 0.0).abs() < 1e-9 && (uv.u.hi - 10.0).abs() < 1e-9);
}

#[test]
fn cylinder_patch_across_seam() {
    let mut b = Builder::new("cyl");
    let s = Surface::cylinder(Frame::world_xy(), 2.0).unwrap();
    b.patch(s, UvBox::new(iv(-1.0, 1.0), iv(0.0, 3.0)));
    let body = b.build();
    let t = Trim::build(&body, 0, 0.001).unwrap();
    assert!(t.contains(0.5, 1.0));
    assert!(t.contains(2.0 * PI - 0.5, 1.0));
    assert!(t.contains(-0.5, 1.0));
    assert!(!t.contains(3.0, 1.0));
    assert!(!t.contains(0.5, 3.5));
    assert!((t.uv_box().u.length() - 2.0).abs() < 1e-6);
}

fn band(same_sense: bool) -> BrepBody {
    let mut b = Builder::new("band");
    let s = Surface::cylinder(Frame::world_xy(), 2.0).unwrap();
    let (c0, r) = iso_v(&s, 0.0, iv(0.0, TAU)).unwrap();
    let (c1, _) = iso_v(&s, 1.0, iv(0.0, TAU)).unwrap();
    let p0 = c0.eval(0.0);
    let p1 = c1.eval(0.0);
    let e0 = b.edge(Some(c0), r, p0);
    let e1 = b.edge(Some(c1), r, p1);
    b.face(s, same_sense, &[vec![(e0, true)], vec![(e1, false)]]);
    b.build()
}

#[test]
fn cylinder_band_of_full_circles() {
    let t = Trim::build(&band(true), 0, 0.001).unwrap();
    for u in [0.0, 1.0, 3.0, 6.2, -2.0, 9.0] {
        assert!(t.contains(u, 0.5), "{u}");
        assert!(!t.contains(u, 1.5), "{u}");
        assert!(!t.contains(u, -0.5), "{u}");
    }
}

fn cap(north: bool) -> BrepBody {
    let mut b = Builder::new("cap");
    let s = Surface::sphere(Frame::world_xy(), 3.0).unwrap();
    let (c, r) = iso_v(&s, 0.5, iv(0.0, TAU)).unwrap();
    let p = c.eval(0.0);
    let e = b.edge(Some(c), r, p);
    // The circle runs +u; the material is on its left (north) when forward.
    b.face(s, true, &[vec![(e, north)]]);
    b.build()
}

#[test]
fn sphere_cap_through_the_pole() {
    let t = Trim::build(&cap(true), 0, 0.001).unwrap();
    assert!(t.contains(1.0, 1.0));
    assert!(t.contains(4.0, 1.5));
    assert!(!t.contains(1.0, 0.2));
    assert!((t.uv_box().v.hi - FRAC_PI_2).abs() < 1e-12);
    let t = Trim::build(&cap(false), 0, 0.001).unwrap();
    assert!(!t.contains(1.0, 1.0));
    assert!(t.contains(4.0, 0.2));
    assert!(t.contains(4.0, -1.5));
}

#[test]
fn reversed_face_sense_flips_the_cap() {
    let mut body = cap(true);
    body.faces[0].same_sense = false;
    let t = Trim::build(&body, 0, 0.001).unwrap();
    assert!(!t.contains(1.0, 1.0));
    assert!(t.contains(1.0, 0.2));
}

#[test]
fn sphere_half_bounded_by_meridians() {
    let mut b = Builder::new("half");
    let s = Surface::sphere(Frame::world_xy(), 3.0).unwrap();
    b.patch(s, UvBox::new(iv(0.0, PI), iv(0.0, FRAC_PI_2)));
    let body = b.build();
    let t = Trim::build(&body, 0, 0.001).unwrap();
    assert!(t.contains(1.0, 1.2));
    assert!(t.contains(3.0, 1.5));
    assert!(!t.contains(4.0, 1.2));
    assert!(!t.contains(1.0, -0.2));
}

#[test]
fn cone_with_apex() {
    let mut b = Builder::new("cone");
    let s = Surface::cone(Frame::world_xy(), 1.0, 0.4).unwrap();
    let apex = -1.0 / libm::tan(0.4);
    b.patch(s, UvBox::new(iv(0.0, TAU), iv(apex, 0.0)));
    let body = b.build();
    let t = Trim::build(&body, 0, 0.001).unwrap();
    assert!(t.contains(1.0, -1.0));
    assert!(t.contains(5.0, apex + 0.01));
    assert!(!t.contains(1.0, 0.5));
}

#[test]
fn tilted_torus_patch() {
    let mut b = Builder::new("torus");
    let f = Frame::new(
        Point3::new(1.0, 2.0, 3.0),
        Vec3::new(0.2, 0.1, 1.0),
        Vec3::new(1.0, 0.0, 0.0),
    )
    .unwrap();
    let s = Surface::torus(f, 5.0, 1.0).unwrap();
    b.patch(s, UvBox::new(iv(5.5, 7.0), iv(2.5, 4.0)));
    let body = b.build();
    let t = Trim::build(&body, 0, 0.001).unwrap();
    assert!(t.contains(6.0, 3.0));
    assert!(t.contains(6.0 - TAU, 3.0 - TAU));
    assert!(!t.contains(6.0, 2.0));
    assert!(!t.contains(5.0, 3.0));
}
