// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tri-dexel grids (D11 §2).
//!
//! Family `k` holds rays along axis `k`. Ray `(i, j)` of a family lies at
//! `u = min_u + (i + ½)·res`, `v = min_v + (j + ½)·res`, where `u = (k + 1) % 3`
//! and `v = (k + 2) % 3`. Its spans are `f32` offsets from `min_k`.
//!
//! Rays are stored in tiles of [`ROWS`] grid rows, each behind an `Arc`. A
//! clone of the stock shares every tile, and a cut copies only the tiles it
//! changes, so checkpoints are cheap (D11 §4).

use crate::stock::Stock;
use fluut_geom::Point3;
use glam::DVec3;
use smallvec::SmallVec;
use std::sync::Arc;

/// Grid rows per tile: the unit of parallel work and of copy-on-write.
pub const ROWS: usize = 2;

/// A material span `[lo, hi]` along a ray, `lo < hi`.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Span {
    pub lo: f32,
    pub hi: f32,
}

/// Spans of one ray: sorted, disjoint, non-empty.
pub type Spans = SmallVec<[Span; 2]>;

/// The rays of [`ROWS`] grid rows, row-major.
pub type Tile = Vec<Spans>;

/// One family of parallel rays.
#[derive(Clone, Debug)]
pub struct DexelGrid {
    /// Ray direction: 0, 1 or 2 for X, Y, Z.
    pub axis: usize,
    pub nu: usize,
    pub nv: usize,
    /// Tile `t` holds rows `t·ROWS..` (fewer in the last tile).
    tiles: Vec<Arc<Tile>>,
}

/// Three dexel grids over the same stock box.
#[derive(Clone, Debug)]
pub struct TriDexel {
    pub res: f64,
    /// Minimum corner of the stock box.
    pub origin: DVec3,
    pub size: DVec3,
    pub grids: [DexelGrid; 3],
}

/// Memory use in bytes.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Memory {
    pub rays: usize,
    /// Ray headers (the inline part of each ray).
    pub inline: usize,
    /// Spans that spilled to the heap.
    pub heap: usize,
    /// Rays whose spans spilled to the heap.
    pub spilled: usize,
}

impl Memory {
    pub fn total(&self) -> usize {
        self.inline + self.heap
    }
}

/// Removes `[a, b]` from `spans`. Returns whether anything changed.
pub fn subtract(spans: &mut Spans, a: f32, b: f32) -> bool {
    if a >= b {
        return false;
    }
    let (Some(first), Some(last)) = (spans.first(), spans.last()) else {
        return false;
    };
    if b <= first.lo || a >= last.hi {
        return false;
    }
    if !spans.iter().any(|s| s.hi > a && s.lo < b) {
        return false;
    }
    let mut out = Spans::new();
    for &s in spans.iter() {
        if s.hi <= a || s.lo >= b {
            out.push(s);
            continue;
        }
        if s.lo < a {
            out.push(Span { lo: s.lo, hi: a });
        }
        if s.hi > b {
            out.push(Span { lo: b, hi: s.hi });
        }
    }
    *spans = out;
    true
}

impl DexelGrid {
    /// Position of ray `(i, j)` at `t = 0` (on the box's minimum face).
    pub fn ray_origin(&self, d: &TriDexel, i: usize, j: usize) -> DVec3 {
        let (u, v) = ((self.axis + 1) % 3, (self.axis + 2) % 3);
        let mut p = d.origin;
        p[u] += (i as f64 + 0.5) * d.res;
        p[v] += (j as f64 + 0.5) * d.res;
        p
    }

    /// Total span length over all rays.
    pub fn length(&self) -> f64 {
        self.rays()
            .flat_map(|r| r.iter())
            .map(|s| f64::from(s.hi) - f64::from(s.lo))
            .sum()
    }

    /// The spans of ray `(i, j)`.
    pub fn ray(&self, i: usize, j: usize) -> &Spans {
        &self.tiles[j / ROWS][(j % ROWS) * self.nu + i]
    }

    /// The spans of ray `(i, j)`, copying its tile first if a checkpoint
    /// shares it.
    pub fn ray_mut(&mut self, i: usize, j: usize) -> &mut Spans {
        let nu = self.nu;
        &mut Arc::make_mut(&mut self.tiles[j / ROWS])[(j % ROWS) * nu + i]
    }

    /// Every ray, row by row.
    pub fn rays(&self) -> impl Iterator<Item = &Spans> {
        self.tiles.iter().flat_map(|t| t.iter())
    }

    pub fn ray_count(&self) -> usize {
        self.nu * self.nv
    }

    /// Drops every ray (measurements that need one family only).
    pub fn clear(&mut self) {
        self.tiles.clear();
        self.nv = 0;
    }

    pub(crate) fn tiles_mut(&mut self) -> &mut [Arc<Tile>] {
        &mut self.tiles
    }

    /// Bytes of the tiles not in `seen`, which then holds them.
    pub(crate) fn unique_bytes(&self, seen: &mut std::collections::BTreeSet<usize>) -> usize {
        let mut n = 0;
        for t in &self.tiles {
            if seen.insert(Arc::as_ptr(t) as usize) {
                n += t.len() * size_of::<Spans>();
                n += t
                    .iter()
                    .filter(|r| r.spilled())
                    .map(|r| r.capacity() * size_of::<Span>())
                    .sum::<usize>();
            }
        }
        n
    }

    /// Tiles held in common with `other`, for tests of copy-on-write.
    pub fn shared_tiles(&self, other: &DexelGrid) -> usize {
        self.tiles
            .iter()
            .zip(&other.tiles)
            .filter(|(a, b)| Arc::ptr_eq(a, b))
            .count()
    }
}

impl TriDexel {
    /// Rays at spacing `res` over the stock's bounding box, filled with the
    /// stock's material.
    pub fn new(stock: &Stock, res: f64) -> TriDexel {
        assert!(res > 0.0, "resolution must be positive");
        let b = stock.bbox();
        let origin = b.min.0;
        let size = b.max.0 - b.min.0;
        let n = |k: usize| ((size[k] / res).ceil() as usize).max(1);
        let mut d = TriDexel {
            res,
            origin,
            size,
            grids: [0, 1, 2].map(|axis| DexelGrid {
                axis,
                nu: n((axis + 1) % 3),
                nv: n((axis + 2) % 3),
                tiles: Vec::new(),
            }),
        };
        for k in 0..3 {
            let g = &d.grids[k];
            let mut tiles = Vec::with_capacity(g.nv.div_ceil(ROWS));
            for j0 in (0..g.nv).step_by(ROWS) {
                let mut rays = Vec::with_capacity(g.nu * ROWS);
                for j in j0..(j0 + ROWS).min(g.nv) {
                    for i in 0..g.nu {
                        let p = g.ray_origin(&d, i, j);
                        let mut spans = Spans::new();
                        if let Some((lo, hi)) = stock.chord(p, k) {
                            spans.push(Span {
                                lo: lo as f32,
                                hi: hi as f32,
                            });
                        }
                        rays.push(spans);
                    }
                }
                tiles.push(Arc::new(rays));
            }
            d.grids[k].tiles = tiles;
        }
        d
    }

    pub fn origin_point(&self) -> Point3 {
        Point3(self.origin)
    }

    /// Material volume estimated from each family: `res² · Σ span lengths`.
    pub fn volumes(&self) -> [f64; 3] {
        self.grids
            .each_ref()
            .map(|g| g.length() * self.res * self.res)
    }

    pub fn memory(&self) -> Memory {
        let mut m = Memory::default();
        for g in &self.grids {
            m.rays += g.ray_count();
            m.inline += g.ray_count() * size_of::<Spans>();
            for r in g.rays() {
                if r.spilled() {
                    m.spilled += 1;
                    m.heap += r.capacity() * size_of::<Span>();
                }
            }
        }
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
        for g in &self.grids {
            for r in g.rays() {
                eat(r.len() as u32);
                for s in r {
                    eat(s.lo.to_bits());
                    eat(s.hi.to_bits());
                }
            }
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(v: &[(f32, f32)]) -> Spans {
        v.iter().map(|&(lo, hi)| Span { lo, hi }).collect()
    }

    #[test]
    fn subtract_cases() {
        let mut s = spans(&[(0.0, 10.0)]);
        assert!(subtract(&mut s, 2.0, 3.0));
        assert_eq!(s, spans(&[(0.0, 2.0), (3.0, 10.0)]));
        assert!(subtract(&mut s, -1.0, 1.0));
        assert_eq!(s, spans(&[(1.0, 2.0), (3.0, 10.0)]));
        assert!(!subtract(&mut s, 2.0, 3.0));
        assert!(!subtract(&mut s, 11.0, 12.0));
        assert!(!subtract(&mut s, 5.0, 5.0));
        assert!(subtract(&mut s, 9.0, 20.0));
        assert_eq!(s, spans(&[(1.0, 2.0), (3.0, 9.0)]));
        assert!(subtract(&mut s, 0.0, 9.0));
        assert!(s.is_empty());
        assert!(!subtract(&mut s, 0.0, 9.0));
    }

    #[test]
    fn box_rays_and_volume() {
        let stock = Stock::Box {
            min: Point3::new(0.0, 0.0, 0.0),
            max: Point3::new(10.0, 6.0, 4.0),
        };
        let d = TriDexel::new(&stock, 0.5);
        assert_eq!((d.grids[2].nu, d.grids[2].nv), (20, 12));
        assert_eq!((d.grids[0].nu, d.grids[0].nv), (12, 8));
        assert_eq!((d.grids[1].nu, d.grids[1].nv), (8, 20));
        for v in d.volumes() {
            assert!((v - 240.0).abs() < 1e-9, "{v}");
        }
        let m = d.memory();
        assert_eq!(m.rays, 240 + 96 + 160);
        assert_eq!(m.spilled, 0);
    }

    #[test]
    fn cylinder_volume_within_resolution() {
        let stock = Stock::Cylinder {
            x0: 0.0,
            x1: 50.0,
            radius: 10.0,
        };
        let d = TriDexel::new(&stock, 0.1);
        let exact = std::f64::consts::PI * 100.0 * 50.0;
        for v in d.volumes() {
            assert!((v - exact).abs() / exact < 2e-3, "{v} vs {exact}");
        }
    }

    #[test]
    fn hash_changes_with_material() {
        let stock = Stock::Box {
            min: Point3::new(0.0, 0.0, 0.0),
            max: Point3::new(2.0, 2.0, 2.0),
        };
        let mut d = TriDexel::new(&stock, 0.5);
        let h = d.hash();
        assert_eq!(h, d.clone().hash());
        let before = d.clone();
        subtract(d.grids[2].ray_mut(0, 0), 1.0, 2.0);
        assert_ne!(h, d.hash());
        // Only the changed tile was copied; the clone still has the old one.
        assert_eq!(before.hash(), h);
        let g = &d.grids[2];
        assert_eq!(g.shared_tiles(&before.grids[2]), g.nv.div_ceil(ROWS) - 1);
    }
}
