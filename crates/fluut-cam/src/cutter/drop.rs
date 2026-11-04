// SPDX-License-Identifier: MIT OR Apache-2.0

//! The drop query (D07 §2–§3): highest tool position along −Z, by
//! best-first branch and bound over the primitive BVH.

use super::geom::{Prim, QueryGeometry};
use super::profile::Cutter;
use super::solve::{self, Axis, Contact, Seeding, SolveStats};
use fluut_geom::Aabb3;

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
    g.bvh.best_first(
        opts.floor,
        |b| bound(b, ax, c) - opts.slack,
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
