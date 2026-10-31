// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spike S1 corpus measurements:
//! `FLUUT_STEP_CORPUS=<dir> cargo run -p fluut-io-step --example s1 --release`.

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

fn main() {
    let Ok(dir) = std::env::var("FLUUT_STEP_CORPUS") else {
        eprintln!("set FLUUT_STEP_CORPUS to the corpus directory");
        return;
    };
    let mut files = Vec::new();
    step_files(Path::new(&dir), &mut files);
    let (mut bytes, mut secs, mut ok, mut ents) = (0usize, 0.0, 0usize, 0usize);
    let mut supported = std::collections::BTreeMap::<String, usize>::new();
    let mut unsupported = std::collections::BTreeMap::<String, (usize, usize)>::new();
    let mut schemas = std::collections::BTreeMap::<String, usize>::new();
    let mut issues = std::collections::BTreeMap::<String, usize>::new();
    let mut skips = std::collections::BTreeMap::<String, usize>::new();
    let (mut n_bodies, mut n_failed, mut n_faces, mut n_skipped) = (0, 0, 0, 0);
    let mut shells_ok_declared = 0;
    let (mut shells, mut shells_ok, mut shells_ok_loose, mut closed, mut closed_ok) =
        (0, 0, 0, 0, 0);
    let (mut files_ok, mut map_secs) = (0, 0.0);
    for f in &files {
        let src = std::fs::read(f).expect("read");
        let t = Instant::now();
        let r = fluut_io_step::parse(&src);
        let dt = t.elapsed().as_secs_f64();
        bytes += src.len();
        secs += dt;
        let name = f.file_name().unwrap_or_default().to_string_lossy();
        match r {
            Ok(m) => {
                ok += 1;
                ents += m.entities.len();
                *schemas
                    .entry(format!("{:?}", fluut_io_step::schema(&m)))
                    .or_default() += 1;
                let t = Instant::now();
                let bodies = fluut_io_step::map_bodies(&m);
                map_secs += t.elapsed().as_secs_f64();
                let mut file_ok = true;
                for r in bodies {
                    n_bodies += 1;
                    let Ok(mb) = r else {
                        n_failed += 1;
                        file_ok = false;
                        continue;
                    };
                    n_faces += mb.body.faces.len();
                    n_skipped += mb.skipped.len();
                    for e in &mb.skipped {
                        *skips.entry(e.msg.clone()).or_default() += 1;
                        file_ok = false;
                    }
                    let strict = fluut_topo::check(&mb.body, mb.body.tol_model);
                    let loose = fluut_topo::check(&mb.body, 1e-3);
                    let declared = fluut_topo::check(
                        &mb.body,
                        mb.units.uncertainty_mm.unwrap_or(mb.body.tol_model),
                    );
                    for i in &strict {
                        let k = format!("{i:?}");
                        let k = k.split([' ', '{']).next().unwrap_or("").to_owned();
                        *issues.entry(k).or_default() += 1;
                    }
                    for (si, sh) in mb.body.shells.iter().enumerate() {
                        let mine = |is: &[fluut_topo::Issue]| {
                            is.iter().any(|i| match i {
                                fluut_topo::Issue::NotManifold { shell, .. } => {
                                    *shell as usize == si
                                }
                                fluut_topo::Issue::OuterLoops { face, .. }
                                | fluut_topo::Issue::LoopGap { face, .. }
                                | fluut_topo::Issue::EdgeOffSurface { face, .. } => {
                                    sh.faces.contains(face)
                                }
                                fluut_topo::Issue::VertexOffCurve { .. } => true,
                            })
                        };
                        shells += 1;
                        let ok = !mine(&strict);
                        shells_ok += ok as usize;
                        shells_ok_loose += !mine(&loose) as usize;
                        shells_ok_declared += !mine(&declared) as usize;
                        if sh.closed {
                            closed += 1;
                            closed_ok += ok as usize;
                        }
                    }
                }
                files_ok += file_ok as usize;
                let c = fluut_io_step::census(&m);
                for (k, n) in c.supported {
                    *supported.entry(k).or_default() += n;
                }
                for (k, n) in c.unsupported {
                    let e = unsupported.entry(k).or_default();
                    e.0 += n;
                    e.1 += 1;
                }
                println!(
                    "{name}: {:.1} MB, {} entities, {:.1} ms",
                    src.len() as f64 / 1e6,
                    m.entities.len(),
                    dt * 1e3
                );
            }
            Err(e) => println!("{name}: PARSE ERROR {e}"),
        }
    }
    println!(
        "parsed {ok}/{} files, {ents} entities, {:.1} MB in {:.3} s = {:.0} MB/s",
        files.len(),
        bytes as f64 / 1e6,
        secs,
        bytes as f64 / 1e6 / secs
    );
    println!(
        "mapping: {n_bodies} bodies ({n_failed} failed), {n_faces} faces mapped, {n_skipped} skipped, {files_ok}/{} files fully mapped, {:.3} s",
        files.len(),
        map_secs
    );
    println!(
        "validity: {shells_ok}/{shells} shells clean at tol_model (uncertainty clamped to [1e-7, 1e-2] mm), {shells_ok_declared} at the declared uncertainty, {shells_ok_loose} at 1e-3 mm; closed shells {closed_ok}/{closed}"
    );
    println!("issues at tol_model: {issues:?}");
    println!("skipped faces: {skips:?}");
    println!("schemas: {schemas:?}");
    println!("supported entity kinds in bodies:");
    for (k, n) in &supported {
        println!("  {k}: {n}");
    }
    println!("unsupported entity kinds in bodies (count, files):");
    for (k, (n, f)) in &unsupported {
        println!("  {k}: {n} in {f} files");
    }
}
