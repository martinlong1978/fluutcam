// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whole 3-D finishing toolpaths on curved parts, end to end, with the time
//! spent in drops: `cargo run -p fluut-cam --release --features bench-stats
//! --example finishing_bench`.
//!
//! Models: the free-form `synth::sculpted` (3 mm and 12 mm faces) and the
//! rough S2 tiles, the analytic `dome` and `rough3d` test parts, and with
//! `FLUUT_STEP_CORPUS=<dir>` the corpus body with the most B-spline
//! surfaces. Types `parallel` (0°), `spiral`, `scallop`, `contour`
//! (waterline) and `steep_and_shallow`, with Ø6 flat, ball and bull r1
//! tools, stepover and step-down 0.5 mm, tolerance 0.01 mm. Each case runs
//! on one thread and on all threads (`rayon` pools).
//!
//! Per case: wall time on one thread and on all, drops, the share of the
//! one-thread time spent in drops, drops per second per thread, and for
//! `parallel` an estimate of what adaptive sampling could skip: of the drops
//! on each pass line, those within the tolerance of the chord between their
//! neighbours, and those a greedy chord merge over all of them leaves out.
//!
//! `FB_MODELS=sculpted3,dome` and `FB_TYPES=parallel` pick models and types,
//! `FB_TOOLS=ball` tools, `FB_STEPOVER` and `FB_TOL` the stepover and
//! tolerance.

use fluut_cam::cutter::synth::{sculpted, tiled};
use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry};
use fluut_cam::surface::stats;
use fluut_cam::{StrategyInput, calculate};
use fluut_geom::{Affine, Surface};
use fluut_model::project::{SetupKind, read};
use fluut_model::resolve::HeightEnv;
use fluut_model::tools::ToolLibrary;
use fluut_path::{Motion, Segment};
use fluut_topo::BrepBody;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn env_list(key: &str) -> Option<Vec<String>> {
    std::env::var(key)
        .ok()
        .map(|s| s.split(',').map(|x| x.trim().to_owned()).collect())
}

fn picked(list: &Option<Vec<String>>, name: &str) -> bool {
    list.as_ref().is_none_or(|l| l.iter().any(|x| x == name))
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

fn step_bodies(path: &Path) -> Vec<BrepBody> {
    let Ok(src) = std::fs::read(path) else {
        return Vec::new();
    };
    let Ok(m) = fluut_io_step::parse(&src) else {
        return Vec::new();
    };
    fluut_io_step::map_bodies(&m)
        .into_iter()
        .flatten()
        .map(|b| b.body)
        .collect()
}

fn test_part(name: &str) -> Option<BrepBody> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fluut-cli/tests/projects")
        .join(name)
        .join(format!("{name}.step"));
    step_bodies(&p).into_iter().max_by_key(|b| b.faces.len())
}

/// The corpus body with the most B-spline surfaces.
fn corpus_freeform() -> Option<(String, BrepBody)> {
    let dir = std::env::var("FLUUT_STEP_CORPUS").ok()?;
    let mut files = Vec::new();
    step_files(Path::new(&dir), &mut files);
    let mut best: Option<(usize, String, BrepBody)> = None;
    for f in &files {
        let name = f.file_name().unwrap_or_default().to_string_lossy();
        for (k, b) in step_bodies(f).into_iter().enumerate() {
            let n = b
                .surfaces
                .iter()
                .filter(|s| matches!(s, Surface::BSpline(_)))
                .count();
            if best.as_ref().is_none_or(|x| n > x.0) {
                best = Some((n, format!("{name}#{k} ({n} B-spline surfaces)"), b));
            }
        }
    }
    best.map(|(_, n, b)| (n, b))
}

struct Case {
    secs: f64,
    drops: u64,
    drop_secs: f64,
    cl: usize,
    points: Vec<[f64; 3]>,
}

fn run(input: &StrategyInput, threads: usize, record: bool) -> Option<Case> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("pool");
    stats::reset(record);
    let t = Instant::now();
    let r = pool.install(|| calculate(input));
    let secs = t.elapsed().as_secs_f64();
    let (drops, ns) = stats::totals();
    let points = stats::take_points();
    stats::reset(false);
    match r {
        Ok(r) => {
            let cl = r
                .result
                .segments()
                .filter(|s: &&Segment| s.motion().is_some_and(|m| m.motion == Motion::Cut))
                .count();
            Some(Case {
                secs,
                drops,
                drop_secs: ns as f64 * 1e-9,
                cl,
                points,
            })
        }
        Err(e) => {
            println!("      failed: {e}");
            None
        }
    }
}

/// Of the drops on `parallel` pass lines (angle 0: one Y per line), how
/// many lie within `tol` (vertically) of the chord between their
/// neighbours along the line, and how many a greedy merge at `tol` leaves
/// out. `(on lines, neighbour test, greedy)`.
fn skippable(points: &[[f64; 3]], line_ys: &[f64], tol: f64) -> (usize, usize, usize) {
    let mut by_y: Vec<[f64; 3]> = points
        .iter()
        .copied()
        .filter(|p| p[2].is_finite() && line_ys.binary_search_by(|y| y.total_cmp(&p[1])).is_ok())
        .collect();
    by_y.sort_by(|a, b| a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0])));
    by_y.dedup_by(|a, b| a[0] == b[0] && a[1] == b[1]);
    let (mut on, mut local, mut greedy) = (0, 0, 0);
    for line in by_y.chunk_by(|a, b| a[1] == b[1]) {
        on += line.len();
        if line.len() < 3 {
            continue;
        }
        let dev = |a: &[f64; 3], b: &[f64; 3], m: &[f64; 3]| {
            let s = (m[0] - a[0]) / (b[0] - a[0]);
            (m[2] - (a[2] + s * (b[2] - a[2]))).abs()
        };
        local += line
            .windows(3)
            .filter(|w| dev(&w[0], &w[2], &w[1]) <= tol)
            .count();
        // Greedy: from each kept point, the farthest point whose chord
        // holds every point between within `tol`.
        let mut kept = 1;
        let mut i = 0;
        while i < line.len() - 1 {
            let mut j = i + 1;
            while j + 1 < line.len()
                && (i + 1..=j).all(|k| dev(&line[i], &line[j + 1], &line[k]) <= tol)
            {
                j += 1;
            }
            kept += 1;
            i = j;
        }
        greedy += line.len() - kept;
    }
    (on, local, greedy)
}

fn main() {
    let models_pick = env_list("FB_MODELS");
    let types_pick = env_list("FB_TYPES");
    let tools_pick = env_list("FB_TOOLS");
    let stepover: f64 = std::env::var("FB_STEPOVER")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.5);
    let tol: f64 = std::env::var("FB_TOL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.01);
    let all = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut models: Vec<(String, String, BrepBody)> = Vec::new();
    let mut add = |key: &str, label: String, b: Option<BrepBody>| {
        if picked(&models_pick, key)
            && let Some(b) = b
        {
            models.push((key.to_owned(), label, b));
        }
    };
    add(
        "sculpted3",
        "sculpted, 24 x 24 faces of 3 mm".into(),
        Some(sculpted(24, 3.0, 5)),
    );
    add(
        "sculpted12",
        "sculpted, 6 x 6 faces of 12 mm".into(),
        Some(sculpted(6, 12.0, 9)),
    );
    add(
        "tiles",
        "rough S2 tiles, 8 x 8 (512 faces)".into(),
        Some(tiled(8, true)),
    );
    add("dome", "dome (analytic)".into(), test_part("dome"));
    add("rough3d", "rough3d (analytic)".into(), test_part("rough3d"));
    if picked(&models_pick, "corpus")
        && let Some((n, b)) = corpus_freeform()
    {
        models.push(("corpus".into(), n, b));
    }
    let lib = "format = \"fluut.tools/1\"\nid = \"lib\"\n\
               [[tool]]\nid = \"f6\"\nshape = \"flat\"\ndiameter = 6\n[[tool.preset]]\n\
               material = \"MDF\"\nrpm = 18000\nfeed = 2000\nplunge = 500\nstepover = 0.5\n\
               [[tool]]\nid = \"b6\"\nshape = \"ball\"\ndiameter = 6\n[[tool.preset]]\n\
               material = \"MDF\"\nrpm = 18000\nfeed = 2000\nplunge = 500\nstepover = 0.5\n\
               [[tool]]\nid = \"r6\"\nshape = \"bull\"\ndiameter = 6\ncorner_radius = 1\n\
               [[tool.preset]]\nmaterial = \"MDF\"\nrpm = 18000\nfeed = 2000\nplunge = 500\n\
               stepover = 0.5\n";
    let tools = ToolLibrary::parse(lib).unwrap().0.tools;
    let sd = stepover;
    let types = [
        ("parallel", format!("stepover = {stepover}")),
        ("spiral", format!("stepover = {stepover}")),
        ("scallop", format!("stepover = {stepover}")),
        ("contour", format!("step_down = {sd}")),
        (
            "steep_and_shallow",
            format!("steep = {{ step_down = {sd} }}\nshallow = {{ stepover = {stepover} }}"),
        ),
    ];
    println!(
        "stepover / step-down {stepover} mm, tolerance {tol} mm, all threads = {all}; \
         times in s, drop µs is the mean time inside one drop"
    );
    for (_, label, body) in models {
        let g = SetupGeometry {
            bodies: vec![SetupBody {
                id: "b-x".into(),
                geom: BodyGeom::Brep(body),
            }],
        };
        let bb = g.bounds(1e-3);
        println!(
            "{label}: {:.0} x {:.0} x {:.0} mm",
            bb.max.x() - bb.min.x(),
            bb.max.y() - bb.min.y(),
            bb.max.z() - bb.min.z()
        );
        let env = HeightEnv {
            setup_clearance: bb.max.z() + 20.0,
            stock_top: bb.max.z() + 1.0,
            stock_bottom: bb.min.z(),
            model_top: bb.max.z(),
            model_bottom: bb.min.z(),
            selection: None,
        };
        for (ty, passes) in &types {
            if !picked(&types_pick, ty) {
                continue;
            }
            for (tname, tid) in [("flat", "f6"), ("ball", "b6"), ("bull", "r6")] {
                if !picked(&tools_pick, tname) {
                    continue;
                }
                let src = format!(
                    "format = \"fluut.project/1\"\n[[setup]]\nid = \"s\"\n[[setup.toolpath]]\n\
                     id = \"t\"\ntype = \"{ty}\"\ntool = \"lib#{tid}\"\n\
                     [setup.toolpath.passes]\nmachining_tolerance = {tol}\n{passes}\n"
                );
                let p = read(&src).unwrap().project;
                let tp = &p.setups[0].toolpaths[0];
                let tool = tools.iter().find(|t| t.id == tid).unwrap();
                let input = StrategyInput {
                    toolpath: tp,
                    geometry: &g,
                    tool,
                    material: Some("MDF"),
                    setup_kind: SetupKind::ThreeAxis,
                    env,
                    to_work: Affine::identity(),
                    stock: None,
                    machine: None,
                    post: None,
                    in_process: None,
                    rotary: None,
                };
                let record = *ty == "parallel";
                let Some(one) = run(&input, 1, record) else {
                    continue;
                };
                let Some(many) = run(&input, all, false) else {
                    continue;
                };
                let mut line = format!(
                    "  {ty:<17} {tname:<4}: 1 thread {:7.2} s, {all} threads {:7.2} s \
                     (x{:.1}); {} drops, {:.0} % in drops, drop {:.1} µs, {:.0} drops/s/thread \
                     ({:.0} on {all}); {} CL points out",
                    one.secs,
                    many.secs,
                    one.secs / many.secs,
                    one.drops,
                    100.0 * one.drop_secs / one.secs,
                    1e6 * one.drop_secs / one.drops.max(1) as f64,
                    one.drops as f64 / one.secs,
                    many.drops as f64 / many.secs / all as f64,
                    one.cl,
                );
                if record {
                    // Pass lines at 0°: the Y of the lines the passes sit on
                    // are the Y values that most drops share.
                    let mut ys: Vec<f64> = one.points.iter().map(|p| p[1]).collect();
                    ys.sort_by(f64::total_cmp);
                    let mut lines: Vec<f64> = ys
                        .chunk_by(|a, b| a == b)
                        .filter(|c| c.len() >= 8)
                        .map(|c| c[0])
                        .collect();
                    lines.dedup();
                    for t in [0.25 * tol, tol] {
                        let (on, local, greedy) = skippable(&one.points, &lines, t);
                        line += &format!(
                            "\n      chord {t} mm: {on} drops on {} pass lines, {local} ({:.0} %) \
                             within it of their neighbours' chord, {greedy} ({:.0} %) left out \
                             by a greedy merge",
                            lines.len(),
                            100.0 * local as f64 / on.max(1) as f64,
                            100.0 * greedy as f64 / on.max(1) as f64,
                        );
                    }
                }
                println!("{line}");
            }
        }
    }
}
