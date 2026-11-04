// SPDX-License-Identifier: MIT OR Apache-2.0

//! Playback of a simulation (D11 §4): the stock and its mesh at a chord
//! count, kept from one frame to the next.
//!
//! A [`Player`] keeps the stock it shows. Seeking forward within the same
//! checkpoint interval cuts on from that stock instead of from the
//! checkpoint, and the mesh rebuilds only the blocks whose rays changed.
//! [`SimPaint`] paints the stock's columns by their rest class against the
//! part and by the toolpath that last cut them.

use crate::dexel::TriDexel;
use crate::mesh::{Column, MeshTiles, OFF, Paint, REST, WITHIN};
use crate::run::Simulation;
use crate::verify::{Clearance, PartHeights};
use glam::DVec3;
use rayon::prelude::*;
use std::collections::BTreeMap;

/// The paint of the stock after `n` chords of a simulation.
#[derive(Debug)]
pub struct SimPaint<'a> {
    sim: &'a Simulation,
    stock: &'a TriDexel,
    /// The stock before each toolpath up to the current one.
    before: Vec<&'a TriDexel>,
}

impl<'a> SimPaint<'a> {
    /// `stock` must be the stock after `n` chords of `sim`.
    pub fn new(sim: &'a Simulation, stock: &'a TriDexel, n: usize) -> SimPaint<'a> {
        let current = n
            .checked_sub(1)
            .and_then(|i| sim.chords.get(i))
            .map_or(0, |c| c.toolpath as usize + 1);
        SimPaint {
            sim,
            stock,
            before: (0..current).map(|k| sim.checkpoint(k)).collect(),
        }
    }
}

impl Paint for SimPaint<'_> {
    fn column(&self, i: usize, j: usize) -> Column {
        let g = &self.stock.grids[2];
        let ray = g.ray(i, j);
        let class = self.sim.part.as_ref().map_or(OFF, |p| {
            let z = self.stock.origin.z;
            let top = ray.last().map_or(z, |s| z + f64::from(s.hi));
            p.class(j * g.nu + i, top)
        });
        // The current toolpath if it changed the ray, else the latest one
        // before it that did.
        let b = &self.before;
        let toolpath = b.len().checked_sub(1).and_then(|m| {
            if b[m].grids[2].ray(i, j) != ray {
                return Some(m);
            }
            (0..m)
                .rev()
                .find(|&k| b[k].grids[2].ray(i, j) != b[k + 1].grids[2].ray(i, j))
        });
        Column {
            class,
            toolpath: toolpath.map(|k| k as u16),
        }
    }

    /// A vertex the columns call rest is rest only if the part is more than
    /// the tolerance away from it (not just below it): the vertical gap at
    /// a wall misstates the thickness. Within and gouge stay: the vertical
    /// gap is an upper bound of the distance.
    fn vertices(&self, v: &mut [(DVec3, u8)]) {
        // The queries of a block, which are slow near walls, in parallel.
        v.par_iter_mut()
            .with_min_len(32)
            .for_each(|(p, class)| *class = self.vertex(*p, *class));
    }

    fn vertex(&self, p: DVec3, class: u8) -> u8 {
        if class != REST {
            return class;
        }
        match &self.sim.part {
            Some(part) => match &part.clearance {
                Some(c) if !self.rest_is_clear(part, c.as_ref(), p) => WITHIN,
                _ => class,
            },
            None => class,
        }
    }
}

/// Columns around a vertex whose part heights bound its distance to the part.
const REACH: i64 = 2;

impl SimPaint<'_> {
    /// Whether the part is more than `part.tol` from `p`, for a vertex the
    /// columns call rest.
    fn rest_is_clear(&self, part: &PartHeights, clear: &dyn Clearance, p: DVec3) -> bool {
        // The query sees the position the mesh stores.
        let p = DVec3::new(
            f64::from(p.x as f32),
            f64::from(p.y as f32),
            f64::from(p.z as f32),
        );
        if self.under(part, p) {
            return false;
        }
        if self.far_above(part, p) {
            return true;
        }
        let key = [p.x as f32, p.y as f32].map(f32::to_bits);
        part.cache
            .get_or(key, || clear.clear_height(p.x, p.y, part.tol))
            .is_none_or(|h| p.z >= h)
    }

    /// True when `p` is below the lowest sampled part height of the 3 x 3
    /// columns around it, with the part under all of them: inside the part
    /// (or under an overhang of it), so not clear, as the query would say.
    /// This is the stock's bottom face and the sides of it below the part's
    /// top, most of the vertices the columns call rest.
    fn under(&self, part: &PartHeights, p: DVec3) -> bool {
        let d = self.stock;
        let g = &d.grids[2];
        let (ci, cj) = (
            ((p.x - d.origin.x) / d.res).floor() as i64,
            ((p.y - d.origin.y) / d.res).floor() as i64,
        );
        let mut low = f64::INFINITY;
        for j in cj - 1..=cj + 1 {
            for i in ci - 1..=ci + 1 {
                if i < 0 || j < 0 || i >= g.nu as i64 || j >= g.nv as i64 {
                    return false;
                }
                let h = part.top[j as usize * g.nu + i as usize];
                if h.is_nan() {
                    return false;
                }
                low = low.min(f64::from(h));
            }
        }
        p.z < low
    }

    /// True when the part heights alone show `p` more than the tolerance
    /// from the part. The part around `p` lies at or under the sampled
    /// heights of the columns near it; a point of a column is at least the
    /// larger of the vertical gap to its height and the sideways gap to the
    /// column away, and beyond [`REACH`] columns it is at least that many
    /// ray spacings away. (Heights are samples: a part feature narrower than
    /// a ray spacing between them is missed, as by the vertical class.)
    fn far_above(&self, part: &PartHeights, p: DVec3) -> bool {
        let d = self.stock;
        let g = &d.grids[2];
        let res = d.res;
        let (fx, fy) = ((p.x - d.origin.x) / res, (p.y - d.origin.y) / res);
        let (ci, cj) = (fx.floor() as i64, fy.floor() as i64);
        let mut near = REACH as f64 * res;
        for j in (cj - REACH).max(0)..=(cj + REACH).min(g.nv as i64 - 1) {
            let dy = ((j as f64 - fy).max(fy - (j + 1) as f64)).max(0.0) * res;
            for i in (ci - REACH).max(0)..=(ci + REACH).min(g.nu as i64 - 1) {
                let h = part.top[j as usize * g.nu + i as usize];
                if h.is_nan() {
                    continue;
                }
                let dx = ((i as f64 - fx).max(fx - (i + 1) as f64)).max(0.0) * res;
                // A lower bound of the sideways distance, √(dx² + dy²).
                let sideways = dx.max(dy);
                near = near.min((p.z - f64::from(h)).max(sideways));
                if near <= part.tol {
                    return false;
                }
            }
        }
        near > part.tol
    }
}

/// Seconds of cutting between two snapshots when the stride is adaptive
/// (`Limits::stride == 0`): a step back re-cuts about half of this on
/// average, at most all of it.
pub const TARGET_SECS: f64 = 0.003;
/// The adaptive stride is kept within these chord counts.
pub const MIN_STRIDE: usize = 8;
pub const MAX_STRIDE: usize = 4096;
/// A seek keeps a snapshot at every stride in the last `FINE` strides before
/// its target and cuts the rest in one batch, as without snapshots: a long
/// cut in stride-sized pieces with a snapshot at each was 1.2 to 3 times
/// slower (every snapshot makes the cut copy the tiles it touches next).
/// Back scrubbing past the fine zone cuts from the checkpoint again, no
/// slower than before.
const FINE: usize = 6;
/// The adaptive stride before any cut has been timed.
const FIRST_STRIDE: usize = 32;
/// Default bytes the snapshots of one player may hold.
pub const DEFAULT_BUDGET: usize = 64 << 20;

/// What a [`Player`] keeps to make steps back fast.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Chords between snapshots; a step back cuts less than this. 0 (the
    /// default) picks it from the measured cutting speed so that a stride
    /// takes about [`TARGET_SECS`], within [`MIN_STRIDE`] and
    /// [`MAX_STRIDE`]: the cost per chord differs by 100 times between
    /// projects (3 µs on the S5 relief, 0.6 ms with a V bit).
    pub stride: usize,
    /// Most bytes the snapshots hold (tiles shared with each other or with
    /// the simulation's checkpoints count once, or not at all). Over it,
    /// the snapshots above the stock shown go first, then the farthest below it.
    pub budget: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            stride: 0,
            budget: DEFAULT_BUDGET,
        }
    }
}

/// Copies of the stock at multiples of the stride, sharing tiles with each
/// other and with the checkpoints (copy on write).
#[derive(Clone, Debug, Default)]
struct Snapshots {
    /// Chord count to the stock and what it adds over the snapshot or
    /// checkpoint before it.
    map: BTreeMap<usize, (TriDexel, usize)>,
    /// Sum of the charges: an upper bound of the bytes held.
    bytes: usize,
}

impl Snapshots {
    /// What `d` at `at` adds over the nearest snapshot or checkpoint
    /// before it.
    fn charge(&self, sim: &Simulation, at: usize, d: &TriDexel) -> usize {
        let (cn, cd) = sim.checkpoint_at(at);
        let before = match self.map.range(..at).next_back() {
            Some((&k, (s, _))) if k > cn => s,
            _ => cd,
        };
        d.bytes_since(Some(before))
    }

    fn insert(&mut self, sim: &Simulation, at: usize, d: &TriDexel, limits: Limits, near: usize) {
        if self.map.contains_key(&at) || sim.checkpoint_at(at).0 == at {
            return;
        }
        let c = self.charge(sim, at, d);
        self.map.insert(at, (d.clone(), c));
        self.bytes += c;
        if self.bytes > limits.budget {
            self.evict(sim, limits.budget, near);
        }
    }

    /// Drops the snapshots farthest from `near` until the bound is within
    /// `budget`. Removing one can make its successor cost more, so the
    /// charges are worked out again after each batch.
    fn evict(&mut self, sim: &Simulation, budget: usize, near: usize) {
        while self.bytes > budget && !self.map.is_empty() {
            let mut keys: Vec<usize> = self.map.keys().copied().collect();
            // Above `near` first (a step back cannot start there, a step
            // forward starts from the stock shown), the farthest first.
            keys.sort_by_key(|&k| {
                let rank = if k > near {
                    (1, k - near)
                } else {
                    (0, near - k)
                };
                (std::cmp::Reverse(rank), k)
            });
            let goal = budget / 2;
            let mut est = self.bytes;
            for k in keys {
                if est <= goal {
                    break;
                }
                if let Some((_, c)) = self.map.remove(&k) {
                    est = est.saturating_sub(c);
                }
            }
            self.recharge(sim);
        }
    }

    fn recharge(&mut self, sim: &Simulation) {
        let mut charges = Vec::with_capacity(self.map.len());
        let mut prev: Option<(usize, &TriDexel)> = None;
        for (&k, (d, _)) in &self.map {
            let (cn, cd) = sim.checkpoint_at(k);
            let before = match prev {
                Some((pk, pd)) if pk > cn => pd,
                _ => cd,
            };
            charges.push(d.bytes_since(Some(before)));
            prev = Some((k, d));
        }
        self.bytes = charges.iter().sum();
        for ((_, c), n) in self.map.values_mut().zip(charges) {
            *c = n;
        }
    }
}

/// The stock and its mesh at a chord count.
#[derive(Clone, Debug)]
pub struct Player {
    n: usize,
    mesh: MeshTiles,
    limits: Limits,
    snaps: Snapshots,
    last_cut: usize,
    /// Seconds per chord measured on recent cuts; 0 before the first.
    rate: f64,
}

impl Player {
    /// The stock after `n` chords of `sim`, meshed whole.
    pub fn new(sim: &Simulation, n: usize) -> Player {
        Player::with_limits(sim, n, Limits::default())
    }

    /// As [`Player::new`], with the snapshot stride and budget given.
    pub fn with_limits(sim: &Simulation, n: usize, limits: Limits) -> Player {
        let n = n.min(sim.chords.len());
        let d = sim.stock_after(n);
        let mesh = MeshTiles::new(&d, &SimPaint::new(sim, &d, n));
        Player {
            n,
            mesh,
            limits,
            snaps: Snapshots::default(),
            last_cut: 0,
            rate: 0.0,
        }
    }

    /// Moves to the stock after `n` chords of `sim`, the simulation this
    /// player was made for. Returns the blocks rebuilt.
    ///
    /// The stock is cut on from the nearest of the stock shown (if before
    /// `n`), a snapshot and a checkpoint, whichever is latest at or before
    /// `n`; so a step back cuts less than a stride. Snapshots are kept at
    /// the multiples of the stride passed on the way. Span subtraction is a
    /// set difference, so the stock is the same whichever it starts from.
    pub fn seek(&mut self, sim: &Simulation, n: usize) -> Vec<usize> {
        let n = n.min(sim.chords.len());
        let (cp, cd) = sim.checkpoint_at(n);
        let mut base = (cp, cd);
        if let Some((&k, (s, _))) = self.snaps.map.range(..=n).next_back()
            && k > base.0
        {
            base = (k, s);
        }
        if self.n <= n && self.n >= base.0 {
            base = (self.n, self.mesh.stock());
        }
        let mut at = base.0;
        let mut d = base.1.clone();
        self.last_cut = n - at;
        // The stride is fixed for this seek, so the stops line up.
        let stride = self.stride();
        let fine = n.saturating_sub(FINE * stride).div_ceil(stride) * stride;
        while at < n {
            // The next place to keep a snapshot: every stride in the fine
            // zone before `n`; before it, the start of the fine zone.
            let stop = if at >= fine {
                (at / stride + 1) * stride
            } else {
                fine
            };
            let to = stop.min(n);
            // Only the pieces of the fine zone give the speed of small cuts.
            let timed = self.limits.stride == 0 && at >= fine && to - at >= MIN_STRIDE;
            let t = timed.then(std::time::Instant::now);
            sim.cut(&mut d, at..to);
            if let Some(t) = t {
                // Quick to follow a faster rate, slow to follow a slower
                // one: copying tiles for the snapshots is not the cut.
                let r = t.elapsed().as_secs_f64() / (to - at) as f64;
                self.rate = if self.rate == 0.0 || r < self.rate {
                    r
                } else {
                    0.9 * self.rate + 0.1 * r
                };
            }
            at = to;
            if at == stop && at % stride == 0 {
                self.snaps.insert(sim, at, &d, self.limits, n);
            }
        }
        let changed = self.mesh.update(&d, &SimPaint::new(sim, &d, n));
        self.n = n;
        changed
    }

    /// Chords between snapshots now.
    pub fn stride(&self) -> usize {
        match self.limits.stride {
            0 if self.rate > 0.0 => {
                ((TARGET_SECS / self.rate) as usize).clamp(MIN_STRIDE, MAX_STRIDE)
            }
            0 => FIRST_STRIDE,
            k => k,
        }
    }

    /// Chords the last [`Player::seek`] cut.
    pub fn last_cut(&self) -> usize {
        self.last_cut
    }

    /// Bytes the snapshots hold, at most (tiles shared with each other or
    /// with the simulation's checkpoints are not counted again).
    pub fn snapshot_bytes(&self) -> usize {
        self.snaps.bytes
    }

    /// Chords cut in the stock shown.
    pub fn chords(&self) -> usize {
        self.n
    }

    pub fn stock(&self) -> &TriDexel {
        self.mesh.stock()
    }

    pub fn mesh(&self) -> &MeshTiles {
        &self.mesh
    }
}

#[cfg(test)]
mod tests;
