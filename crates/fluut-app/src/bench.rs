// SPDX-License-Identifier: MIT OR Apache-2.0

//! Measurement harness, from the S4 spike (`spikes/s4-common`):
//!
//! - `FLUUT_BENCH=<n>`: orbit the camera a fixed step every frame, redraw
//!   continuously, print frame-interval statistics over `n` frames after a
//!   warm-up, then exit. With a project, it is opened and calculated first,
//!   and timing starts once its toolpaths show. `FLUUT_BENCH_SIM=1` also
//!   simulates the first Setup, so the stock is drawn. A header line names
//!   the adapter, backend, driver, window size and vsync; the frame line
//!   is the interval between frame starts (what the user sees) and a second
//!   line is the CPU time of each frame, which excludes waiting for vsync.
//! - `FLUUT_NO_VSYNC=1`: present without waiting for vsync, so the frame
//!   interval is the real cost of a frame, not the refresh interval.
//! - `FLUUT_STARTUP=1`: print the time from process start to the start of
//!   the second frame (the first one has been presented by then), then exit.

use crate::viewport::Camera;
use eframe::egui;
use std::time::{Duration, Instant};

const WARMUP: usize = 20;
/// Orbit step per bench frame, pixels of drag.
const ORBIT_STEP: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Interactive,
    Frames(usize),
    Startup,
}

/// Frame-time and startup measurement.
#[derive(Debug)]
pub struct Bench {
    mode: Mode,
    t0: Instant,
    frame: usize,
    last: Option<Instant>,
    samples: Vec<Duration>,
    /// CPU time of the frame before the current one, from eframe.
    cpu_prev: Option<f32>,
    /// CPU time of the timed frames, seconds.
    cpu: Vec<f32>,
    simulate: bool,
    frame_header_done: bool,
}

/// Whether `FLUUT_NO_VSYNC` asks to present without vsync.
#[must_use]
pub fn no_vsync() -> bool {
    std::env::var_os("FLUUT_NO_VSYNC").is_some_and(|v| v != "0")
}

impl Bench {
    /// Read the mode from the environment. `t0` is the process start, taken
    /// first thing in `main`.
    #[must_use]
    pub fn from_env(t0: Instant) -> Self {
        let mode = if let Some(n) = env_usize("FLUUT_BENCH") {
            Mode::Frames(n.max(1))
        } else if std::env::var_os("FLUUT_STARTUP").is_some() {
            Mode::Startup
        } else {
            Mode::Interactive
        };
        Self {
            mode,
            t0,
            frame: 0,
            last: None,
            samples: Vec::new(),
            cpu_prev: None,
            cpu: Vec::new(),
            frame_header_done: false,
            simulate: std::env::var_os("FLUUT_BENCH_SIM").is_some_and(|v| v != "0"),
        }
    }

    /// Whether `FLUUT_BENCH_SIM` asks for a simulated Setup.
    #[must_use]
    pub fn simulate(&self) -> bool {
        self.simulate
    }

    /// The CPU time of the previous frame (eframe's `cpu_usage`).
    pub fn cpu_usage(&mut self, secs: Option<f32>) {
        self.cpu_prev = secs;
    }

    /// Prints what was measured on, once, when timing is about to start.
    /// `adapter` is `None` without a GPU. `px` is the window size in
    /// physical pixels; `sim` whether a simulation is on show.
    pub fn header(&mut self, adapter: Option<String>, px: egui::Vec2, sim: bool) {
        if !self.timing_frames() || self.frame_header_done {
            return;
        }
        self.frame_header_done = true;
        println!(
            "bench {} window={}x{} vsync={} simulation={sim}",
            adapter.unwrap_or_else(|| "adapter=none".into()),
            px.x.round(),
            px.y.round(),
            if no_vsync() { "off" } else { "on" },
        );
    }

    /// Whether frames are being timed (`FLUUT_BENCH`).
    #[must_use]
    pub fn timing_frames(&self) -> bool {
        matches!(self.mode, Mode::Frames(_))
    }

    /// Whether the app must redraw continuously.
    #[must_use]
    pub fn continuous(&self) -> bool {
        self.mode != Mode::Interactive
    }

    /// Call at the start of every frame, before the scene is drawn. Moves the
    /// camera in bench mode. Returns `true` when the app should exit.
    pub fn frame(&mut self, cam: &mut Camera) -> bool {
        let now = Instant::now();
        self.frame += 1;
        match self.mode {
            Mode::Interactive => false,
            Mode::Startup => {
                if self.frame == 2 {
                    println!("startup_ms={:.1}", ms(now - self.t0));
                    true
                } else {
                    false
                }
            }
            Mode::Frames(n) => {
                cam.orbit(ORBIT_STEP, 0.0);
                if let Some(last) = self.last
                    && self.frame > WARMUP
                {
                    self.samples.push(now - last);
                    if let Some(c) = self.cpu_prev {
                        self.cpu.push(c);
                    }
                }
                self.last = Some(now);
                if self.samples.len() >= n {
                    println!("{}", summary(&mut self.samples));
                    if !self.cpu.is_empty() {
                        let mut cpu: Vec<Duration> = self
                            .cpu
                            .iter()
                            .map(|c| Duration::from_secs_f32(c.max(0.0)))
                            .collect();
                        println!("cpu {}", summary(&mut cpu));
                    }
                    // Print once; the window closes after this frame.
                    self.mode = Mode::Interactive;
                    true
                } else {
                    false
                }
            }
        }
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// `frames=… mean_ms=… p50_ms=… p95_ms=… max_ms=…` for a set of frame times.
#[must_use]
pub fn summary(samples: &mut [Duration]) -> String {
    samples.sort();
    let n = samples.len();
    if n == 0 {
        return "frames=0".into();
    }
    let mean = samples.iter().map(|d| ms(*d)).sum::<f64>() / n as f64;
    let pct = |p: f64| ms(samples[((n - 1) as f64 * p).round() as usize]);
    format!(
        "frames={n} mean_ms={mean:.2} p50_ms={:.2} p95_ms={:.2} p99_ms={:.2} max_ms={:.2}",
        pct(0.5),
        pct(0.95),
        pct(0.99),
        ms(samples[n - 1])
    )
}

fn env_usize(key: &str) -> Option<usize> {
    std::env::var(key).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_percentiles() {
        let mut s: Vec<_> = (1..=100).map(Duration::from_millis).collect();
        let out = summary(&mut s);
        assert!(out.contains("frames=100"));
        assert!(
            out.contains("p50_ms=51.00") || out.contains("p50_ms=50.00"),
            "{out}"
        );
        assert!(
            out.contains("p99_ms=99.00") || out.contains("p99_ms=100.00"),
            "{out}"
        );
        assert!(out.contains("max_ms=100.00"));
    }

    #[test]
    fn interactive_never_exits() {
        let mut b = Bench {
            mode: Mode::Interactive,
            t0: Instant::now(),
            frame: 0,
            last: None,
            samples: Vec::new(),
            cpu_prev: None,
            cpu: Vec::new(),
            simulate: false,
            frame_header_done: false,
        };
        let mut cam = Camera::framing(glam::Vec3::ZERO, glam::Vec3::ONE);
        for _ in 0..10 {
            assert!(!b.frame(&mut cam));
        }
    }

    #[test]
    fn frames_mode_times_after_warmup_and_records_cpu() {
        let mut b = Bench {
            mode: Mode::Frames(3),
            t0: Instant::now(),
            frame: 0,
            last: None,
            samples: Vec::new(),
            cpu_prev: None,
            cpu: Vec::new(),
            simulate: false,
            frame_header_done: false,
        };
        let mut cam = Camera::framing(glam::Vec3::ZERO, glam::Vec3::ONE);
        let mut exit_at = None;
        for i in 0..100 {
            b.cpu_usage(Some(0.002));
            if b.frame(&mut cam) {
                exit_at = Some(i + 1);
                break;
            }
        }
        // The warm-up frames and the first timed frame (no interval yet)
        // are not counted: 3 samples come from frames WARMUP + 2 ..= WARMUP + 4.
        assert_eq!(exit_at, Some(WARMUP + 3));
        assert!(!b.timing_frames(), "prints once");
    }
}
