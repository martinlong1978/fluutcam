// SPDX-License-Identifier: MIT OR Apache-2.0

//! Material removal on the tri-dexel (D11 §3).
//!
//! Chords are applied in batches. Within a batch, each family's rays are cut
//! in parallel by tiles of rows; only tiles that some chord reaches are
//! scheduled. Every tile applies the batch's chords in program order, so the
//! result does not depend on the thread count.

use crate::dexel::{DexelGrid, Spans, TriDexel, subtract};
use crate::sweep::Sweep;
use glam::DVec3;
use rayon::prelude::*;

/// Chords per batch.
pub(crate) const BATCH: usize = 32768;
/// Grid rows per tile.
pub(crate) const ROWS: usize = 2;

/// Work counters, per family where indexed.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Stats {
    pub chords: u64,
    /// Rays inside a chord's box that still had material there.
    pub tests: [u64; 3],
    /// Rays the chord cut.
    pub cuts: [u64; 3],
    /// Wall-clock time per family, in seconds.
    pub secs: [f64; 3],
}

impl Stats {
    pub(crate) fn add(&mut self, o: &Stats) {
        self.chords += o.chords;
        for k in 0..3 {
            self.tests[k] += o.tests[k];
            self.cuts[k] += o.cuts[k];
        }
    }
}

/// Removes the volume swept by each chord, in order.
pub fn cut(d: &mut TriDexel, sweeps: &[Sweep]) -> Stats {
    let mut stats = Stats {
        chords: sweeps.len() as u64,
        ..Stats::default()
    };
    let (res, origin) = (d.res, d.origin);
    for batch in sweeps.chunks(BATCH) {
        for g in &mut d.grids {
            let t = std::time::Instant::now();
            stats.add(&cut_family(g, res, origin, batch));
            stats.secs[g.axis] += t.elapsed().as_secs_f64();
        }
    }
    stats
}

/// Index range of rays whose centre lies in `[lo, hi]` along an axis.
pub(crate) fn ray_range(
    lo: f64,
    hi: f64,
    origin: f64,
    res: f64,
    n: usize,
) -> Option<(usize, usize)> {
    let a = ((lo - origin) / res - 0.5).ceil().max(0.0);
    let b = ((hi - origin) / res - 0.5).floor();
    if b < a || a >= n as f64 {
        return None;
    }
    Some((a as usize, (b as usize).min(n - 1)))
}

fn cut_family(g: &mut DexelGrid, res: f64, origin: DVec3, batch: &[Sweep]) -> Stats {
    let k = g.axis;
    let (u, v) = ((k + 1) % 3, (k + 2) % 3);
    let (nu, nv) = (g.nu, g.nv);
    let ntiles = nv.div_ceil(ROWS);
    // Per tile, the chords that may touch it, in program order.
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); ntiles];
    for (si, s) in batch.iter().enumerate() {
        if let Some((j0, j1)) = ray_range(s.min[v], s.max[v], origin[v], res, nv) {
            for bin in &mut bins[j0 / ROWS..=j1 / ROWS] {
                bin.push(si as u32);
            }
        }
    }
    let mut e = DVec3::ZERO;
    e[k] = 1.0;
    let tiles = g.rays.chunks_mut(nu * ROWS).enumerate().zip(&bins);
    par_tiles(tiles, |(tile, rays), bin| {
        let mut st = Stats::default();
        let row0 = tile * ROWS;
        for &si in bin {
            let s = &batch[si as usize];
            let Some((i0, i1)) = ray_range(s.min[u], s.max[u], origin[u], res, nu) else {
                continue;
            };
            let Some((j0, j1)) = ray_range(s.min[v], s.max[v], origin[v], res, nv) else {
                continue;
            };
            let (ta, tb) = ((s.min[k] - origin[k]) as f32, (s.max[k] - origin[k]) as f32);
            for j in j0.max(row0)..=j1.min(row0 + ROWS - 1) {
                for i in i0..=i1 {
                    let spans: &mut Spans = &mut rays[(j - row0) * nu + i];
                    if !spans.iter().any(|sp| sp.hi > ta && sp.lo < tb) {
                        continue;
                    }
                    st.tests[k] += 1;
                    let mut o = origin;
                    o[u] += (i as f64 + 0.5) * res;
                    o[v] += (j as f64 + 0.5) * res;
                    if let Some((t0, t1)) = s.ray(o, e)
                        && subtract(spans, t0 as f32, t1 as f32)
                    {
                        st.cuts[k] += 1;
                    }
                }
            }
        }
        st
    })
}

/// Cuts every tile that has chords, each as its own parallel work item, and
/// sums the counters. On a raster a batch covers a narrow band of the grid,
/// so most tiles are empty; leaving them out lets the busy band spread over
/// all threads.
pub(crate) fn par_tiles<'a, T: Send>(
    tiles: impl Iterator<Item = (T, &'a Vec<u32>)>,
    f: impl Fn(T, &[u32]) -> Stats + Sync,
) -> Stats {
    let work: Vec<(T, &[u32])> = tiles
        .filter(|(_, bin)| !bin.is_empty())
        .map(|(t, bin)| (t, bin.as_slice()))
        .collect();
    work.into_par_iter()
        .map(|(t, bin)| f(t, bin))
        .reduce(Stats::default, |mut a, b| {
            a.add(&b);
            a
        })
}

#[cfg(test)]
mod tests;
