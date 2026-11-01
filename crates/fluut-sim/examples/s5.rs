// SPDX-License-Identifier: MIT OR Apache-2.0

//! S5 measurements: memory, update time and error of the tri-dexel on the
//! relief and rotary stand-ins.
//!
//! `cargo run -p fluut-sim --example s5 --release -- [relief|rotary|error|rays] [res...]`

use fluut_geom::Point3;
use fluut_path::Segment;
use fluut_sim::spike::probe::top_error;
use fluut_sim::spike::{relief, rotary};
use fluut_sim::sweep::Method;
use fluut_sim::{Chorder, Pose, Stock, Sweep, Tool, TriDexel, cut};
use glam::DVec3;
use std::time::Instant;

fn pool(n: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build()
        .expect("thread pool")
}

fn mb(b: usize) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

/// One toolpath of a project.
struct Stage<'a> {
    label: &'a str,
    tool: Tool,
    segs: &'a [Segment],
}

fn project(name: &str, stock: Stock, rotary: bool, stages: &[Stage], res: &[f64]) {
    let all = std::thread::available_parallelism().map_or(1, |n| n.get());
    println!("\n## {name}");
    for st in stages {
        println!(
            "- {}: {} segments, tool R{} r{}",
            st.label,
            st.segs.len(),
            st.tool.radius,
            st.tool.corner
        );
    }
    println!(
        "\n| res | stage | rays | chords | cut 1 thread (s) | cut {all} threads (s) | µs/move ({all} thr) | s per M chords ({all} thr) | memory (MB) | spilled rays | tests X/Y/Z per chord | time X/Y/Z ({all} thr, s) | removed (cm³) |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for &r in res {
        let fresh = TriDexel::new(&stock, r);
        let before = fresh.memory();
        println!(
            "| {r} | stock | {} | | | | | | {:.0} | 0 | | | |",
            before.rays,
            mb(before.total())
        );
        let chords: Vec<Vec<Sweep>> = stages
            .iter()
            .map(|st| Chorder::new(st.tool, stock, rotary, r).chords(st.segs))
            .collect();
        let one = r >= 0.2 || std::env::var("S5_ONE").is_ok();
        let mut d1 = fresh.clone();
        let mut dn = fresh.clone();
        let p1 = pool(1);
        let pn = pool(all);
        for (st, ch) in stages.iter().zip(&chords) {
            let t1 = one.then(|| {
                let t = Instant::now();
                p1.install(|| cut(&mut d1, ch));
                t.elapsed().as_secs_f64()
            });
            let v0 = dn.volumes()[2];
            let t = Instant::now();
            let stats = pn.install(|| cut(&mut dn, ch));
            let tn = t.elapsed().as_secs_f64();
            let mem = dn.memory();
            let nc = ch.len().max(1) as f64;
            println!(
                "| {r} | {} | | {} | {} | {:.2} | {:.1} | {:.1} | {:.0} | {} | {:.0} / {:.0} / {:.0} | {:.1} / {:.1} / {:.1} | {:.1} |",
                st.label,
                ch.len(),
                t1.map_or("–".to_string(), |t| format!("{t:.2}")),
                tn,
                tn / st.segs.len() as f64 * 1e6,
                tn / nc * 1e6,
                mb(mem.total()),
                mem.spilled,
                stats.tests[0] as f64 / nc,
                stats.tests[1] as f64 / nc,
                stats.tests[2] as f64 / nc,
                stats.secs[0],
                stats.secs[1],
                stats.secs[2],
                (v0 - dn.volumes()[2]) / 1000.0,
            );
        }
        if one {
            assert_eq!(d1.hash(), dn.hash(), "thread count changed the result");
        }
    }
}

/// Deterministic points in a rectangle.
fn samples(x0: f64, x1: f64, y0: f64, y1: f64, n: usize) -> Vec<(f64, f64)> {
    let mut s: u64 = 12345;
    let mut next = || {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    (0..n)
        .map(|_| (x0 + (x1 - x0) * next(), y0 + (y1 - y0) * next()))
        .collect()
}

fn error(name: &str, stock: Stock, tool: Tool, rotary: bool, segs: &[Segment], res: &[f64]) {
    let b = stock.bbox();
    let m = 1.0;
    let pts = samples(
        b.min.x() + m,
        b.max.x() - m,
        b.min.y() + m,
        b.max.y() - m,
        4000,
    );
    println!("\n## Error, {name}: window {:?} – {:?}", b.min, b.max);
    println!("| res | samples | max (mm) | p99 (mm) | RMS (mm) | cut time (s) |");
    println!("|---|---|---|---|---|---|");
    // Exact probes use chords at the finest resolution's tolerance.
    let fine = Chorder::new(tool, stock, rotary, 0.002).chords(segs);
    for &r in res {
        let mut d = TriDexel::new(&stock, r);
        // Only the Z family is measured; drop the others to save time.
        for k in 0..2 {
            d.grids[k].rays.clear();
            d.grids[k].nv = 0;
        }
        let chords = Chorder::new(tool, stock, rotary, r).chords(segs);
        let t = Instant::now();
        cut(&mut d, &chords);
        let tc = t.elapsed().as_secs_f64();
        let e = top_error(&d, &stock, &fine, &pts);
        println!(
            "| {r} | {} | {:.4} | {:.4} | {:.4} | {:.1} |",
            e.samples, e.max, e.p99, e.rms, tc
        );
    }
}

/// Time per ray for each intersection method.
fn rays() {
    println!("\n## Ray cost (one thread)");
    println!("| method | tool | ns per ray | hits |");
    println!("|---|---|---|---|");
    let pose = |tip: DVec3, axis: DVec3| Pose {
        tip,
        axis,
        length: 15.0,
    };
    let ramp = DVec3::new(0.45, 0.2, -0.1);
    let level = DVec3::new(0.45, 0.2, 0.0);
    let cases = [
        (
            "ball, side line",
            Tool::ball(1.5),
            DVec3::Z,
            DVec3::Z,
            DVec3::X,
        ),
        ("axial", Tool::ball(1.5), DVec3::Z, DVec3::Z, DVec3::Z),
        ("axial", Tool::bull(3.0, 1.0), DVec3::Z, DVec3::Z, DVec3::Z),
        ("axial", Tool::flat(3.0), DVec3::Z, DVec3::Z, DVec3::Z),
        (
            "side, level chord",
            Tool::bull(3.0, 1.0),
            DVec3::Z,
            DVec3::Z,
            DVec3::X,
        ),
        (
            "side, level chord",
            Tool::flat(3.0),
            DVec3::Z,
            DVec3::Z,
            DVec3::X,
        ),
        (
            "side, ramp",
            Tool::bull(3.0, 1.0),
            DVec3::Z,
            DVec3::Z,
            DVec3::X,
        ),
        ("side, ramp", Tool::flat(3.0), DVec3::Z, DVec3::Z, DVec3::X),
        (
            "ball, rotary chord",
            Tool::ball(3.0),
            DVec3::Z,
            DVec3::new(0.0, 0.03, 1.0).normalize(),
            DVec3::Y,
        ),
        (
            "GJK, rotary chord",
            Tool::bull(3.0, 1.0),
            DVec3::Z,
            DVec3::new(0.0, 0.03, 1.0).normalize(),
            DVec3::Y,
        ),
    ];
    for (label, tool, w0, w1, e) in cases {
        let d = if label.contains("level") { level } else { ramp };
        let s = Sweep::new(tool, pose(DVec3::ZERO, w0), pose(d, w1));
        let n = 200_000;
        let mut hits = 0;
        let t = Instant::now();
        for i in 0..n {
            let f = (i % 1000) as f64 / 1000.0;
            let g = (i / 1000) as f64 / 200.0;
            let (u, v) = (
                -3.5 + 7.5 * f,
                if e == DVec3::Z {
                    -3.5 + 7.5 * g
                } else {
                    -0.5 + 10.0 * g
                },
            );
            let o = if e == DVec3::X {
                DVec3::new(-10.0, u, v)
            } else if e == DVec3::Y {
                DVec3::new(u, -10.0, v)
            } else {
                DVec3::new(u, v, -10.0)
            };
            if std::hint::black_box(s.ray(o, e)).is_some() {
                hits += 1;
            }
        }
        let ns = t.elapsed().as_secs_f64() / n as f64 * 1e9;
        let m = s.method(e);
        assert!(match label {
            l if l.starts_with("ball") => m == Method::Ball,
            l if l.starts_with("axial") => m == Method::Axial,
            l if l.starts_with("side") => m == Method::Side,
            _ => m == Method::Gjk,
        });
        println!(
            "| {label} | R{} r{} | {ns:.0} | {hits} |",
            tool.radius, tool.corner
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let what = args.first().map_or("all", String::as_str);
    let res: Vec<f64> = args.iter().skip(1).filter_map(|a| a.parse().ok()).collect();
    let pick = |default: &[f64]| {
        if res.is_empty() {
            default.to_vec()
        } else {
            res.clone()
        }
    };
    if what == "rays" || what == "all" {
        rays();
    }
    if what == "relief" || what == "all" {
        let rough = relief::roughing(2.4, 0.5);
        let finish = relief::program(0.3, 0.5);
        let rough_stage = Stage {
            label: "rough (flat Ø6, Z-levels)",
            tool: relief::rough_tool(),
            segs: &rough,
        };
        let finish_stage = Stage {
            label: "finish (ball Ø3 raster)",
            tool: relief::tool(),
            segs: &finish,
        };
        let levels = pick(&[0.4, 0.2, 0.1]);
        project(
            "Relief, rough then finish",
            relief::STOCK,
            false,
            &[rough_stage, finish_stage],
            &levels,
        );
        let finish_stage = Stage {
            label: "finish from full stock",
            tool: relief::tool(),
            segs: &finish,
        };
        project(
            "Relief, finish only (worst case)",
            relief::STOCK,
            false,
            &[finish_stage],
            &levels,
        );
    }
    if what == "rotary" || what == "all" {
        let segs = rotary::program(0.5, 2.0);
        let stage = Stage {
            label: "spiral wrap (ball Ø6)",
            tool: rotary::tool(),
            segs: &segs,
        };
        project(
            "Rotary",
            rotary::STOCK,
            true,
            &[stage],
            &pick(&[0.4, 0.2, 0.1]),
        );
    }
    if what == "error" || what == "all" {
        let levels = pick(&[0.4, 0.2, 0.1, 0.05, 0.025]);
        let w = Stock::Box {
            min: Point3::new(95.0, 70.0, 0.0),
            max: Point3::new(105.0, 80.0, 25.0),
        };
        error(
            "relief",
            w,
            relief::tool(),
            false,
            &relief::program(0.3, 0.5),
            &levels,
        );
        let w = Stock::Box {
            min: Point3::new(65.0, -6.0, 18.0),
            max: Point3::new(75.0, 6.0, 31.0),
        };
        error(
            "rotary",
            w,
            rotary::tool(),
            true,
            &rotary::program(0.5, 2.0),
            &levels,
        );
    }
}
