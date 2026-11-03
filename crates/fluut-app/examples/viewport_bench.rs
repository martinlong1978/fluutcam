// SPDX-License-Identifier: MIT OR Apache-2.0

//! The viewport renderer alone, headless (no window, no `egui`), against the
//! D13 §5.4 targets.
//!
//! ```text
//! cargo run --release -p fluut-app --example viewport_bench -- <project.fluut> [segments]
//! ```
//!
//! Opens and calculates the project through the job layer, uploads its
//! first Setup, adds `segments` synthetic toolpath segments (a zig-zag over
//! the stock, default 2 000 000), then prints: scene build and upload times,
//! frame times while orbiting (waiting for the GPU each frame), and pick
//! latency.

use fluut_app::jobs::{CalcTargets, Jobs};
use fluut_app::scene::{Bounds, Lines};
use fluut_app::viewport::render::{self, Renderer};
use fluut_app::viewport::{Camera, FrameState};
use std::time::{Duration, Instant};

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn stats(mut v: Vec<Duration>) -> String {
    v.sort();
    let n = v.len();
    let pct = |p: f64| ms(v[((n - 1) as f64 * p).round() as usize]);
    format!("n={n} p50_ms={:.1} p95_ms={:.1}", pct(0.5), pct(0.95))
}

/// `n` segments zig-zagging over the top of `b`.
fn zigzag(b: &Bounds, n: usize) -> Lines {
    let mut l = Lines::default();
    let rows = (n as f64).sqrt().max(1.0) as usize;
    let per_row = n.div_ceil(rows);
    let (x0, x1, y0, y1, z) = (b.min[0], b.max[0], b.min[1], b.max[1], b.max[2]);
    let mut prev: Option<[f32; 3]> = None;
    for i in 0..n {
        let (r, k) = (i / per_row, i % per_row);
        let t = k as f32 / per_row as f32;
        let x = if r % 2 == 0 {
            x0 + (x1 - x0) * t
        } else {
            x1 - (x1 - x0) * t
        };
        let y = y0 + (y1 - y0) * r as f32 / rows as f32;
        let p = [x, y, z + 0.5 * (x * 0.3).sin()];
        if let Some(a) = prev {
            l.points.extend([a, p]);
            l.tag.extend([1, 1]);
        }
        prev = Some(p);
    }
    l
}

fn main() {
    let mut args = std::env::args().skip(1);
    let project = args
        .next()
        .expect("usage: viewport_bench <project.fluut> [segments]");
    let segments: usize = args
        .next()
        .map_or(2_000_000, |s| s.parse().expect("segments"));
    let Some((device, queue)) = render::headless() else {
        eprintln!("no wgpu adapter");
        std::process::exit(1);
    };

    let t = Instant::now();
    let mut jobs = Jobs::start(|| {});
    jobs.open(project.into());
    jobs.wait();
    let opened = t.elapsed();
    let t = Instant::now();
    jobs.calculate(CalcTargets::All);
    jobs.wait();
    let calculated = t.elapsed();
    let snap = jobs.snapshot.clone().expect("project opened");
    let setup = &snap.project.setups[0];
    let scene = &snap.scenes[&setup.id];
    println!(
        "open_ms={:.1} calc_ms={:.1} faces={} triangles={} edge_segments={}",
        ms(opened),
        ms(calculated),
        scene.faces.len(),
        scene.mesh.positions.len() / 3,
        scene.edges.segment_count()
    );

    let mut r = Renderer::new(&device);
    let t = Instant::now();
    r.set_scene(&device, &scene.mesh, &scene.edges, scene.stock.as_ref());
    let bounds = scene.bounds.unwrap_or(Bounds {
        min: [0.0; 3],
        max: [100.0; 3],
    });
    let t_lines = Instant::now();
    let synthetic = zigzag(&bounds, segments);
    let built = t_lines.elapsed();
    let mut paths: Vec<Option<&Lines>> = setup
        .toolpaths
        .iter()
        .map(|tp| snap.paths.get(&tp.id).map(|p| &p.lines))
        .collect();
    paths.push(Some(&synthetic));
    let t_up = Instant::now();
    r.set_toolpaths(&device, &paths);
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    println!(
        "synthetic_segments={} build_lines_ms={:.1} upload_ms={:.1} scene_total_ms={:.1}",
        synthetic.segment_count(),
        ms(built),
        ms(t_up.elapsed()),
        ms(t.elapsed())
    );

    let (w, h) = (1280, 800);
    r.ensure_size(&device, w, h);
    let mut camera = Camera::fit(Some(&bounds));
    let mut frames = Vec::new();
    for i in 0..60 {
        camera.orbit(6.0, 0.0);
        let f = FrameState {
            camera,
            dark: true,
            face: None,
            toolpath: None,
            stock_colour: Default::default(),
        };
        let t = Instant::now();
        r.render(&device, &queue, &f);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        if i >= 10 {
            frames.push(t.elapsed());
        }
    }
    println!("frame {}", stats(frames));
    let f = FrameState {
        camera,
        dark: true,
        face: None,
        toolpath: None,
        stock_colour: Default::default(),
    };
    let mut picks = Vec::new();
    for i in 0..20 {
        let t = Instant::now();
        let _ = r.pick(&device, &queue, &f, w / 2 + i, h / 2);
        picks.push(t.elapsed());
    }
    println!("pick {}", stats(picks));
}
