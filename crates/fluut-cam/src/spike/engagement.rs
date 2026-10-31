// SPDX-License-Identifier: MIT OR Apache-2.0

//! Exact tool engagement: which part of the tool circle lies in material.
//!
//! The circle is cut at its exact intersections with every boundary segment
//! of the region; each arc between cuts is in or out as a whole, decided at
//! its midpoint. No grid, no sampling.

use crate::region::{Inside, Region2, Seg2};
use fluut_geom::Point2;
use std::f64::consts::TAU;

/// Angular intervals `(start, end)` with `0 ≤ start < end ≤ 2π`.
pub type Arcs = Vec<(f64, f64)>;

fn angle_of(c: Point2, q: Point2) -> f64 {
    libm::atan2(q.y() - c.y(), q.x() - c.x()).rem_euclid(TAU)
}

/// Angles on the circle (centre `c`, radius `r`) where it meets `s`.
fn cuts(c: Point2, r: f64, s: &Seg2, out: &mut Vec<f64>) {
    match s.arc() {
        None => {
            let d = s.b - s.a;
            let w = s.a - c;
            let (qa, qb, qc) = (d.dot(d), 2.0 * w.dot(d), w.dot(w) - r * r);
            let disc = qb * qb - 4.0 * qa * qc;
            if disc < 0.0 || qa == 0.0 {
                return;
            }
            let sq = disc.sqrt();
            for u in [(-qb - sq) / (2.0 * qa), (-qb + sq) / (2.0 * qa)] {
                if (0.0..=1.0).contains(&u) {
                    out.push(angle_of(c, s.a + d * u));
                }
            }
        }
        Some(g) => {
            let dv = g.centre - c;
            let dist = dv.length();
            if dist == 0.0 || dist > r + g.radius || dist < (r - g.radius).abs() {
                return;
            }
            // Circle–circle: angle at c between dv and the hit points.
            let cos_h = (r * r + dist * dist - g.radius * g.radius) / (2.0 * r * dist);
            let h = libm::acos(cos_h.clamp(-1.0, 1.0));
            let base = libm::atan2(dv.y(), dv.x());
            for a in [base - h, base + h] {
                let q = Point2::new(c.x() + r * libm::cos(a), c.y() + r * libm::sin(a));
                let qa = libm::atan2(q.y() - g.centre.y(), q.x() - g.centre.x());
                if g.contains_angle(qa) {
                    out.push(a.rem_euclid(TAU));
                }
            }
        }
    }
}

/// The parts of the circle that lie inside `region`.
pub fn circle_in(c: Point2, r: f64, region: &Region2) -> Arcs {
    let (lo, hi) = region.bbox();
    if c.x() + r < lo.x() || c.x() - r > hi.x() || c.y() + r < lo.y() || c.y() - r > hi.y() {
        return Vec::new();
    }
    let mut a: Vec<f64> = Vec::new();
    for s in region.segs() {
        cuts(c, r, &s, &mut a);
    }
    a.push(0.0);
    a.push(TAU);
    a.sort_by(f64::total_cmp);
    a.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
    let mut out: Arcs = Vec::new();
    for w in a.windows(2) {
        let m = 0.5 * (w[0] + w[1]);
        let q = Point2::new(c.x() + r * libm::cos(m), c.y() + r * libm::sin(m));
        if region.contains(q) == Inside::In {
            match out.last_mut() {
                Some(l) if l.1 == w[0] => l.1 = w[1],
                _ => out.push((w[0], w[1])),
            }
        }
    }
    out
}

pub fn measure(a: &Arcs) -> f64 {
    a.iter().map(|(s, e)| e - s).sum()
}

/// `a` minus the circle interval `(centre − half, centre + half)`.
pub fn minus_window(a: &Arcs, centre: f64, half: f64) -> Arcs {
    if half <= 0.0 {
        return a.clone();
    }
    let s = (centre - half).rem_euclid(TAU);
    let e = s + 2.0 * half;
    let windows = if e <= TAU {
        vec![(s, e)]
    } else {
        vec![(s, TAU), (0.0, e - TAU)]
    };
    let mut out = a.clone();
    for (ws, we) in windows {
        out = out
            .into_iter()
            .flat_map(|(x, y)| {
                let mut v = Vec::new();
                if x < ws.min(y) {
                    v.push((x, ws.min(y)));
                }
                if y > we.max(x) {
                    v.push((we.max(x), y));
                }
                v
            })
            .collect();
    }
    out
}

/// Engagement angle of a tool of radius `r` at `p`: the part of the circle
/// in `uncut` that the tool has not already cut at any of the positions
/// `prev` (earlier points of the same pass, not yet part of `uncut`).
pub fn engagement(p: Point2, prev: &[Point2], r: f64, uncut: &Region2) -> f64 {
    let mut arcs = circle_in(p, r, uncut);
    for &q in prev {
        let d = p.distance(q);
        if d >= 2.0 * r || d == 0.0 {
            continue;
        }
        // Points of circle(p) within distance r of q.
        let half = libm::acos(d / (2.0 * r));
        arcs = minus_window(&arcs, angle_of(p, q), half);
    }
    measure(&arcs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// Numerical engagement: fraction of 200 000 circle points in `region`.
    fn numeric(c: Point2, r: f64, region: &Region2) -> f64 {
        let n = 200_000;
        let inside = (0..n)
            .filter(|i| {
                let a = (*i as f64 + 0.5) / n as f64 * TAU;
                let q = Point2::new(c.x() + r * libm::cos(a), c.y() + r * libm::sin(a));
                region.contains(q) == Inside::In
            })
            .count();
        inside as f64 / n as f64 * TAU
    }

    #[test]
    fn half_plane_engagement() {
        // Tool centre on the edge of a big block: half the circle in it.
        let block = Region2::rect(0.0, -50.0, 100.0, 50.0);
        let e = measure(&circle_in(Point2::new(0.0, 0.0), 3.0, &block));
        assert!((e - PI).abs() < 1e-12, "{e}");
        // Centre 1.5 outside: 2·acos(0.5) = 120°.
        let e = measure(&circle_in(Point2::new(-1.5, 0.0), 3.0, &block));
        assert!((e - 2.0 * PI / 3.0).abs() < 1e-12, "{e}");
    }

    #[test]
    fn matches_numeric_on_arc_regions() {
        let region = crate::spike::shapes::plate()
            .subtract(&Region2::circle(20.0, 20.0, 8.0))
            .subtract(&Region2::rect(70.0, 25.0, 90.0, 35.0));
        for (x, y, r) in [
            (40.0, 30.0, 3.0),
            (60.5, 30.0, 4.0),
            (27.0, 20.0, 3.0),
            (70.0, 33.0, 2.5),
            (1.0, 59.0, 3.0),
        ] {
            let p = Point2::new(x, y);
            let exact = measure(&circle_in(p, r, &region));
            let num = numeric(p, r, &region);
            // The numeric estimate has error ≈ 2π / 200 000 per crossing.
            assert!((exact - num).abs() < 1e-4, "{x},{y}: {exact} vs {num}");
        }
    }

    #[test]
    fn previous_tool_window() {
        let block = Region2::rect(0.0, -50.0, 100.0, 50.0);
        // Moving along +x inside the block: the window removes the back.
        let p = Point2::new(10.0, 0.0);
        let e = engagement(p, &[Point2::new(9.0, 0.0)], 3.0, &block);
        let half = libm::acos(1.0 / 6.0);
        assert!((e - (TAU - 2.0 * half)).abs() < 1e-12, "{e}");
    }
}
