// SPDX-License-Identifier: MIT OR Apache-2.0

//! Parse speed on the corpus with 1, 2, 4 and all threads (D04 §2):
//! `FLUUT_STEP_CORPUS=<dir> cargo run -p fluut-io-step --example parse_speed --release`.
//!
//! Each file is read into memory first; the time is the best of `RUNS`
//! parses of the whole corpus, without dropping the models (a caller keeps
//! them). Every parallel result is checked against the serial one.

use std::path::{Path, PathBuf};
use std::time::Instant;

const RUNS: usize = 5;

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

type Parsed = Result<fluut_io_step::RawModel, fluut_io_step::ParseError>;

/// Best time of `RUNS` runs of `f` over all files, in seconds; the models
/// are dropped after the clock stops.
fn best(srcs: &[Vec<u8>], f: impl Fn(&[u8]) -> Parsed) -> f64 {
    (0..RUNS)
        .map(|_| {
            let t = Instant::now();
            let models: Vec<Parsed> = srcs.iter().map(|s| f(s)).collect();
            let dt = t.elapsed().as_secs_f64();
            drop(models);
            dt
        })
        .fold(f64::INFINITY, f64::min)
}

fn main() {
    let Ok(dir) = std::env::var("FLUUT_STEP_CORPUS") else {
        eprintln!("set FLUUT_STEP_CORPUS to the corpus directory");
        return;
    };
    let mut files = Vec::new();
    step_files(Path::new(&dir), &mut files);
    let srcs: Vec<Vec<u8>> = files
        .iter()
        .map(|f| std::fs::read(f).expect("read"))
        .collect();
    let mb = srcs.iter().map(Vec::len).sum::<usize>() as f64 / 1e6;
    let largest = srcs.iter().map(Vec::len).max().unwrap_or(0) as f64 / 1e6;
    println!(
        "{} files, {mb:.1} MB (largest {largest:.1} MB), best of {RUNS} runs",
        srcs.len()
    );
    let serial: Vec<_> = srcs
        .iter()
        .map(|s| fluut_io_step::parse_serial(s))
        .collect();
    let t = best(&srcs, fluut_io_step::parse_serial);
    println!("serial:      {:6.0} MB/s", mb / t);
    let all = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut counts = vec![1, 2, 4];
    if !counts.contains(&all) {
        counts.push(all);
    }
    for n in counts {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .expect("thread pool");
        pool.install(|| {
            for (s, want) in srcs.iter().zip(&serial) {
                assert!(
                    fluut_io_step::parse(s) == *want,
                    "parallel parse differs from serial"
                );
            }
            let t = best(&srcs, fluut_io_step::parse);
            // The largest file alone, cut for this pool.
            let big = srcs.iter().max_by_key(|s| s.len()).expect("files");
            let tb = best(std::slice::from_ref(big), fluut_io_step::parse);
            println!(
                "{n:2} threads:  {:6.0} MB/s (largest file {:6.0} MB/s)",
                mb / t,
                largest / tb
            );
        });
    }
}
