// SPDX-License-Identifier: MIT OR Apache-2.0

//! M7 simulation measurements (D11 §7): memory, time per move and full
//! simulation time on the reference projects and the S5 stand-ins, plus
//! scrub and mesh times.
//!
//! `cargo run -p fluut-core --example m7 --release -- [projects|standins|all] [error…]`

use fluut_core::fluut_sim::spike::{relief, rotary};
use fluut_core::fluut_sim::{
    Assembly, Feeds, Kinematics, Program, Quiet, Settings, Simulation, Stock, resolution, surface,
};
use fluut_core::{Session, SimSettings, Targets};
use fluut_path::{Pass, PassKind, Segment, ToolpathResult};
use std::path::PathBuf;
use std::time::Instant;

fn mb(b: usize) -> f64 {
    b as f64 / (1 << 20) as f64
}

fn threads() -> usize {
    rayon::current_num_threads()
}

/// Stock time at the middle of the last toolpath, and the mesh then.
fn scrub(sim: &Simulation) -> (f64, f64, usize) {
    let Some(last) = sim.toolpaths.last() else {
        return (0.0, 0.0, 0);
    };
    let t = Instant::now();
    let d = sim.stock_at(0.5 * (last.start + last.end));
    let ts = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let m = surface(&d);
    (ts, t.elapsed().as_secs_f64(), m.triangle_count())
}

fn projects(errors: &[f64]) {
    println!("\n## Reference projects ({} threads)", threads());
    println!(
        "| project | error (mm) | res (mm) | segments | chords | memory (MB) | simulate (s) | µs per segment | scrub (s) | mesh (s) | triangles | findings |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fluut-cli/tests/projects");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    names.sort();
    for name in names {
        let p = dir.join(&name).join(format!("{name}.fluut"));
        let Ok(mut s) = Session::open(&p, &[]) else {
            continue;
        };
        s.calculate(&Targets::All);
        let setups: Vec<String> = s.project.setups.iter().map(|x| x.id.clone()).collect();
        for setup in setups {
            for &error in errors {
                let st = SimSettings {
                    error,
                    ..SimSettings::default()
                };
                let t = Instant::now();
                let r = match s.simulate(&setup, &st, &Quiet) {
                    Ok(r) => r,
                    Err(d) => {
                        println!("| {name}/{setup} | {error} | – | {} |", d[0]);
                        continue;
                    }
                };
                let secs = t.elapsed().as_secs_f64();
                let sim = &r.sim;
                let segs: usize = s
                    .project
                    .setup(&setup)
                    .unwrap()
                    .toolpaths
                    .iter()
                    .filter_map(|t| s.result(&t.id))
                    .map(|r| r.segments().count())
                    .sum();
                let (ts, tm, tri) = scrub(sim);
                println!(
                    "| {name}/{setup} | {error} | {} | {segs} | {} | {:.1} | {secs:.3} | {:.1} | {ts:.3} | {tm:.3} | {tri} | {} |",
                    sim.res,
                    sim.chords.len(),
                    mb(sim.memory()),
                    secs / segs.max(1) as f64 * 1e6,
                    sim.findings.len(),
                );
            }
        }
    }
}

fn result(segs: Vec<Segment>) -> ToolpathResult {
    ToolpathResult::new(vec![Pass::new(PassKind::Cut, segs)]).unwrap()
}

fn standins(errors: &[f64]) {
    println!(
        "\n## S5 stand-ins through `Simulation::run` ({} threads)",
        threads()
    );
    println!(
        "| stand-in | res (mm) | segments | chords | memory (MB) | run (s) | µs per segment | scrub (s) | mesh (s) | triangles |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    let rough = result(relief::roughing(2.4, 0.5));
    let finish = result(relief::program(0.3, 0.5));
    let wrap = result(rotary::program(0.5, 2.0));
    let cases: [(&str, Stock, bool, Vec<(&ToolpathResult, _)>); 2] = [
        (
            "relief rough + finish",
            relief::STOCK,
            false,
            vec![(&rough, relief::rough_tool()), (&finish, relief::tool())],
        ),
        (
            "rotary wrap",
            rotary::STOCK,
            true,
            vec![(&wrap, rotary::tool())],
        ),
    ];
    for (name, stock, rot, progs) in cases {
        for &error in errors {
            let res = resolution(&stock, error, fluut_core::fluut_sim::run::DEFAULT_BUDGET);
            let programs: Vec<Program> = progs
                .iter()
                .enumerate()
                .map(|(i, (r, tool))| Program {
                    id: format!("t{i}"),
                    result: r,
                    tool: Assembly::bare(*tool),
                    feeds: Feeds::all(2000.0),
                })
                .collect();
            let settings = Settings {
                res,
                rotary: rot,
                kinematics: Kinematics::default(),
                tol: 0.01,
            };
            let t = Instant::now();
            let sim = Simulation::run(stock, &programs, &settings, None, &Quiet).unwrap();
            let secs = t.elapsed().as_secs_f64();
            let segs: usize = progs.iter().map(|(r, _)| r.segments().count()).sum();
            let (ts, tm, tri) = scrub(&sim);
            println!(
                "| {name} | {res} | {segs} | {} | {:.0} | {secs:.2} | {:.1} | {ts:.2} | {tm:.2} | {tri} |",
                sim.chords.len(),
                mb(sim.memory()),
                secs / segs as f64 * 1e6,
            );
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let what = args.next().unwrap_or_else(|| "all".into());
    let errors: Vec<f64> = args.filter_map(|a| a.parse().ok()).collect();
    let errors = if errors.is_empty() {
        vec![0.1, 0.05]
    } else {
        errors
    };
    if what == "projects" || what == "all" {
        projects(&errors);
    }
    if what == "standins" || what == "all" {
        standins(&errors);
    }
}
