// SPDX-License-Identifier: MIT OR Apache-2.0

//! Gouge and rest-material checks against the part, and the index turn
//! check (D11 §5).
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
//!
//! [`index_turn`] checks the stock against the tool while A turns between
//! Setups.

use crate::dexel::TriDexel;
use crate::mesh::{GOUGE, OFF, REST, WITHIN};
use crate::run::{Assembly, Check, Finding, Simulation};
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

/// The part's top at every Z ray of the stock, kept from the verification
/// to paint the stock (D11 §5).
#[derive(Clone, Debug, PartialEq)]
pub struct PartHeights {
    /// Gouge tolerance, mm.
    pub tol: f64,
    /// By Z ray, row by row; NaN where the ray misses the part or the
    /// stock.
    pub top: Vec<f32>,
}

impl PartHeights {
    /// The rest class of Z ray `idx` whose material top is `top`: above the
    /// part by more than the tolerance, below it by more than the gouge
    /// threshold, or within. For display: there is no ring test.
    pub fn class(&self, idx: usize, top: f64) -> u8 {
        let p = self.top[idx];
        if p.is_nan() {
            return OFF;
        }
        let p = f64::from(p);
        if top - p > self.tol {
            REST
        } else if top < p - self.tol * (1.0 + SAGITTA) {
            GOUGE
        } else {
            WITHIN
        }
    }
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
    part: Option<f64>,
}

pub(crate) fn gouges_and_rest(
    sim: &Simulation,
    part: &dyn Part,
    tol: f64,
    stop: &(dyn Fn() -> bool + Sync),
) -> Option<(Vec<Finding>, Rest, PartHeights)> {
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
            part: Some(pt),
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
    let heights = PartHeights {
        tol,
        top: rows
            .iter()
            .flatten()
            .map(|s| s.part.map_or(f32::NAN, |p| p as f32))
            .collect(),
    };
    Some((attribute(sim, &gouges, limit), rest, heights))
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

/// How far material turning about the X axis reaches into a tool assembly
/// that points up from `tip`, or `None` when the turn is clear (D11 §5
/// "index turn"). Conservative: the material is turned a full revolution,
/// each Z ray's radius is grown by the ray spacing for the material between
/// rays, and the assembly is a vertical cylinder of its largest radius
/// from the tip up.
pub fn index_turn(stock: &TriDexel, tool: &Assembly, tip: DVec3) -> Option<f64> {
    let r = [tool.shank, tool.holder.map(|h| h.0)]
        .into_iter()
        .flatten()
        .fold(tool.tool.radius, f64::max);
    // The assembly's nearest distance from the axis.
    let dy = (tip.y.abs() - r).max(0.0);
    let dz = tip.z.max(0.0);
    let reach = (dy * dy + dz * dz).sqrt();
    let g = &stock.grids[2];
    let res = stock.res;
    let mut worst = f64::NEG_INFINITY;
    for i in 0..g.nu {
        for j in 0..g.nv {
            let o = g.ray_origin(stock, i, j);
            if (o.x - tip.x).abs() > r + 0.5 * res {
                continue;
            }
            for s in g.ray(i, j) {
                let z = (o.z + f64::from(s.lo))
                    .abs()
                    .max((o.z + f64::from(s.hi)).abs());
                worst = worst.max((o.y * o.y + z * z).sqrt() + res);
            }
        }
    }
    (worst > reach).then_some(worst - reach)
}
