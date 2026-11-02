// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use proptest::prelude::*;
use std::f64::consts::{FRAC_PI_4, PI};

fn tol() -> Tol {
    Tol::for_machining(1e-6, 1e-3)
}

fn tilted_frame() -> Frame {
    Frame::new(
        Point3::new(1.0, -2.0, 3.0),
        Vec3::new(0.3, -0.5, 1.0),
        Vec3::new(1.0, 0.2, 0.0),
    )
    .unwrap()
}

fn iv(lo: f64, hi: f64) -> Interval {
    Interval::new(lo, hi).unwrap()
}

/// Every analytic kind on a tilted frame, with a parameter box well away from
/// singular points.
fn samples() -> Vec<(Surface, UvBox)> {
    let f = tilted_frame();
    let full_u = iv(0.0, TAU);
    vec![
        (
            Surface::plane(f),
            UvBox::new(iv(-10.0, 10.0), iv(-10.0, 10.0)),
        ),
        (
            Surface::cylinder(f, 2.5).unwrap(),
            UvBox::new(full_u, iv(-5.0, 5.0)),
        ),
        (
            Surface::cone(f, 3.0, 0.4).unwrap(),
            UvBox::new(full_u, iv(-2.0, 5.0)),
        ),
        (
            Surface::sphere(f, 4.0).unwrap(),
            UvBox::new(full_u, iv(-1.2, 1.2)),
        ),
        (
            Surface::torus(f, 5.0, 1.5).unwrap(),
            UvBox::new(full_u, iv(0.0, TAU)),
        ),
    ]
}

#[test]
fn constructors_reject_bad_values() {
    let f = Frame::world_xy();
    assert!(Surface::cylinder(f, 0.0).is_none());
    assert!(Surface::sphere(f, f64::NAN).is_none());
    assert!(Surface::cone(f, 1.0, 0.0).is_none());
    assert!(Surface::cone(f, 1.0, FRAC_PI_2).is_none());
    assert!(Surface::cone(f, -1.0, 0.3).is_none());
    assert!(Surface::torus(f, 1.0, 0.0).is_none());
    assert!(
        Surface::torus(f, 0.5, 1.0).is_some(),
        "spindle torus allowed"
    );
}

#[test]
fn plane_eval_frame() {
    let f = tilted_frame();
    let s = Surface::plane(f);
    let p = s.eval(2.0, -3.0);
    assert!(p.distance(f.origin + f.x.get() * 2.0 - f.y().get() * 3.0) < 1e-15);
    assert!((s.normal(0.0, 0.0).unwrap().get() - f.z.get()).length() < 1e-15);
}

#[test]
fn derivs_match_finite_difference() {
    let h = 1e-6;
    for (s, uv) in samples() {
        for (fu, fv) in [(0.2, 0.3), (0.7, 0.6), (0.45, 0.9)] {
            let (u, v) = (uv.u.at(fu), uv.v.at(fv));
            let d = s.derivs(u, v);
            let du = s.derivs(u + h, v);
            let dum = s.derivs(u - h, v);
            let dv = s.derivs(u, v + h);
            let dvm = s.derivs(u, v - h);
            let close = |a: Vec3, b: Vec3, what: &str| {
                assert!(
                    (a - b).length() <= 1e-6 * (1.0 + a.length()),
                    "{:?} {what} at ({u},{v}): {a:?} vs {b:?}",
                    s.kind()
                );
            };
            close(d.su, (du.p - dum.p) / (2.0 * h), "Su");
            close(d.sv, (dv.p - dvm.p) / (2.0 * h), "Sv");
            close(d.suu, (du.su - dum.su) / (2.0 * h), "Suu");
            close(d.suv, (dv.su - dvm.su) / (2.0 * h), "Suv");
            close(d.svv, (dv.sv - dvm.sv) / (2.0 * h), "Svv");
        }
    }
}

#[test]
fn normal_points_outward() {
    let f = tilted_frame();
    let sph = Surface::sphere(f, 4.0).unwrap();
    let (u, v) = (1.0, 0.3);
    assert!(sph.normal(u, v).unwrap().dot(sph.eval(u, v) - f.origin) > 0.0);
    let cyl = Surface::cylinder(f, 2.0).unwrap();
    let n = cyl.normal(u, v).unwrap().get();
    assert!(n.dot(f.z.get()).abs() < 1e-15, "cylinder normal is radial");
    let tor = Surface::torus(f, 5.0, 1.0).unwrap();
    let centre_circle = f.origin + (f.x.get() * libm::cos(u) + f.y().get() * libm::sin(u)) * 5.0;
    assert!(
        tor.normal(u, v)
            .unwrap()
            .dot(tor.eval(u, v) - centre_circle)
            > 0.0
    );
}

#[test]
fn sphere_pole_normal_none() {
    let s = Surface::sphere(Frame::world_xy(), 2.0).unwrap();
    assert!(s.normal(0.3, FRAC_PI_2).is_none());
    assert!(s.normal(0.3, -FRAC_PI_2).is_none());
    assert!(s.curvatures(0.3, FRAC_PI_2).is_none());
}

#[test]
fn cone_apex_normal_none() {
    let s = Surface::cone(Frame::world_xy(), 0.0, 0.5).unwrap();
    assert!(s.normal(1.0, 0.0).is_none());
    assert!(s.normal(1.0, 1.0).is_some());
}

#[test]
fn curvatures_closed_form() {
    let f = tilted_frame();
    let close = |a: f64, b: f64| (a - b).abs() < 1e-10;
    // Outward normals: convex surfaces bend away from the normal (negative).
    let c = Surface::cylinder(f, 2.0)
        .unwrap()
        .curvatures(0.4, 1.0)
        .unwrap();
    assert!(close(c.k1, 0.0) && close(c.k2, -0.5), "{c:?}");
    assert!(
        c.d1.dot(f.z.get()).abs() > 1.0 - 1e-10,
        "zero curvature along axis"
    );
    let s = Surface::sphere(f, 4.0)
        .unwrap()
        .curvatures(0.4, 0.2)
        .unwrap();
    assert!(close(s.k1, -0.25) && close(s.k2, -0.25), "{s:?}");
    assert!(s.d1.dot(s.d2).abs() < 1e-12);
    // Torus outer equator (v = 0): −1/ρ around the tube, −1/(R+ρ) around the axis.
    let t = Surface::torus(f, 5.0, 1.0)
        .unwrap()
        .curvatures(0.4, 0.0)
        .unwrap();
    assert!(close(t.k1, -1.0 / 6.0) && close(t.k2, -1.0), "{t:?}");
    // Inner equator (v = π) is saddle-shaped: +1/(R−ρ) around the axis.
    let t = Surface::torus(f, 5.0, 1.0)
        .unwrap()
        .curvatures(0.4, PI)
        .unwrap();
    assert!(close(t.k1, 1.0 / 4.0) && close(t.k2, -1.0), "{t:?}");
    let p = Surface::plane(f).curvatures(1.0, 2.0).unwrap();
    assert!(close(p.k1, 0.0) && close(p.k2, 0.0));
}

#[test]
fn closest_on_axis_finite() {
    let f = tilted_frame();
    let uv = UvBox::new(iv(0.0, TAU), iv(-3.0, 3.0));
    let cyl = Surface::cylinder(f, 2.0).unwrap();
    let c = cyl.closest(f.origin + f.z.get() * 1.0, uv, &tol());
    assert!(c.u.is_finite() && (c.v - 1.0).abs() < 1e-12 && (c.dist - 2.0).abs() < 1e-12);
    let sph = Surface::sphere(f, 3.0).unwrap();
    let c = sph.closest(f.origin, UvBox::new(iv(0.0, TAU), iv(-1.5, 1.5)), &tol());
    assert!(c.u.is_finite() && c.v.is_finite() && (c.dist - 3.0).abs() < 1e-12);
}

#[test]
fn closest_clamped_to_box() {
    // Point beyond the top of a cylinder patch: closest is on the top edge.
    let cyl = Surface::cylinder(Frame::world_xy(), 1.0).unwrap();
    let uv = UvBox::new(iv(0.0, PI), iv(0.0, 2.0));
    let c = cyl.closest(Point3::new(0.0, 3.0, 5.0), uv, &tol());
    assert!(
        (c.v - 2.0).abs() < 1e-12 && (c.u - FRAC_PI_2).abs() < 1e-9,
        "{c:?}"
    );
    // Point on the far side: angle 3π/2 lies outside [0, π]; both ends are π/2
    // away, and the result must be one of them.
    let c = cyl.closest(Point3::new(0.0, -3.0, 1.0), uv, &tol());
    assert!(c.u == 0.0 || c.u == PI, "{c:?}");
    // Sphere with u limited: true optimum needs both u and v at the box.
    let sph = Surface::sphere(Frame::world_xy(), 1.0).unwrap();
    let uv = UvBox::new(iv(0.0, FRAC_PI_4), iv(-0.5, 0.5));
    let p = Point3::new(-1.0, 2.0, 3.0);
    let c = sph.closest(p, uv, &tol());
    // Brute force over a fine grid must not beat it.
    let mut best = f64::INFINITY;
    for i in 0..=200 {
        for j in 0..=200 {
            let q = sph.eval(uv.u.at(i as f64 / 200.0), uv.v.at(j as f64 / 200.0));
            best = best.min(q.distance(p));
        }
    }
    assert!(c.dist <= best + 1e-9, "{} vs grid {best}", c.dist);
}

#[test]
fn concave_offset_none() {
    let f = Frame::world_xy();
    assert!(Surface::cylinder(f, 2.0).unwrap().offset(-3.0).is_none());
    assert!(Surface::sphere(f, 2.0).unwrap().offset(-2.0).is_none());
    assert!(Surface::torus(f, 5.0, 1.0).unwrap().offset(-1.5).is_none());
    assert!(Surface::cone(f, 1.0, 0.5).unwrap().offset(-2.0).is_none());
}

#[test]
fn mirror_transform() {
    let s = Surface::cylinder(Frame::world_xy(), 2.0).unwrap();
    let m = Affine::scale(-1.0, 1.0, 1.0);
    let t = s.transformed(&m).unwrap();
    for (u, v) in [(0.0, 0.0), (1.0, 2.0), (4.0, -1.0)] {
        assert!(t.eval(-u, v).distance(m.apply_point(s.eval(u, v))) < 1e-14);
    }
    // Plane: v reverses.
    let pl = Surface::plane(tilted_frame());
    let tp = pl.transformed(&m).unwrap();
    assert!(
        tp.eval(1.5, -2.0)
            .distance(m.apply_point(pl.eval(1.5, 2.0)))
            < 1e-14
    );
    // Proper rotation keeps the parametrisation.
    let r = Affine::from_rigid(&crate::Rigid::from_rotation_translation(
        glam::DQuat::from_rotation_y(0.7),
        Vec3::new(1.0, 2.0, 3.0),
    ));
    let tor = Surface::torus(tilted_frame(), 5.0, 1.0).unwrap();
    let tt = tor.transformed(&r).unwrap();
    assert!(
        tt.eval(0.3, 2.0)
            .distance(r.apply_point(tor.eval(0.3, 2.0)))
            < 1e-13
    );
    assert!(s.transformed(&Affine::scale(1.0, 2.0, 1.0)).is_none());
    let big = s.transformed(&Affine::scale(3.0, 3.0, 3.0)).unwrap();
    assert_eq!(big, Surface::cylinder(Frame::world_xy(), 6.0).unwrap());
}

proptest! {
    #[test]
    fn closest_round_trip(i in 0usize..5, fu in 0.05f64..0.95, fv in 0.05f64..0.95) {
        let (s, uv) = samples().swap_remove(i);
        let (u, v) = (uv.u.at(fu), uv.v.at(fv));
        let p = s.eval(u, v);
        let c = s.closest(p, uv, &tol());
        prop_assert!(c.dist < 1e-9, "{:?} {} {}: {:?}", s.kind(), u, v, c);
        prop_assert!(c.point.distance(p) < 1e-9);
    }

    #[test]
    fn closest_off_surface_is_normal(i in 0usize..5, fu in 0.1f64..0.9, fv in 0.1f64..0.9, d in -0.5f64..0.5) {
        // A point displaced along the normal projects back to its foot.
        let (s, uv) = samples().swap_remove(i);
        let (u, v) = (uv.u.at(fu), uv.v.at(fv));
        let n = s.normal(u, v).unwrap().get();
        let p = s.eval(u, v) + n * d;
        let c = s.closest(p, uv, &tol());
        prop_assert!((c.dist - d.abs()).abs() < 1e-9, "{:?}: {:?}", s.kind(), c);
    }

    #[test]
    fn offset_then_distance(i in 0usize..5, fu in 0.1f64..0.9, fv in 0.1f64..0.9, d in -0.5f64..0.5) {
        let (s, uv) = samples().swap_remove(i);
        let o = s.offset(d).unwrap();
        let (u, v) = (uv.u.at(fu), uv.v.at(fv));
        // Same parameters on the offset surface lie along the base normal at |d|.
        let q = o.eval(u, v);
        let want = s.eval(u, v) + s.normal(u, v).unwrap().get() * d;
        prop_assert!(q.distance(want) < 1e-12, "{:?}", s.kind());
    }
}

// --- B-spline surfaces ----------------------------------------------------

fn bilinear() -> Surface {
    let p = Point3::new;
    Surface::BSpline(Arc::new(
        BSplineSurface::new(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            vec![p(0., 0., 0.), p(0., 1., 0.), p(1., 0., 0.), p(1., 1., 1.)],
            2,
            2,
            None,
        )
        .unwrap(),
    ))
}

/// Quarter of a unit cylinder, height 2: rational degree 2 in u, linear in v.
fn nurbs_quarter_cylinder() -> Surface {
    let p = Point3::new;
    let w = std::f64::consts::FRAC_1_SQRT_2;
    Surface::BSpline(Arc::new(
        BSplineSurface::new(
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                p(1., 0., 0.),
                p(1., 0., 2.),
                p(1., 1., 0.),
                p(1., 1., 2.),
                p(0., 1., 0.),
                p(0., 1., 2.),
            ],
            3,
            2,
            Some(vec![1.0, 1.0, w, w, 1.0, 1.0]),
        )
        .unwrap(),
    ))
}

fn unit_box() -> UvBox {
    UvBox::new(iv(0.0, 1.0), iv(0.0, 1.0))
}

#[test]
fn bspline_surface_rejects_bad_input() {
    let p = Point3::ORIGIN;
    let r = BSplineSurface::new(
        1,
        1,
        vec![0., 0., 1., 1.],
        vec![0., 0., 1., 1.],
        vec![p; 3],
        2,
        2,
        None,
    );
    assert_eq!(r, Err(crate::BSplineError::TooFewPoles));
    let r = BSplineSurface::new(
        1,
        1,
        vec![0., 0., 1.],
        vec![0., 0., 1., 1.],
        vec![p; 4],
        2,
        2,
        None,
    );
    assert_eq!(r, Err(crate::BSplineError::BadKnotCount));
}

#[test]
fn bilinear_patch() {
    let s = bilinear();
    // S(u,v) = (u, v, u·v)
    for (u, v) in [(0.0, 0.0), (0.25, 0.75), (1.0, 1.0), (0.5, 0.5)] {
        let q = s.eval(u, v);
        assert!(
            q.distance(Point3::new(u, v, u * v)) < 1e-15,
            "({u},{v}) -> {q:?}"
        );
    }
    let d = s.derivs(0.3, 0.6);
    assert!((d.suv - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-14);
}

#[test]
fn rational_cylinder_patch_matches_cylinder() {
    let s = nurbs_quarter_cylinder();
    let cyl = Surface::cylinder(Frame::world_xy(), 1.0).unwrap();
    let cb = UvBox::new(iv(0.0, FRAC_PI_2), iv(0.0, 2.0));
    for i in 0..=20 {
        for j in 0..=4 {
            let q = s.eval(i as f64 / 20.0, j as f64 / 4.0);
            assert!(cyl.closest(q, cb, &tol()).dist < 1e-12);
        }
    }
    // Normal is radial (same direction as the exact cylinder's).
    let q = s.eval(0.4, 0.5);
    let n = s.normal(0.4, 0.5).unwrap().get();
    let radial = Vec3::new(q.x(), q.y(), 0.0);
    assert!((n - radial / radial.length()).length() < 1e-12);
    // Curvature matches the exact cylinder: 0 and −1.
    let c = s.curvatures(0.4, 0.5).unwrap();
    assert!(c.k1.abs() < 1e-10 && (c.k2 + 1.0).abs() < 1e-10, "{c:?}");
}

#[test]
fn bspline_derivs_match_finite_difference() {
    let h = 1e-6;
    for s in [bilinear(), nurbs_quarter_cylinder()] {
        for (u, v) in [(0.2, 0.3), (0.7, 0.6)] {
            let d = s.derivs(u, v);
            let (du, dum) = (s.derivs(u + h, v), s.derivs(u - h, v));
            let (dv, dvm) = (s.derivs(u, v + h), s.derivs(u, v - h));
            let close = |a: Vec3, b: Vec3| (a - b).length() <= 1e-6 * (1.0 + a.length());
            assert!(close(d.su, (du.p - dum.p) / (2.0 * h)));
            assert!(close(d.sv, (dv.p - dvm.p) / (2.0 * h)));
            assert!(close(d.suu, (du.su - dum.su) / (2.0 * h)));
            assert!(close(d.suv, (dv.su - dvm.su) / (2.0 * h)));
            assert!(close(d.svv, (dv.sv - dvm.sv) / (2.0 * h)));
        }
    }
}

/// A rational surface of degree `du × 2` with uneven knots and weights.
fn wavy_nurbs(du: u8) -> Surface {
    let (pu, pv) = (du as usize, 2usize);
    let (nu, nv) = (pu + 3, 4);
    let knots = |p: usize, n: usize| {
        let mut k = vec![0.0; p + 1];
        for i in 1..n - p {
            k.push(i as f64 / (n - p) as f64 + 0.01 * i as f64 * i as f64);
        }
        let last = k[k.len() - 1] + 0.37;
        k.extend(std::iter::repeat_n(last, p + 1));
        k
    };
    let mut poles = Vec::new();
    let mut w = Vec::new();
    for i in 0..nu {
        for j in 0..nv {
            let (x, y) = (i as f64, j as f64);
            poles.push(Point3::new(
                x,
                y,
                libm::sin(0.7 * x) * libm::cos(1.3 * y) + 0.1 * x * y,
            ));
            w.push(1.0 + 0.3 * libm::sin(x + 2.0 * y));
        }
    }
    Surface::BSpline(Arc::new(
        BSplineSurface::new(du, 2, knots(pu, nu), knots(pv, nv), poles, nu, nv, Some(w)).unwrap(),
    ))
}

#[test]
fn bspline_lower_orders_are_bit_identical() {
    // `eval` and `derivs1` give the same bits as the matching fields of
    // `derivs`: the cutter queries mix them.
    let bits = |v: Vec3| [v.x().to_bits(), v.y().to_bits(), v.z().to_bits()];
    for s in [
        bilinear(),
        nurbs_quarter_cylinder(),
        wavy_nurbs(3),
        wavy_nurbs(9),
    ] {
        let Surface::BSpline(b) = &s else {
            unreachable!()
        };
        let d = b.domain();
        for i in 0..=12 {
            for j in 0..=12 {
                let (u, v) = (d.u.at(i as f64 / 12.0), d.v.at(j as f64 / 12.0));
                let full = s.derivs(u, v);
                assert_eq!(bits(s.eval(u, v).to_vec()), bits(full.p.to_vec()));
                let (p, su, sv) = s.derivs1(u, v);
                assert_eq!(bits(p.to_vec()), bits(full.p.to_vec()));
                assert_eq!(bits(su), bits(full.su));
                assert_eq!(bits(sv), bits(full.sv));
            }
        }
    }
}

#[test]
fn bspline_transform_any_affine() {
    let s = bilinear();
    let a = Affine::scale(2.0, 1.0, -3.0);
    let t = s.transformed(&a).unwrap();
    assert!(t.eval(0.3, 0.8).distance(a.apply_point(s.eval(0.3, 0.8))) < 1e-15);
}

proptest! {
    #[test]
    fn bspline_closest_round_trip(fu in 0.02f64..0.98, fv in 0.02f64..0.98, d in -0.3f64..0.3) {
        let s = nurbs_quarter_cylinder();
        let n = s.normal(fu, fv).unwrap().get();
        let p = s.eval(fu, fv) + n * d;
        let c = s.closest(p, unit_box(), &tol());
        prop_assert!((c.dist - d.abs()).abs() < 1e-9, "{:?}", c);
    }
}

// --- Extrusion, revolution, offset -----------------------------------------

use crate::curve::Curve3;

fn extruded_circle() -> Surface {
    Surface::extrusion(Curve3::circle(Frame::world_xy(), 2.0).unwrap(), UnitVec3::Z)
}

/// Torus R = 5, ρ = 1.5 as a revolution of a circle in the XZ plane.
fn revolved_circle() -> Surface {
    let profile_frame = Frame::new(
        Point3::new(5.0, 0.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
    )
    .unwrap();
    Surface::revolution(
        Curve3::circle(profile_frame, 1.5).unwrap(),
        Point3::ORIGIN,
        UnitVec3::Z,
    )
}

/// Revolution of a free-form (B-spline) profile: a vase-like surface.
fn revolved_bspline() -> Surface {
    let p = Point3::new;
    let profile = crate::BSplineCurve3::new(
        3,
        vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
        vec![
            p(2.0, 0.0, 0.0),
            p(3.0, 0.0, 1.0),
            p(1.5, 0.0, 2.0),
            p(2.5, 0.0, 3.0),
            p(2.0, 0.0, 4.0),
        ],
        None,
    )
    .unwrap();
    Surface::revolution(
        Curve3::BSpline(Arc::new(profile)),
        Point3::new(0.0, 0.0, 0.0),
        UnitVec3::Z,
    )
}

fn swept_samples() -> Vec<(Surface, UvBox)> {
    vec![
        (extruded_circle(), UvBox::new(iv(0.0, TAU), iv(-3.0, 3.0))),
        (revolved_circle(), UvBox::new(iv(0.0, TAU), iv(0.0, TAU))),
        (revolved_bspline(), UvBox::new(iv(0.0, TAU), iv(0.0, 1.0))),
        (
            revolved_bspline().offset(0.3).unwrap(),
            UvBox::new(iv(0.0, TAU), iv(0.0, 1.0)),
        ),
        (
            nurbs_quarter_cylinder().offset(-0.2).unwrap(),
            UvBox::new(iv(0.0, 1.0), iv(0.0, 1.0)),
        ),
    ]
}

#[test]
fn extrusion_of_circle_is_cylinder() {
    let cyl = Surface::cylinder(Frame::world_xy(), 2.0).unwrap();
    let ext = extruded_circle();
    for (u, v) in [(0.0, 0.0), (1.0, 2.0), (4.0, -1.0)] {
        assert!(ext.eval(u, v).distance(cyl.eval(u, v)) < 1e-15);
        let (a, b) = (ext.normal(u, v).unwrap(), cyl.normal(u, v).unwrap());
        assert!((a.get() - b.get()).length() < 1e-15);
    }
    assert_eq!(ext.periodicity(), (Some(TAU), None));
}

#[test]
fn revolution_of_circle_is_torus() {
    let tor = Surface::torus(Frame::world_xy(), 5.0, 1.5).unwrap();
    let rev = revolved_circle();
    for (u, v) in [(0.0, 0.0), (1.0, 2.0), (4.0, 5.0)] {
        assert!(rev.eval(u, v).distance(tor.eval(u, v)) < 1e-14, "({u},{v})");
        let (a, b) = (rev.normal(u, v).unwrap(), tor.normal(u, v).unwrap());
        assert!((a.get() - b.get()).length() < 1e-14);
    }
    let (c1, c2) = (
        rev.curvatures(1.0, 0.0).unwrap(),
        tor.curvatures(1.0, 0.0).unwrap(),
    );
    assert!((c1.k1 - c2.k1).abs() < 1e-12 && (c1.k2 - c2.k2).abs() < 1e-12);
}

#[test]
fn revolution_of_line_is_cone() {
    let alpha: f64 = 0.4;
    let t = libm::tan(alpha);
    let cone = Surface::cone(Frame::world_xy(), 3.0, alpha).unwrap();
    let dir = Vec3::new(t, 0.0, 1.0).normalized().unwrap();
    let rev = Surface::revolution(
        Curve3::line(Point3::new(3.0, 0.0, 0.0), dir),
        Point3::ORIGIN,
        UnitVec3::Z,
    );
    // The line parameter is arc length along the generatrix.
    let k = (1.0 + t * t).sqrt();
    for (u, v) in [(0.0, 0.0), (1.0, 2.0), (4.0, -1.0)] {
        assert!(rev.eval(u, v * k).distance(cone.eval(u, v)) < 1e-14);
    }
}

#[test]
fn swept_derivs_match_finite_difference() {
    let h = 1e-6;
    for (s, uv) in swept_samples() {
        // Offsets have numeric second derivatives.
        let rel2 = if s.kind() == SurfaceKind::Offset {
            1e-4
        } else {
            1e-6
        };
        for (fu, fv) in [(0.2, 0.3), (0.7, 0.6)] {
            let (u, v) = (uv.u.at(fu), uv.v.at(fv));
            let d = s.derivs(u, v);
            let (du, dum) = (s.derivs(u + h, v), s.derivs(u - h, v));
            let (dv, dvm) = (s.derivs(u, v + h), s.derivs(u, v - h));
            let close = |a: Vec3, b: Vec3, rel: f64, what: &str| {
                assert!(
                    (a - b).length() <= rel * (1.0 + a.length()),
                    "{:?} {what} at ({u},{v}): {a:?} vs {b:?}",
                    s.kind()
                );
            };
            close(d.su, (du.p - dum.p) / (2.0 * h), 1e-6, "Su");
            close(d.sv, (dv.p - dvm.p) / (2.0 * h), 1e-6, "Sv");
            close(d.suu, (du.su - dum.su) / (2.0 * h), rel2, "Suu");
            close(d.suv, (dv.su - dvm.su) / (2.0 * h), rel2, "Suv");
            close(d.svv, (dv.sv - dvm.sv) / (2.0 * h), rel2, "Svv");
        }
    }
}

#[test]
fn offset_wrapper_matches_analytic_offset() {
    // D02 §9: analytic offset vs numeric Offset wrapper agree within 1e-10.
    let f = tilted_frame();
    for (base, d) in [
        (Surface::sphere(f, 4.0).unwrap(), 0.7),
        (Surface::torus(f, 5.0, 1.5).unwrap(), -0.4),
        (Surface::cylinder(f, 2.0).unwrap(), 1.1),
    ] {
        let exact = base.offset(d).unwrap();
        let wrapped = Surface::Offset {
            basis: Box::new(base.clone()),
            dist: d,
        };
        for (u, v) in [(0.3, 0.2), (2.0, -0.5), (5.0, 1.0)] {
            assert!(wrapped.eval(u, v).distance(exact.eval(u, v)) < 1e-10);
            let (a, b) = (wrapped.derivs(u, v), exact.derivs(u, v));
            assert!((a.su - b.su).length() < 1e-10 && (a.sv - b.sv).length() < 1e-10);
            let (ka, kb) = (
                wrapped.curvatures(u, v).unwrap(),
                exact.curvatures(u, v).unwrap(),
            );
            assert!((ka.k1 - kb.k1).abs() < 1e-5 && (ka.k2 - kb.k2).abs() < 1e-5);
        }
    }
}

#[test]
fn offset_of_offset_accumulates() {
    let o = revolved_bspline().offset(0.2).unwrap().offset(0.3).unwrap();
    let Surface::Offset { basis, dist } = &o else {
        panic!("expected an offset surface")
    };
    assert_eq!(*dist, 0.5);
    assert_eq!(basis.kind(), SurfaceKind::Revolution);
}

fn all_samples() -> Vec<(Surface, UvBox)> {
    let mut v = samples();
    v.push((nurbs_quarter_cylinder(), unit_box()));
    v.extend(swept_samples());
    v
}

#[test]
fn transformed_param_and_normal_flip_are_consistent() {
    let rot = Affine::from_rigid(&crate::Rigid::from_rotation_translation(
        glam::DQuat::from_axis_angle(glam::DVec3::new(1.0, 2.0, 0.5).normalize(), 0.9),
        Vec3::new(-3.0, 1.0, 2.0),
    ));
    let scaled = Affine::scale(2.5, 2.5, 2.5).then(&rot);
    let mirrored = Affine::scale(1.0, -1.0, 1.0).then(&scaled);
    for (s, uv) in all_samples() {
        for a in [rot, scaled, mirrored] {
            let t = s.transformed(&a).unwrap();
            for (fu, fv) in [(0.2, 0.3), (0.6, 0.8)] {
                let (u, v) = (uv.u.at(fu), uv.v.at(fv));
                let (tu, tv) = s.transformed_param(&a, u, v);
                let want = a.apply_point(s.eval(u, v));
                assert!(
                    t.eval(tu, tv).distance(want) < 1e-9 * (1.0 + want.to_vec().length()),
                    "{:?} det {}: point",
                    s.kind(),
                    a.determinant()
                );
                let n = s.normal(u, v).unwrap().get();
                let mapped = a.apply_vec(n);
                let tn = t.normal(tu, tv).unwrap().get();
                let flipped = tn.dot(mapped) < 0.0;
                assert_eq!(
                    flipped,
                    s.transform_flips_normal(&a),
                    "{:?} det {}: normal",
                    s.kind(),
                    a.determinant()
                );
            }
        }
    }
}

proptest! {
    #[test]
    fn swept_closest_round_trip(i in 0usize..5, fu in 0.05f64..0.95, fv in 0.05f64..0.95, d in -0.2f64..0.2) {
        let (s, uv) = swept_samples().swap_remove(i);
        let (u, v) = (uv.u.at(fu), uv.v.at(fv));
        let n = s.normal(u, v).unwrap().get();
        let p = s.eval(u, v) + n * d;
        let c = s.closest(p, uv, &tol());
        prop_assert!((c.dist - d.abs()).abs() < 1e-8, "{:?} ({},{}) d={}: {:?}", s.kind(), u, v, d, c);
    }
}

// --- Plane sections -----------------------------------------------------------

fn section_tol() -> Tol {
    Tol::for_machining(1e-6, 0.01)
}

/// Every piece lies on the plane and on the surface (within `eps`), inside the
/// box. Returns the number of sample points checked.
fn check_section(
    s: &Surface,
    uv: UvBox,
    plane: &SectionPlane,
    sec: &SurfaceSection,
    eps: f64,
) -> usize {
    let mut n = 0;
    for c in &sec.curves {
        for i in 0..=40 {
            let p = c.curve.eval(c.range.at(i as f64 / 40.0));
            assert!(
                plane.signed_distance(p).abs() <= eps,
                "{:?}: off plane by {}",
                s.kind(),
                plane.signed_distance(p)
            );
            let cl = s.closest(p, uv, &section_tol());
            assert!(cl.dist <= eps, "{:?}: off surface by {}", s.kind(), cl.dist);
            n += 1;
        }
    }
    n
}

fn plane_through(origin: Point3, normal: Vec3) -> SectionPlane {
    let x_ref = if normal.cross(Vec3::new(1.0, 0.0, 0.0)).length() > 0.1 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    SectionPlane::new(Frame::new(origin, normal, x_ref).unwrap())
}

#[test]
fn section_plane_plane_clipped_line() {
    let s = Surface::plane(Frame::world_xy());
    let uv = UvBox::new(iv(-1.0, 1.0), iv(-2.0, 2.0));
    let plane = plane_through(Point3::new(0.5, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
    let sec = plane_section(&s, uv, &plane, &section_tol());
    assert_eq!(sec.curves.len(), 1);
    let c = &sec.curves[0];
    assert!((c.range.length() - 4.0).abs() < 1e-12);
    let (a, b) = (c.curve.eval(c.range.lo), c.curve.eval(c.range.hi));
    assert!((a.x() - 0.5).abs() < 1e-15 && (a.y().abs() - 2.0).abs() < 1e-12);
    assert!((b.y() + a.y()).abs() < 1e-12);
    check_section(&s, uv, &plane, &sec, 1e-12);
}

#[test]
fn section_plane_coincident_and_parallel() {
    let s = Surface::plane(Frame::world_xy());
    let uv = UvBox::new(iv(-1.0, 1.0), iv(-1.0, 1.0));
    let sec = plane_section(&s, uv, &SectionPlane::horizontal(0.0), &section_tol());
    assert!(sec.coincident && sec.curves.is_empty());
    let sec = plane_section(&s, uv, &SectionPlane::horizontal(1.0), &section_tol());
    assert!(!sec.coincident && sec.curves.is_empty());
}

#[test]
fn section_cylinder_cases() {
    let s = Surface::cylinder(Frame::world_xy(), 2.0).unwrap();
    let uv = UvBox::new(iv(0.0, TAU), iv(0.0, 5.0));
    // Perpendicular: full circle.
    let sec = plane_section(&s, uv, &SectionPlane::horizontal(3.0), &section_tol());
    assert_eq!(sec.curves.len(), 1);
    assert!(sec.curves[0].closed);
    assert_eq!(sec.curves[0].curve.as_circle().unwrap().1, 2.0);
    check_section(&s, uv, &SectionPlane::horizontal(3.0), &sec, 1e-12);
    // Outside the v range: nothing.
    assert!(
        plane_section(&s, uv, &SectionPlane::horizontal(6.0), &section_tol())
            .curves
            .is_empty()
    );
    // Parallel: two lines.
    let plane = plane_through(Point3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
    let sec = plane_section(&s, uv, &plane, &section_tol());
    assert_eq!(sec.curves.len(), 2);
    assert!(sec.curves.iter().all(|c| !c.tangent));
    check_section(&s, uv, &plane, &sec, 1e-12);
    // Tangent: one line, flagged.
    let plane = plane_through(Point3::new(2.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
    let sec = plane_section(&s, uv, &plane, &section_tol());
    assert_eq!(sec.curves.len(), 1);
    assert!(sec.curves[0].tangent);
    // Oblique: ellipse, clipped where it leaves 0 ≤ v ≤ 5.
    let plane = plane_through(Point3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 1.0));
    let sec = plane_section(&s, uv, &plane, &section_tol());
    assert_eq!(sec.curves.len(), 1, "{sec:?}");
    assert!(!sec.curves[0].closed);
    assert!(matches!(sec.curves[0].curve, Curve3::Ellipse { .. }));
    // Ends are clipped to v = 0 by bisection to tol.solver.
    check_section(&s, uv, &plane, &sec, 1e-6);
    let c = &sec.curves[0];
    for t in [c.range.lo, c.range.hi] {
        assert!(c.curve.eval(t).z().abs() < 1e-6, "ends on v = 0");
    }
}

#[test]
fn section_sphere_circle_and_touch() {
    let s = Surface::sphere(tilted_frame(), 3.0).unwrap();
    let uv = UvBox::new(iv(0.0, TAU), iv(-FRAC_PI_2, FRAC_PI_2));
    let o = tilted_frame().origin;
    let plane = plane_through(o + Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 1.0));
    let sec = plane_section(&s, uv, &plane, &section_tol());
    assert_eq!(sec.curves.len(), 1);
    let (_, r) = sec.curves[0].curve.as_circle().unwrap();
    assert!((r - 8f64.sqrt()).abs() < 1e-12);
    check_section(&s, uv, &plane, &sec, 1e-9);
    let plane = plane_through(o + Vec3::new(0.0, 0.0, 3.0), Vec3::new(0.0, 0.0, 1.0));
    let sec = plane_section(&s, uv, &plane, &section_tol());
    assert!(sec.curves.is_empty() && sec.touch_points.len() == 1);
}

#[test]
fn section_cone_cases() {
    let alpha = 0.5;
    let s = Surface::cone(Frame::world_xy(), 1.0, alpha).unwrap();
    let uv = UvBox::new(iv(0.0, TAU), iv(0.0, 4.0));
    let sec = plane_section(&s, uv, &SectionPlane::horizontal(2.0), &section_tol());
    let (_, r) = sec.curves[0].curve.as_circle().unwrap();
    assert!((r - (1.0 + 2.0 * libm::tan(alpha))).abs() < 1e-12);
    // Through the axis: two generatrix lines over the whole v range.
    let plane = plane_through(Point3::ORIGIN, Vec3::new(1.0, 1.0, 0.0));
    let sec = plane_section(&s, uv, &plane, &section_tol());
    assert_eq!(sec.curves.len(), 2);
    check_section(&s, uv, &plane, &sec, 1e-12);
    for c in &sec.curves {
        assert!((c.curve.eval(c.range.hi).z() - 4.0).abs() < 1e-12);
    }
    // Oblique: marched conic, within half the machining tolerance.
    let plane = plane_through(Point3::new(0.0, 0.0, 2.0), Vec3::new(0.3, 0.0, 1.0));
    let sec = plane_section(&s, uv, &plane, &section_tol());
    assert_eq!(sec.curves.len(), 1, "{:?}", sec.curves.len());
    assert!(sec.curves[0].closed);
    assert!(check_section(&s, uv, &plane, &sec, 0.005) > 0);
}

#[test]
fn section_torus_cases() {
    let s = Surface::torus(Frame::world_xy(), 5.0, 1.0).unwrap();
    let uv = UvBox::new(iv(0.0, TAU), iv(0.0, TAU));
    // Perpendicular at z = 0.5: two circles, radii 5 ± √0.75.
    let sec = plane_section(&s, uv, &SectionPlane::horizontal(0.5), &section_tol());
    let mut radii: Vec<f64> = sec
        .curves
        .iter()
        .map(|c| c.curve.as_circle().unwrap().1)
        .collect();
    radii.sort_by(f64::total_cmp);
    assert_eq!(radii.len(), 2);
    assert!((radii[0] - (5.0 - 0.75f64.sqrt())).abs() < 1e-12);
    assert!((radii[1] - (5.0 + 0.75f64.sqrt())).abs() < 1e-12);
    // Tangent at the top: one circle, flagged.
    let sec = plane_section(&s, uv, &SectionPlane::horizontal(1.0), &section_tol());
    assert_eq!(sec.curves.len(), 1);
    assert!(sec.curves[0].tangent);
    // Through the axis: two meridian circles of radius 1.
    let plane = plane_through(Point3::ORIGIN, Vec3::new(0.0, 1.0, 0.0));
    let sec = plane_section(&s, uv, &plane, &section_tol());
    assert_eq!(sec.curves.len(), 2);
    check_section(&s, uv, &plane, &sec, 1e-9);
    // Oblique through the tube: marched, two closed loops.
    let plane = plane_through(Point3::new(0.0, 0.0, 0.2), Vec3::new(0.05, 0.0, 1.0));
    let sec = plane_section(&s, uv, &plane, &section_tol());
    assert_eq!(sec.curves.len(), 2, "{}", sec.curves.len());
    assert!(sec.curves.iter().all(|c| c.closed));
    check_section(&s, uv, &plane, &sec, 0.005);
}

#[test]
fn section_marched_bspline_ends_on_box() {
    // Quarter cylinder patch at z = 1: a quarter circle from (1,0,1) to (0,1,1).
    let s = nurbs_quarter_cylinder();
    let uv = unit_box();
    let plane = SectionPlane::horizontal(1.0);
    let sec = plane_section(&s, uv, &plane, &section_tol());
    assert_eq!(sec.curves.len(), 1);
    let c = &sec.curves[0];
    assert!(!c.closed);
    check_section(&s, uv, &plane, &sec, 0.005);
    let ends = [c.curve.eval(c.range.lo), c.curve.eval(c.range.hi)];
    let want = [Point3::new(1.0, 0.0, 1.0), Point3::new(0.0, 1.0, 1.0)];
    for w in want {
        assert!(ends.iter().any(|e| e.distance(w) < 1e-6), "{ends:?}");
    }
    for i in 0..=20 {
        let p = c.curve.eval(c.range.at(i as f64 / 20.0));
        assert!((libm::hypot(p.x(), p.y()) - 1.0).abs() < 0.005);
    }
}

#[test]
fn section_marched_revolution_is_closed_loop() {
    let s = revolved_bspline();
    let uv = UvBox::new(iv(0.0, TAU), iv(0.0, 1.0));
    let plane = SectionPlane::horizontal(2.2);
    let sec = plane_section(&s, uv, &plane, &section_tol());
    assert!(!sec.curves.is_empty());
    assert!(sec.curves.iter().all(|c| c.closed), "{sec:?}");
    check_section(&s, uv, &plane, &sec, 0.005);
}
