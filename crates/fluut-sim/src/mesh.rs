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
//! The mesh is for display only; it is `f32` and need not be deterministic,
//! but it is.

use crate::dexel::{Spans, TriDexel};
use glam::DVec3;
use smallvec::SmallVec;

/// Triangles, three vertices each, with one flat normal per vertex.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StockMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
}

impl StockMesh {
    pub fn triangle_count(&self) -> usize {
        self.positions.len() / 3
    }

    /// Enclosed volume, mm³ (for tests).
    pub fn volume(&self) -> f64 {
        self.positions
            .chunks(3)
            .map(|t| {
                let p = |k: usize| DVec3::new(t[k][0].into(), t[k][1].into(), t[k][2].into());
                p(0).dot(p(1).cross(p(2))) / 6.0
            })
            .sum()
    }
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

/// Cell key: cell `c` lies between lattice points `c` and `c + 1`; `c ≥ −1`.
fn key(c: [i64; 3]) -> u64 {
    ((c[0] + 1) as u64) << 42 | ((c[1] + 1) as u64) << 21 | (c[2] + 1) as u64
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

/// The stock surface of `d`.
pub fn surface(d: &TriDexel) -> StockMesh {
    let res = d.res;
    let o = d.origin;
    let (gx, gy, gz) = (&d.grids[0], &d.grids[1], &d.grids[2]);
    let (nx, ny, nz) = (gz.nu as i64, gz.nv as i64, gx.nv as i64);
    let lattice = |n: i64| (n as f64 + 0.5) * res;
    let zr = |i: i64, j: i64| -> SmallVec<[(i64, i64); 2]> {
        if i < 0 || j < 0 || i >= nx || j >= ny {
            SmallVec::new()
        } else {
            ranges(gz.ray(i as usize, j as usize), res)
        }
    };
    let mut edges = Vec::new();
    // Along Z: the ends of each Z ray's lattice ranges.
    for j in 0..ny {
        for i in 0..nx {
            let spans = gz.ray(i as usize, j as usize);
            for (a, b) in zr(i, j) {
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
    }
    // Along X and Y: where neighbouring Z rays differ.
    let mut diff = Vec::new();
    for (axis, ni, nj) in [(0usize, nx, ny), (1, ny, nx)] {
        for j in 0..nj {
            for i in -1..ni {
                // Lattice points (i, j) and (i + 1, j) along `axis`.
                let at = |i: i64| if axis == 0 { (i, j) } else { (j, i) };
                let (pa, pb) = (at(i), at(i + 1));
                let (ra, rb) = (zr(pa.0, pa.1), zr(pb.0, pb.1));
                differ(&ra, &rb, &mut diff);
                for &(k0, k1) in &diff {
                    for k in k0..=k1 {
                        let inside_low = ra.iter().any(|&(x, y)| x <= k && k <= y);
                        // The ray on this edge, if the grid has it.
                        let spans = if axis == 0 {
                            (j < ny && k >= 0 && k < nz).then(|| gx.ray(j as usize, k as usize))
                        } else {
                            (j < nx && k >= 0 && k < nz).then(|| gy.ray(k as usize, j as usize))
                        };
                        let (lo, hi) = (lattice(i), lattice(i + 1));
                        let t = spans
                            .and_then(|s| crossing(s, lo, hi))
                            .unwrap_or(0.5 * (lo + hi));
                        let mut p = DVec3::new(lattice(pa.0), lattice(pa.1), lattice(k));
                        p[axis] = t;
                        let mut c = [pa.0, pa.1, k];
                        c[axis] = i;
                        edges.push(Edge {
                            at: c,
                            axis,
                            inside_low,
                            p: o + p,
                        });
                    }
                }
            }
        }
    }
    // One vertex per cell: the mean of its crossings.
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
    let vertex = |c: [i64; 3]| verts[keys.binary_search(&key(c)).expect("cell has a vertex")];
    let mut m = StockMesh::default();
    let f = |p: DVec3| [p.x as f32, p.y as f32, p.z as f32];
    for e in &edges {
        let mut q = cells(e).map(vertex);
        if !e.inside_low {
            q.reverse();
        }
        let nrm = (q[2] - q[0]).cross(q[3] - q[1]).normalize_or_zero();
        for t in [[0, 1, 2], [0, 2, 3]] {
            for &v in &t {
                m.positions.push(f(q[v]));
                m.normals.push(f(nrm));
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Chorder, Stock, Tool, cut};
    use fluut_geom::Point3;
    use fluut_path::{AxisPos, FeedRef, Motion, MotionInfo, Segment};

    fn block() -> Stock {
        Stock::Box {
            min: Point3::new(0.0, 0.0, 0.0),
            max: Point3::new(10.0, 6.0, 4.0),
        }
    }

    #[test]
    fn ranges_and_differences() {
        let s: Spans = [crate::Span { lo: 0.0, hi: 1.0 }].into_iter().collect();
        assert_eq!(ranges(&s, 0.25).as_slice(), &[(0, 3)]);
        let mut out = Vec::new();
        differ(&[(0, 9)], &[(0, 3), (6, 9)], &mut out);
        assert_eq!(out, [(4, 5)]);
        differ(&[], &[(2, 4)], &mut out);
        assert_eq!(out, [(2, 4)]);
        differ(&[(1, 2)], &[(1, 2)], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn block_is_closed_with_its_volume() {
        let d = TriDexel::new(&block(), 0.5);
        let m = surface(&d);
        // Each face has an n × m grid of quads plus the rounded edges.
        assert!(m.triangle_count() > 0);
        let v = m.volume();
        assert!((v - 240.0).abs() / 240.0 < 0.05, "{v}");
        // Vertices on the faces lie on the faces.
        for p in &m.positions {
            let inside = (0..3).all(|k| p[k] >= -1e-6 && p[k] <= [10.0, 6.0, 4.0][k] + 1e-6);
            assert!(inside, "{p:?}");
        }
    }

    #[test]
    fn slot_lowers_the_volume() {
        let s = block();
        let mut d = TriDexel::new(&s, 0.25);
        let before = surface(&d).volume();
        let seg = Segment::Line {
            a: AxisPos::xyz(Point3::new(-5.0, 3.0, 2.0)),
            b: AxisPos::xyz(Point3::new(15.0, 3.0, 2.0)),
            m: MotionInfo::new(Motion::Cut, FeedRef::Cut, 0.01),
        };
        let chords = Chorder::new(Tool::flat(1.0), s, false, 0.25).chords(&[seg]);
        cut(&mut d, &chords);
        let after = surface(&d).volume();
        // The slot is 2 wide, 2 deep and 10 long.
        assert!(((before - after) - 40.0).abs() < 2.0, "{before} → {after}");
    }
}
