// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spike S2 measurements: `cargo run -p fluut-cam --example s2 --release`.
//! With `FLUUT_STEP_CORPUS=<dir>` it also runs on the corpus bodies and on
//! an array of the largest one. `S2_QUICK=1` shrinks every sample.

use fluut_cam::cutter::reference::{Density, reference};
use fluut_cam::cutter::solve::{Axis, SolveStats};
use fluut_cam::cutter::synth::{Lcg, tiled};
use fluut_cam::cutter::{BuildOptions, Contact, Cutter, DropOptions, QueryGeometry, Seeding, drop};
use fluut_geom::{Aabb3, Affine, Vec3};
use fluut_topo::BrepBody;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn quick() -> bool {
    std::env::var("S2_QUICK").is_ok()
}

fn cutters() -> [(&'static str, Cutter); 3] {
    [
        ("flat 6", Cutter::flat(6.0).unwrap()),
        ("ball 6", Cutter::ball(6.0).unwrap()),
        ("bull 6 r1", Cutter::new(6.0, 1.0).unwrap()),
    ]
}

fn body_box(b: &BrepBody) -> Aabb3 {
    let mut bx = Aabb3::empty();
    for v in &b.vertices {
        bx = bx.with_point(v.p);
    }
    bx
}

fn pct(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

/// Raster of drop queries: `cols` points per line, `rows` lines.
struct Run {
    times_us: Vec<f64>,
    stats: SolveStats,
    contacts: usize,
}

fn raster(g: &QueryGeometry, c: &Cutter, o: &DropOptions, cols: usize, rows: usize) -> Run {
    let b = body_box(&g.body).expand(c.radius());
    let mut run = Run {
        times_us: Vec::with_capacity(cols * rows),
        stats: SolveStats::default(),
        contacts: 0,
    };
    for j in 0..rows {
        let y = b.min.y() + (b.max.y() - b.min.y()) * (j as f64 + 0.5) / rows as f64;
        for i in 0..cols {
            let x = b.min.x() + (b.max.x() - b.min.x()) * (i as f64 + 0.5) / cols as f64;
            let t = Instant::now();
            let r = drop(g, c, x, y, o, &mut run.stats);
            run.times_us.push(t.elapsed().as_secs_f64() * 1e6);
            run.contacts += r.is_some() as usize;
        }
    }
    run.times_us.sort_by(f64::total_cmp);
    run
}

fn report(label: &str, run: &Run) {
    let n = run.times_us.len() as f64;
    let mean = run.times_us.iter().sum::<f64>() / n;
    let s = &run.stats;
    println!(
        "  {label}: {} queries, median {:.1} µs, p99 {:.1} µs, mean {:.1} µs; per query: {:.2} faces numeric, {:.2} closed form, {:.2} edges, {:.2} vertices, Newton {:.1} runs ({:.0} % converged)",
        run.times_us.len(),
        pct(&run.times_us, 0.5),
        pct(&run.times_us, 0.99),
        mean,
        s.faces_numeric as f64 / n,
        s.faces_closed_form as f64 / n,
        s.edges as f64 / n,
        s.vertices as f64 / n,
        s.newton_runs as f64 / n,
        100.0 * s.newton_converged as f64 / s.newton_runs.max(1) as f64,
    );
}

/// Misses against the reference at `n` random points over the body.
#[derive(Default)]
struct Misses {
    points: usize,
    misses: usize,
    worst: f64,
    over: usize,
    /// Misses that remain with 1e-6 mm trim polylines (the rest come from
    /// the trim chord), and the worst of them.
    real: usize,
    real_worst: f64,
}

fn misses(g: &QueryGeometry, c: &Cutter, seeding: Seeding, n: usize, seed: u64) -> Misses {
    let b = body_box(&g.body).expand(0.5 * c.radius());
    let mut rng = Lcg(seed);
    let o = DropOptions {
        seeding,
        ..Default::default()
    };
    let mut m = Misses::default();
    let mut fine: Option<QueryGeometry> = None;
    let dens = Density {
        face: 40,
        edge: 300,
    };
    for _ in 0..n {
        let x = b.min.x() + (b.max.x() - b.min.x()) * rng.uniform();
        let y = b.min.y() + (b.max.y() - b.min.y()) * rng.uniform();
        let mut st = SolveStats::default();
        let got: Option<Contact> = drop(g, c, x, y, &o, &mut st);
        let want = reference(g, c, Axis { x, y }, dens);
        m.points += 1;
        let tau = got.map_or(f64::NEG_INFINITY, |r| r.tau);
        if let Some(w) = want {
            if tau < w - 1e-7 {
                m.misses += 1;
                m.worst = m.worst.max(w - tau);
                let f = fine.get_or_insert_with(|| {
                    QueryGeometry::new(
                        &g.body,
                        &BuildOptions {
                            trim_chord: 1e-6,
                            ..Default::default()
                        },
                    )
                });
                let ft = drop(f, c, x, y, &o, &mut st).map_or(f64::NEG_INFINITY, |r| r.tau);
                let fw = reference(f, c, Axis { x, y }, dens).unwrap_or(f64::NEG_INFINITY);
                if ft < fw - 1e-7 {
                    m.real += 1;
                    m.real_worst = m.real_worst.max(fw - ft);
                    println!(
                        "    miss: {} {c:?} at ({x}, {y}): {ft} < {fw} with fine trims",
                        g.body.name
                    );
                }
            }
            if tau > w + 1e-5 {
                m.over += 1;
            }
        }
    }
    m
}

fn print_misses(label: &str, m: &Misses) {
    println!(
        "  {label}: {} / {} points miss the reference by > 1e-7 (worst {:.2e} mm), {} with 1e-6 mm trims (worst {:.2e} mm); {} above it by > 1e-5",
        m.misses, m.points, m.worst, m.real, m.real_worst, m.over
    );
}

fn measure(name: &str, body: &BrepBody, raster_n: (usize, usize), miss_n: usize) {
    let t = Instant::now();
    let g = QueryGeometry::new(body, &BuildOptions::default());
    let build = t.elapsed().as_secs_f64();
    let b = body_box(body);
    println!(
        "{name}: {} faces, {} edges, {} primitives, {:.0} × {:.0} × {:.0} mm, build {:.2} s, {} faces without trim",
        body.faces.len(),
        body.edges.len(),
        g.prims.len(),
        b.max.x() - b.min.x(),
        b.max.y() - b.min.y(),
        b.max.z() - b.min.z(),
        build,
        g.issues.len()
    );
    for (label, c) in cutters() {
        let run = raster(&g, &c, &DropOptions::default(), raster_n.0, raster_n.1);
        report(label, &run);
        if miss_n > 0 {
            print_misses(label, &misses(&g, &c, Seeding::default(), miss_n, 99));
        }
    }
}

/// Seeding study (CH-12): miss rates and speed of the face solver.
fn seeding_study(name: &str, body: &BrepBody, n: usize) {
    let g = QueryGeometry::new(body, &BuildOptions::default());
    println!("seeding on {name} ({n} reference points per tool):");
    for seeding in [
        Seeding {
            grid: 2,
            rings: 0,
            starts: 1,
        },
        Seeding {
            grid: 4,
            rings: 1,
            starts: 1,
        },
        Seeding::default(),
        Seeding {
            grid: 6,
            rings: 3,
            starts: 6,
        },
    ] {
        for (label, c) in cutters() {
            let o = DropOptions {
                seeding,
                ..Default::default()
            };
            let run = raster(&g, &c, &o, 100, 25);
            let m = misses(&g, &c, seeding, n, 5);
            println!(
                "  grid {} rings {} starts {} {label}: median {:.1} µs, {} / {} misses (worst {:.2e} mm), {} with fine trims",
                seeding.grid,
                seeding.rings,
                seeding.starts,
                pct(&run.times_us, 0.5),
                m.misses,
                m.points,
                m.worst,
                m.real
            );
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

/// `body` repeated on an `nx × ny` grid with gaps of `gap` mm.
fn array(body: &BrepBody, nx: usize, ny: usize, gap: f64) -> BrepBody {
    let b = body_box(body);
    let (sx, sy) = (b.max.x() - b.min.x() + gap, b.max.y() - b.min.y() + gap);
    let mut out = BrepBody {
        name: format!("{} × {}", body.name, nx * ny),
        tol_model: body.tol_model,
        ..Default::default()
    };
    for i in 0..nx {
        for j in 0..ny {
            let a = Affine::translation(Vec3::new(i as f64 * sx, j as f64 * sy, 0.0));
            let (ns, nc, nv, ne, nce, nl) = (
                out.surfaces.len() as u32,
                out.curves.len() as u32,
                out.vertices.len() as u32,
                out.edges.len() as u32,
                out.coedges.len() as u32,
                out.loops.len() as u32,
            );
            let nf = out.faces.len() as u32;
            out.surfaces.extend(
                body.surfaces
                    .iter()
                    .map(|s| s.transformed(&a).expect("translation")),
            );
            out.curves.extend(
                body.curves
                    .iter()
                    .map(|c| c.transformed(&a).expect("translation")),
            );
            out.vertices
                .extend(body.vertices.iter().map(|v| fluut_topo::Vertex {
                    p: a.apply_point(v.p),
                }));
            out.edges
                .extend(body.edges.iter().map(|e| fluut_topo::Edge {
                    curve: e.curve.map(|c| c + nc),
                    range: e.range,
                    v0: e.v0 + nv,
                    v1: e.v1 + nv,
                }));
            out.coedges
                .extend(body.coedges.iter().map(|c| fluut_topo::Coedge {
                    edge: c.edge + ne,
                    forward: c.forward,
                }));
            out.loops
                .extend(body.loops.iter().map(|l| fluut_topo::Loop {
                    outer: l.outer,
                    coedges: l.coedges.iter().map(|c| c + nce).collect(),
                }));
            out.faces
                .extend(body.faces.iter().map(|f| fluut_topo::Face {
                    surface: f.surface + ns,
                    same_sense: f.same_sense,
                    loops: f.loops.iter().map(|l| l + nl).collect(),
                }));
            out.shells
                .extend(body.shells.iter().map(|s| fluut_topo::Shell {
                    faces: s.faces.iter().map(|f| f + nf).collect(),
                    closed: s.closed,
                }));
        }
    }
    out
}

fn corpus(dir: &str) {
    let mut files = Vec::new();
    step_files(Path::new(dir), &mut files);
    let mut bodies: Vec<(String, BrepBody)> = Vec::new();
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
            bodies.push((format!("{name}#{k}"), mb.body));
        }
    }
    // Trims and build over the whole corpus.
    let (mut faces, mut no_trim, mut prims, mut secs) = (0, 0, 0, 0.0);
    let mut kinds = std::collections::BTreeMap::<String, (usize, usize)>::new();
    for (name, b) in &bodies {
        let t = Instant::now();
        let g = QueryGeometry::new(b, &BuildOptions::default());
        secs += t.elapsed().as_secs_f64();
        faces += b.faces.len();
        no_trim += g.issues.len();
        prims += g.prims.len();
        for (fi, f) in b.faces.iter().enumerate() {
            let k = kinds
                .entry(format!("{:?}", b.surfaces[f.surface as usize].kind()))
                .or_default();
            k.0 += 1;
            k.1 += g.faces[fi].is_none() as usize;
        }
        for (f, e) in &g.issues {
            println!("  no trim: {name} face {f}: {e:?}");
        }
    }
    println!(
        "corpus: {} bodies, {faces} faces, {no_trim} without trim, {prims} primitives, build {secs:.2} s",
        bodies.len()
    );
    println!("  face kinds (count, without trim): {kinds:?}");
    // Drops on every body.
    let n_ref = if quick() { 5 } else { 40 };
    let mut all: Vec<(&str, Run, Misses)> = Vec::new();
    for (label, c) in cutters() {
        let mut times = Vec::new();
        let mut stats = SolveStats::default();
        let mut tot = Misses::default();
        for (bi, (_, b)) in bodies.iter().enumerate() {
            let g = QueryGeometry::new(b, &BuildOptions::default());
            let r = raster(&g, &c, &DropOptions::default(), 60, 30);
            times.extend(r.times_us);
            stats.faces_numeric += r.stats.faces_numeric;
            stats.faces_closed_form += r.stats.faces_closed_form;
            stats.edges += r.stats.edges;
            stats.vertices += r.stats.vertices;
            stats.newton_runs += r.stats.newton_runs;
            stats.newton_converged += r.stats.newton_converged;
            let m = misses(&g, &c, Seeding::default(), n_ref, 1000 + bi as u64);
            tot.points += m.points;
            tot.misses += m.misses;
            tot.over += m.over;
            tot.worst = tot.worst.max(m.worst);
            tot.real += m.real;
            tot.real_worst = tot.real_worst.max(m.real_worst);
        }
        times.sort_by(f64::total_cmp);
        all.push((
            label,
            Run {
                times_us: times,
                stats,
                contacts: 0,
            },
            tot,
        ));
    }
    println!("corpus drops (60 × 30 raster per body):");
    for (label, run, m) in &all {
        report(label, run);
        print_misses(label, m);
    }
    // The largest body, and an array of it with at least 5 000 faces.
    if let Some((name, big)) = bodies.iter().max_by_key(|(_, b)| b.faces.len()) {
        let k = (5000 + big.faces.len() - 1) / big.faces.len();
        let nx = (k as f64).sqrt().ceil() as usize;
        let ny = k.div_ceil(nx);
        measure(
            &format!("largest corpus body {name}"),
            big,
            (200, 100),
            if quick() { 5 } else { 50 },
        );
        let arr = array(big, nx, ny, 5.0);
        measure(
            &format!("corpus array {name} {nx} × {ny}"),
            &arr,
            (400, 200),
            if quick() { 5 } else { 50 },
        );
        seeding_study(
            &format!("largest corpus body {name}"),
            big,
            if quick() { 5 } else { 40 },
        );
    }
}

fn main() {
    let q = quick();
    for freeform in [false, true] {
        let body = tiled(25, freeform);
        let name = if freeform {
            "synthetic 5 000 faces, B-spline"
        } else {
            "synthetic 5 000 faces, mixed"
        };
        measure(
            name,
            &body,
            if q { (100, 50) } else { (500, 250) },
            if q { 10 } else { 100 },
        );
    }
    seeding_study(
        "synthetic B-spline tiles (2 × 2)",
        &tiled(2, true),
        if q { 10 } else { 100 },
    );
    if let Ok(dir) = std::env::var("FLUUT_STEP_CORPUS") {
        corpus(&dir);
    } else {
        println!("FLUUT_STEP_CORPUS not set: corpus skipped");
    }
}
