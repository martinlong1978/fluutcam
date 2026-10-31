// SPDX-License-Identifier: MIT OR Apache-2.0

//! Boolean consistency on the regions adaptive clearing builds:
//! `cargo run -p fluut-cam --example s3_booleans --release`.
//!
//! Replays the swept-region unions of every adaptive pass on the S3 shapes
//! and checks `area(A ∪ B) + area(A ∩ B) = area(A) + area(B)` and
//! `area(A ∖ B) = area(A) − area(A ∩ B)` for each pair. Consecutive steps
//! give stadiums with nearly collinear edges and nearly tangent caps.

use fluut_cam::Region2;
use fluut_cam::spike::{adaptive, pocket, shapes};

fn main() {
    let p = adaptive::Params {
        r: 3.0,
        phi: 60f64.to_radians(),
        step: 0.75,
        max_steps: 4000,
        max_passes: 400,
    };
    for (name, clear) in shapes::all() {
        let a = adaptive::adaptive(&clear, &p);
        let (mut pairs, mut bad, mut worst) = (0, 0, 0.0f64);
        let mut check = |x: &Region2, y: &Region2| -> Region2 {
            let (u, i, d) = (x.union(y), x.intersect(y), x.subtract(y));
            let e1 = (u.area() + i.area() - x.area() - y.area()).abs();
            let e2 = (d.area() - x.area() + i.area()).abs();
            pairs += 1;
            worst = worst.max(e1.max(e2));
            if e1.max(e2) > 1e-6 {
                bad += 1;
            }
            u
        };
        for path in &a.passes {
            // The same balanced tree of unions as `pocket::swept_path`.
            let mut parts: Vec<Region2> = path
                .windows(2)
                .map(|w| pocket::stadium(w[0], w[1], p.r))
                .collect();
            while parts.len() > 1 {
                parts = parts
                    .chunks(2)
                    .map(|c| match c {
                        [x, y] => check(x, y),
                        [x] => x.clone(),
                        _ => unreachable!(),
                    })
                    .collect();
            }
            if let Some(swept) = parts.pop() {
                check(&clear, &swept);
            }
        }
        println!("  {name}: {pairs} pairs, {bad} off by more than 1e-6, worst {worst:.1e} mm²");
    }
}
