// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spike S1: load the corpus with `truck-stepio`, and compare NURBS surface
//! evaluation between `truck-geometry` and `fluut-geom`.
//!
//! `FLUUT_STEP_CORPUS=<dir> cargo run --release` (in this directory).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use truck_stepio::r#in::Table;
use truck_topology::{Shell, shell::ShellCondition};

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

fn corpus() {
    let Ok(dir) = std::env::var("FLUUT_STEP_CORPUS") else {
        eprintln!("set FLUUT_STEP_CORPUS to run the corpus part");
        return;
    };
    let mut files = Vec::new();
    step_files(Path::new(&dir), &mut files);
    let (mut parsed, mut secs, mut bytes) = (0, 0.0, 0usize);
    let (mut shells, mut converted, mut extracted, mut closed, mut faces) = (0, 0, 0, 0, 0);
    let mut panics = 0;
    for f in &files {
        let raw = std::fs::read(f).expect("read");
        bytes += raw.len();
        // ruststep needs a &str; the corpus is ASCII apart from rare Latin-1.
        let src: String = raw.iter().map(|&b| b as char).collect();
        let name = f.file_name().unwrap_or_default().to_string_lossy();
        let t = Instant::now();
        let r = catch_unwind(AssertUnwindSafe(|| {
            let table = Table::from_step(&src)?;
            let mut out = Vec::new();
            for sh in table.shell.values() {
                let c = table.to_compressed_shell(sh).ok();
                let info = c.map(|c| {
                    let nf = c.faces.len();
                    let cond = Shell::extract(c).ok().map(|s| s.shell_condition());
                    (nf, cond)
                });
                out.push(info);
            }
            Some(out)
        }));
        let dt = t.elapsed().as_secs_f64();
        secs += dt;
        match r {
            Err(_) => {
                panics += 1;
                println!("{name}: PANIC");
            }
            Ok(None) => println!("{name}: parse failed"),
            Ok(Some(list)) => {
                parsed += 1;
                let mut line = format!("{name}: {:.1} ms, {} shells:", dt * 1e3, list.len());
                for s in list {
                    shells += 1;
                    match s {
                        None => line.push_str(" convert-failed"),
                        Some((nf, cond)) => {
                            converted += 1;
                            faces += nf;
                            match cond {
                                None => line.push_str(&format!(" {nf}f/extract-failed")),
                                Some(c) => {
                                    extracted += 1;
                                    closed += (c == ShellCondition::Closed) as usize;
                                    line.push_str(&format!(" {nf}f/{c:?}"));
                                }
                            }
                        }
                    }
                }
                println!("{line}");
            }
        }
    }
    println!(
        "truck: parsed {parsed}/{} files ({panics} panics), {:.1} MB in {secs:.2} s = {:.1} MB/s; \
         shells {shells}: converted {converted}, extracted {extracted}, closed {closed}; faces {faces}",
        files.len(),
        bytes as f64 / 1e6,
        bytes as f64 / 1e6 / secs
    );
}

/// A small deterministic generator (no rand dependency).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn nurbs() {
    use truck_geometry::prelude::*;
    let (n, deg) = (8usize, 3usize);
    let mut rng = Lcg(42);
    let mut knots = vec![0.0; deg + 1];
    for i in 1..(n - deg) {
        knots.push(i as f64 / (n - deg) as f64);
    }
    knots.extend(vec![1.0; deg + 1]);
    let mut grid = Vec::new();
    for i in 0..n {
        let mut row = Vec::new();
        for j in 0..n {
            row.push([i as f64 * 10.0, j as f64 * 10.0, rng.next() * 5.0]);
        }
        grid.push(row);
    }
    let truck_s = BSplineSurface::new(
        (KnotVec::from(knots.clone()), KnotVec::from(knots.clone())),
        grid.iter()
            .map(|r| r.iter().map(|p| Point3::new(p[0], p[1], p[2])).collect())
            .collect(),
    );
    let poles: Vec<fluut_geom::Point3> = grid
        .iter()
        .flatten()
        .map(|p| fluut_geom::Point3::new(p[0], p[1], p[2]))
        .collect();
    let fluut_s = fluut_geom::Surface::BSpline(Arc::new(
        fluut_geom::BSplineSurface::new(3, 3, knots.clone(), knots, poles, n, n, None)
            .expect("surface"),
    ));
    let pts: Vec<(f64, f64)> = (0..200_000).map(|_| (rng.next(), rng.next())).collect();

    let t = Instant::now();
    let mut acc = 0.0;
    for &(u, v) in &pts {
        let p = truck_s.subs(u, v);
        let du = truck_s.uder(u, v);
        let dv = truck_s.vder(u, v);
        acc += p.z + du.z + dv.z;
    }
    let truck_ns = t.elapsed().as_secs_f64() * 1e9 / pts.len() as f64;

    let t = Instant::now();
    let mut acc2 = 0.0;
    for &(u, v) in &pts {
        let d = fluut_s.derivs(u, v);
        acc2 += d.p.z() + d.su.z() + d.sv.z();
    }
    let fluut_ns = t.elapsed().as_secs_f64() * 1e9 / pts.len() as f64;

    let mut max_dp: f64 = 0.0;
    let mut max_dd: f64 = 0.0;
    for &(u, v) in pts.iter().take(10_000) {
        let a = truck_s.subs(u, v);
        let d = fluut_s.derivs(u, v);
        let dp = (a.x - d.p.x()).abs() + (a.y - d.p.y()).abs() + (a.z - d.p.z()).abs();
        max_dp = max_dp.max(dp);
        let du = truck_s.uder(u, v);
        let dd = (du.x - d.su.x()).abs() + (du.y - d.su.y()).abs() + (du.z - d.su.z()).abs();
        max_dd = max_dd.max(dd);
    }
    println!(
        "NURBS 8x8 bicubic, point + first derivatives, 200 000 evaluations: \
         truck {truck_ns:.0} ns, fluut-geom {fluut_ns:.0} ns per evaluation; \
         max difference point {max_dp:.1e}, du {max_dd:.1e} (checksums {acc:.3}, {acc2:.3})"
    );
}

fn main() {
    corpus();
    nurbs();
}
