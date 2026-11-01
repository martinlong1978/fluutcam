// SPDX-License-Identifier: MIT OR Apache-2.0

//! Measurement harness, from the S4 spike (`spikes/s4-common`):
//!
//! - `FLUUT_BENCH=<n>`: orbit the camera a fixed step every frame, redraw
//!   continuously, print frame-interval statistics over `n` frames after a
//!   warm-up, then exit. With a project, it is opened and calculated first,
//!   and timing starts once its toolpaths show.
//! - `FLUUT_STARTUP=1`: print the time from process start to the start of
//!   the second frame (the first one has been presented by then), then exit.

use crate::viewport::Camera;
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
        }
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
                }
                self.last = Some(now);
                if self.samples.len() >= n {
                    println!("{}", summary(&mut self.samples));
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
        "frames={n} mean_ms={mean:.2} p50_ms={:.2} p95_ms={:.2} max_ms={:.2}",
        pct(0.5),
        pct(0.95),
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
