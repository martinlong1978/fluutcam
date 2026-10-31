// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::motion::Chorder;
use crate::stock::Stock;
use crate::tool::Tool;
use fluut_geom::{Point3, UnitVec3};
use fluut_path::{AxisPos, FeedRef, Motion, MotionInfo, Segment};

fn m() -> MotionInfo {
    MotionInfo::new(Motion::Cut, FeedRef::Cut, 0.01)
}

fn xyz(x: f64, y: f64, z: f64) -> AxisPos {
    AxisPos::xyz(Point3::new(x, y, z))
}

fn line(a: AxisPos, b: AxisPos) -> Segment {
    Segment::Line { a, b, m: m() }
}

fn boxed(x: f64, y: f64, z: f64) -> Stock {
    Stock::Box {
        min: Point3::new(0.0, 0.0, 0.0),
        max: Point3::new(x, y, z),
    }
}

fn run(stock: Stock, tool: Tool, rotary: bool, res: f64, segs: &[Segment]) -> TriDexel {
    let mut d = TriDexel::new(&stock, res);
    let chords = Chorder::new(tool, stock, rotary, res).chords(segs);
    cut(&mut d, &chords);
    d
}

/// Top of the material on Z ray `(i, j)`, in stock coordinates.
fn top(d: &TriDexel, i: usize, j: usize) -> Option<f64> {
    let g = &d.grids[2];
    g.rays[j * g.nu + i]
        .last()
        .map(|s| d.origin.z + f64::from(s.hi))
}

#[test]
fn ball_slot_leaves_round_profile() {
    let d = run(
        boxed(20.0, 10.0, 10.0),
        Tool::ball(2.0),
        false,
        0.1,
        &[line(xyz(-5.0, 5.0, 7.0), xyz(25.0, 5.0, 7.0))],
    );
    let g = &d.grids[2];
    for j in 0..g.nv {
        let y = (j as f64 + 0.5) * 0.1;
        let want = if (y - 5.0).abs() < 2.0 {
            9.0 - (4.0 - (y - 5.0) * (y - 5.0)).sqrt()
        } else {
            10.0
        };
        for i in [0, 77, 199] {
            let got = top(&d, i, j).unwrap();
            assert!((got - want).abs() < 1e-5, "y={y}: {got} vs {want}");
        }
    }
    // X rays inside the slot's cross-section are empty.
    let gx = &d.grids[0];
    let (iy, kz) = (50, 85); // y = 5.05, z = 8.55
    assert!(gx.rays[kz * gx.nu + iy].is_empty());
    let (iy, kz) = (50, 60); // z = 6.05, below the slot
    assert_eq!(gx.rays[kz * gx.nu + iy].len(), 1);
}

#[test]
fn arc_groove_follows_circle() {
    let a = xyz(15.0, 10.0, 8.0);
    let arc = Segment::Arc {
        a,
        b: a,
        centre: Point3::new(10.0, 10.0, 8.0),
        normal: UnitVec3::Z,
        sweep: std::f64::consts::TAU,
        m: m(),
    };
    let res = 0.1;
    let d = run(boxed(20.0, 20.0, 10.0), Tool::ball(1.0), false, res, &[arc]);
    let g = &d.grids[2];
    let profile = |e: f64| {
        if e.abs() < 1.0 {
            9.0 - (1.0 - e * e).sqrt()
        } else {
            10.0
        }
    };
    // Chords lie inside the circle by at most the sagitta, res / 4, so the
    // groove is the true one shifted radially by up to that much.
    let s = res / 4.0 + 1e-6;
    for j in 0..g.nv {
        for i in 0..g.nu {
            let (x, y) = ((i as f64 + 0.5) * res, (j as f64 + 0.5) * res);
            let e = ((x - 10.0) * (x - 10.0) + (y - 10.0) * (y - 10.0)).sqrt() - 5.0;
            let (lo, hi) = (0..=20)
                .map(|n| profile(e - s + 2.0 * s * f64::from(n) / 20.0))
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
                    (a.min(p), b.max(p))
                });
            let got = top(&d, i, j).unwrap();
            assert!(
                got >= lo - 1e-5 && got <= hi + 1e-5,
                "({x}, {y}): {got} not in [{lo}, {hi}]"
            );
        }
    }
}

#[test]
fn full_rotary_turn_cuts_a_ring() {
    let stock = Stock::Cylinder {
        x0: 0.0,
        x1: 20.0,
        radius: 10.0,
    };
    let p = |a: f64| AxisPos {
        a: Some(a),
        ..xyz(10.0, 0.0, 8.0)
    };
    let seg = Segment::Rotary {
        a: p(0.0),
        b: p(360.0),
        radius: 8.0,
        m: m(),
    };
    let res = 0.1;
    let d = run(stock, Tool::ball(1.0), true, res, &[seg]);
    // Rays through x = 10.05 along Y and Z: material from −r to r with r
    // between 8 (tip radius) and 8 + sagitta.
    let tol = res / 4.0 + 1e-4;
    for k in [1, 2] {
        let g = &d.grids[k];
        // Ray (100, 100): x = 10.05 and 0.05 off the axis (family 1 has
        // u = z, v = x; family 2 has u = x, v = y).
        let spans = &g.rays[100 * g.nu + 100];
        assert_eq!(spans.len(), 1, "family {k}: {spans:?}");
        let (lo, hi) = (f64::from(spans[0].lo) - 10.0, f64::from(spans[0].hi) - 10.0);
        let r = 8.0 + 1.0 - (1.0 - 0.05 * 0.05_f64).sqrt();
        let r = (r * r - 0.05 * 0.05).sqrt();
        assert!(
            (lo + r).abs() < tol && (hi - r).abs() < tol,
            "family {k}: {lo} {hi} vs ±{r}"
        );
    }
    // Outside the ring the cylinder is untouched.
    let g = &d.grids[2];
    assert_eq!(g.rays[100 * g.nu + 20].len(), 1);
    let s = g.rays[100 * g.nu + 20][0];
    assert!(f64::from(s.hi - s.lo) > 19.9);
}

fn zigzag(w: f64, h: f64, z: f64, step: f64, x0: f64, y0: f64) -> Vec<Segment> {
    let mut segs = Vec::new();
    let mut y = y0;
    let mut forward = true;
    let mut prev = xyz(x0, y0, z);
    loop {
        let x = if forward { x0 + w } else { x0 };
        let next = xyz(x, y, z);
        segs.push(line(prev, next));
        prev = next;
        if y >= y0 + h {
            break;
        }
        y = (y + step).min(y0 + h);
        let next = xyz(x, y, z);
        segs.push(line(prev, next));
        prev = next;
        forward = !forward;
    }
    segs
}

#[test]
fn pocket_volume_matches_analytic() {
    // Tool centres cover a 10 × 6 rectangle; the cut is that rectangle grown by
    // the radius, 3 deep.
    let r = 2.0;
    let segs = zigzag(10.0, 6.0, 7.0, 1.0, 5.0, 5.0);
    for tool in [Tool::flat(r), Tool::bull(r, 0.5)] {
        let d = run(boxed(20.0, 16.0, 10.0), tool, false, 0.05, &segs);
        let area = 60.0 + 2.0 * r * 16.0 + std::f64::consts::PI * r * r;
        // A bull nose leaves a fillet of section c² − πc²/4 along the wall,
        // whose centroid is 0.2234·c from the wall (Pappus for the corners).
        let c = tool.corner;
        let fillet = (c * c - std::f64::consts::FRAC_PI_4 * c * c)
            * (32.0 + std::f64::consts::TAU * (r - 0.2234 * c));
        let removed = area * 3.0 - fillet;
        let full = 20.0 * 16.0 * 10.0;
        for v in d.volumes() {
            let got = full - v;
            assert!(
                (got - removed).abs() / removed < 0.005,
                "{tool:?}: {got} vs {removed}"
            );
        }
    }
}

#[test]
fn same_hash_on_one_and_many_threads() {
    let segs = zigzag(30.0, 20.0, 6.0, 0.7, -2.0, -2.0);
    let stock = boxed(25.0, 18.0, 8.0);
    let tool = Tool::bull(1.5, 0.5);
    let run_on = |n: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .unwrap();
        pool.install(|| run(stock, tool, false, 0.1, &segs).hash())
    };
    assert_eq!(run_on(1), run_on(4));
}
