// SPDX-License-Identifier: MIT OR Apache-2.0

//! Gouge and rest-material checks against the part (D11 §5).
//!
//! The part is given by its exact top height on vertical lines ([`Part`]),
//! which is all a three-axis tool can reach. Each Z ray of the final stock
//! is a sample: its material top is compared with the part's top there.
//!
//! - **Gouge:** the stock top is below the part's top by more than the
//!   threshold, and so is it at every point of a ring of that radius around
//!   the ray. The ring keeps a ray next to a vertical wall, whose top is the
//!   wall's but whose distance to the wall is tiny, from counting.
//! - **Rest:** material above the part by more than the tolerance, summed
//!   over the part's footprint, and the stock left where there is no part.
//!
//! A gouge is attributed to the first chord that cut its ray below the
//! part. The dexel spacing only sets where samples are taken; distances come
//! from the exact part.

use crate::run::{Check, Finding, Simulation};
use crate::sim::ray_range;
use glam::DVec3;
use rayon::prelude::*;

/// The part to verify against, in the stock frame.
pub trait Part: Sync {
    /// The top of the part on the vertical line through `(x, y)`, or `None`
    /// where the line misses it.
    fn top(&self, x: f64, y: f64) -> Option<f64>;
}

/// Material left against the part.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Rest {
    /// Volume above the part by more than the tolerance, over its
    /// footprint, mm³.
    pub on_part: f64,
    /// Stock left where there is no part below, mm³.
    pub off_part: f64,
    /// Thickest rest on the part, mm, and the stock top there.
    pub max: f64,
    pub at: DVec3,
}

/// Points on the ring around a gouge candidate.
const RING: usize = 8;

/// Threshold over the tolerance: the chords of an arc lie inside it by up
/// to a quarter of the tolerance (see `Simulation::run`).
const SAGITTA: f64 = 0.25;

/// A gouged Z ray.
#[derive(Copy, Clone, Debug)]
struct Gouge {
    i: usize,
    j: usize,
    part: f64,
    top: f64,
}

/// One Z ray's result: a gouge, or rest on or off the part.
#[derive(Copy, Clone, Debug, Default)]
struct Sample {
    gouge: Option<Gouge>,
    on: f64,
    off: f64,
    rest: f64,
    top: f64,
}

pub(crate) fn gouges_and_rest(
    sim: &Simulation,
    part: &dyn Part,
    tol: f64,
    stop: &(dyn Fn() -> bool + Sync),
) -> Option<(Vec<Finding>, Rest)> {
    let d = sim.stock_final();
    let g = &d.grids[2];
    let g0 = &sim.stock_initial().grids[2];
    let (nu, nv, res, o) = (g.nu, g.nv, d.res, d.origin);
    let limit = tol * (1.0 + SAGITTA);
    let area = res * res;
    let sample = |i: usize, j: usize| -> Sample {
        let Some(first) = g0.ray(i, j).last() else {
            return Sample::default();
        };
        let (x, y) = (o.x + (i as f64 + 0.5) * res, o.y + (j as f64 + 0.5) * res);
        let spans = g.ray(i, j);
        let top = spans.last().map_or(o.z, |s| o.z + f64::from(s.hi));
        let cut = top < o.z + f64::from(first.hi);
        let Some(pt) = part.top(x, y) else {
            let len: f64 = spans.iter().map(|s| f64::from(s.hi - s.lo)).sum();
            return Sample {
                off: len * area,
                ..Sample::default()
            };
        };
        let mut s = Sample {
            top,
            ..Sample::default()
        };
        if top - pt > tol {
            s.on = (top - pt) * area;
            s.rest = top - pt;
        }
        if cut && top < pt - limit {
            let ring = (0..RING).all(|k| {
                let (sn, cs) = libm::sincos(k as f64 * std::f64::consts::TAU / RING as f64);
                part.top(x + limit * cs, y + limit * sn)
                    .is_some_and(|h| h >= top + limit)
            });
            if ring {
                s.gouge = Some(Gouge {
                    i,
                    j,
                    part: pt,
                    top,
                });
            }
        }
        s
    };
    let rows: Option<Vec<Vec<Sample>>> = (0..nv)
        .into_par_iter()
        .map(|j| (!stop()).then(|| (0..nu).map(|i| sample(i, j)).collect()))
        .collect();
    let rows = rows?;
    let mut rest = Rest::default();
    let mut gouges = Vec::new();
    // In row order, so the sums do not depend on the thread count.
    for s in rows.iter().flatten() {
        rest.on_part += s.on;
        rest.off_part += s.off;
        if s.rest > rest.max {
            rest.max = s.rest;
            rest.at = DVec3::new(0.0, 0.0, s.top);
        }
        gouges.extend(s.gouge);
    }
    if rest.max > 0.0
        && let Some((i, j)) = rows
            .iter()
            .enumerate()
            .find_map(|(j, r)| r.iter().position(|s| s.rest == rest.max).map(|i| (i, j)))
    {
        rest.at.x = o.x + (i as f64 + 0.5) * res;
        rest.at.y = o.y + (j as f64 + 0.5) * res;
    }
    Some((attribute(sim, &gouges, limit), rest))
}

/// The first chord that cut each gouged ray below the part (or, failing
/// that, down to its final top), as findings.
fn attribute(sim: &Simulation, gouges: &[Gouge], limit: f64) -> Vec<Finding> {
    if gouges.is_empty() {
        return Vec::new();
    }
    let d = sim.stock_final();
    let g = &d.grids[2];
    let (nu, nv, res, o) = (g.nu, g.nv, d.res, d.origin);
    let mut index = vec![u32::MAX; nu * nv];
    for (k, x) in gouges.iter().enumerate() {
        index[x.j * nu + x.i] = k as u32;
    }
    let mut owner = vec![u32::MAX; gouges.len()];
    let mut fallback = vec![u32::MAX; gouges.len()];
    let mut left = gouges.len();
    let highest = gouges
        .iter()
        .map(|x| x.part)
        .fold(f64::NEG_INFINITY, f64::max);
    for (ci, c) in sim.chords.iter().enumerate() {
        let s = &c.sweep;
        if left == 0 {
            break;
        }
        if s.min.z > highest {
            continue;
        }
        let (Some((i0, i1)), Some((j0, j1))) = (
            ray_range(s.min.x, s.max.x, o.x, res, nu),
            ray_range(s.min.y, s.max.y, o.y, res, nv),
        ) else {
            continue;
        };
        for j in j0..=j1 {
            for i in i0..=i1 {
                let k = index[j * nu + i];
                if k == u32::MAX || owner[k as usize] != u32::MAX {
                    continue;
                }
                let x = &gouges[k as usize];
                let ray = DVec3::new(
                    o.x + (i as f64 + 0.5) * res,
                    o.y + (j as f64 + 0.5) * res,
                    o.z,
                );
                let Some((t0, _)) = s.ray(ray, DVec3::Z) else {
                    continue;
                };
                let bottom = o.z + t0;
                if bottom < x.part - limit {
                    owner[k as usize] = ci as u32;
                    left -= 1;
                } else if bottom <= x.top + 1e-3 && fallback[k as usize] == u32::MAX {
                    fallback[k as usize] = ci as u32;
                }
            }
        }
    }
    let mut out: Vec<Finding> = gouges
        .iter()
        .zip(owner.iter().zip(&fallback))
        .filter_map(|(x, (&a, &b))| {
            let ci = if a != u32::MAX { a } else { b };
            let c = sim.chords.get(ci as usize)?;
            Some(Finding {
                check: Check::Gouge,
                toolpath: c.toolpath as usize,
                segment: c.segment as usize,
                position: DVec3::new(
                    o.x + (x.i as f64 + 0.5) * res,
                    o.y + (x.j as f64 + 0.5) * res,
                    x.top,
                ),
                depth: x.part - x.top,
            })
        })
        .collect();
    // Deepest first, so that it is the one kept per segment.
    out.sort_by(|a, b| b.depth.total_cmp(&a.depth));
    out
}

#[cfg(test)]
mod tests;
