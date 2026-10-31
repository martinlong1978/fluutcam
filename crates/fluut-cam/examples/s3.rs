// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spike S3 measurements: `cargo run -p fluut-cam --example s3 --release`.

use fluut_cam::Region2;
use fluut_cam::spike::{adaptive, pocket, shapes};
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
    let p = adaptive::Params {
        r,
        phi: 60f64.to_radians(),
        step: r / 4.0,
        max_steps: 4000,
        max_passes: 400,
    };
    println!("Adaptive, R = {r}, target 60°, step {}", p.step);
    for (name, clear) in shapes::all() {
        let a = adaptive::adaptive(&clear, &p);
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
