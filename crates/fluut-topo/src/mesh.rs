// SPDX-License-Identifier: MIT OR Apache-2.0

//! Mesh bodies (spec 03 §3.2.5, D03 §1.1) and point welding (D05 §2.3,
//! without the union-find: see [`weld_points`]).
//!
//! Regions and the BVH of D03 §1.1 come with the first mesh strategy; the
//! adjacency they need is built here.

use fluut_geom::Point3;
use std::collections::BTreeMap;

/// Marks a mesh edge with no unique neighbour (open or non-manifold).
pub const NONE: u32 = u32::MAX;

/// An indexed triangle mesh: welded positions, triangles in counter-clockwise
/// order seen from outside, and for each triangle edge the triangle across it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MeshBody {
    pub name: String,
    pub tol_model: f64,
    pub positions: Vec<Point3>,
    pub tris: Vec<[u32; 3]>,
    /// `neighbours[t][k]` is the triangle across edge `(tris[t][k],
    /// tris[t][(k + 1) % 3])`, or [`NONE`] when that edge is open or used by
    /// more than two triangles.
    pub neighbours: Vec<[u32; 3]>,
}

/// Merges points within `tol` of each other. Returns, for each point, the
/// index of the point it was merged into, which is always an earlier point
/// that was itself kept (a representative).
///
/// Points are taken in input order. Each one merges into the first
/// representative within `tol`, found in the 27 grid cells (of size `2·tol`)
/// around it, or becomes a representative itself. Representatives are more
/// than `tol` apart, so a cell holds only a few of them and the run time is
/// close to linear even when many points coincide. Cells are kept in a
/// `BTreeMap`, so the result depends only on the input. Points that are not
/// finite, or too far out for the grid, are never merged.
pub fn weld_points(points: &[Point3], tol: f64) -> Vec<u32> {
    let mut rep: Vec<u32> = (0..points.len() as u32).collect();
    if !(tol.is_finite() && tol > 0.0) {
        return rep;
    }
    let cell = 2.0 * tol;
    let key = |p: Point3| -> Option<[i64; 3]> {
        let k = [p.x() / cell, p.y() / cell, p.z() / cell];
        k.iter()
            .all(|c| c.is_finite() && c.abs() < 1e15)
            .then(|| k.map(|c| c.floor() as i64))
    };
    let mut cells: BTreeMap<[i64; 3], Vec<u32>> = BTreeMap::new();
    for (i, &p) in points.iter().enumerate() {
        let Some(k) = key(p) else { continue };
        let mut best: Option<u32> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(list) = cells.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) else {
                        continue;
                    };
                    for &j in list {
                        if points[j as usize].distance(p) <= tol && best.is_none_or(|b| j < b) {
                            best = Some(j);
                        }
                    }
                }
            }
        }
        match best {
            Some(j) => rep[i] = j,
            None => cells.entry(k).or_default().push(i as u32),
        }
    }
    rep
}

fn find(parent: &mut [u32], mut i: u32) -> u32 {
    let mut root = i;
    while parent[root as usize] != root {
        root = parent[root as usize];
    }
    while parent[i as usize] != root {
        let next = parent[i as usize];
        parent[i as usize] = root;
        i = next;
    }
    root
}

/// Joins the groups of `a` and `b`; the smaller index becomes the root.
fn union(parent: &mut [u32], a: u32, b: u32) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        let (lo, hi) = (ra.min(rb), ra.max(rb));
        parent[hi as usize] = lo;
    }
}

/// Per-edge neighbours of `tris` (see [`MeshBody::neighbours`]), and counts of
/// the edges that are open, non-manifold, or shared by two triangles running
/// the same way (inconsistent orientation; such pairs get no neighbour).
pub fn adjacency(tris: &[[u32; 3]]) -> (Vec<[u32; 3]>, EdgeCounts) {
    // (low vertex, high vertex, triangle, edge index, forward).
    let mut edges: Vec<(u32, u32, u32, u8, bool)> = Vec::with_capacity(tris.len() * 3);
    for (t, tri) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (tri[k], tri[(k + 1) % 3]);
            edges.push((a.min(b), a.max(b), t as u32, k as u8, a < b));
        }
    }
    edges.sort_unstable();
    let mut nb = vec![[NONE; 3]; tris.len()];
    let mut counts = EdgeCounts::default();
    let mut i = 0;
    while i < edges.len() {
        let mut j = i + 1;
        while j < edges.len() && edges[j].0 == edges[i].0 && edges[j].1 == edges[i].1 {
            j += 1;
        }
        match j - i {
            1 => counts.open += 1,
            2 => {
                let (x, y) = (edges[i], edges[i + 1]);
                if x.4 == y.4 {
                    counts.same_direction += 1;
                } else {
                    nb[x.2 as usize][x.3 as usize] = y.2;
                    nb[y.2 as usize][y.3 as usize] = x.2;
                }
            }
            _ => counts.non_manifold += 1,
        }
        i = j;
    }
    (nb, counts)
}

/// Edge counts from [`adjacency`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EdgeCounts {
    pub open: u32,
    pub non_manifold: u32,
    pub same_direction: u32,
}

/// The open edges of `tris` (used by one triangle only), as `(from, to)` in
/// triangle order, sorted.
pub fn open_edges(tris: &[[u32; 3]]) -> Vec<(u32, u32)> {
    let mut edges: Vec<(u32, u32, u32, u32)> = Vec::with_capacity(tris.len() * 3);
    for tri in tris {
        for k in 0..3 {
            let (a, b) = (tri[k], tri[(k + 1) % 3]);
            edges.push((a.min(b), a.max(b), a, b));
        }
    }
    edges.sort_unstable();
    let mut out = Vec::new();
    let mut i = 0;
    while i < edges.len() {
        let mut j = i + 1;
        while j < edges.len() && edges[j].0 == edges[i].0 && edges[j].1 == edges[i].1 {
            j += 1;
        }
        if j - i == 1 {
            out.push((edges[i].2, edges[i].3));
        }
        i = j;
    }
    out.sort_unstable();
    out
}

/// Number of loops the open edges form: connected groups of open edges,
/// joined at shared vertices.
pub fn boundary_loops(open: &[(u32, u32)]) -> u32 {
    let mut verts: Vec<u32> = open.iter().flat_map(|&(a, b)| [a, b]).collect();
    verts.sort_unstable();
    verts.dedup();
    let idx = |v: u32| verts.binary_search(&v).unwrap_or(0) as u32;
    let mut parent: Vec<u32> = (0..verts.len() as u32).collect();
    for &(a, b) in open {
        union(&mut parent, idx(a), idx(b));
    }
    (0..verts.len() as u32)
        .filter(|&i| find(&mut parent, i) == i)
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weld_merges_to_the_first_point() {
        let tol = 1e-3;
        let pts = [
            Point3::new(1.0, 1.0, 1.0),
            Point3::new(5.0, 0.0, 0.0),
            Point3::new(1.0 + 0.4 * tol, 1.0, 1.0),
            Point3::new(1.0 + 3.0 * tol, 1.0, 1.0),
            Point3::new(5.0, 0.0, 0.4 * tol),
            Point3::new(f64::NAN, 0.0, 0.0),
        ];
        assert_eq!(weld_points(&pts, tol), vec![0, 1, 0, 3, 1, 5]);
    }

    #[test]
    fn weld_across_cell_borders() {
        // Neighbours on either side of a cell boundary (cell = 2·tol).
        let tol = 0.5;
        let pts = [Point3::new(0.999, 0.0, 0.0), Point3::new(1.001, 0.0, 0.0)];
        assert_eq!(weld_points(&pts, tol), vec![0, 0]);
        let far = [Point3::new(-1e300, 0.0, 0.0), Point3::new(1e300, 0.0, 0.0)];
        assert_eq!(weld_points(&far, tol), vec![0, 1]);
    }

    #[test]
    fn adjacency_of_a_tetrahedron_and_a_fin() {
        let tet = [[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]];
        let (nb, c) = adjacency(&tet);
        assert_eq!(c, EdgeCounts::default());
        assert!(nb.iter().flatten().all(|&t| t != NONE));
        assert_eq!(nb[0][0], 3); // edge 0-2 is shared with [2, 0, 3]
        // A third triangle on edge 0-1 makes it non-manifold and opens 2 edges.
        let mut fin = tet.to_vec();
        fin.push([1, 0, 4]);
        let (nb, c) = adjacency(&fin);
        assert_eq!(c.non_manifold, 1);
        assert_eq!(c.open, 2);
        assert_eq!(nb[0][2], NONE);
        let open = open_edges(&fin);
        assert_eq!(open, vec![(0, 4), (4, 1)]);
        assert_eq!(boundary_loops(&open), 1);
    }

    #[test]
    fn same_direction_pair_is_counted() {
        let (nb, c) = adjacency(&[[0, 1, 2], [0, 1, 3]]);
        assert_eq!(c.same_direction, 1);
        assert_eq!(nb[0][0], NONE);
    }
}
