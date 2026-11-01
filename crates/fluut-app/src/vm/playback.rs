// SPDX-License-Identifier: MIT OR Apache-2.0

//! Simulation playback (D11 §4, D13 §7, S 08 §8.7): the playhead, the
//! toolpath clips of the timeline and the verification findings.
//!
//! [`Playback`] is UI state: where the playhead is and whether it moves.
//! Playing runs the whole program in about [`PLAY_SECONDS`]; the stock at
//! the playhead is asked from the worker, which scrubs (D11 §4).

use crate::session::SimView;
use crate::text::{data, tr, tr_args};
use crate::vm::fmt_num;
use fluut_core::fluut_sim::Check;

/// Seconds that playing the whole program takes, at least 1 s of program
/// time per second.
pub const PLAY_SECONDS: f64 = 20.0;

/// The playhead of one Setup's simulation.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Playback {
    /// The Setup it belongs to.
    pub setup: Option<String>,
    /// Program time, s.
    pub t: f64,
    pub playing: bool,
    /// Time of the stock last asked for.
    pub sent: Option<f64>,
    /// Length of the program it was set up for, s.
    pub duration: f64,
}

impl Playback {
    /// A new simulation: the playhead at its end, paused.
    pub fn reset(&mut self, setup: &str, duration: f64) {
        *self = Playback {
            setup: Some(setup.to_owned()),
            t: duration,
            playing: false,
            sent: Some(duration),
            duration,
        };
    }

    /// Program seconds per real second while playing.
    pub fn rate(duration: f64) -> f64 {
        (duration / PLAY_SECONDS).max(1.0)
    }

    /// Advances by `dt` real seconds while playing; stops at the end.
    /// Returns whether the playhead moved.
    pub fn tick(&mut self, dt: f64, duration: f64) -> bool {
        if !self.playing {
            return false;
        }
        let t = (self.t + dt * Playback::rate(duration)).min(duration);
        if t >= duration {
            self.playing = false;
        }
        let moved = t != self.t;
        self.t = t;
        moved
    }

    /// Moves the playhead, within the program.
    pub fn seek(&mut self, t: f64, duration: f64) {
        self.t = t.clamp(0.0, duration.max(0.0));
    }

    /// Plays or pauses; playing from the end starts again.
    pub fn toggle(&mut self, duration: f64) {
        if !self.playing && self.t >= duration {
            self.t = 0.0;
        }
        self.playing = !self.playing && duration > 0.0;
    }

    /// The time to ask the worker for, when the shown stock is not at the
    /// playhead and no request is waiting.
    pub fn wanted(&self, waiting: bool) -> Option<f64> {
        (!waiting && self.sent != Some(self.t)).then_some(self.t)
    }
}

/// `m:ss`, or `h:mm:ss` from an hour.
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct ClipVm {
    pub name: String,
    pub start: f64,
    /// Share of the program, 0–1.
    pub from: f32,
    pub to: f32,
    /// The playhead is in it.
    pub current: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct FindingVm {
    pub text: String,
    pub error: bool,
    /// Where to look, in the Setup's work frame.
    pub position: [f32; 3],
}

#[derive(Clone, PartialEq, Debug)]
pub struct PlaybackVm {
    /// `now / total`.
    pub time: String,
    pub play_label: String,
    pub duration: f64,
    pub clips: Vec<ClipVm>,
    pub findings: Vec<FindingVm>,
    /// Ray spacing and rest material.
    pub summary: String,
}

impl PlaybackVm {
    pub fn new(v: &SimView, p: &Playback) -> PlaybackVm {
        let d = v.duration;
        let share = |t: f64| if d > 0.0 { (t / d) as f32 } else { 0.0 };
        let clips = v
            .clips
            .iter()
            .enumerate()
            .map(|(i, c)| ClipVm {
                name: data(&c.name),
                start: c.start,
                from: share(c.start),
                to: share(c.end),
                current: p.t >= c.start
                    && (p.t < c.end || (i + 1 == v.clips.len() && p.t <= c.end)),
            })
            .collect();
        let findings = v
            .findings
            .iter()
            .map(|f| {
                let id = match f.check {
                    Check::Gouge => "sim-finding-gouge",
                    Check::RapidInStock => "sim-finding-rapid",
                    Check::Holder => "sim-finding-holder",
                    Check::Shank => "sim-finding-shank",
                    Check::FluteLength => "sim-finding-flute",
                };
                FindingVm {
                    text: tr_args(
                        id,
                        &[
                            ("name", &f.name),
                            ("segment", &f.segment.to_string()),
                            ("depth", &fmt_num(f.depth)),
                        ],
                    ),
                    error: f.check.is_error(),
                    position: f.position,
                }
            })
            .collect();
        let (rest, max) = v.rest.unwrap_or_default();
        PlaybackVm {
            time: tr_args("sim-time", &[("now", &clock(p.t)), ("total", &clock(d))]),
            play_label: tr(if p.playing { "sim-pause" } else { "sim-play" }),
            duration: d,
            clips,
            findings,
            summary: tr_args(
                "sim-summary",
                &[
                    ("res", &fmt_num(v.res)),
                    ("rest", &fmt_num(rest.round())),
                    ("max", &fmt_num(max)),
                ],
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_pause_tick_and_end() {
        let mut p = Playback::default();
        p.reset("s1", 100.0);
        assert_eq!((p.t, p.playing), (100.0, false));
        assert_eq!(p.wanted(false), None);
        // Playing from the end starts again.
        p.toggle(100.0);
        assert!(p.playing);
        assert_eq!(p.t, 0.0);
        // 100 s of program plays in 20 s: 5 s per second.
        assert!(p.tick(1.0, 100.0));
        assert_eq!(p.t, 5.0);
        assert_eq!(p.wanted(false), Some(5.0));
        assert_eq!(p.wanted(true), None);
        p.toggle(100.0);
        assert!(!p.tick(1.0, 100.0));
        assert_eq!(p.t, 5.0);
        // It stops at the end.
        p.toggle(100.0);
        assert!(p.tick(50.0, 100.0));
        assert_eq!((p.t, p.playing), (100.0, false));
        // A short program plays at real time.
        assert_eq!(Playback::rate(3.0), 1.0);
    }

    #[test]
    fn seek_clamps() {
        let mut p = Playback::default();
        p.seek(-3.0, 10.0);
        assert_eq!(p.t, 0.0);
        p.seek(30.0, 10.0);
        assert_eq!(p.t, 10.0);
        assert_eq!(clock(59.6), "1:00");
        assert_eq!(clock(3725.0), "1:02:05");
    }
}
