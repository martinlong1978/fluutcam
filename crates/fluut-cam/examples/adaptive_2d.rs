// SPDX-License-Identifier: MIT OR Apache-2.0

//! Speed of the adaptive clearing engine on the S3 shapes (S3 report §5,
//! target < 1 s per level): `cargo run -p fluut-cam --example adaptive_2d
//! --release`.

use fluut_cam::Region2;
use fluut_cam::adaptive::{Params, clear};
use fluut_cam::spike::shapes;
use fluut_geom::Point2;
use std::time::Instant;

/// The middle of the last non-empty inward offset of `centres`.
fn deepest(centres: &Region2, step: f64) -> Point2 {
    let mut last = centres.clone();
    loop {
        let next = last.offset(-step);
        if next.is_empty() {
            break;
        }
        last = next;
    }
    let l = last
        .loops
        .iter()
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap();
    let n = l.verts.len() as f64;
    let (sx, sy) = l
        .verts
        .iter()
        .fold((0.0, 0.0), |(x, y), v| (x + v.p.x(), y + v.p.y()));
    Point2::new(sx / n, sy / n)
}

/// Twice the largest inward offset that leaves something of `r`.
fn thickest(r: &Region2) -> f64 {
    let mut d = 0.0;
    while !r.offset(-(d + 0.005)).is_empty() && d < 10.0 {
        d += 0.005;
    }
    2.0 * d
}

fn main() {
    let r = 3.0;
    // Load 1.5 mm on R 3: φ* = 60°, as in the S3 runs.
    let p = Params::new(r, 1.5);
    println!("R = {r}, φ* = {:.1}°, step {}", p.phi.to_degrees(), p.step);
    for (name, material) in shapes::all() {
        let centres = material.offset(-r);
        let e = deepest(&centres, p.step);
        let k0 = Region2::circle(e.x(), e.y(), 1.5 * r).intersect(&material);
        let t = Instant::now();
        let c = clear(&material, &centres, k0, e, &p);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let len: f64 = c
            .passes
            .iter()
            .map(|q| q.pts.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>())
            .sum();
        let reach = centres.offset(r);
        let contour = centres
            .loops
            .iter()
            .map(|l| fluut_cam::spike::pocket::swept_loop(l, r))
            .fold(Region2::empty(), |a, b| a.union(&b));
        let after = reach.subtract(&c.cleared).subtract(&contour);
        println!(
            "  {name}: {} passes, {} steps, path {len:.0} mm, RMS {:.1}°, > 10° off {}, \
             {} evaluations, left in reach {:.2} mm², after the final contour {:.3} mm² \
             (thickest {:.3} mm), {ms:.0} ms ({})",
            c.stats.passes,
            c.stats.steps,
            c.stats.rms_err.to_degrees(),
            c.stats.off_target,
            c.stats.evaluations,
            reach.subtract(&c.cleared).area(),
            after.area(),
            thickest(&after),
            c.stats.stop
        );
    }
}
