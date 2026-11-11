// SPDX-License-Identifier: MIT OR Apache-2.0

//! Drop cost on B-spline-heavy models, in raster order (the order a toolpath
//! queries in): `cargo run -p fluut-cam --example drop_bench --release`.
//! With `FLUUT_STEP_CORPUS=<dir>` it also runs on the corpus bodies that
//! have B-spline faces. `S2_QUICK=1` shrinks the rasters.
//!
//! Per model and tool: median and p99 of one drop in µs (the best of
//! several repetitions, the machine being shared), and solves per drop.
//! `VERIFY=1` instead compares the drop with the drop that solves every
//! primitive at random points (`VERIFY_SEED=<n>` for other points;
//! `VERIFY_WINDOW=<w>` also checks a windowed drop, as a waterline makes).
//! `DUMP=<file>` writes the tip height of every raster drop (first
//! repetition), or with `VERIFY` of every drop at the random points, to the
//! file, to compare two builds.

use fluut_cam::cutter::drop::drop_all;
use fluut_cam::cutter::solve::SolveStats;
use fluut_cam::cutter::synth::{Lcg, sculpted, tiled};
use fluut_cam::cutter::{BuildOptions, Cutter, DropOptions, QueryGeometry, Seeding, drop};
use fluut_geom::{Aabb3, Surface};
use fluut_topo::BrepBody;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn quick() -> bool {
    std::env::var("S2_QUICK").is_ok()
}

fn body_box(b: &BrepBody) -> Aabb3 {
    let mut bx = Aabb3::empty();
    for v in &b.vertices {
        bx = bx.with_point(v.p);
    }
    bx
}

fn pct(sorted: &[f64], q: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

struct Run {
    times_us: Vec<f64>,
    stats: SolveStats,
    /// Tip heights, −∞ where nothing is under the tool.
    taus: Vec<f64>,
}

fn raster(g: &QueryGeometry, c: &Cutter, cols: usize, rows: usize) -> Run {
    let b = body_box(&g.body).expand(0.5 * c.radius());
    let o = DropOptions::default();
    let mut run = Run {
        times_us: Vec::new(),
        stats: SolveStats::default(),
        taus: Vec::new(),
    };
    for j in 0..rows {
        let y = b.min.y() + (b.max.y() - b.min.y()) * (j as f64 + 0.5) / rows as f64;
        for i in 0..cols {
            // Boustrophedon, as a zig-zag toolpath.
            let i = if j % 2 == 0 { i } else { cols - 1 - i };
            let x = b.min.x() + (b.max.x() - b.min.x()) * (i as f64 + 0.5) / cols as f64;
            let t = Instant::now();
            let r = drop(g, c, x, y, &o, &mut run.stats);
            run.times_us.push(t.elapsed().as_secs_f64() * 1e6);
            run.taus.push(r.map_or(f64::NEG_INFINITY, |r| r.tau));
        }
    }
    run
}

/// Median, p99 and mean of one run, µs.
fn stats_of(run: &Run) -> [f64; 3] {
    let mut t = run.times_us.clone();
    t.sort_by(f64::total_cmp);
    [
        pct(&t, 0.5),
        pct(&t, 0.99),
        t.iter().sum::<f64>() / t.len() as f64,
    ]
}

/// The runs repeat (the machine is shared): each figure is the best of the
/// repetitions.
fn best_of(runs: &[Run]) -> [f64; 3] {
    let mut best = [f64::INFINITY; 3];
    for r in runs {
        for (b, v) in best.iter_mut().zip(stats_of(r)) {
            *b = b.min(v);
        }
    }
    best
}

fn report(label: &str, runs: &[Run]) {
    let run = &runs[0];
    let n = run.times_us.len() as f64;
    let s = &run.stats;
    let [median, p99, mean] = best_of(runs);
    println!(
        "  {label}: {} drops, median {median:.1} µs, p99 {p99:.1} µs, mean {mean:.1} µs; per drop: {:.2} numeric faces, {:.2} closed form, {:.2} edges, Newton {:.1} runs, {:.3} rescue solves",
        run.times_us.len(),
        s.faces_numeric as f64 / n,
        s.faces_closed_form as f64 / n,
        s.edges as f64 / n,
        s.newton_runs as f64 / n,
        s.rescues as f64 / n,
    );
}

fn measure(name: &str, body: &BrepBody, cols: usize, rows: usize) {
    let t = Instant::now();
    let g = QueryGeometry::new(body, &BuildOptions::default());
    println!(
        "{name}: {} faces, {} primitives, build {:.2} s",
        body.faces.len(),
        g.prims.len(),
        t.elapsed().as_secs_f64()
    );
    for (label, c) in [
        ("flat 6", Cutter::flat(6.0).unwrap()),
        ("ball 6", Cutter::ball(6.0).unwrap()),
        ("bull 6 r1", Cutter::new(6.0, 1.0).unwrap()),
    ] {
        let reps = if quick() { 3 } else { 5 };
        let runs: Vec<Run> = (0..reps).map(|_| raster(&g, &c, cols, rows)).collect();
        report(label, &runs);
        if let Ok(path) = std::env::var("DUMP") {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("dump file");
            for (k, t) in runs[0].taus.iter().enumerate() {
                writeln!(f, "{name}|{label}|{k}|{t:?}").expect("dump write");
            }
        }
    }
}

/// Misses of the drop against the drop that solves every primitive (no
/// pruning), at `n` points of a random walk.
fn verify(name: &str, body: &BrepBody, n: usize) {
    // `VERIFY=<text>` runs only the models whose name has the text.
    if let Ok(only) = std::env::var("VERIFY")
        && only != "1"
        && !name.contains(&only)
    {
        return;
    }
    let g = QueryGeometry::new(body, &BuildOptions::default());
    let b = body_box(body);
    println!("{name}: misses against the full scan, {n} points per tool");
    for (label, c) in [
        ("flat 6", Cutter::flat(6.0).unwrap()),
        ("ball 6", Cutter::ball(6.0).unwrap()),
        ("bull 6 r1", Cutter::new(6.0, 1.0).unwrap()),
    ] {
        let mut rng = Lcg(std::env::var("VERIFY_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4242));
        let o = DropOptions::default();
        let mut st = SolveStats::default();
        let b = b.expand(3.0);
        let (mut below, mut above, mut worst) = (0, 0, 0.0f64);
        // `VERIFY_WINDOW=<w>`: also a windowed drop (floor and ceiling
        // `w` either side of a level near the answer, as a waterline's).
        let window: Option<f64> = std::env::var("VERIFY_WINDOW")
            .ok()
            .and_then(|s| s.parse().ok());
        let mut win_bad = 0;
        let mut dump = std::env::var("DUMP").ok().map(|path| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("dump file")
        });
        for k in 0..n {
            let x = b.min.x() + (b.max.x() - b.min.x()) * rng.uniform();
            let y = b.min.y() + (b.max.y() - b.min.y()) * rng.uniform();
            let tau =
                |r: Option<fluut_cam::cutter::Contact>| r.map_or(f64::NEG_INFINITY, |r| r.tau);
            let all = tau(drop_all(&g, &c, x, y, &Seeding::default(), &mut st));
            let got = tau(drop(&g, &c, x, y, &o, &mut st));
            if got < all - 1e-7 {
                below += 1;
                println!("    miss at ({x:?}, {y:?}): {got} against {all}");
                worst = worst.max(all - got);
            }
            above += (got > all + 1e-7) as usize;
            if let Some(w) = window {
                // A level near the answer and the windowed drop a
                // waterline makes there.
                let z = if all.is_finite() {
                    all + w * (3.0 * rng.uniform() - 1.5)
                } else {
                    0.0
                };
                let wo = DropOptions {
                    floor: z - w,
                    ceiling: z + w,
                    ..o
                };
                let win = tau(drop(&g, &c, x, y, &wo, &mut st));
                let bad = if all >= z + w {
                    win < z + w
                } else if all > z - w {
                    win < all - 1e-7
                } else {
                    win > z - w && win > all + 1e-7
                };
                if bad {
                    win_bad += 1;
                    println!("    window miss at ({x:?}, {y:?}) level {z}: {win} against {all}");
                }
                if let Some(f) = &mut dump {
                    use std::io::Write;
                    writeln!(f, "{name}|{label} window|{k}|{win:?}").expect("dump write");
                }
            }
            if let Some(f) = &mut dump {
                use std::io::Write;
                writeln!(f, "{name}|{label}|{k}|{got:?}").expect("dump write");
            }
        }
        println!(
            "  {label}: {below} below (worst {worst:.2e} mm), {above} above; {} rescue solves",
            st.rescues
        );
        if window.is_some() {
            println!("  {label}: {win_bad} windowed drops wrong");
        }
    }
}

fn step_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            step_files(&p, out);
        } else if p
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("stp") || e.eq_ignore_ascii_case("step"))
        {
            out.push(p);
        }
    }
}

fn main() {
    let q = quick();
    if std::env::var("VERIFY").is_ok() {
        let n = std::env::var("VERIFY_N")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(if q { 100 } else { 3000 });
        verify("sculpted 4 x 4 faces of 12 mm", &sculpted(4, 12.0, 9), n);
        verify("sculpted 6 x 6 faces of 12 mm", &sculpted(6, 12.0, 9), n);
        verify("sculpted 12 x 12 faces of 3 mm", &sculpted(12, 3.0, 5), n);
        verify("rough tiles 4 x 4", &tiled(4, true), n);
        return;
    }
    let (cols, rows) = if q { (150, 40) } else { (500, 120) };
    measure(
        "synthetic 5 000 faces, B-spline, rough (S2 tiles)",
        &tiled(25, true),
        cols,
        rows,
    );
    measure(
        "sculpted, 4 900 faces of 3 mm",
        &sculpted(70, 3.0, 5),
        cols,
        rows,
    );
    measure(
        "sculpted, 256 faces of 12 mm",
        &sculpted(16, 12.0, 9),
        cols,
        rows,
    );
    let Ok(dir) = std::env::var("FLUUT_STEP_CORPUS") else {
        println!("FLUUT_STEP_CORPUS not set: corpus skipped");
        return;
    };
    let mut files = Vec::new();
    step_files(Path::new(&dir), &mut files);
    let mut free: Vec<(usize, String, BrepBody)> = Vec::new();
    for f in &files {
        let Ok(src) = std::fs::read(f) else { continue };
        let Ok(m) = fluut_io_step::parse(&src) else {
            continue;
        };
        let name = f
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        for (k, mb) in fluut_io_step::map_bodies(&m)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let n = mb
                .body
                .surfaces
                .iter()
                .filter(|s| matches!(s, Surface::BSpline(_)))
                .count();
            if n > 0 {
                free.push((n, format!("{name}#{k}"), mb.body));
            }
        }
    }
    free.sort_by_key(|f| std::cmp::Reverse(f.0));
    println!("corpus bodies with B-spline faces: {}", free.len());
    for (n, name, body) in free.iter().take(2) {
        measure(
            &format!("{name} ({n} B-spline surfaces)"),
            body,
            cols / 2,
            rows,
        );
    }
}
