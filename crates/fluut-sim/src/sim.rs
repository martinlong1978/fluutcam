// SPDX-License-Identifier: MIT OR Apache-2.0

//! Material removal on the tri-dexel (D11 §3).
//!
//! Chords are applied in batches. Within a batch, each family's rays are cut
//! in parallel by tiles of rows; only tiles that some chord reaches are
//! scheduled. Every tile applies the batch's chords in program order, so the
//! result does not depend on the thread count.
//!
//! Before a chord cuts, its probes (the non-cutting parts of the tool, D11
//! §5) are tested against the material. Hits are collected per tile and
//! sorted, so they do not depend on the thread count either.

use crate::dexel::{DexelGrid, Spans, TriDexel, subtract};
use crate::sweep::Sweep;
use glam::DVec3;
use rayon::prelude::*;
use std::sync::Arc;

pub(crate) use crate::dexel::ROWS;

/// Chords per batch.
pub(crate) const BATCH: usize = 32768;

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
            self.secs[k] += o.secs[k];
        }
    }
}

/// What the removal loop needs from a chord besides its sweep.
pub(crate) trait Chord: Sync {
    fn sweep(&self) -> &Sweep;

    /// Box of the sweep and of every probe.
    fn reach(&self) -> (DVec3, DVec3) {
        (self.sweep().min, self.sweep().max)
    }

    /// Material the sweep meets is reported (a rapid move).
    fn reports_contact(&self) -> bool {
        false
    }
}

impl Chord for Sweep {
    fn sweep(&self) -> &Sweep {
        self
    }
}

/// The non-cutting parts of a chord's tool, each with a kind, to test
/// against the material before the chord cuts.
pub(crate) type Probes<'a, C> = &'a (dyn Fn(&C, &mut Vec<(u8, Sweep)>) + Sync);

/// Kind of a [`Hit`] from a chord that reports contact.
pub(crate) const CONTACT: u8 = 0;

/// Material met by a probe, or by a chord that reports contact.
#[derive(Copy, Clone, PartialEq, Debug)]
pub(crate) struct Hit {
    /// Chord index, from the start of the slice given to [`cut_chords`].
    pub chord: u32,
    pub kind: u8,
    pub family: u8,
    /// Ray `j·nu + i` of the family.
    pub ray: u32,
    /// Where the material starts along the ray.
    pub point: DVec3,
}

impl Hit {
    fn key(&self) -> (u32, u8, u8, u32) {
        (self.chord, self.kind, self.family, self.ray)
    }
}

/// Removes the volume swept by each chord, in order.
pub fn cut(d: &mut TriDexel, sweeps: &[Sweep]) -> Stats {
    cut_chords(d, sweeps, None, &|_| false).0
}

/// Removes the volume of each chord in order and reports the first hit of
/// each chord and kind, sorted. Before each batch `stop` gets the number of
/// chords done; when it returns `true` the run stops there, with the stock
/// cut part of the way.
pub(crate) fn cut_chords<C: Chord>(
    d: &mut TriDexel,
    chords: &[C],
    probes: Option<Probes<C>>,
    stop: &dyn Fn(usize) -> bool,
) -> (Stats, Vec<Hit>) {
    let mut stats = Stats {
        chords: chords.len() as u64,
        ..Stats::default()
    };
    let mut hits = Vec::new();
    let (res, origin) = (d.res, d.origin);
    for (bi, batch) in chords.chunks(BATCH).enumerate() {
        if stop(bi * BATCH) {
            break;
        }
        for g in &mut d.grids {
            let t = std::time::Instant::now();
            let base = (bi * BATCH) as u32;
            let (st, h) = cut_family(g, res, origin, batch, base, probes);
            stats.add(&st);
            hits.extend(h);
            stats.secs[g.axis] += t.elapsed().as_secs_f64();
        }
    }
    // Each tile kept its first hit per chord and kind; the first over all
    // tiles has the smallest key.
    hits.sort_by_key(Hit::key);
    hits.dedup_by_key(|h| (h.chord, h.kind));
    (stats, hits)
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

/// Where `[a, b]` first overlaps a span, if it does.
fn overlap(spans: &Spans, a: f32, b: f32) -> Option<f32> {
    spans
        .iter()
        .find(|s| s.hi > a && s.lo < b)
        .map(|s| s.lo.max(a))
}

/// Rays of a family inside a box: `(i0, i1, j0, j1)` and the box's range
/// along the rays.
type Window = (usize, usize, usize, usize, f32, f32);

fn cut_family<C: Chord>(
    g: &mut DexelGrid,
    res: f64,
    origin: DVec3,
    batch: &[C],
    base: u32,
    probes: Option<Probes<C>>,
) -> (Stats, Vec<Hit>) {
    let k = g.axis;
    let (u, v) = ((k + 1) % 3, (k + 2) % 3);
    let (nu, nv) = (g.nu, g.nv);
    let ntiles = nv.div_ceil(ROWS);
    // Per tile, the chords that may touch it, in program order.
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); ntiles];
    for (si, s) in batch.iter().enumerate() {
        let (lo, hi) = s.reach();
        if let Some((j0, j1)) = ray_range(lo[v], hi[v], origin[v], res, nv) {
            for bin in &mut bins[j0 / ROWS..=j1 / ROWS] {
                bin.push(si as u32);
            }
        }
    }
    let mut e = DVec3::ZERO;
    e[k] = 1.0;
    let ray_origin = |i: usize, j: usize| {
        let mut o = origin;
        o[u] += (i as f64 + 0.5) * res;
        o[v] += (j as f64 + 0.5) * res;
        o
    };
    // Rays of rows `row0..row0 + rows` inside the box `lo..hi`.
    let window = |lo: DVec3, hi: DVec3, row0: usize, rows: usize| -> Option<Window> {
        let (i0, i1) = ray_range(lo[u], hi[u], origin[u], res, nu)?;
        let (j0, j1) = ray_range(lo[v], hi[v], origin[v], res, nv)?;
        let (j0, j1) = (j0.max(row0), j1.min(row0 + rows - 1));
        let (ta, tb) = ((lo[k] - origin[k]) as f32, (hi[k] - origin[k]) as f32);
        (j0 <= j1).then_some((i0, i1, j0, j1, ta, tb))
    };
    let tiles = g.tiles_mut().iter_mut().enumerate().zip(&bins);
    par_tiles_with(
        tiles,
        |(tile, rays): (usize, &mut Arc<Vec<Spans>>), bin| {
            let mut st = Stats::default();
            let mut hits: Vec<Hit> = Vec::new();
            let mut parts = Vec::new();
            let row0 = tile * ROWS;
            let rows = rays.len() / nu.max(1);
            let mut hit = |chord: u32, kind: u8, ray: usize, p: DVec3| {
                if !hits.iter().any(|h| h.chord == chord && h.kind == kind) {
                    hits.push(Hit {
                        chord,
                        kind,
                        family: k as u8,
                        ray: ray as u32,
                        point: p,
                    });
                }
            };
            for &si in bin {
                let c = &batch[si as usize];
                let chord = base + si;
                if let Some(pr) = probes {
                    parts.clear();
                    pr(c, &mut parts);
                    for (kind, p) in &parts {
                        let Some((i0, i1, j0, j1, ta, tb)) = window(p.min, p.max, row0, rows)
                        else {
                            continue;
                        };
                        for j in j0..=j1 {
                            for i in i0..=i1 {
                                let spans = &rays[(j - row0) * nu + i];
                                if overlap(spans, ta, tb).is_none() {
                                    continue;
                                }
                                let o = ray_origin(i, j);
                                if let Some((t0, t1)) = p.ray(o, e)
                                    && let Some(t) = overlap(spans, t0 as f32, t1 as f32)
                                {
                                    hit(chord, *kind, j * nu + i, o + e * f64::from(t));
                                }
                            }
                        }
                    }
                }
                let s = c.sweep();
                let Some((i0, i1, j0, j1, ta, tb)) = window(s.min, s.max, row0, rows) else {
                    continue;
                };
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        let idx = (j - row0) * nu + i;
                        if overlap(&rays[idx], ta, tb).is_none() {
                            continue;
                        }
                        st.tests[k] += 1;
                        let o = ray_origin(i, j);
                        let Some((t0, t1)) = s.ray(o, e) else {
                            continue;
                        };
                        let (t0, t1) = (t0 as f32, t1 as f32);
                        let Some(t) = overlap(&rays[idx], t0, t1) else {
                            continue;
                        };
                        if c.reports_contact() {
                            hit(chord, CONTACT, j * nu + i, o + e * f64::from(t));
                        }
                        // Copies the tile only while a checkpoint shares it.
                        if subtract(&mut Arc::make_mut(rays)[idx], t0, t1) {
                            st.cuts[k] += 1;
                        }
                    }
                }
            }
            (st, hits)
        },
        |(mut a, mut ha), (b, hb)| {
            a.add(&b);
            ha.extend(hb);
            (a, ha)
        },
    )
}

/// Cuts every tile that has chords, each as its own parallel work item, and
/// sums the counters. On a raster a batch covers a narrow band of the grid,
/// so most tiles are empty; leaving them out lets the busy band spread over
/// all threads.
pub(crate) fn par_tiles<'a, T: Send>(
    tiles: impl Iterator<Item = (T, &'a Vec<u32>)>,
    f: impl Fn(T, &[u32]) -> Stats + Sync,
) -> Stats {
    par_tiles_with(tiles, f, |mut a, b| {
        a.add(&b);
        a
    })
}

/// As [`par_tiles`], with any result and a way to join two.
pub(crate) fn par_tiles_with<'a, T: Send, R: Send + Default>(
    tiles: impl Iterator<Item = (T, &'a Vec<u32>)>,
    f: impl Fn(T, &[u32]) -> R + Sync,
    join: impl Fn(R, R) -> R + Sync + Send,
) -> R {
    let work: Vec<(T, &[u32])> = tiles
        .filter(|(_, bin)| !bin.is_empty())
        .map(|(t, bin)| (t, bin.as_slice()))
        .collect();
    work.into_par_iter()
        .map(|(t, bin)| f(t, bin))
        .reduce(R::default, join)
}

#[cfg(test)]
mod tests;
