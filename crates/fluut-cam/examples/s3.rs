// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spike S3 measurements: `cargo run -p fluut-cam --example s3 --release`.

use fluut_cam::Region2;
use fluut_cam::spike::{optimised_load, pocket, shapes};
use std::time::Instant;

fn main() {
    let (r, stepover) = (3.0, 2.4);
    println!("Offset pocket, R = {r}, stepover = {stepover}");
    for (name, clear) in shapes::all() {
        let t = Instant::now();
        let rings = pocket::rings(&clear, r, stepover);
        let ms_rings = t.elapsed().as_secs_f64() * 1e3;
        let len: f64 = rings.iter().map(|l| l.length()).sum();
        let t = Instant::now();
        let swept = rings
            .iter()
            .map(|l| pocket::swept_loop(l, r))
            .fold(Region2::empty(), |a, b| a.union(&b));
        let ms_swept = t.elapsed().as_secs_f64() * 1e3;
        let uncut = clear.subtract(&swept);
        println!(
            "  {name}: area {:.1}, {} rings, length {len:.1}, rings {ms_rings:.2} ms, \
             swept union {ms_swept:.1} ms, uncut {:.4} mm² in {} loops",
            clear.area(),
            rings.len(),
            uncut.area(),
            uncut.loops.len()
        );
    }
    println!("Booleans of two n-gon stars with 4 n segments in all");
    for n in [500, 2000, 8000] {
        let star = |cx: f64, phase: f64| {
            let pts: Vec<(f64, f64)> = (0..n)
                .map(|i| {
                    let t = std::f64::consts::TAU * (f64::from(i) + phase) / f64::from(n);
                    let r = if i % 2 == 0 { 40.0 } else { 36.0 };
                    let (s, c) = libm::sincos(t);
                    (cx + r * c, r * s)
                })
                .collect();
            Region2::polygon(&pts)
        };
        let (a, b) = (star(0.0, 0.0), star(5.0, 0.3));
        let t = Instant::now();
        let u = a.union(&b);
        let i = a.intersect(&b);
        let d = a.subtract(&b);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        println!(
            "  n = {n}: union, intersect, subtract {ms:.1} ms; areas {:.3} {:.3} {:.3}",
            u.area(),
            i.area(),
            d.area()
        );
    }
    let p = optimised_load::Params {
        r,
        phi: 60f64.to_radians(),
        step: r / 4.0,
        max_steps: 4000,
        max_passes: 400,
    };
    println!("Optimised load, R = {r}, target 60°, step {}", p.step);
    for (name, clear) in shapes::all() {
        let a = optimised_load::optimised_load(&clear, &p);
        let s = &a.stats;
        println!(
            "  {name}: {} passes, {} steps, length {:.1}, err max {:.1}° rms {:.1}°, \
             off >10° {}, uncut {:.2} mm² ({:.2} %), {} ms, stop: {}, cleared region {} loops / {} segments",
            s.passes,
            s.steps,
            s.length,
            s.max_err.to_degrees(),
            s.rms_err.to_degrees(),
            s.off_target,
            s.uncut_area,
            100.0 * s.uncut_area / clear.area(),
            s.millis,
            s.stop,
            a.cleared.loops.len(),
            a.cleared.segs().count()
        );
    }
}
