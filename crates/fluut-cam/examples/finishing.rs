// SPDX-License-Identifier: MIT OR Apache-2.0

//! Times the 3-D finishing types (D15b §5) on the largest body of a STEP
//! corpus.
//!
//! `cargo run -p fluut-cam --example finishing --release` with
//! `FLUUT_STEP_CORPUS=<dir>`; `FLUUT_TYPES=contour,spiral` picks types and
//! `FLUUT_STEP_DOWN` sets the waterline step-down (default 2 mm).
//! Ø6 ball (Ø6 flat for `horizontal`), every face a drive face, tolerance
//! 0.01 mm. Waterline levels and seed lines run on all cores;
//! `RAYON_NUM_THREADS=1` gives one thread. Reports the time per output CL
//! point.

use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry};
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
    let lib = "format = \"fluut.tools/1\"\nid = \"lib\"\n\
               [[tool]]\nid = \"b6\"\nshape = \"ball\"\ndiameter = 6\n[[tool.preset]]\n\
               material = \"MDF\"\nrpm = 18000\nfeed = 2000\nplunge = 500\nstepover = 2\n\
               [[tool]]\nid = \"f6\"\nshape = \"flat\"\ndiameter = 6\n[[tool.preset]]\n\
               material = \"MDF\"\nrpm = 18000\nfeed = 2000\nplunge = 500\nstepover = 3\n";
    let tools = ToolLibrary::parse(lib).unwrap().0.tools;
    let env = HeightEnv {
        setup_clearance: bb.max.z() + 20.0,
        stock_top: bb.max.z() + 1.0,
        stock_bottom: bb.min.z(),
        model_top: bb.max.z(),
        model_bottom: bb.min.z(),
        selection: None,
    };
    let pick = std::env::var("FLUUT_TYPES").unwrap_or_default();
    // `FLUUT_STEP_DOWN` sets the waterline step-down (default 2 mm).
    let sd = std::env::var("FLUUT_STEP_DOWN").unwrap_or_else(|_| "2".into());
    let contour = format!("step_down = {sd}");
    let sns = format!("steep = {{ step_down = {sd} }}\nshallow = {{ stepover = 2 }}");
    let cases = [
        ("contour", "b6", contour.as_str()),
        ("horizontal", "f6", "stepover = 3"),
        ("spiral", "b6", "stepover = 2"),
        ("scallop", "b6", "stepover = 2"),
        ("steep_and_shallow", "b6", sns.as_str()),
    ];
    for (ty, tl, passes) in cases {
        if !pick.is_empty() && !pick.split(',').any(|p| p == ty) {
            continue;
        }
        let src = format!(
            "format = \"fluut.project/1\"\n[[setup]]\nid = \"s\"\n[[setup.toolpath]]\nid = \"t\"\n\
             type = \"{ty}\"\ntool = \"lib#{tl}\"\n[setup.toolpath.passes]\n\
             machining_tolerance = 0.01\n{passes}\n"
        );
        let p = read(&src).unwrap().project;
        let tp = &p.setups[0].toolpaths[0];
        let tool = tools.iter().find(|t| t.id == tl).unwrap();
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
        let t = Instant::now();
        let r = calculate(&input);
        let secs = t.elapsed().as_secs_f64();
        match r {
            Ok(r) => {
                let pts = r
                    .result
                    .segments()
                    .filter(|s: &&Segment| s.motion().is_some_and(|m| m.motion == Motion::Cut))
                    .count();
                println!(
                    "{ty}: {secs:.1} s, {pts} output CL points, {:.0} µs per point; {:?}",
                    1e6 * secs / pts.max(1) as f64,
                    r.diagnostics
                        .iter()
                        .map(|d| d.code.as_str())
                        .collect::<Vec<_>>()
                );
            }
            Err(e) => println!("{ty}: failed after {secs:.1} s: {e}"),
        }
    }
}
