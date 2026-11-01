// SPDX-License-Identifier: MIT OR Apache-2.0

//! Measurement harness, driven by environment variables so both prototypes
//! measure the same way:
//!
//! - `S4_BENCH=<n>`: orbit the camera a fixed step every frame, redraw
//!   continuously, print frame-interval statistics over `n` frames after a
//!   warm-up, then exit.
//! - `S4_STARTUP=1`: print the time from process start to the start of the
//!   second frame (the first one has been presented by then), then exit.
//! - `S4_GRID=<n>`: mesh grid size (default [`crate::mesh::GRID_1M`]).
//! - `S4_TREE=<setups>x<toolpaths>`: use a large mock project.

use crate::camera::Camera;
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
}

impl Bench {
    /// Read the mode from the environment. `t0` is the process start, taken
    /// first thing in `main`.
    #[must_use]
    pub fn from_env(t0: Instant) -> Self {
        let mode = if let Some(n) = env_usize("S4_BENCH") {
            Mode::Frames(n.max(1))
        } else if std::env::var_os("S4_STARTUP").is_some() {
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
        }
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
                }
                self.last = Some(now);
                if self.samples.len() >= n {
                    println!("{}", summary(&mut self.samples));
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
        "frames={n} mean_ms={mean:.2} p50_ms={:.2} p95_ms={:.2} max_ms={:.2}",
        pct(0.5),
        pct(0.95),
        ms(samples[n - 1])
    )
}

fn env_usize(key: &str) -> Option<usize> {
    std::env::var(key).ok()?.parse().ok()
}

/// Mesh grid size from `S4_GRID`.
#[must_use]
pub fn grid() -> usize {
    env_usize("S4_GRID").unwrap_or(crate::mesh::GRID_1M).max(2)
}

/// The mock project, large if `S4_TREE=<setups>x<toolpaths>` is set.
#[must_use]
pub fn project() -> Vec<crate::mock::Setup> {
    if let Ok(v) = std::env::var("S4_TREE")
        && let Some((a, b)) = v.split_once('x')
        && let (Ok(a), Ok(b)) = (a.parse(), b.parse())
    {
        return crate::mock::large_project(a, b);
    }
    crate::mock::setups()
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
        };
        let mut cam = Camera::framing(glam::Vec3::ZERO, glam::Vec3::ONE);
        for _ in 0..10 {
            assert!(!b.frame(&mut cam));
        }
    }
}
