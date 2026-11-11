// SPDX-License-Identifier: MIT OR Apache-2.0

//! Benchmarks for the D02 §8 performance targets (single thread, release):
//!
//! | Operation | Target |
//! |---|---|
//! | eval: analytic surface | < 20 ns |
//! | eval: bicubic B-spline surface (with derivs to 1st order) | < 250 ns |
//! | closest point: analytic | < 50 ns |
//! | closest point: bicubic B-spline | < 5 µs median |
//! | plane ∩ cylinder/cone/sphere | < 200 ns |
//!
//! `derivs1` is the first-order figure; `derivs` computes second derivatives
//! too.

use criterion::{Criterion, criterion_group, criterion_main};
use fluut_geom::{
    BSplineSurface, Frame, Interval, Point3, SectionPlane, Surface, Tol, UvBox, Vec3, plane_section,
};
use std::hint::black_box;
use std::sync::Arc;

fn tilted() -> Frame {
    Frame::new(
        Point3::new(1.0, -2.0, 3.0),
        Vec3::new(0.3, -0.5, 1.0),
        Vec3::new(1.0, 0.2, 0.0),
    )
    .expect("valid frame")
}

/// A wavy bicubic patch, 8 × 8 poles.
fn bicubic() -> Surface {
    let n = 8;
    let mut poles = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            let z = libm::sin(i as f64 * 0.7) * libm::cos(j as f64 * 0.5);
            poles.push(Point3::new(i as f64 * 10.0, j as f64 * 10.0, z * 5.0));
        }
    }
    let knots: Vec<f64> = [0.0, 0.0, 0.0]
        .into_iter()
        .chain((0..=5).map(|k| k as f64 / 5.0))
        .chain([1.0, 1.0, 1.0])
        .collect();
    Surface::BSpline(Arc::new(
        BSplineSurface::new(3, 3, knots.clone(), knots, poles, n, n, None).expect("valid surface"),
    ))
}

fn unit_box() -> UvBox {
    UvBox::new(Interval { lo: 0.0, hi: 1.0 }, Interval { lo: 0.0, hi: 1.0 })
}

fn benches(c: &mut Criterion) {
    let tol = Tol::for_machining(1e-6, 0.01);
    let torus = Surface::torus(tilted(), 5.0, 1.5).expect("valid torus");
    let sphere = Surface::sphere(tilted(), 4.0).expect("valid sphere");
    let cylinder = Surface::cylinder(Frame::world_xy(), 3.0).expect("valid cylinder");
    let bs = bicubic();
    let full_u = Interval {
        lo: 0.0,
        hi: std::f64::consts::TAU,
    };

    c.bench_function("eval analytic (torus)", |b| {
        b.iter(|| black_box(&torus).eval(black_box(0.3), black_box(1.2)))
    });
    c.bench_function("derivs1 bicubic B-spline", |b| {
        b.iter(|| black_box(&bs).derivs1(black_box(0.37), black_box(0.61)))
    });
    c.bench_function("derivs bicubic B-spline", |b| {
        b.iter(|| black_box(&bs).derivs(black_box(0.37), black_box(0.61)))
    });
    let sphere_box = UvBox::new(full_u, Interval { lo: -1.5, hi: 1.5 });
    let p = Point3::new(4.0, 2.0, 7.0);
    c.bench_function("closest analytic (sphere)", |b| {
        b.iter(|| black_box(&sphere).closest(black_box(p), sphere_box, &tol))
    });
    let q = bs.eval(0.43, 0.57) + Vec3::new(0.0, 0.0, 0.5);
    c.bench_function("closest bicubic B-spline", |b| {
        b.iter(|| black_box(&bs).closest(black_box(q), unit_box(), &tol))
    });
    let cyl_box = UvBox::new(full_u, Interval { lo: 0.0, hi: 10.0 });
    let plane = SectionPlane::horizontal(4.0);
    c.bench_function("plane section (cylinder)", |b| {
        b.iter(|| plane_section(black_box(&cylinder), cyl_box, &plane, &tol))
    });
}

criterion_group!(geom, benches);
criterion_main!(geom);
