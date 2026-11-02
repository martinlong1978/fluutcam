// SPDX-License-Identifier: MIT OR Apache-2.0

//! Constant-engagement clearing (optimised load) prototype (design D09 §2.3, CH-16) on exact regions.
//!
//! The tool enters on a helix at the deepest point and the cleared region
//! `K` starts as that helix's disc. Each pass marches from a start point on
//! the edge of `K`, choosing every step's direction by bisection so that the
//! engagement in the uncut region equals the target, with the material on
//! the right. A pass ends when it closes, leaves the tool-centre region or
//! runs out of material; its swept region is then added to `K`.

use super::engagement::engagement;
use super::pocket::swept_path;
use crate::region::{Inside, Region2};
use fluut_geom::{Point2, Vec2};
use std::f64::consts::PI;
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Tool radius.
    pub r: f64,
    /// Target engagement angle (radians).
    pub phi: f64,
    /// Step length along the path.
    pub step: f64,
    /// Upper bound on steps per pass and on passes.
    pub max_steps: usize,
    pub max_passes: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub passes: usize,
    pub steps: usize,
    pub length: f64,
    /// Engagement error over all cutting steps (radians).
    pub max_err: f64,
    pub rms_err: f64,
    /// Steps whose engagement is more than 10° from the target.
    pub off_target: usize,
    /// Area of `clear` left uncut before any final contour pass.
    pub uncut_area: f64,
    pub millis: u128,
    /// Why the run stopped.
    pub stop: &'static str,
}

#[derive(Debug, Clone)]
pub struct OptimisedLoad {
    pub passes: Vec<Vec<Point2>>,
    pub cleared: Region2,
    pub stats: Stats,
}

pub fn optimised_load(clear: &Region2, p: &Params) -> OptimisedLoad {
    let t0 = Instant::now();
    let centres = clear.offset(-p.r);
    let mut stats = Stats::default();
    let Some(entry) = deepest(&centres, p.step) else {
        stats.stop = "tool does not fit";
        return OptimisedLoad {
            passes: vec![],
            cleared: Region2::empty(),
            stats,
        };
    };
    // Helix of radius r/2: clears a disc of radius 1.5 r.
    let mut k = Region2::circle(entry.x(), entry.y(), 1.5 * p.r).intersect(clear);
    let mut passes = Vec::new();
    let (mut sq, mut cutting) = (0.0, 0usize);
    let mut cur = entry;
    stats.stop = "max passes";
    for _ in 0..p.max_passes {
        let uncut = clear.subtract(&k);
        if uncut.area() < 1e-6 {
            stats.stop = "cleared";
            break;
        }
        let Some(start) = find_start(&uncut, &centres, &k, cur, p) else {
            stats.stop = "no start point";
            break;
        };
        let path = march(
            start,
            &uncut,
            &centres,
            p,
            &mut stats,
            &mut sq,
            &mut cutting,
        );
        if path.len() < 2 {
            stats.stop = "stuck";
            break;
        }
        k = k.union(&swept_path(&path, p.r));
        cur = *path.last().unwrap_or(&start);
        stats.length += path.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>();
        stats.steps += path.len() - 1;
        passes.push(path);
    }
    stats.passes = passes.len();
    stats.rms_err = if cutting > 0 {
        (sq / cutting as f64).sqrt()
    } else {
        0.0
    };
    stats.uncut_area = clear.subtract(&k).area();
    stats.millis = t0.elapsed().as_millis();
    OptimisedLoad {
        passes,
        cleared: k,
        stats,
    }
}

/// A point of `centres` far from its boundary: the middle of the last
/// non-empty inward offset.
fn deepest(centres: &Region2, step: f64) -> Option<Point2> {
    let mut last = centres.clone();
    if last.is_empty() {
        return None;
    }
    loop {
        let next = last.offset(-step);
        if next.is_empty() {
            break;
        }
        last = next;
    }
    let l = last
        .loops
        .iter()
        .max_by(|a, b| a.area().total_cmp(&b.area()))?;
    let n = l.verts.len() as f64;
    let (sx, sy) = l
        .verts
        .iter()
        .fold((0.0, 0.0), |(x, y), v| (x + v.p.x(), y + v.p.y()));
    let c = Point2::new(sx / n, sy / n);
    Some(if centres.contains(c) == Inside::In {
        c
    } else {
        l.verts[0].p
    })
}

/// The nearest point to `cur`, half a radius inside the edge of `k`, where
/// the tool reaches enough material.
fn find_start(
    uncut: &Region2,
    centres: &Region2,
    k: &Region2,
    cur: Point2,
    p: &Params,
) -> Option<Point2> {
    let edge = k.offset(-0.5 * p.r);
    let mut best: Option<(f64, Point2)> = None;
    for s in edge.segs() {
        let n = (s.length() / p.step).ceil().max(1.0) as usize;
        for i in 0..n {
            let q = s.point_at(i as f64 / n as f64);
            if centres.contains(q) == Inside::Out {
                continue;
            }
            if engagement(q, &[], p.r, uncut) < 0.5 * p.phi {
                continue;
            }
            let d = q.distance(cur);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, q));
            }
        }
    }
    best.map(|(_, q)| q)
}

fn dir(a: f64) -> Vec2 {
    Vec2::new(libm::cos(a), libm::sin(a))
}

fn march(
    start: Point2,
    uncut: &Region2,
    centres: &Region2,
    p: &Params,
    stats: &mut Stats,
    sq: &mut f64,
    cutting: &mut usize,
) -> Vec<Point2> {
    let mut path = vec![start];
    let mut heading: Option<f64> = None;
    let mut pos = start;
    for _ in 0..p.max_steps {
        // Candidate headings from left to right of the current one; on the
        // first step, the whole circle.
        let (h0, span, n) = match heading {
            Some(h) => (h, 2.0 * PI / 3.0, 16),
            None => (0.0, PI, 48),
        };
        let at = |a: f64| pos + dir(a) * p.step;
        // Earlier points of this pass that can overlap the next tool disc.
        let near: Vec<Point2> = path
            .iter()
            .copied()
            .filter(|q| q.distance(pos) < 2.0 * p.r + p.step)
            .collect();
        let err = |a: f64| engagement(at(a), &near, p.r, uncut) - p.phi;
        let angles: Vec<f64> = (0..=n)
            .map(|i| h0 + span - 2.0 * span * i as f64 / n as f64)
            .filter(|&a| centres.contains(at(a)) != Inside::Out)
            .collect();
        if angles.is_empty() {
            break;
        }
        let errs: Vec<f64> = angles.iter().map(|&a| err(a)).collect();
        // First crossing from too little to too much engagement, scanning
        // left to right: the material is then on the right.
        let mut chosen = None;
        for i in 1..angles.len() {
            if errs[i - 1] < 0.0 && errs[i] >= 0.0 {
                let (mut lo, mut hi) = (angles[i - 1], angles[i]);
                for _ in 0..30 {
                    let m = 0.5 * (lo + hi);
                    if err(m) < 0.0 { lo = m } else { hi = m }
                }
                chosen = Some(0.5 * (lo + hi));
                break;
            }
        }
        let a = match chosen {
            Some(a) => a,
            None => {
                let (i, e) = errs
                    .iter()
                    .copied()
                    .enumerate()
                    .max_by(|x, y| x.1.total_cmp(&y.1))
                    .unwrap_or((0, -p.phi));
                if e + p.phi < 0.1 * p.phi {
                    break; // no material left within reach
                }
                if e < 0.0 {
                    angles[i]
                } else {
                    // Too much everywhere: take the least.
                    let (j, _) = errs
                        .iter()
                        .copied()
                        .enumerate()
                        .min_by(|x, y| x.1.total_cmp(&y.1))
                        .unwrap_or((0, 0.0));
                    angles[j]
                }
            }
        };
        let next = at(a);
        let e = err(a);
        *sq += e * e;
        *cutting += 1;
        stats.max_err = stats.max_err.max(e.abs());
        if e.abs() > 10f64.to_radians() {
            stats.off_target += 1;
        }
        path.push(next);
        pos = next;
        heading = Some(a);
        if path.len() > 8 && pos.distance(start) < p.step {
            break; // closed
        }
    }
    path
}
