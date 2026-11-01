// SPDX-License-Identifier: MIT OR Apache-2.0

//! Time of the exact medial axis (CH-17) on the S3 shapes and on text-like
//! outlines with many arcs: `cargo run -p fluut-cam --example medial_axis
//! --release`.

use fluut_cam::region::medial::medial_axis;
use fluut_cam::spike::shapes;
use fluut_cam::{Loop2, Region2, Vertex};
use std::f64::consts::{PI, TAU};
use std::time::Instant;

fn add(r: &mut Region2, verts: &[(f64, f64, f64)], dx: f64, dy: f64, hole: bool) {
    let l = Loop2 {
        verts: verts
            .iter()
            .map(|&(x, y, b)| Vertex::new(x + dx, y + dy, b))
            .collect(),
    };
    let l = if l.is_ccw() == hole { l.reversed() } else { l };
    r.loops.push(l);
}

/// A line of `n` groups of three lettering-like shapes: a curved stroke
/// with round ends, a bowl with a hole and a serifed stem.
fn text(n: usize) -> Region2 {
    let b = libm::tan(PI / 8.0);
    let mut r = Region2::empty();
    for k in 0..n {
        let dx = 60.0 * k as f64;
        add(
            &mut r,
            &[
                (10.0, 6.0, 1.0),
                (10.0, 24.0, 1.0),
                (10.0, 21.5, -1.0),
                (10.0, 8.5, 1.0),
            ],
            dx,
            0.0,
            false,
        );
        add(
            &mut r,
            &[
                (32.0, 3.0, b),
                (39.0, 15.0, b),
                (32.0, 27.0, b),
                (25.0, 15.0, b),
            ],
            dx,
            0.0,
            false,
        );
        add(
            &mut r,
            &[
                (32.0, 7.0, b),
                (35.0, 15.0, b),
                (32.0, 23.0, b),
                (29.0, 15.0, b),
            ],
            dx,
            0.0,
            true,
        );
        add(
            &mut r,
            &[
                (45.0, 3.0, 0.0),
                (55.0, 3.0, 0.0),
                (55.0, 5.0, 0.0),
                (51.5, 6.0, 0.0),
                (51.5, 24.0, 0.0),
                (55.0, 25.0, 0.0),
                (55.0, 27.0, 0.0),
                (45.0, 27.0, 0.0),
                (45.0, 25.0, 0.0),
                (48.5, 24.0, 0.0),
                (48.5, 6.0, 0.0),
                (45.0, 5.0, 0.0),
            ],
            dx,
            0.0,
            false,
        );
    }
    r
}

/// A badge: a scalloped outline of `n` arcs round a scalloped hole of
/// `n / 2` arcs, the way fitted font outlines are chains of short arcs.
fn badge(n: usize) -> Region2 {
    let ring = |rad: f64, m: usize, amp: f64| -> Vec<(f64, f64, f64)> {
        (0..m)
            .map(|i| {
                let a = TAU * i as f64 / m as f64;
                let rr = rad + if i % 2 == 0 { amp } else { -amp };
                let b = if i % 2 == 0 { 0.15 } else { -0.15 };
                (rr * libm::cos(a), rr * libm::sin(a), b)
            })
            .collect()
    };
    let mut r = Region2::empty();
    add(&mut r, &ring(60.0, n, 1.5), 0.0, 0.0, false);
    add(&mut r, &ring(25.0, n / 2, 1.0), 0.0, 0.0, true);
    r
}

fn time(name: &str, r: &Region2) {
    let sites: usize = r.loops.iter().map(|l| l.verts.len()).sum();
    let arcs = r.segs().filter(|s| !s.is_line()).count();
    let runs = 5;
    let mut best = f64::INFINITY;
    let mut m = None;
    for _ in 0..runs {
        let t = Instant::now();
        let a = medial_axis(r).expect("medial axis");
        best = best.min(t.elapsed().as_secs_f64());
        m = Some(a);
    }
    let m = m.unwrap();
    println!(
        "{name:<22} {sites:>5} segments ({arcs:>4} arcs) {:>6} nodes {:>6} edges {:>9.2} ms",
        m.nodes.len(),
        m.edges.len(),
        best * 1e3
    );
}

fn main() {
    println!("best of 5 runs, one thread");
    for (name, r) in shapes::all() {
        time(name, &r);
    }
    time("text x1", &text(1));
    time("text x10", &text(10));
    time("text x40", &text(40));
    time("badge 200 arcs", &badge(200));
    time("badge 1000 arcs", &badge(1000));
}
