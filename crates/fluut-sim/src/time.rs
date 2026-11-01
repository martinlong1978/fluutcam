// SPDX-License-Identifier: MIT OR Apache-2.0

//! Machining time estimate (D11 §6, spec 04 §4.10).
//!
//! Each segment runs at its programmed feed, limited by the machine's axis
//! feeds. With an acceleration, speeds at the junctions come from the
//! junction deviation (GRBL's rule) and each segment follows a trapezoidal
//! profile between them; without one, a segment takes its length over its
//! speed. Dwells add their time and the result is scaled by the machine's
//! correction factor.

use fluut_path::{FeedRef, MarkerKind, Motion, Segment, linearize};
use glam::DVec3;

/// Feeds of a toolpath in mm/min, by [`FeedRef`]. A feed of 0 or less is
/// unknown: the machine's cutting limit is used.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Feeds {
    pub cut: f64,
    pub plunge: f64,
    pub ramp: f64,
    pub lead_in: f64,
    pub lead_out: f64,
    pub transition: f64,
}

impl Feeds {
    /// The same feed for every move.
    pub fn all(f: f64) -> Feeds {
        Feeds {
            cut: f,
            plunge: f,
            ramp: f,
            lead_in: f,
            lead_out: f,
            transition: f,
        }
    }

    fn of(&self, r: FeedRef) -> f64 {
        match r {
            FeedRef::Rapid => 0.0,
            FeedRef::Cut => self.cut,
            FeedRef::Plunge => self.plunge,
            FeedRef::Ramp => self.ramp,
            FeedRef::LeadIn => self.lead_in,
            FeedRef::LeadOut => self.lead_out,
            FeedRef::Transition => self.transition,
        }
    }
}

/// What the estimate needs from the machine (spec 02 §2.3).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Kinematics {
    /// Largest cutting feed of each linear axis, mm/min.
    pub max_feed: [f64; 3],
    /// Rapid feed of each linear axis, mm/min.
    pub rapid: [f64; 3],
    /// Acceleration of the linear axes, mm/s²; `None` when unknown.
    pub accel: Option<f64>,
    /// Junction deviation δ, mm.
    pub junction: f64,
    /// Time of one tool change, s.
    pub tool_change: f64,
    /// Correction factor applied to every time.
    pub factor: f64,
}

/// Rapid feed used when the machine gives none, mm/min.
pub const DEFAULT_RAPID: f64 = 5000.0;
/// Cutting limit used when the machine gives none, mm/min.
pub const DEFAULT_MAX_FEED: f64 = 10000.0;

impl Default for Kinematics {
    fn default() -> Self {
        Kinematics {
            max_feed: [DEFAULT_MAX_FEED; 3],
            rapid: [DEFAULT_RAPID; 3],
            accel: None,
            junction: 0.01,
            tool_change: 0.0,
            factor: 1.0,
        }
    }
}

impl Kinematics {
    /// Every axis with the same limits.
    pub fn uniform(max_feed: f64, rapid: f64, accel: Option<f64>) -> Kinematics {
        Kinematics {
            max_feed: [max_feed; 3],
            rapid: [rapid; 3],
            accel,
            ..Kinematics::default()
        }
    }
}

/// Speed limit along unit direction `d` with per-axis limits `axis`: the
/// first axis to reach its limit sets it.
fn axis_limited(d: DVec3, axis: [f64; 3]) -> f64 {
    (0..3)
        .filter(|&k| d[k].abs() > 1e-12)
        .map(|k| axis[k] / d[k].abs())
        .fold(f64::INFINITY, f64::min)
}

/// One move as the planner sees it, speeds in mm/s.
#[derive(Copy, Clone, Debug)]
struct Move {
    len: f64,
    vmax: f64,
    /// Unit directions at the start and the end; `None` stops the machine.
    d0: Option<DVec3>,
    d1: Option<DVec3>,
    /// Time that is not motion (dwell, cycle retract), s.
    extra: f64,
}

/// Unit tangent of an arc or helix at fraction `f` of its sweep.
fn arc_tangent(seg: &Segment, f: f64) -> Option<DVec3> {
    let (Segment::Arc {
        a,
        b,
        centre,
        normal,
        sweep,
        ..
    }
    | Segment::Helix {
        a,
        b,
        centre,
        normal,
        sweep,
        ..
    }) = seg
    else {
        return None;
    };
    let n = normal.get().0;
    let c = centre.0;
    let ra = a.xyz.0 - c;
    let ra = ra - n * ra.dot(n);
    let r = ra.length();
    let (s, co) = libm::sincos(sweep * f);
    // `ra` turned by the swept angle about `n`.
    let rot = ra * co + n.cross(ra) * s;
    let horiz = n.cross(rot) * sweep.signum() * (r * sweep.abs()) / r.max(1e-300);
    let dh = (b.xyz.0 - a.xyz.0).dot(n);
    let t = horiz + n * dh;
    (t.length() > 0.0).then(|| t.normalize())
}

fn direction(a: DVec3, b: DVec3) -> Option<DVec3> {
    let d = b - a;
    (d.length() > 0.0).then(|| d.normalize())
}

/// The planner's view of `seg`, or `None` for a segment that takes no
/// time and does not stop the machine (a comment).
fn plan(seg: &Segment, feeds: &Feeds, k: &Kinematics) -> Option<Move> {
    let accel = k.accel;
    let speed = |m: &fluut_path::MotionInfo, d: Option<DVec3>| -> f64 {
        let limit = |axes: [f64; 3]| match d {
            Some(d) => axis_limited(d, axes),
            None => axes.iter().copied().fold(f64::INFINITY, f64::min),
        };
        if m.motion == Motion::Rapid || m.feed == FeedRef::Rapid {
            limit(k.rapid) / 60.0
        } else {
            let f = feeds.of(m.feed);
            let cap = limit(k.max_feed);
            (if f > 0.0 { f.min(cap) } else { cap }) / 60.0
        }
    };
    match seg {
        Segment::Line { a, b, m } => {
            let d = direction(a.xyz.0, b.xyz.0);
            Some(Move {
                len: seg.length(),
                vmax: speed(m, d),
                d0: d,
                d1: d,
                extra: 0.0,
            })
        }
        Segment::Arc {
            a,
            centre,
            normal,
            m,
            ..
        }
        | Segment::Helix {
            a,
            centre,
            normal,
            m,
            ..
        } => {
            let n = normal.get().0;
            let ra = a.xyz.0 - centre.0;
            let r = (ra - n * ra.dot(n)).length();
            let mut v = speed(m, None);
            if let Some(acc) = accel {
                // Centripetal limit.
                v = v.min((acc * r).sqrt());
            }
            Some(Move {
                len: seg.length(),
                vmax: v,
                d0: arc_tangent(seg, 0.0),
                d1: arc_tangent(seg, 1.0),
                extra: 0.0,
            })
        }
        Segment::Spline { m, .. } => {
            let pieces = linearize(seg, 1e-3);
            let ends = |p: &Segment| direction(p.start()?.xyz.0, p.end()?.xyz.0);
            Some(Move {
                len: seg.length(),
                vmax: speed(m, None),
                d0: pieces.first().and_then(ends),
                d1: pieces.last().and_then(ends),
                extra: 0.0,
            })
        }
        // A rotary move stops the machine at both ends: its tip direction
        // turns with the part.
        Segment::Rotary { m, .. } => Some(Move {
            len: seg.length(),
            vmax: speed(m, None),
            d0: None,
            d1: None,
            extra: 0.0,
        }),
        Segment::Cycle { cycle, m, .. } => {
            // Feed down from the retract plane, rapid back up.
            let depth = (cycle.retract_z - cycle.depth_z).abs();
            let down = speed(m, Some(DVec3::NEG_Z));
            let up = axis_limited(DVec3::Z, k.rapid) / 60.0;
            let back = profile(depth, 0.0, 0.0, up, accel);
            Some(Move {
                len: depth,
                vmax: down,
                d0: None,
                d1: None,
                extra: back + cycle.dwell_s.unwrap_or(0.0),
            })
        }
        Segment::Marker { kind } => match kind {
            MarkerKind::Dwell(s) => Some(Move {
                len: 0.0,
                vmax: 0.0,
                d0: None,
                d1: None,
                extra: *s,
            }),
            MarkerKind::Stop | MarkerKind::OptionalStop => Some(Move {
                len: 0.0,
                vmax: 0.0,
                d0: None,
                d1: None,
                extra: 0.0,
            }),
            MarkerKind::Comment(_) | MarkerKind::PassThrough(_) => None,
        },
    }
}

/// Time to run `len` at most at `vmax`, entering at `vin` and leaving at
/// `vout` (both reachable), with acceleration `a`; length over speed
/// without one.
fn profile(len: f64, vin: f64, vout: f64, vmax: f64, a: Option<f64>) -> f64 {
    if len <= 0.0 || vmax <= 0.0 {
        return 0.0;
    }
    let Some(a) = a else {
        return len / vmax;
    };
    let d1 = (vmax * vmax - vin * vin) / (2.0 * a);
    let d2 = (vmax * vmax - vout * vout) / (2.0 * a);
    if d1 + d2 <= len {
        (vmax - vin) / a + (vmax - vout) / a + (len - d1 - d2) / vmax
    } else {
        // Triangular: the peak speed is not reached.
        let vp = ((2.0 * a * len + vin * vin + vout * vout) / 2.0).sqrt();
        (vp - vin) / a + (vp - vout) / a
    }
}

/// Speed limit at the junction from a move ending along `d1` to one
/// starting along `d0` (D11 §6): `v² = a·δ·sin(θ/2) / (1 − sin(θ/2))`,
/// `θ` the angle between the reversed first direction and the second.
fn junction(d1: Option<DVec3>, d0: Option<DVec3>, a: f64, delta: f64) -> f64 {
    let (Some(p), Some(q)) = (d1, d0) else {
        return 0.0;
    };
    let cos = (-p.dot(q)).clamp(-1.0, 1.0);
    let sin_half = ((1.0 - cos) / 2.0).sqrt();
    if sin_half >= 1.0 - 1e-12 {
        return f64::INFINITY;
    }
    (a * delta * sin_half / (1.0 - sin_half)).sqrt()
}

/// Durations of each segment in seconds, in order (0 for comments), with
/// the correction factor applied. The machine is at rest before the first
/// segment and after the last.
pub fn segment_times<'a>(
    segments: impl IntoIterator<Item = &'a Segment>,
    feeds: &Feeds,
    k: &Kinematics,
) -> Vec<f64> {
    let segs: Vec<&Segment> = segments.into_iter().collect();
    let moves: Vec<Option<Move>> = segs.iter().map(|s| plan(s, feeds, k)).collect();
    let mut out = vec![0.0; segs.len()];
    // The moves that take part, in order.
    let idx: Vec<usize> = (0..segs.len()).filter(|&i| moves[i].is_some()).collect();
    let mv = |i: usize| moves[idx[i]].expect("planned");
    let n = idx.len();
    // Speed at the start of move i; entry[n] is the final stop.
    let mut entry = vec![0.0; n + 1];
    if let Some(a) = k.accel {
        for (i, e) in entry.iter_mut().enumerate().take(n).skip(1) {
            let (p, q) = (mv(i - 1), mv(i));
            let stops = p.len <= 0.0 || q.len <= 0.0 || p.extra > 0.0;
            *e = if stops {
                0.0
            } else {
                junction(p.d1, q.d0, a, k.junction).min(p.vmax).min(q.vmax)
            };
        }
        // Backward: each move must be able to slow down to the next entry.
        for i in (0..n).rev() {
            let m = mv(i);
            entry[i] = entry[i].min((entry[i + 1] * entry[i + 1] + 2.0 * a * m.len).sqrt());
        }
        // Forward: and to speed up from its own entry.
        for i in 0..n {
            let m = mv(i);
            entry[i + 1] = entry[i + 1].min((entry[i] * entry[i] + 2.0 * a * m.len).sqrt());
        }
    }
    for i in 0..n {
        let m = mv(i);
        let t = profile(m.len, entry[i], entry[i + 1], m.vmax, k.accel) + m.extra;
        out[idx[i]] = t * k.factor;
    }
    out
}

/// Total time of `segments` in seconds.
pub fn estimate<'a>(
    segments: impl IntoIterator<Item = &'a Segment>,
    feeds: &Feeds,
    k: &Kinematics,
) -> f64 {
    segment_times(segments, feeds, k).iter().sum()
}

#[cfg(test)]
mod tests;
