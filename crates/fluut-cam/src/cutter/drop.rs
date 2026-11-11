// SPDX-License-Identifier: MIT OR Apache-2.0

//! The drop query (D07 §2–§3): highest tool position along −Z, by
//! best-first branch and bound over the primitive BVH.

use super::geom::{Prim, QueryGeometry, SUB_GRID, SubBox};
use super::profile::Cutter;
use super::solve::{self, Axis, Contact, Seeding, SolveStats};
use fluut_geom::{Aabb3, UvBox};

/// Options of a drop query.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct DropOptions {
    pub seeding: Seeding,
    /// Stop when no bound beats the best contact by more than this (mm).
    pub slack: f64,
    /// Ignore contacts below this tip height.
    pub floor: f64,
    /// Stop at the first contact at or above this tip height: the result
    /// is then some contact that high, not the highest.
    pub ceiling: f64,
    /// Search the boundaries of faces whose contact fell outside their trim
    /// (see [`drop`]). Off, a model gap between edges can be missed; the
    /// query is then much cheaper over holes.
    pub rims: bool,
}

impl Default for DropOptions {
    fn default() -> Self {
        DropOptions {
            seeding: Seeding::default(),
            slack: 1e-7,
            floor: f64::NEG_INFINITY,
            ceiling: f64::INFINITY,
            rims: true,
        }
    }
}

/// Upper bound of `p.z − h(d(p))` over a box; −∞ outside the footprint.
pub fn bound(b: &Aabb3, ax: Axis, c: &Cutter) -> f64 {
    let dx = (b.min.x() - ax.x).max(ax.x - b.max.x()).max(0.0);
    let dy = (b.min.y() - ax.y).max(ax.y - b.max.y()).max(0.0);
    let d = libm::hypot(dx, dy);
    if d > c.radius() {
        f64::NEG_INFINITY
    } else {
        b.max.z() - c.h(d)
    }
}

/// [`bound`] over the sub-boxes of a B-spline patch or edge: the highest
/// bound of any of them, which is at most the bound of the whole box.
pub fn sub_bound(subs: &[SubBox], ax: Axis, c: &Cutter) -> f64 {
    // A cell's bound is at most its top (`h ≥ 0` in the footprint), so a
    // cell whose top does not beat the best so far cannot change the
    // maximum and is skipped.
    let mut best = f64::NEG_INFINITY;
    for b in subs {
        if f64::from(b.max_z) > best {
            best = best.max(cell_bound(b, ax, c));
        }
    }
    best
}

fn cell_bound(b: &SubBox, ax: Axis, c: &Cutter) -> f64 {
    let dx = (f64::from(b.min_x) - ax.x)
        .max(ax.x - f64::from(b.max_x))
        .max(0.0);
    let dy = (f64::from(b.min_y) - ax.y)
        .max(ax.y - f64::from(b.max_y))
        .max(0.0);
    // `hypot` is at least the larger side: such a cell is out of reach.
    if dx.max(dy) > c.radius() {
        return f64::NEG_INFINITY;
    }
    let d = libm::hypot(dx, dy);
    if d <= c.radius() {
        f64::from(b.max_z) - c.h(d)
    } else {
        f64::NEG_INFINITY
    }
}

/// [`sub_bound`] over only the cells of patch `uv` that lie along the
/// boundary of its face `face_uv` (−∞ for a patch inside the face).
fn face_edge_bound(subs: &[SubBox], uv: UvBox, face_uv: UvBox, ax: Axis, c: &Cutter) -> f64 {
    let n = SUB_GRID;
    let on = |a: f64, b: f64| (a - b).abs() <= 1e-9 * (1.0 + b.abs());
    let (u0, u1) = (on(uv.u.lo, face_uv.u.lo), on(uv.u.hi, face_uv.u.hi));
    let (v0, v1) = (on(uv.v.lo, face_uv.v.lo), on(uv.v.hi, face_uv.v.hi));
    let mut best = f64::NEG_INFINITY;
    for (k, b) in subs.iter().enumerate() {
        let (i, j) = (k / n, k % n);
        if (u0 && i == 0) || (u1 && i == n - 1) || (v0 && j == 0) || (v1 && j == n - 1) {
            best = best.max(cell_bound(b, ax, c));
        }
    }
    best
}

/// Newton starts of a patch solved again (see [`drop`]).
pub const RESCUE_STARTS: usize = 8;

/// The parameters of the centres of the (at most [`RESCUE_STARTS`]) cells
/// of patch `uv` with the highest bounds above `floor`, highest first.
fn rescue_starts(subs: &[SubBox], uv: UvBox, ax: Axis, c: &Cutter, floor: f64) -> Vec<(f64, f64)> {
    let n = SUB_GRID;
    let mut cells: Vec<(f64, usize)> = subs
        .iter()
        .enumerate()
        .map(|(k, b)| (cell_bound(b, ax, c), k))
        .filter(|&(cb, _)| cb > floor)
        .collect();
    // Ties in cell order, so the starts do not depend on the sort.
    cells.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    cells
        .into_iter()
        .take(RESCUE_STARTS)
        .map(|(_, k)| {
            let (i, j) = (k / n, k % n);
            (
                uv.u.at((i as f64 + 0.5) / n as f64),
                uv.v.at((j as f64 + 0.5) / n as f64),
            )
        })
        .collect()
}

/// Highest contact on primitive `i`.
pub fn solve_prim(
    g: &QueryGeometry,
    i: u32,
    ax: Axis,
    c: &Cutter,
    seeding: &Seeding,
    stats: &mut SolveStats,
) -> Option<Contact> {
    match g.prims[i as usize] {
        Prim::Vertex(v) => {
            stats.vertices += 1;
            solve::vertex(g, v, ax, c)
        }
        Prim::Edge(e) => {
            stats.edges += 1;
            if seeding.split_edges {
                solve::edge_split(g, e, ax, c)
            } else {
                solve::edge(g, e, ax, c)
            }
        }
        Prim::Face { face, uv } => solve::face(g, face, uv, g.prim_box(i), ax, c, seeding, stats),
    }
}

/// The highest tool position with its axis through `(x, y)`, or `None` if
/// nothing above `opts.floor` is under the tool.
pub fn drop(
    g: &QueryGeometry,
    c: &Cutter,
    x: f64,
    y: f64,
    opts: &DropOptions,
    stats: &mut SolveStats,
) -> Option<Contact> {
    let ax = Axis { x, y };
    let mut best: Option<Contact> = None;
    // Faces whose highest stationary point is outside their trim, with its τ.
    let mut trimmed: Vec<(u32, f64)> = Vec::new();
    // B-spline patches solved, with the τ each gave (−∞ for none).
    let mut patches: Vec<(u32, f64)> = Vec::new();
    g.bvh.best_first_refined(
        opts.floor,
        |b| bound(b, ax, c) - opts.slack,
        // A B-spline patch or edge goes back into the queue under the
        // bound of its sub-boxes, so that the primitives are solved in the
        // order of the tighter bounds and those that cannot beat the best
        // contact are never solved.
        |i, bound| {
            let subs = g.sub_boxes(i);
            if subs.is_empty() {
                bound
            } else {
                bound.min(sub_bound(subs, ax, c) - opts.slack)
            }
        },
        |i, _| {
            let r = match g.prims[i as usize] {
                Prim::Face { face, uv } => {
                    let (r, t) = solve::face_trimmed(
                        g,
                        face,
                        uv,
                        g.prim_box(i),
                        ax,
                        c,
                        &opts.seeding,
                        stats,
                    );
                    if t > r.map_or(f64::NEG_INFINITY, |r| r.tau) {
                        trimmed.push((face, t));
                    }
                    if !g.sub_boxes(i).is_empty() {
                        let tau = r.map_or(f64::NEG_INFINITY, |r| r.tau);
                        patches.push((i, tau));
                    }
                    r
                }
                _ => solve_prim(g, i, ax, c, &opts.seeding, stats),
            }?;
            if r.tau > opts.floor && best.is_none_or(|b| r.tau > b.tau) {
                best = Some(r);
            }
            // At the ceiling nothing more is needed: end the search.
            Some(if r.tau >= opts.ceiling {
                f64::INFINITY
            } else {
                r.tau
            })
        },
    );
    // A patch beside the boundary of its face whose own solve gave nothing,
    // or less than the best contact, while its sub-boxes along that
    // boundary still allow more, may have missed a contact there (Newton
    // from its seeds can fail or stop short beside the boundary, and
    // neighbouring patches, which used to be solved as well, no longer
    // cover for it). It is solved again, with Newton started from the
    // centres of its cells whose bounds beat the best contact, highest
    // first: starts where a better contact may be.
    let have = |best: Option<Contact>| best.map_or(opts.floor, |b| b.tau.max(opts.floor));
    if have(best) < opts.ceiling {
        for &(i, tau) in &patches {
            let Prim::Face { face, uv } = g.prims[i as usize] else {
                continue;
            };
            let Some(q) = g.faces[face as usize].as_ref() else {
                continue;
            };
            if tau >= have(best)
                || face_edge_bound(g.sub_boxes(i), uv, q.uv, ax, c) - opts.slack <= have(best)
            {
                continue;
            }
            stats.rescues += 1;
            let starts = rescue_starts(g.sub_boxes(i), uv, ax, c, have(best) + opts.slack);
            let (r, t) = solve::face_from(g, face, ax, c, &starts, stats);
            if t > r.map_or(f64::NEG_INFINITY, |r| r.tau) {
                trimmed.push((face, t));
            }
            if let Some(r) = r
                && r.tau > opts.floor
                && best.is_none_or(|b| r.tau > b.tau)
            {
                best = Some(r);
            }
        }
    }
    // The boundary of such a face, where it beats every contact by more
    // than the trim chord: the edges missed it (a model gap).
    let floor = best.map_or(opts.floor, |b| b.tau.max(opts.floor));
    if opts.rims && floor < opts.ceiling {
        // Highest τ first per face, so `dedup` keeps it.
        trimmed.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)));
        trimmed.dedup_by(|a, b| a.0 == b.0);
        for (face, t) in trimmed {
            if t <= floor + g.trim_chord {
                continue;
            }
            stats.rims += 1;
            let base = floor + g.trim_chord;
            let above = best.map_or(base, |b| b.tau.max(base));
            if let Some(r) = solve::rim(g, face, ax, c, above)
                && r.tau > floor + g.trim_chord
                && best.is_none_or(|b| r.tau > b.tau)
            {
                best = Some(r);
            }
        }
    }
    best
}

/// As [`drop`], but solves every primitive (no pruning); for tests.
pub fn drop_all(
    g: &QueryGeometry,
    c: &Cutter,
    x: f64,
    y: f64,
    seeding: &Seeding,
    stats: &mut SolveStats,
) -> Option<Contact> {
    let ax = Axis { x, y };
    let mut best: Option<Contact> = None;
    for i in 0..g.prims.len() as u32 {
        if let Some(r) = solve_prim(g, i, ax, c, seeding, stats)
            && best.is_none_or(|b| r.tau > b.tau)
        {
            best = Some(r);
        }
    }
    best
}
