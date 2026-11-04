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

/// Bytes of one tile: its rays and their spilled spans.
fn tile_bytes(t: &Tile) -> usize {
    t.len() * size_of::<Spans>()
        + t.iter()
            .filter(|r| r.spilled())
            .map(|r| r.capacity() * size_of::<Span>())
            .sum::<usize>()
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

    pub(crate) fn tiles(&self) -> &[Arc<Tile>] {
        &self.tiles
    }

    pub(crate) fn tiles_mut(&mut self) -> &mut [Arc<Tile>] {
        &mut self.tiles
    }

    /// Bytes of the tiles not in `seen`, which then holds them.
    pub(crate) fn unique_bytes(&self, seen: &mut std::collections::BTreeSet<usize>) -> usize {
        let mut n = 0;
        for t in &self.tiles {
            if seen.insert(Arc::as_ptr(t) as usize) {
                n += tile_bytes(t);
            }
        }
        n
    }

    /// Bytes of the tiles that are not the same allocation as the tile at
    /// the same place in `prev`, plus the list of tile pointers.
    pub(crate) fn bytes_since(&self, prev: Option<&DexelGrid>) -> usize {
        let mut n = self.tiles.len() * size_of::<Arc<Tile>>();
        for (k, t) in self.tiles.iter().enumerate() {
            if !prev.is_some_and(|p| p.tiles.get(k).is_some_and(|q| Arc::ptr_eq(q, t))) {
                // Spilled spans estimated at 20 % (S5), not counted: this
                // is called per snapshot and must not touch every ray.
                n += t.len() * size_of::<Spans>() * 6 / 5;
            }
        }
        n
    }

    /// A family from its rays, row by row.
    fn from_rays(axis: usize, nu: usize, nv: usize, rays: Vec<Spans>) -> DexelGrid {
        let mut tiles = Vec::with_capacity(nv.div_ceil(ROWS));
        let mut it = rays.into_iter();
        for j0 in (0..nv).step_by(ROWS) {
            let rows = (j0 + ROWS).min(nv) - j0;
            tiles.push(Arc::new(it.by_ref().take(rows * nu).collect()));
        }
        DexelGrid {
            axis,
            nu,
            nv,
            tiles,
        }
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

    /// The stock turned a quarter turn about X, `(x, y, z)` to
    /// `(x, −z, y)` (a child Setup indexed 90° on, D11 §2). The rays map
    /// onto rays: the new grid's origin is the image of the far corner of
    /// this grid's last cells, so nothing is resampled. Spans along the
    /// reversed axis are mirrored in that grid's extent.
    pub fn turned_quarter(&self) -> TriDexel {
        let [gx, gy, gz] = &self.grids;
        let (nx, ny, nz) = (gy.nv, gx.nu, gx.nv);
        let ez = (nz as f64 * self.res) as f32;
        let mirrored = |r: &Spans| -> Spans {
            r.iter()
                .rev()
                .map(|s| Span {
                    lo: ez - s.hi,
                    hi: ez - s.lo,
                })
                .collect()
        };
        // New X rays at (y', z') = (k_y' = nz − 1 − k_z, k_z' = k_y).
        let mut x = Vec::with_capacity(nz * ny);
        for j in 0..ny {
            for i in 0..nz {
                x.push(gx.ray(j, nz - 1 - i).clone());
            }
        }
        // New Y rays (along −z) at (z', x) = (k_y, k_x): old Z rays.
        let mut y = Vec::with_capacity(ny * nx);
        for j in 0..nx {
            for i in 0..ny {
                y.push(mirrored(gz.ray(j, i)));
            }
        }
        // New Z rays (along y) at (x, y') = (k_x, nz − 1 − k_z): old Y rays.
        let mut z = Vec::with_capacity(nx * nz);
        for j in 0..nz {
            for i in 0..nx {
                z.push(gy.ray(nz - 1 - j, i).clone());
            }
        }
        let o = self.origin;
        TriDexel {
            res: self.res,
            origin: DVec3::new(o.x, -(o.z + nz as f64 * self.res), o.y),
            size: DVec3::new(self.size.x, self.size.z, self.size.y),
            grids: [
                DexelGrid::from_rays(0, nz, ny, x),
                DexelGrid::from_rays(1, ny, nx, y),
                DexelGrid::from_rays(2, nx, nz, z),
            ],
        }
    }

    pub fn origin_point(&self) -> Point3 {
        Point3(self.origin)
    }

    /// Bytes this stock holds that `prev` does not (tiles are compared by
    /// allocation), counting the tile lists: what keeping it next to `prev`
    /// costs.
    pub fn bytes_since(&self, prev: Option<&TriDexel>) -> usize {
        (0..3)
            .map(|k| self.grids[k].bytes_since(prev.map(|p| &p.grids[k])))
            .sum()
    }

    /// Bytes of the tiles not in `seen`, which then holds them.
    pub fn unique_bytes(&self, seen: &mut std::collections::BTreeSet<usize>) -> usize {
        self.grids.iter().map(|g| g.unique_bytes(seen)).sum()
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

    #[test]
    fn a_quarter_turn_maps_rays_onto_rays() {
        // A box with a notch, turned: the same as the turned box built
        // directly, ray for ray (the sizes are whole multiples of the
        // spacing, so both grids start at the box corner).
        let b = |min: [f64; 3], max: [f64; 3]| Stock::Box {
            min: Point3::new(min[0], min[1], min[2]),
            max: Point3::new(max[0], max[1], max[2]),
        };
        let d = TriDexel::new(&b([0.0, 0.0, 0.0], [10.0, 20.0, 30.0]), 1.0);
        let t = d.turned_quarter();
        let want = TriDexel::new(&b([0.0, -30.0, 0.0], [10.0, 0.0, 20.0]), 1.0);
        assert_eq!(t.origin, want.origin);
        assert_eq!(t.hash(), want.hash());
        // A notch at y 0…1, z 29…30 (a Z ray and an X ray shortened) ends
        // up at y′ −30…−29, z′ 0…1.
        let mut d = d;
        for i in 0..10 {
            subtract(d.grids[2].ray_mut(i, 0), 29.0, 30.0);
        }
        subtract(d.grids[0].ray_mut(0, 29), 0.0, 10.0);
        let t = d.turned_quarter();
        assert!(t.grids[0].ray(0, 0).is_empty());
        assert_eq!(t.grids[1].ray(0, 3)[0], Span { lo: 1.0, hi: 30.0 });
        // Four quarter turns give the stock back.
        let back = t.turned_quarter().turned_quarter().turned_quarter();
        assert_eq!(back.origin, d.origin);
        assert_eq!(back.hash(), d.hash());
        let [a, b2, c] = d.volumes();
        let [ta, tb, tc] = t.volumes();
        assert_eq!((ta, tb, tc), (a, c, b2));
    }
}
