// SPDX-License-Identifier: MIT OR Apache-2.0

//! Offset-pattern pocket (design D09 §2.2), one level.

use crate::region::{Loop2, Region2, Vertex};
use fluut_geom::{Point2, Vec2};

/// Rings of an offset pocket, inside-out: every loop of the offsets of
/// `clear` by −r, −r − s, −r − 2s, … until the offset is empty.
pub fn rings(clear: &Region2, r: f64, stepover: f64) -> Vec<Loop2> {
    let mut levels = Vec::new();
    let mut d = r;
    loop {
        let o = clear.offset(-d);
        if o.is_empty() {
            break;
        }
        levels.push(o);
        d += stepover;
    }
    levels.into_iter().rev().flat_map(|o| o.loops).collect()
}

/// The region a tool of radius `r` sweeps along a closed loop.
pub fn swept_loop(l: &Loop2, r: f64) -> Region2 {
    let single = Region2 {
        loops: vec![if l.is_ccw() { l.clone() } else { l.reversed() }],
    };
    single.offset(r).subtract(&single.offset(-r))
}

/// The region a tool of radius `r` sweeps along an open polyline.
pub fn swept_path(pts: &[Point2], r: f64) -> Region2 {
    let mut parts: Vec<Region2> = pts
        .windows(2)
        .map(|w| stadium(w[0], w[1], r).cleaned())
        .collect();
    if parts.is_empty() {
        return pts
            .first()
            .map_or_else(Region2::empty, |p| Region2::circle(p.x(), p.y(), r));
    }
    // Pairwise union, as a balanced tree.
    while parts.len() > 1 {
        parts = parts
            .chunks(2)
            .map(|c| match c {
                [a, b] => a.union(b).cleaned(),
                [a] => a.clone(),
                _ => unreachable!(),
            })
            .collect();
    }
    parts.pop().unwrap_or_default().cleaned()
}

/// A segment from `a` to `b` widened by `r` with round ends.
pub fn stadium(a: Point2, b: Point2, r: f64) -> Region2 {
    let d = b - a;
    let len = d.length();
    // A move shorter than the region tolerance is a disc: thinner
    // stadiums trip `cavalier_contours`.
    if len <= 1e-7 {
        return Region2::circle(a.x(), a.y(), r);
    }
    let n = Vec2::new(-d.y(), d.x()) * (r / len);
    let p = |q: Point2, s: f64| q + n * s;
    let v = |q: Point2, bulge| Vertex { p: q, bulge };
    Region2 {
        loops: vec![Loop2 {
            verts: vec![
                v(p(a, -1.0), 0.0),
                v(p(b, -1.0), 1.0),
                v(p(b, 1.0), 0.0),
                v(p(a, 1.0), 1.0),
            ],
        }],
    }
}
