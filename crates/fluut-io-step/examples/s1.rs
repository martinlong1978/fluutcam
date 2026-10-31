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
}
