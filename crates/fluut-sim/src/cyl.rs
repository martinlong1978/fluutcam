// SPDX-License-Identifier: MIT OR Apache-2.0

//! Cylindrical dexels for rotary Setups (S5 follow-up).
//!
//! The stock is the rotary cylinder on the X axis. Radial rays start on the
//! axis and run outward on an (x, θ) grid: ray `(i, j)` is at
//! `x = x0 + (i + ½)·res` along `(0, sin θ_j, cos θ_j)`, `θ_j = j·2π/nθ`, which
//! is the tool axis at `A = θ_j`. Near the tool a radial ray runs almost
//! along the tool axis. X rays lie on the tri-dexel's (y, z) grid, but only
//! the rays inside the stock circle are stored.

use crate::dexel::{Memory, Span, Spans, subtract};
use crate::sim::{BATCH, ROWS, Stats, par_tiles, ray_range};
use crate::stock::Stock;
use crate::sweep::Sweep;
use glam::DVec3;
use std::f64::consts::{PI, TAU};

/// Index of the X-ray family in [`Stats`].
pub const X: usize = 0;
/// Index of the radial family in [`Stats`].
pub const RADIAL: usize = 1;

#[derive(Clone, Debug)]
pub struct CylDexel {
    pub res: f64,
    pub x0: f64,
    pub radius: f64,
    /// Radial rays along X and around the axis.
    pub nx: usize,
    pub nt: usize,
    /// Radial rays, index `j·nx + i`; spans are radii.
    pub radial: Vec<Spans>,
    /// `(sin θ_j, cos θ_j)`.
    dirs: Vec<(f64, f64)>,
    /// X rays: the grid is `n × n` over `[−R, R]²`, `y` by column `i` and
    /// `z` by row `j`. Row `j` stores columns `first[j]..` from
    /// `start[j]` to `start[j + 1]`. Spans are offsets from `x0`.
    pub n: usize,
    first: Vec<usize>,
    start: Vec<usize>,
    pub xrays: Vec<Spans>,
}

impl CylDexel {
    /// Rays at spacing `res` (at most `res` apart at the outer radius),
    /// filled with the cylinder's material.
    ///
    /// # Panics
    /// If `stock` is not a cylinder or `res` is not positive.
    pub fn new(stock: &Stock, res: f64) -> CylDexel {
        let Stock::Cylinder { x0, x1, radius } = *stock else {
            panic!("cylindrical dexels need a cylinder stock");
        };
        assert!(res > 0.0, "resolution must be positive");
        let nx = (((x1 - x0) / res).ceil() as usize).max(1);
        let nt = ((TAU * radius / res).ceil() as usize).max(3);
        let dirs = (0..nt)
            .map(|j| libm::sincos(j as f64 * TAU / nt as f64))
            .collect();
        let full = Spans::from_slice(&[Span {
            lo: 0.0,
            hi: radius as f32,
        }]);
        let radial = vec![full; nx * nt];
        let n = ((2.0 * radius / res).ceil() as usize).max(1);
        let (mut first, mut start) = (Vec::with_capacity(n), Vec::with_capacity(n + 1));
        start.push(0);
        let at = |k: usize| -radius + (k as f64 + 0.5) * res;
        for j in 0..n {
            let z = at(j);
            let inside: Vec<usize> = (0..n)
                .filter(|&i| at(i) * at(i) + z * z <= radius * radius)
                .collect();
            first.push(inside.first().copied().unwrap_or(0));
            start.push(start[j] + inside.len());
        }
        let len = Spans::from_slice(&[Span {
            lo: 0.0,
            hi: (x1 - x0) as f32,
        }]);
        let xrays = vec![len; start[n]];
        CylDexel {
            res,
            x0,
            radius,
            nx,
            nt,
            radial,
            dirs,
            n,
            first,
            start,
            xrays,
        }
    }

    /// Angle of radial row `j`.
    pub fn theta(&self, j: usize) -> f64 {
        j as f64 * TAU / self.nt as f64
    }

    /// Origin and direction of radial ray `(i, j)`.
    pub fn radial_ray(&self, i: usize, j: usize) -> (DVec3, DVec3) {
        let (s, c) = self.dirs[j];
        (
            DVec3::new(self.x0 + (i as f64 + 0.5) * self.res, 0.0, 0.0),
            DVec3::new(0.0, s, c),
        )
    }

    /// The stored X ray at grid position `(i, j)`, if any, and its origin.
    pub fn x_ray(&self, i: usize, j: usize) -> Option<(&Spans, DVec3)> {
        let k = i.checked_sub(self.first[j])?;
        let idx = self.start[j] + k;
        (idx < self.start[j + 1]).then(|| {
            let at = |k: usize| -self.radius + (k as f64 + 0.5) * self.res;
            (&self.xrays[idx], DVec3::new(self.x0, at(i), at(j)))
        })
    }

    /// Material volume from each family: `[X, radial]`.
    pub fn volumes(&self) -> [f64; 2] {
        let dt = TAU / self.nt as f64;
        let radial: f64 = self
            .radial
            .iter()
            .flat_map(|r| r.iter())
            .map(|s| {
                let (lo, hi) = (f64::from(s.lo), f64::from(s.hi));
                0.5 * (hi * hi - lo * lo)
            })
            .sum::<f64>()
            * dt
            * self.res;
        let x: f64 = self
            .xrays
            .iter()
            .flat_map(|r| r.iter())
            .map(|s| f64::from(s.hi) - f64::from(s.lo))
            .sum::<f64>()
            * self.res
            * self.res;
        [x, radial]
    }

    pub fn memory(&self) -> Memory {
        let mut m = Memory::default();
        for rays in [&self.radial, &self.xrays] {
            m.rays += rays.len();
            m.inline += rays.capacity() * size_of::<Spans>();
            for r in rays.iter().filter(|r| r.spilled()) {
                m.spilled += 1;
                m.heap += r.capacity() * size_of::<Span>();
            }
        }
        m.inline += self.dirs.capacity() * size_of::<(f64, f64)>()
            + (self.first.capacity() + self.start.capacity()) * size_of::<usize>();
        m
    }

    /// FNV-1a hash of every span's bits in ray order, for determinism tests.
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |x: u32| {
            for b in x.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for r in self.radial.iter().chain(&self.xrays) {
            eat(r.len() as u32);
            for s in r {
                eat(s.lo.to_bits());
                eat(s.hi.to_bits());
            }
        }
        h
    }

    /// Removes the volume swept by each chord, in order. In the returned
    /// [`Stats`], family [`X`] is the X rays and [`RADIAL`] the radial rays.
    pub fn cut(&mut self, sweeps: &[Sweep]) -> Stats {
        let mut stats = Stats {
            chords: sweeps.len() as u64,
            ..Stats::default()
        };
        for batch in sweeps.chunks(BATCH) {
            let t = std::time::Instant::now();
            stats.add(&self.cut_radial(batch));
            stats.secs[RADIAL] += t.elapsed().as_secs_f64();
            let t = std::time::Instant::now();
            stats.add(&self.cut_x(batch));
            stats.secs[X] += t.elapsed().as_secs_f64();
        }
        stats
    }

    /// θ rows `j0..=j1` (unwrapped; may run past `nt` or below 0) and the
    /// radius range a chord can touch.
    fn radial_reach(&self, s: &Sweep) -> Option<(i64, i64, f32, f32)> {
        let (y0, y1, z0, z1) = (s.min.y, s.max.y, s.min.z, s.max.z);
        let near_y = 0.0_f64.clamp(y0, y1);
        let near_z = 0.0_f64.clamp(z0, z1);
        let rmin = (near_y * near_y + near_z * near_z).sqrt();
        let fy = y0.abs().max(y1.abs());
        let fz = z0.abs().max(z1.abs());
        let rmax = (fy * fy + fz * fz).sqrt();
        if rmin > self.radius {
            return None;
        }
        let nt = self.nt as i64;
        let (j0, j1) = if rmin == 0.0 {
            (0, nt - 1)
        } else {
            // The box misses the axis, so its corners bound its angles,
            // which span less than π.
            let mid = libm::atan2(0.5 * (y0 + y1), 0.5 * (z0 + z1));
            let (mut lo, mut hi) = (0.0_f64, 0.0_f64);
            for (y, z) in [(y0, z0), (y0, z1), (y1, z0), (y1, z1)] {
                let mut d = libm::atan2(y, z) - mid;
                if d > PI {
                    d -= TAU;
                } else if d < -PI {
                    d += TAU;
                }
                lo = lo.min(d);
                hi = hi.max(d);
            }
            let dt = TAU / self.nt as f64;
            let j0 = ((mid + lo) / dt).ceil() as i64;
            let j1 = ((mid + hi) / dt).floor() as i64;
            if j1 < j0 {
                return None;
            }
            (j0, j1.min(j0 + nt - 1))
        };
        Some((j0, j1, rmin as f32, rmax as f32))
    }

    fn cut_radial(&mut self, batch: &[Sweep]) -> Stats {
        let (nx, nt, res, x0) = (self.nx, self.nt, self.res, self.x0);
        let ntiles = nt.div_ceil(ROWS);
        let reach: Vec<_> = batch.iter().map(|s| self.radial_reach(s)).collect();
        let mut bins: Vec<Vec<u32>> = vec![Vec::new(); ntiles];
        for (si, r) in reach.iter().enumerate() {
            let Some((j0, j1, ..)) = *r else { continue };
            let mut last = usize::MAX;
            for j in j0..=j1 {
                let tile = j.rem_euclid(nt as i64) as usize / ROWS;
                if tile != last {
                    bins[tile].push(si as u32);
                    last = tile;
                }
            }
        }
        // A chord whose rows wrap may revisit a tile; keep each once, in order.
        for bin in &mut bins {
            bin.dedup();
        }
        let dirs = &self.dirs;
        let tiles = self.radial.chunks_mut(nx * ROWS).enumerate().zip(&bins);
        par_tiles(tiles, |(tile, rays), bin| {
            let mut st = Stats::default();
            let row0 = tile * ROWS;
            let rows = rays.len() / nx;
            for &si in bin {
                let s = &batch[si as usize];
                let Some((j0, j1, ta, tb)) = reach[si as usize] else {
                    continue;
                };
                let Some((i0, i1)) = ray_range(s.min.x, s.max.x, x0, res, nx) else {
                    continue;
                };
                for j in row0..row0 + rows {
                    if (j as i64 - j0).rem_euclid(nt as i64) > j1 - j0 {
                        continue;
                    }
                    let (sn, cs) = dirs[j];
                    let e = DVec3::new(0.0, sn, cs);
                    for i in i0..=i1 {
                        let spans: &mut Spans = &mut rays[(j - row0) * nx + i];
                        if !spans.iter().any(|sp| sp.hi > ta && sp.lo < tb) {
                            continue;
                        }
                        st.tests[RADIAL] += 1;
                        let o = DVec3::new(x0 + (i as f64 + 0.5) * res, 0.0, 0.0);
                        if let Some((t0, t1)) = s.ray(o, e)
                            && subtract(spans, t0 as f32, t1 as f32)
                        {
                            st.cuts[RADIAL] += 1;
                        }
                    }
                }
            }
            st
        })
    }

    fn cut_x(&mut self, batch: &[Sweep]) -> Stats {
        let (n, res, x0, rad) = (self.n, self.res, self.x0, self.radius);
        let ntiles = n.div_ceil(ROWS);
        let mut bins: Vec<Vec<u32>> = vec![Vec::new(); ntiles];
        for (si, s) in batch.iter().enumerate() {
            if let Some((j0, j1)) = ray_range(s.min.z, s.max.z, -rad, res, n) {
                for bin in &mut bins[j0 / ROWS..=j1 / ROWS] {
                    bin.push(si as u32);
                }
            }
        }
        // Split the rays into tiles of ROWS rows.
        let mut tiles = Vec::with_capacity(ntiles);
        let mut rest: &mut [Spans] = &mut self.xrays;
        for t in 0..ntiles {
            let rows = (t * ROWS)..((t + 1) * ROWS).min(n);
            let (head, tail) = rest.split_at_mut(self.start[rows.end] - self.start[rows.start]);
            tiles.push((rows, head));
            rest = tail;
        }
        let (first, start) = (&self.first, &self.start);
        par_tiles(tiles.into_iter().zip(&bins), |(rows, rays), bin| {
            let mut st = Stats::default();
            let base = start[rows.start];
            for &si in bin {
                let s = &batch[si as usize];
                let Some((i0, i1)) = ray_range(s.min.y, s.max.y, -rad, res, n) else {
                    continue;
                };
                let Some((j0, j1)) = ray_range(s.min.z, s.max.z, -rad, res, n) else {
                    continue;
                };
                let (ta, tb) = ((s.min.x - x0) as f32, (s.max.x - x0) as f32);
                for j in j0.max(rows.start)..=j1.min(rows.end - 1) {
                    let (f, len) = (first[j], start[j + 1] - start[j]);
                    let (a, b) = (i0.max(f), i1.min((f + len).saturating_sub(1)));
                    if len == 0 || a > b {
                        continue;
                    }
                    for i in a..=b {
                        let spans: &mut Spans = &mut rays[start[j] - base + i - f];
                        if !spans.iter().any(|sp| sp.hi > ta && sp.lo < tb) {
                            continue;
                        }
                        st.tests[X] += 1;
                        let o = DVec3::new(
                            x0,
                            -rad + (i as f64 + 0.5) * res,
                            -rad + (j as f64 + 0.5) * res,
                        );
                        if let Some((t0, t1)) = s.ray(o, DVec3::X)
                            && subtract(spans, t0 as f32, t1 as f32)
                        {
                            st.cuts[X] += 1;
                        }
                    }
                }
            }
            st
        })
    }
}

#[cfg(test)]
mod tests;
