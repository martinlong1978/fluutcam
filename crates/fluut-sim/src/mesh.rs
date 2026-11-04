// SPDX-License-Identifier: MIT OR Apache-2.0

//! Display mesh of the stock (D11 §4): dual contouring on the tri-dexel.
//!
//! The rays of all three families pass through the points of one lattice,
//! `origin + (i + ½, j + ½, k + ½)·res`. Inside and outside at a lattice
//! point come from the Z family alone, so every sign change is consistent and
//! the surface is closed. Where the sign changes along a lattice edge, the
//! ray on that edge gives the exact crossing: the Z rays for edges along Z,
//! the X and Y rays for the others. Each cell with crossings gets one vertex,
//! the mean of its crossings (no normals are stored, so this is the mass
//! point of dual contouring rather than its QEF), and every crossing edge a
//! quad joining the four cells around it.
//!
//! The mesh is built in square blocks of [`BLOCK`] lattice columns
//! ([`MeshTiles`]). A block owns the crossing edges whose lower lattice
//! point lies in it, and reads the rays up to [`MARGIN`] lattice points
//! around it: enough for the vertices of its quads and for their normals,
//! which average the quads around each vertex. A block therefore depends on
//! no other block, and after a cut only the blocks whose margin holds a
//! changed ray are rebuilt (D11 §4). Each block is an indexed mesh; a
//! vertex on a seam is repeated in both blocks, at the same position.
//!
//! The mesh is for display only; it is `f32`, but deterministic: a block is
//! the same however it was reached and on any thread count.

use crate::dexel::{ROWS, Spans, TriDexel};
use glam::DVec3;
use rayon::prelude::*;
use smallvec::SmallVec;
use std::sync::Arc;

/// Lattice columns along each side of a block.
pub const BLOCK: usize = 16;
/// Lattice points around a block whose edges it reads.
pub const MARGIN: i64 = 2;

/// An indexed triangle mesh with a normal and a tag per vertex.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StockMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Rest class and toolpath, see [`tag`].
    pub tags: Vec<u32>,
    /// Three per triangle, counter-clockwise seen from outside.
    pub indices: Vec<u32>,
}

impl StockMesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Enclosed volume, mm³ (for tests).
    pub fn volume(&self) -> f64 {
        let p = |k: u32| {
            let v = self.positions[k as usize];
            DVec3::new(v[0].into(), v[1].into(), v[2].into())
        };
        self.indices
            .chunks(3)
            .map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0)
            .sum()
    }

    /// Appends `other`, its indices moved past this mesh's vertices.
    pub fn append(&mut self, other: &StockMesh) {
        let n = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.tags.extend_from_slice(&other.tags);
        self.indices.extend(other.indices.iter().map(|i| i + n));
    }
}

/// Rest classes of the stock against the part (D11 §5): no part below.
pub const OFF: u8 = 0;
/// Above the part by more than the tolerance.
pub const REST: u8 = 1;
/// Within the tolerance of the part.
pub const WITHIN: u8 = 2;
/// Below the part by more than the gouge threshold.
pub const GOUGE: u8 = 3;

/// What paints one Z ray's column: its rest class and the toolpath that
/// last cut it.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Column {
    pub class: u8,
    pub toolpath: Option<u16>,
}

/// The paint of the stock's columns. A column's paint may depend only on
/// its ray and on how the ray came to be, never on other rays, so that a
/// block whose rays did not change keeps its paint.
pub trait Paint: Sync {
    fn column(&self, i: usize, j: usize) -> Column;

    /// The class of the vertex at `p` (stock frame) whose columns gave
    /// `class`. May depend only on `p` and `class`, so that a block whose
    /// rays did not change keeps its paint.
    fn vertex(&self, _p: DVec3, class: u8) -> u8 {
        class
    }
}

/// Plain stock.
#[derive(Copy, Clone, Debug, Default)]
pub struct NoPaint;

impl Paint for NoPaint {
    fn column(&self, _: usize, _: usize) -> Column {
        Column::default()
    }
}

/// A vertex tag: the class in bits 16–17, the toolpath + 1 in bits 0–15
/// (0 for none).
pub fn tag(class: u8, toolpath: Option<u16>) -> u32 {
    u32::from(class & 3) << 16 | toolpath.map_or(0, |t| u32::from(t.min(0xFFFE)) + 1)
}

pub fn tag_class(tag: u32) -> u8 {
    (tag >> 16 & 3) as u8
}

pub fn tag_toolpath(tag: u32) -> Option<u16> {
    (tag & 0xFFFF).checked_sub(1).map(|t| t as u16)
}

/// The paint of a vertex from the four columns around it: a gouge only
/// where every column over the part is gouged (so a wall beside the part
/// is not one), else rest where any is, else within; the latest toolpath.
pub fn combine(cols: &[Column; 4]) -> (u8, Option<u16>) {
    let toolpath = cols.iter().map(|c| c.toolpath).max().flatten();
    let on = cols.iter().filter(|c| c.class != OFF);
    let class = if on.clone().count() == 0 {
        OFF
    } else if on.clone().all(|c| c.class == GOUGE) {
        GOUGE
    } else if on.clone().any(|c| c.class == REST) {
        REST
    } else {
        WITHIN
    };
    (class, toolpath)
}

/// Lattice indices `k` inside each span of a ray, inclusive.
fn ranges(spans: &Spans, res: f64) -> SmallVec<[(i64, i64); 2]> {
    spans
        .iter()
        .filter_map(|s| {
            let a = (f64::from(s.lo) / res - 0.5).ceil() as i64;
            let b = (f64::from(s.hi) / res - 0.5).floor() as i64;
            (a <= b).then_some((a, b))
        })
        .collect()
}

/// The `k` where exactly one of two range lists holds, as ranges.
fn differ(a: &[(i64, i64)], b: &[(i64, i64)], out: &mut Vec<(i64, i64)>) {
    out.clear();
    let mut ev: SmallVec<[(i64, u8); 8]> = SmallVec::new();
    for &(x, y) in a {
        ev.extend([(x, 0), (y + 1, 0)]);
    }
    for &(x, y) in b {
        ev.extend([(x, 1), (y + 1, 1)]);
    }
    ev.sort_unstable();
    let mut inside = [false; 2];
    for w in 0..ev.len() {
        let (k, which) = ev[w];
        inside[which as usize] = !inside[which as usize];
        if let Some(&(next, _)) = ev.get(w + 1)
            && next > k
            && inside[0] != inside[1]
        {
            out.push((k, next - 1));
        }
    }
}

/// The span end of `spans` in `[lo, hi]` nearest its middle.
fn crossing(spans: &Spans, lo: f64, hi: f64) -> Option<f64> {
    let mid = 0.5 * (lo + hi);
    spans
        .iter()
        .flat_map(|s| [f64::from(s.lo), f64::from(s.hi)])
        .filter(|&e| e >= lo && e <= hi)
        .min_by(|a, b| (a - mid).abs().total_cmp(&(b - mid).abs()))
}

/// A crossing edge: the lattice point at its lower end, its axis, whether
/// the inside is at the lower end, and the crossing point.
struct Edge {
    at: [i64; 3],
    axis: usize,
    inside_low: bool,
    p: DVec3,
}

/// The four cells around an edge, counter-clockwise seen from the edge's
/// positive direction.
fn cells(e: &Edge) -> [[i64; 3]; 4] {
    let (a, b, c) = (e.axis, (e.axis + 1) % 3, (e.axis + 2) % 3);
    [(-1, -1), (0, -1), (0, 0), (-1, 0)].map(|(db, dc)| {
        let mut x = e.at;
        x[a] = e.at[a];
        x[b] += db;
        x[c] += dc;
        x
    })
}

/// The stock surface of `d`: every block, joined.
pub fn surface(d: &TriDexel) -> StockMesh {
    MeshTiles::new(d, &NoPaint).joined()
}

/// Lattice points along one axis of block `b`: the lower ends of the edges
/// it owns, `[lo, hi)`. Lattice points run from −1 (outside the first ray)
/// to `n − 1`.
fn owned(b: usize) -> (i64, i64) {
    let lo = (b * BLOCK) as i64 - 1;
    (lo, lo + BLOCK as i64)
}

/// The blocks along one axis, of `n`, that read a lattice point in
/// `a0..=a1`.
fn reading(a0: i64, a1: i64, n: usize) -> std::ops::Range<usize> {
    let b = BLOCK as i64;
    // Block `k` reads `k·b − 1 − MARGIN ..= k·b + b − 2 + MARGIN`.
    let lo = -(-(a0 - b + 2 - MARGIN)).div_euclid(b);
    let hi = (a1 + 1 + MARGIN).div_euclid(b) + 1;
    let (lo, hi) = (lo.max(0), hi.min(n as i64));
    lo as usize..(hi.max(lo) as usize)
}

/// Where two rays differ: the span ends of one that the other lacks lie in
/// the returned range, and so does every point covered by only one.
fn changed(a: &Spans, b: &Spans) -> Option<(f32, f32)> {
    if a == b {
        return None;
    }
    let ends = |s: &Spans| -> SmallVec<[f32; 4]> { s.iter().flat_map(|x| [x.lo, x.hi]).collect() };
    let (ea, eb) = (ends(a), ends(b));
    let p = ea.iter().zip(&eb).take_while(|(x, y)| x == y).count();
    let q = ea
        .iter()
        .rev()
        .zip(eb.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
        .min(ea.len() - p)
        .min(eb.len() - p);
    let mid = ea[p..ea.len() - q].iter().chain(&eb[p..eb.len() - q]);
    let lo = mid.clone().copied().fold(f32::INFINITY, f32::min);
    let hi = mid.copied().fold(f32::NEG_INFINITY, f32::max);
    Some((lo, hi))
}

/// The stock mesh in blocks, kept up to date with a changing stock.
#[derive(Clone, Debug)]
pub struct MeshTiles {
    /// Blocks along X and Y.
    pub nbx: usize,
    pub nby: usize,
    /// Row by row along X.
    blocks: Vec<Arc<StockMesh>>,
    /// The stock the blocks show.
    stock: TriDexel,
}

/// Whether two stocks have the same rays (and so the same blocks).
fn same_layout(a: &TriDexel, b: &TriDexel) -> bool {
    a.res == b.res
        && a.origin == b.origin
        && a.grids
            .iter()
            .zip(&b.grids)
            .all(|(x, y)| x.nu == y.nu && x.nv == y.nv)
}

impl MeshTiles {
    /// Every block of `d`, built in parallel.
    pub fn new(d: &TriDexel, paint: &dyn Paint) -> MeshTiles {
        let nbx = (d.grids[2].nu + 1).div_ceil(BLOCK);
        let nby = (d.grids[2].nv + 1).div_ceil(BLOCK);
        let blocks = (0..nbx * nby)
            .into_par_iter()
            .map(|b| Arc::new(build(d, paint, b % nbx, b / nbx)))
            .collect();
        MeshTiles {
            nbx,
            nby,
            blocks,
            stock: d.clone(),
        }
    }

    pub fn blocks(&self) -> &[Arc<StockMesh>] {
        &self.blocks
    }

    /// The stock the blocks show.
    pub fn stock(&self) -> &TriDexel {
        &self.stock
    }

    /// Every block in one mesh.
    pub fn joined(&self) -> StockMesh {
        let mut m = StockMesh::default();
        for b in &self.blocks {
            m.append(b);
        }
        m
    }

    /// Shows `d`: rebuilds, in parallel, the blocks that read a ray that
    /// differs from the stock shown so far. Returns their indices, in
    /// order. `paint` must paint `d` as the kept blocks were painted.
    pub fn update(&mut self, d: &TriDexel, paint: &dyn Paint) -> Vec<usize> {
        if !same_layout(d, &self.stock) {
            *self = MeshTiles::new(d, paint);
            return (0..self.blocks.len()).collect();
        }
        let dirty = self.dirty(d);
        let built: Vec<StockMesh> = dirty
            .par_iter()
            .map(|&b| build(d, paint, b % self.nbx, b / self.nbx))
            .collect();
        for (&b, m) in dirty.iter().zip(built) {
            self.blocks[b] = Arc::new(m);
        }
        self.stock = d.clone();
        dirty
    }

    /// The blocks that read a ray of `d` that differs from the stock shown.
    fn dirty(&self, d: &TriDexel) -> Vec<usize> {
        let mut mark = vec![false; self.blocks.len()];
        let res = d.res;
        let (nbx, nby) = (self.nbx, self.nby);
        let mut rect = |x0: i64, x1: i64, y0: i64, y1: i64| {
            for by in reading(y0, y1, nby) {
                for bx in reading(x0, x1, nbx) {
                    mark[by * nbx + bx] = true;
                }
            }
        };
        // The lattice points `a` whose edge `[a, a + 1]` along a ray meets
        // the changed range `[lo, hi]`.
        let along = |lo: f32, hi: f32| {
            let a0 = (f64::from(lo) / res - 1.5).floor() as i64;
            let a1 = (f64::from(hi) / res - 0.5).ceil() as i64;
            (a0, a1)
        };
        for (k, (old, new)) in self.stock.grids.iter().zip(&d.grids).enumerate() {
            let nu = new.nu;
            for (t, (ta, tb)) in old.tiles().iter().zip(new.tiles()).enumerate() {
                if Arc::ptr_eq(ta, tb) {
                    continue;
                }
                for (idx, (ra, rb)) in ta.iter().zip(tb.iter()).enumerate() {
                    let Some((lo, hi)) = changed(ra, rb) else {
                        continue;
                    };
                    let (i, j) = ((idx % nu) as i64, (t * ROWS + idx / nu) as i64);
                    match k {
                        // Ray (y, z): X edges at (a, y).
                        0 => {
                            let (a0, a1) = along(lo, hi);
                            rect(a0, a1, i, i);
                        }
                        // Ray (z, x): Y edges at (x, a).
                        1 => {
                            let (a0, a1) = along(lo, hi);
                            rect(j, j, a0, a1);
                        }
                        // Column (x, y): its Z edges and the X and Y edges
                        // on both sides.
                        _ => rect(i - 1, i, j - 1, j),
                    }
                }
            }
        }
        (0..mark.len()).filter(|&b| mark[b]).collect()
    }
}

/// Block `(bx, by)` of `d`.
fn build(d: &TriDexel, paint: &dyn Paint, bx: usize, by: usize) -> StockMesh {
    let res = d.res;
    let o = d.origin;
    let (gx, gy, gz) = (&d.grids[0], &d.grids[1], &d.grids[2]);
    let (nx, ny, nz) = (gz.nu as i64, gz.nv as i64, gx.nv as i64);
    let lattice = |n: i64| (n as f64 + 0.5) * res;
    let ((x0, x1), (y0, y1)) = (owned(bx), owned(by));
    // Lattice points whose edges are read, inclusive.
    let (ex0, ex1) = (x0 - MARGIN, x1 - 1 + MARGIN);
    let (ey0, ey1) = (y0 - MARGIN, y1 - 1 + MARGIN);
    // Z ranges of the columns `ex0..=ex1 + 1` by `ey0..=ey1 + 1`.
    let w = (ex1 - ex0 + 2) as usize;
    let mut zrs: Vec<SmallVec<[(i64, i64); 2]>> = Vec::with_capacity(w * w);
    for j in ey0..=ey1 + 1 {
        for i in ex0..=ex1 + 1 {
            zrs.push(if i < 0 || j < 0 || i >= nx || j >= ny {
                SmallVec::new()
            } else {
                ranges(gz.ray(i as usize, j as usize), res)
            });
        }
    }
    let zr = |i: i64, j: i64| &zrs[(j - ey0) as usize * w + (i - ex0) as usize];
    let mut edges = Vec::new();
    let mut diff = Vec::new();
    for j in ey0.max(-1)..=ey1.min(ny - 1) {
        for i in ex0.max(-1)..=ex1.min(nx - 1) {
            // Along Z: the ends of the column's lattice ranges.
            if i >= 0 && j >= 0 {
                let spans = gz.ray(i as usize, j as usize);
                for &(a, b) in zr(i, j) {
                    for (k, low, lo, hi) in [(a - 1, false, a - 1, a), (b, true, b, b + 1)] {
                        let z = crossing(spans, lattice(lo), lattice(hi))
                            .unwrap_or(0.5 * (lattice(lo) + lattice(hi)));
                        edges.push(Edge {
                            at: [i, j, k],
                            axis: 2,
                            inside_low: low,
                            p: o + DVec3::new(lattice(i), lattice(j), z),
                        });
                    }
                }
            }
            // Along X and Y: where the next column differs. An X edge needs
            // a row of rays (`j ≥ 0`), a Y edge a column (`i ≥ 0`).
            for axis in [0usize, 1] {
                let (next, has) = if axis == 0 {
                    ((i + 1, j), j >= 0)
                } else {
                    ((i, j + 1), i >= 0)
                };
                if !has {
                    continue;
                }
                let ra = zr(i, j);
                differ(ra, zr(next.0, next.1), &mut diff);
                for &(k0, k1) in &diff {
                    for k in k0..=k1 {
                        let inside_low = ra.iter().any(|&(x, y)| x <= k && k <= y);
                        // The ray on this edge, if the grid has it.
                        let on_grid = k >= 0 && k < nz;
                        let (spans, lo, hi) = if axis == 0 {
                            let s = on_grid.then(|| gx.ray(j as usize, k as usize));
                            (s, lattice(i), lattice(i + 1))
                        } else {
                            let s = on_grid.then(|| gy.ray(k as usize, i as usize));
                            (s, lattice(j), lattice(j + 1))
                        };
                        let t = spans
                            .and_then(|s| crossing(s, lo, hi))
                            .unwrap_or(0.5 * (lo + hi));
                        let mut p = DVec3::new(lattice(i), lattice(j), lattice(k));
                        p[axis] = t;
                        edges.push(Edge {
                            at: [i, j, k],
                            axis,
                            inside_low,
                            p: o + p,
                        });
                    }
                }
            }
        }
    }
    // One vertex per cell: the mean of its crossings, summed in edge order.
    let key = |c: [i64; 3]| -> u64 {
        ((c[1] - ey0 + 2) as u64) << 42 | ((c[0] - ex0 + 2) as u64) << 21 | (c[2] + 1) as u64
    };
    let mut contrib: Vec<(u64, DVec3)> = Vec::with_capacity(edges.len() * 4);
    for e in &edges {
        for c in cells(e) {
            contrib.push((key(c), e.p));
        }
    }
    contrib.sort_by_key(|x| x.0);
    let mut keys: Vec<u64> = Vec::new();
    let mut verts: Vec<DVec3> = Vec::new();
    let mut n = 0.0;
    for (k, p) in contrib {
        if keys.last() == Some(&k) {
            *verts.last_mut().expect("a vertex") += p;
            n += 1.0;
        } else {
            if let Some(v) = verts.last_mut() {
                *v /= n;
            }
            keys.push(k);
            verts.push(p);
            n = 1.0;
        }
    }
    if let Some(v) = verts.last_mut() {
        *v /= n;
    }
    let vertex = |c: [i64; 3]| keys.binary_search(&key(c)).expect("cell has a vertex");
    // Edges `m` or more lattice points inside the read range.
    let inner = |e: &Edge, m: i64| {
        e.at[0] >= ex0 + m && e.at[0] <= ex1 - m && e.at[1] >= ey0 + m && e.at[1] <= ey1 - m
    };
    // The quad of an edge, counter-clockwise seen from outside.
    let quad = |e: &Edge| {
        let mut q = cells(e).map(vertex);
        if !e.inside_low {
            q.reverse();
        }
        q
    };
    // Vertex normals, area-weighted over the quads one point in: their
    // cells have all their crossings.
    let mut normals = vec![DVec3::ZERO; verts.len()];
    for e in edges.iter().filter(|e| inner(e, 1)) {
        let q = quad(e);
        let nrm = (verts[q[2]] - verts[q[0]]).cross(verts[q[3]] - verts[q[1]]);
        for v in q {
            normals[v] += nrm;
        }
    }
    // Paint of the columns `x0 − 1..=x1` by `y0 − 1..=y1`, around the cells
    // of the owned quads.
    let pw = (x1 - x0 + 2) as usize;
    let mut cols = Vec::with_capacity(pw * pw);
    for j in y0 - 1..=y1 {
        for i in x0 - 1..=x1 {
            let (ci, cj) = (i.clamp(0, nx - 1) as usize, j.clamp(0, ny - 1) as usize);
            cols.push(paint.column(ci, cj));
        }
    }
    let col = |i: i64, j: i64| cols[(j - y0 + 1) as usize * pw + (i - x0 + 1) as usize];
    let mut m = StockMesh::default();
    let mut local = vec![u32::MAX; verts.len()];
    let f = |p: DVec3| [p.x as f32, p.y as f32, p.z as f32];
    for e in edges.iter().filter(|e| inner(e, MARGIN)) {
        let q = quad(e);
        for &v in &q {
            if local[v] == u32::MAX {
                local[v] = m.positions.len() as u32;
                m.positions.push(f(verts[v]));
                m.normals.push(f(normals[v].normalize_or_zero()));
                let c = keys[v];
                let cx = (c >> 21 & 0x1F_FFFF) as i64 + ex0 - 2;
                let cy = (c >> 42) as i64 + ey0 - 2;
                let (class, tp) = combine(&[
                    col(cx, cy),
                    col(cx + 1, cy),
                    col(cx, cy + 1),
                    col(cx + 1, cy + 1),
                ]);
                m.tags.push(tag(paint.vertex(verts[v], class), tp));
            }
        }
        let q = q.map(|v| local[v]);
        m.indices.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
    }
    m
}

#[cfg(test)]
mod tests;
