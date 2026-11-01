// SPDX-License-Identifier: MIT OR Apache-2.0

//! Times the `parallel` strategy on the largest body of a STEP corpus.
//!
//! `cargo run -p fluut-cam --example parallel --release` with
//! `FLUUT_STEP_CORPUS=<dir>`. Ø6 ball, every face a drive face, 2 mm
//! stepover at 0° and at 45°, tolerance 0.01 mm, one thread.

use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry};
use fluut_cam::surface::{FaceSets, SurfaceQuery};
use fluut_cam::{StrategyInput, calculate};
use fluut_geom::Affine;
use fluut_model::project::{SetupKind, read};
use fluut_model::resolve::HeightEnv;
use fluut_model::tools::ToolLibrary;
use fluut_path::{Motion, Segment};
use fluut_topo::BrepBody;
use std::path::{Path, PathBuf};
use std::time::Instant;

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

fn largest(dir: &str) -> Option<(String, BrepBody)> {
    let mut files = Vec::new();
    step_files(Path::new(dir), &mut files);
    let mut best: Option<(String, BrepBody)> = None;
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
            if best
                .as_ref()
                .is_none_or(|b| mb.body.faces.len() > b.1.faces.len())
            {
                best = Some((format!("{name}#{k}"), mb.body));
            }
        }
    }
    best
}

fn main() {
    let Ok(dir) = std::env::var("FLUUT_STEP_CORPUS") else {
        println!("FLUUT_STEP_CORPUS not set");
        return;
    };
    let Some((name, body)) = largest(&dir) else {
        println!("no bodies in {dir}");
        return;
    };
    let faces = body.faces.len();
    let g = SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-x".into(),
            geom: BodyGeom::Brep(body),
        }],
    };
    let bb = g.bounds(1e-3);
    println!(
        "{name}: {faces} faces, {:.0} × {:.0} × {:.0} mm",
        bb.max.x() - bb.min.x(),
        bb.max.y() - bb.min.y(),
        bb.max.z() - bb.min.z()
    );
    let lib = "format = \"fluut.tools/1\"\nid = \"lib\"\n[[tool]]\nid = \"b6\"\nshape = \"ball\"\n\
               diameter = 6\n[[tool.preset]]\nmaterial = \"MDF\"\nrpm = 18000\nfeed = 2000\n\
               plunge = 500\nstepover = 2\n";
    let tool = ToolLibrary::parse(lib).unwrap().0.tools.remove(0);
    let env = HeightEnv {
        setup_clearance: bb.max.z() + 20.0,
        stock_top: bb.max.z() + 1.0,
        stock_bottom: bb.min.z(),
        model_top: bb.max.z(),
        model_bottom: bb.min.z(),
        selection: None,
    };
    // Drop time alone, on a raster over the body.
    let c = fluut_cam::cutter::Cutter::ball(6.0).unwrap();
    let q = SurfaceQuery::new(&g, &FaceSets::default(), c, 0.0).unwrap();
    let (nx, ny) = (300, 300);
    let t = Instant::now();
    for i in 0..nx {
        for j in 0..ny {
            let x = bb.min.x() + (bb.max.x() - bb.min.x()) * (i as f64 + 0.5) / nx as f64;
            let y = bb.min.y() + (bb.max.y() - bb.min.y()) * (j as f64 + 0.5) / ny as f64;
            std::hint::black_box(q.drop(x, y));
        }
    }
    let per_drop = 1e6 * t.elapsed().as_secs_f64() / (nx * ny) as f64;
    println!("drop: {per_drop:.1} µs mean over a {nx} × {ny} raster");
    for angle in [0.0, 45.0] {
        let src = format!(
            "format = \"fluut.project/1\"\n[[setup]]\nid = \"s\"\n[[setup.toolpath]]\nid = \"t\"\n\
             type = \"parallel\"\ntool = \"lib#b6\"\n[setup.toolpath.passes]\npass_angle_deg = {angle}\n"
        );
        let tp = read(&src).unwrap().project.setups[0].toolpaths[0].clone();
        let input = StrategyInput {
            toolpath: &tp,
            geometry: &g,
            tool: &tool,
            material: Some("MDF"),
            setup_kind: SetupKind::ThreeAxis,
            env,
            stock: None,
            to_work: Affine::identity(),
            machine: None,
            post: None,
        };
        let t = Instant::now();
        let r = match calculate(&input) {
            Ok(r) => r,
            Err(e) => {
                println!("{angle}°: {e}");
                continue;
            }
        };
        let secs = t.elapsed().as_secs_f64();
        let cl = r
            .result
            .segments()
            .filter(|s| {
                s.motion()
                    .is_some_and(|m| matches!(m.motion, Motion::Cut | Motion::Link))
                    && matches!(s, Segment::Line { .. })
            })
            .count();
        let codes: Vec<&str> = r.diagnostics.iter().map(|d| d.code.as_str()).collect();
        println!(
            "{angle}°: {secs:.2} s, {cl} CL points out, {:.1} µs per CL point \
             (≈ {:.1} drops); cut {:.0} mm; {codes:?}",
            1e6 * secs / cl.max(1) as f64,
            1e6 * secs / cl.max(1) as f64 / per_drop,
            r.result.stats().cut_length,
        );
    }
}
