// SPDX-License-Identifier: MIT OR Apache-2.0

//! Waterline diagnostics on the largest body of a STEP corpus.
//!
//! `cargo run -p fluut-cam --example waterline --release` with
//! `FLUUT_STEP_CORPUS=<dir>`. `FLUUT_STEP_DOWN` sets the level step
//! (default 20 mm), `FLUUT_LEVELS=z1,z2` picks levels instead, and
//! `FLUUT_FLATS` places them as `contour` does (even steps between the
//! flats). Ø6 ball,
//! every face a drive face, tolerance 0.01 mm, levels one at a time. Per level: the
//! contours, how many are lost (`cam.contour.loop_open`), where they end,
//! points, drops and the time per point.

use fluut_cam::StrategyInput;
use fluut_cam::follow::setup;
use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry};
use fluut_cam::waterline::{End, Waterline};
use fluut_geom::Affine;
use fluut_model::project::{SetupKind, read};
use fluut_model::resolve::HeightEnv;
use fluut_model::tools::ToolLibrary;
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
    let g = SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-x".into(),
            geom: BodyGeom::Brep(body),
        }],
    };
    let bb = g.bounds(1e-3);
    println!("{name}: Z {:.3} .. {:.3}", bb.min.z(), bb.max.z());
    let lib = "format = \"fluut.tools/1\"\nid = \"lib\"\n\
               [[tool]]\nid = \"b6\"\nshape = \"ball\"\ndiameter = 6\n[[tool.preset]]\n\
               material = \"MDF\"\nrpm = 18000\nfeed = 2000\nplunge = 500\nstepover = 2\n";
    let tools = ToolLibrary::parse(lib).unwrap().0.tools;
    let env = HeightEnv {
        setup_clearance: bb.max.z() + 20.0,
        stock_top: bb.max.z() + 1.0,
        stock_bottom: bb.min.z(),
        model_top: bb.max.z(),
        model_bottom: bb.min.z(),
        selection: None,
    };
    let sd: f64 = std::env::var("FLUUT_STEP_DOWN")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20.0);
    let src = "format = \"fluut.project/1\"\n[[setup]]\nid = \"s\"\n[[setup.toolpath]]\n\
               id = \"t\"\ntype = \"contour\"\ntool = \"lib#b6\"\n[setup.toolpath.passes]\n\
               machining_tolerance = 0.01\nstep_down = 20\n";
    let p = read(src).unwrap().project;
    let tp = &p.setups[0].toolpaths[0];
    let tool = &tools[0];
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
    let mut st = setup(&input, "contour").unwrap();
    let r = tool.radius();
    let tol = 0.01;
    let region = st.region(&input, r + tol).unwrap();
    let t = Instant::now();
    let d0 = st.q.drops();
    let wl = Waterline::new(&st.q, &region, tol, 0.5 * r, 0.5 * r);
    println!(
        "seeds: {:.1} s, {} drops",
        t.elapsed().as_secs_f64(),
        st.q.drops() - d0
    );
    let zs: Vec<f64> = match std::env::var("FLUUT_LEVELS") {
        Ok(s) => s.split(',').filter_map(|v| v.parse().ok()).collect(),
        Err(_) if std::env::var("FLUUT_FLATS").is_ok() => {
            // As `contour`: even steps between the flats.
            let mut flats: Vec<f64> = Vec::new();
            let BodyGeom::Brep(body) = &g.bodies[0].geom else {
                unreachable!()
            };
            for (f, face) in body.faces.iter().enumerate() {
                if fluut_cam::drive::is_horizontal_plane(&body.surfaces[face.surface as usize]) {
                    let z = fluut_cam::geometry::flat_z(body, f as u32, 1e-3);
                    if z > bb.min.z() + 1e-9 && z <= bb.max.z() + 1e-9 {
                        flats.push(z.min(bb.max.z()));
                    }
                }
            }
            flats.sort_by(|a, b| b.total_cmp(a));
            flats.dedup_by(|a, b| (*a - *b).abs() <= 1e-6);
            let mut v = Vec::new();
            let mut hi = bb.max.z();
            for b in flats.into_iter().chain(std::iter::once(bb.min.z())) {
                if b < hi - 1e-9 {
                    v.extend(fluut_cam::layout::levels(hi, b, Some(sd), None));
                    hi = b;
                }
            }
            v
        }
        Err(_) => {
            let mut v = Vec::new();
            let mut z = bb.max.z() - sd;
            while z > bb.min.z() {
                v.push(z);
                z -= sd;
            }
            v
        }
    };
    let (mut all_pts, mut all_secs, mut all_lost, mut all_n) = (0usize, 0.0, 0usize, 0usize);
    for z in zs {
        let t = Instant::now();
        let d0 = st.q.drops();
        let cs = wl.level(z);
        let secs = t.elapsed().as_secs_f64();
        println!();
        let drops = st.q.drops() - d0;
        let pts: usize = cs.iter().map(|c| c.pts.len()).sum();
        let lost: Vec<_> = cs
            .iter()
            .filter(|c| !c.closed && (c.ends.0 == End::Lost || c.ends.1 == End::Lost))
            .collect();
        println!(
            "z {z:.3}: {} contours, {} closed, {} lost; {pts} points, {drops} drops \
             ({:.1}/point), {secs:.1} s, {:.0} µs/point",
            cs.len(),
            cs.iter().filter(|c| c.closed).count(),
            lost.len(),
            drops as f64 / pts.max(1) as f64,
            1e6 * secs / pts.max(1) as f64
        );
        if std::env::var("FLUUT_ALL").is_ok() {
            for c in &cs {
                let (mut lo, mut hi) = (
                    (f64::INFINITY, f64::INFINITY),
                    (f64::NEG_INFINITY, f64::NEG_INFINITY),
                );
                for w in &c.pts {
                    lo = (lo.0.min(w.p.x()), lo.1.min(w.p.y()));
                    hi = (hi.0.max(w.p.x()), hi.1.max(w.p.y()));
                }
                let len: f64 = c.pts.windows(2).map(|w| (w[1].p - w[0].p).length()).sum();
                println!(
                    "  contour {} pts closed {} len {len:.3} box ({:.3},{:.3})-({:.3},{:.3})",
                    c.pts.len(),
                    c.closed,
                    lo.0,
                    lo.1,
                    hi.0,
                    hi.1
                );
            }
        }
        if std::env::var("FLUUT_VERBOSE").is_ok() {
            for c in &lost {
                let (a, b) = (c.pts[0], c.pts[c.pts.len() - 1]);
                println!(
                    "  lost {:?}: {} pts, start ({:.4}, {:.4}) n ({:.3}, {:.3}), \
                     end ({:.4}, {:.4}) n ({:.3}, {:.3})",
                    c.ends,
                    c.pts.len(),
                    a.p.x(),
                    a.p.y(),
                    a.n.x(),
                    a.n.y(),
                    b.p.x(),
                    b.p.y(),
                    b.n.x(),
                    b.n.y()
                );
            }
        }
        all_pts += pts;
        all_secs += secs;
        all_lost += lost.len();
        all_n += cs.len();
    }
    println!(
        "total: {all_n} contours, {all_lost} lost, {all_pts} points, {all_secs:.1} s, {:.0} µs/point",
        1e6 * all_secs / all_pts.max(1) as f64
    );
}
