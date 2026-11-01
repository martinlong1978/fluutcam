// SPDX-License-Identifier: MIT OR Apache-2.0

//! Constant-engagement clearing (D09 §2.3, CH-16), promoted from the S3
//! spike (`spike/adaptive.rs`).
//!
//! The cleared region `K` grows pass by pass. Each pass starts half a tool
//! radius inside the edge of `K`, where the tool meets enough material,
//! and marches in steps whose heading is found by bisection so that the
//! engagement in the uncut region equals the target with the material on
//! the cutting side. Engagement is exact: the tool circle is cut at its
//! intersections with the uncut region's boundary, and the parts within
//! reach of the pass's own earlier positions are taken out (the S3 rule).
//!
//! Speed work over the spike (S3 report §5): the uncut region is indexed
//! and its segment geometry cached; arcs of the tool circle are classified
//! by the direction of each crossing instead of a containment test; the
//! uncut region shrinks by each pass's sweep instead of being rebuilt; 12
//! bisection steps; start candidates are tried nearest first.

use crate::region::boolean::Indexed;
use crate::region::{Inside, Region2};
use crate::spike::engagement::{Arcs, measure, minus_window};
use crate::spike::pocket::swept_path;
use fluut_geom::{Point2, Vec2};
use std::f64::consts::{PI, TAU};

#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Tool radius.
    pub r: f64,
    /// Target engagement angle φ* (radians).
    pub phi: f64,
    /// Step length along the path.
    pub step: f64,
    /// Smallest radius of the tool-centre path; 0 for no limit.
    pub min_radius: f64,
    /// Material on the right of travel (climb, spindle M3); else on the
    /// left.
    pub climb: bool,
    /// Caps on steps per pass and on passes.
    pub max_steps: usize,
    pub max_passes: usize,
}

impl Params {
    /// Defaults for a tool of radius `r` and a radial load: φ* = acos(1 −
    /// load / R), steps of R/4.
    pub fn new(r: f64, load: f64) -> Params {
        Params {
            r,
            phi: libm::acos((1.0 - load / r).clamp(-1.0, 1.0)),
            step: 0.25 * r,
            min_radius: 0.0,
            climb: true,
            max_steps: 100_000,
            max_passes: 20_000,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub passes: usize,
    pub steps: usize,
    pub evaluations: usize,
    /// Engagement error over all cutting steps (radians).
    pub max_err: f64,
    pub rms_err: f64,
    /// Steps more than 10° from the target.
    pub off_target: usize,
    /// Area of the material region left uncut.
    pub uncut_area: f64,
    /// Why the search stopped.
    pub stop: &'static str,
}

/// One pass: tool-centre points, and the region where the tool centre may
/// move without cutting before the pass (K shrunk by R).
#[derive(Debug, Clone)]
pub struct Pass {
    pub pts: Vec<Point2>,
    pub safe: Region2,
}

#[derive(Debug, Clone)]
pub struct Cleared {
    pub passes: Vec<Pass>,
    /// K after the last pass.
    pub cleared: Region2,
    pub stats: Stats,
}

/// Clears `material` (the region the tool should sweep) with the tool
/// centre kept in `centres`, starting from the cleared region `k0`, the
/// first pass near `from`.
pub fn clear(
    material: &Region2,
    centres: &Region2,
    k0: Region2,
    from: Point2,
    p: &Params,
) -> Cleared {
    let cen = Indexed::new(centres);
    let mut stats = Stats {
        stop: "max passes",
        ..Stats::default()
    };
    let mut k = k0;
    let mut uncut = material.subtract(&k);
    let mut passes = Vec::new();
    let (mut sq, mut cutting) = (0.0, 0usize);
    let mut cur = from;
    let mut min_start = 0.5 * p.phi;
    // Starts where no step could be made; not tried again.
    let mut dead: Vec<Point2> = Vec::new();
    for _ in 0..p.max_passes {
        if uncut.area() < 1e-6 {
            stats.stop = "cleared";
            break;
        }
        let fu = Fast::new(&uncut);
        // Starts need half the target engagement; when there are none
        // left, slivers are taken at a fifth of it (the rest phase).
        let mut start = None;
        while start.is_none() && min_start > 0.0 {
            start = find_start(&fu, &cen, &k, cur, p, min_start, &dead, &mut stats);
            if start.is_none() {
                min_start = if min_start > REST * p.phi {
                    REST * p.phi
                } else {
                    0.0
                };
            }
        }
        let Some(start) = start else {
            stats.stop = "no start point";
            break;
        };
        // A pass ends where the engagement falls below a tenth of the
        // target (half the start threshold in the rest phase).
        let floor = (0.1 * p.phi).min(0.5 * min_start);
        let path = march(
            start,
            &fu,
            &cen,
            p,
            floor,
            &mut stats,
            &mut sq,
            &mut cutting,
        );
        if path.len() < 2 {
            dead.push(start);
            if dead.len() > p.max_passes {
                stats.stop = "stuck";
                break;
            }
            continue;
        }
        // Enter where the whole tool is in cleared material.
        let safe = shrink(&k, p.r - 1e-6);
        let entry = nearest_on(&safe, start).unwrap_or(start);
        let mut pts = Vec::with_capacity(path.len() + 1);
        if entry.distance(start) > 1e-9 {
            pts.push(entry);
        }
        pts.extend(path);
        let swept = swept_path(&pts, p.r);
        k = k.union(&swept);
        uncut = uncut.subtract(&swept);
        cur = pts[pts.len() - 1];
        stats.steps += pts.len() - 1;
        passes.push(Pass { pts, safe });
    }
    stats.passes = passes.len();
    stats.rms_err = if cutting > 0 {
        (sq / cutting as f64).sqrt()
    } else {
        0.0
    };
    stats.uncut_area = uncut.area();
    Cleared {
        passes,
        cleared: k,
        stats,
    }
}

/// `r` shrunk by `d` > 0: each outer loop shrunk, minus each hole grown
/// (exact: the erosion of A ∖ H is the erosion of A minus the dilation of
/// H). Offsetting the whole region at once can fail, giving only empty
/// loops, when it has tiny holes, as the cleared region does around
/// uncut slivers.
pub fn shrink(r: &Region2, d: f64) -> Region2 {
    if r.loops.iter().all(|l| l.is_ccw()) {
        return offset_in(r, d);
    }
    let mut outer = Region2::empty();
    let mut holes = Region2::empty();
    for l in &r.loops {
        if l.is_ccw() {
            let o = offset_in(
                &Region2 {
                    loops: vec![l.clone()],
                },
                d,
            );
            outer = if outer.is_empty() { o } else { outer.union(&o) };
        } else {
            let h = Region2 {
                loops: vec![l.reversed()],
            }
            .offset(d);
            holes = if holes.is_empty() { h } else { holes.union(&h) };
        }
    }
    if holes.is_empty() {
        outer
    } else {
        outer.subtract(&holes)
    }
}

/// `r.offset(-d)`, retried a little less deep when the offset fails and
/// gives nothing although the region is wider than `2d` (seen with
/// `cavalier_contours` on long swept outlines): at most 0.01 mm less.
fn offset_in(r: &Region2, d: f64) -> Region2 {
    let mut o = Region2::empty();
    for e in [0.0, 1e-4, 1e-3, 1e-2] {
        o = r.offset(-(d - e));
        if o.area() > 0.0 {
            return o;
        }
    }
    o
}

/// The point of the boundary of `r` nearest to `q`, or `q` when it is
/// inside `r`; `None` for an empty region.
fn nearest_on(r: &Region2, q: Point2) -> Option<Point2> {
    if r.is_empty() {
        return None;
    }
    if r.contains(q) == Inside::In {
        return Some(q);
    }
    r.segs()
        .map(|s| s.point_at(crate::path2::seg_closest(&s, q)))
        .min_by(|a, b| a.distance(q).total_cmp(&b.distance(q)))
}

/// A region with its boundary indexed and its segment geometry cached,
/// for fast tool-circle queries.
pub struct Fast {
    ix: Indexed,
    geo: Vec<Geo>,
    region: Region2,
}

#[derive(Clone, Copy)]
enum Geo {
    Line {
        a: Point2,
        d: Vec2,
    },
    Arc {
        c: Point2,
        r: f64,
        start: f64,
        sweep: f64,
        b: Point2,
    },
}

impl std::fmt::Debug for Fast {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fast({} segments)", self.geo.len())
    }
}

impl Fast {
    pub fn new(r: &Region2) -> Fast {
        let ix = Indexed::new(r);
        let geo = ix
            .segs
            .iter()
            .map(|s| match s.arc() {
                None => Geo::Line {
                    a: s.a,
                    d: s.b - s.a,
                },
                Some(g) => Geo::Arc {
                    c: g.centre,
                    r: g.radius,
                    start: g.start,
                    sweep: g.sweep,
                    b: s.b,
                },
            })
            .collect();
        Fast {
            ix,
            geo,
            region: r.clone(),
        }
    }

    pub fn contains(&self, p: Point2) -> bool {
        self.ix.contains(p) == Inside::In
    }

    /// The parts of the circle about `c` of radius `r` inside the region.
    pub fn circle_in(&self, c: Point2, r: f64) -> Arcs {
        let b = (c.x() - r, c.y() - r, c.x() + r, c.y() + r);
        let mut hits: Vec<(f64, Option<bool>)> = Vec::new();
        for i in self.ix.query(&b) {
            hits_on(&self.geo[i], c, r, &mut hits);
        }
        if hits.is_empty() {
            return if self.contains(Point2::new(c.x() + r, c.y())) {
                vec![(0.0, TAU)]
            } else {
                Vec::new()
            };
        }
        hits.sort_by(|x, y| x.0.total_cmp(&y.0));
        if let Some(a) = from_crossings(&hits) {
            return a;
        }
        // Degenerate crossings (vertices, tangencies): the spike's exact
        // classification of every arc by its midpoint.
        crate::spike::engagement::circle_in(c, r, &self.region)
    }

    /// Engagement at `p`: the part of the tool circle in the region that
    /// the tool has not already cut at the positions `prev` of this pass.
    pub fn engagement(&self, p: Point2, prev: &[Point2], r: f64) -> f64 {
        let mut arcs = self.circle_in(p, r);
        for &q in prev {
            let d = p.distance(q);
            if d >= 2.0 * r || d == 0.0 || arcs.is_empty() {
                continue;
            }
            let half = libm::acos(d / (2.0 * r));
            let ang = libm::atan2(q.y() - p.y(), q.x() - p.x()).rem_euclid(TAU);
            arcs = minus_window(&arcs, ang, half);
        }
        measure(&arcs)
    }
}

/// Crossings of the circle about `c` (radius `r`) with one boundary
/// segment, half open (the segment's end belongs to the next one): the
/// angle, and whether the circle, run counter-clockwise, enters the region
/// there (`None` when tangent).
fn hits_on(g: &Geo, c: Point2, r: f64, out: &mut Vec<(f64, Option<bool>)>) {
    let mut push = |q: Point2, seg_dir: Vec2| {
        let rad = q - c;
        let ang = libm::atan2(rad.y(), rad.x()).rem_euclid(TAU);
        // Counter-clockwise tangent of the circle at q (times r).
        let t = Vec2::new(-rad.y(), rad.x());
        let cr = seg_dir.x() * t.y() - seg_dir.y() * t.x();
        let scale = seg_dir.length() * r;
        let enter = if cr.abs() <= 1e-12 * scale {
            None
        } else {
            Some(cr > 0.0)
        };
        out.push((ang, enter));
    };
    match *g {
        Geo::Line { a, d } => {
            let w = a - c;
            let qa = d.dot(d);
            if qa == 0.0 {
                return;
            }
            let qb = 2.0 * w.dot(d);
            let qc = w.dot(w) - r * r;
            let disc = qb * qb - 4.0 * qa * qc;
            if disc <= 0.0 {
                return; // missed, or tangent: no change of side
            }
            let sq = disc.sqrt();
            for u in [(-qb - sq) / (2.0 * qa), (-qb + sq) / (2.0 * qa)] {
                if (0.0..1.0).contains(&u) {
                    push(a + d * u, d);
                }
            }
        }
        Geo::Arc {
            c: gc,
            r: gr,
            start,
            sweep,
            b,
        } => {
            let dv = gc - c;
            let dist = dv.length();
            if dist == 0.0 || dist >= r + gr || dist <= (r - gr).abs() {
                return;
            }
            let cos_h = (r * r + dist * dist - gr * gr) / (2.0 * r * dist);
            let h = libm::acos(cos_h.clamp(-1.0, 1.0));
            let base = libm::atan2(dv.y(), dv.x());
            for a in [base - h, base + h] {
                let (s, co) = libm::sincos(a);
                let q = Point2::new(c.x() + r * co, c.y() + r * s);
                if q.distance(b) <= 1e-12 * gr.max(1.0) {
                    continue;
                }
                let qa = libm::atan2(q.y() - gc.y(), q.x() - gc.x());
                let dd = (qa - start).rem_euclid(TAU);
                let inside = if sweep >= 0.0 {
                    dd <= sweep
                } else {
                    dd == 0.0 || dd >= TAU + sweep
                };
                if !inside {
                    continue;
                }
                let rad = q - gc;
                let ccw = Vec2::new(-rad.y(), rad.x());
                push(q, if sweep >= 0.0 { ccw } else { ccw * -1.0 });
            }
        }
    }
}

/// The inside arcs from crossings sorted by angle, when they alternate
/// cleanly between entering and leaving; `None` otherwise.
fn from_crossings(hits: &[(f64, Option<bool>)]) -> Option<Arcs> {
    let n = hits.len();
    if !n.is_multiple_of(2) || hits.iter().any(|h| h.1.is_none()) {
        return None;
    }
    for i in 0..n {
        let (a, b) = (hits[i], hits[(i + 1) % n]);
        if a.1 == b.1 || (b.0 - a.0).abs() < 1e-12 {
            return None;
        }
    }
    let mut out: Arcs = Vec::new();
    let mut wrap = None;
    for i in 0..n {
        if hits[i].1 != Some(true) {
            continue;
        }
        let s = hits[i].0;
        if i + 1 < n {
            out.push((s, hits[i + 1].0));
        } else {
            // Enters at the last crossing, leaves at the first: wraps 2π.
            wrap = Some((s, hits[0].0));
        }
    }
    if let Some((s, e)) = wrap {
        if e > 0.0 {
            out.insert(0, (0.0, e));
        }
        out.push((s, TAU));
    }
    Some(out)
}

/// Engagement, as a fraction of the target, that a start of the rest
/// phase needs.
const REST: f64 = 0.05;

/// The nearest point to `cur`, half a radius inside the edge of `k` and in
/// `centres`, where the tool meets at least `min` engagement.
#[allow(clippy::too_many_arguments)]
fn find_start(
    uncut: &Fast,
    centres: &Indexed,
    k: &Region2,
    cur: Point2,
    p: &Params,
    min: f64,
    dead: &[Point2],
    stats: &mut Stats,
) -> Option<Point2> {
    let edge = k.offset(-0.5 * p.r);
    let mut cands: Vec<(f64, usize, Point2)> = Vec::new();
    for s in edge.segs() {
        let n = (s.length() / p.step).ceil().max(1.0) as usize;
        for i in 0..n {
            let q = s.point_at(i as f64 / n as f64);
            cands.push((q.distance(cur), cands.len(), q));
        }
    }
    cands.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    for (_, _, q) in cands {
        if centres.contains(q) == Inside::Out || dead.iter().any(|d| d.distance(q) < p.step) {
            continue;
        }
        stats.evaluations += 1;
        if uncut.engagement(q, &[], p.r) >= min {
            return Some(q);
        }
    }
    None
}

fn dir(a: f64) -> Vec2 {
    let (s, c) = libm::sincos(a);
    Vec2::new(c, s)
}

#[allow(clippy::too_many_arguments)]
fn march(
    start: Point2,
    uncut: &Fast,
    centres: &Indexed,
    p: &Params,
    floor: f64,
    stats: &mut Stats,
    sq: &mut f64,
    cutting: &mut usize,
) -> Vec<Point2> {
    let mut path = vec![start];
    let mut heading: Option<f64> = None;
    let mut pos = start;
    // Largest heading change per step for the smallest path radius.
    let turn = if p.min_radius > 0.0 {
        2.0 * libm::asin((p.step / (2.0 * p.min_radius)).min(1.0))
    } else {
        PI
    };
    let side = if p.climb { 1.0 } else { -1.0 };
    for _ in 0..p.max_steps {
        // Candidate headings scanned from the cutting side's opposite to
        // the cutting side; on the first step, the whole circle.
        let (h0, span, n) = match heading {
            Some(h) => (h, (2.0 * PI / 3.0).min(turn), 16),
            None => (0.0, PI, 48),
        };
        let at = |a: f64| pos + dir(a) * p.step;
        let near: Vec<Point2> = path
            .iter()
            .copied()
            .filter(|q| q.distance(pos) < 2.0 * p.r + p.step)
            .collect();
        let mut evals = 0usize;
        let mut err = |a: f64| {
            evals += 1;
            uncut.engagement(at(a), &near, p.r) - p.phi
        };
        // Near the previous heading first: step towards the material until
        // the engagement crosses the target, then bisect.
        let mut chosen = None;
        if let Some(h) = heading {
            let g = |t: f64| h - side * t;
            let delta = span / 8.0;
            let inside = |a: f64| centres.contains(at(a)) != Inside::Out;
            if inside(h) {
                let e0 = err(h);
                let towards = e0 < 0.0;
                let mut prev = (0.0, e0);
                for k in 1..=8 {
                    let t = if towards {
                        delta * k as f64
                    } else {
                        -delta * k as f64
                    };
                    if !inside(g(t)) {
                        break;
                    }
                    let e = err(g(t));
                    if (e >= 0.0) == towards {
                        // Bracket in t: low side below the target.
                        let (mut lo, mut hi) = if towards { (prev.0, t) } else { (t, prev.0) };
                        let start_lo = lo;
                        for _ in 0..10 {
                            let m = 0.5 * (lo + hi);
                            if err(g(m)) < 0.0 {
                                lo = m;
                            } else {
                                hi = m;
                            }
                        }
                        chosen = Some(inside_of(g(start_lo), g(0.5 * (lo + hi)), &inside));
                        break;
                    }
                    prev = (t, e);
                }
            }
        }
        let angles: Vec<f64> = if chosen.is_some() {
            Vec::new()
        } else {
            (0..=n)
                .map(|i| h0 + side * (span - 2.0 * span * i as f64 / n as f64))
                .filter(|&a| centres.contains(at(a)) != Inside::Out)
                .collect()
        };
        if chosen.is_none() && angles.is_empty() {
            stats.evaluations += evals;
            break;
        }
        let errs: Vec<f64> = angles.iter().map(|&a| err(a)).collect();
        for i in 1..angles.len() {
            if chosen.is_some() {
                break;
            }
            if errs[i - 1] < 0.0 && errs[i] >= 0.0 {
                let (mut lo, mut hi) = (angles[i - 1], angles[i]);
                let start_lo = lo;
                for _ in 0..12 {
                    let m = 0.5 * (lo + hi);
                    if err(m) < 0.0 {
                        lo = m;
                    } else {
                        hi = m;
                    }
                }
                let inside = |a: f64| centres.contains(at(a)) != Inside::Out;
                chosen = Some(inside_of(start_lo, 0.5 * (lo + hi), &inside));
                break;
            }
        }
        let a = match chosen {
            Some(a) => a,
            None => {
                let (i, e) = errs
                    .iter()
                    .copied()
                    .enumerate()
                    .max_by(|x, y| x.1.total_cmp(&y.1))
                    .unwrap_or((0, -p.phi));
                if e + p.phi < floor {
                    stats.evaluations += evals;
                    break; // no material left within reach
                }
                if e < 0.0 {
                    angles[i]
                } else {
                    // Too much everywhere: take the least.
                    let (j, _) = errs
                        .iter()
                        .copied()
                        .enumerate()
                        .min_by(|x, y| x.1.total_cmp(&y.1))
                        .unwrap_or((0, 0.0));
                    angles[j]
                }
            }
        };
        let e = err(a);
        stats.evaluations += evals;
        *sq += e * e;
        *cutting += 1;
        stats.max_err = stats.max_err.max(e.abs());
        if e.abs() > 10f64.to_radians() {
            stats.off_target += 1;
        }
        let next = at(a);
        path.push(next);
        pos = next;
        heading = Some(a);
        if path.len() > 8 && pos.distance(start) < p.step {
            break; // closed
        }
    }
    path
}

/// `want` if the step there stays in the tool-centre region; else the
/// heading between `safe` (which does) and `want` where the step meets the
/// region's edge, so the tool runs along it instead of into a wall.
fn inside_of(safe: f64, want: f64, inside: &impl Fn(f64) -> bool) -> f64 {
    if inside(want) {
        return want;
    }
    let (mut a, mut b) = (safe, want);
    for _ in 0..12 {
        let m = 0.5 * (a + b);
        if inside(m) {
            a = m;
        } else {
            b = m;
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spike::engagement::circle_in as slow;

    #[test]
    fn fast_engagement_matches_the_spike() {
        let region = crate::spike::shapes::plate()
            .subtract(&Region2::circle(20.0, 20.0, 8.0))
            .subtract(&Region2::rect(70.0, 25.0, 90.0, 35.0));
        let f = Fast::new(&region);
        let mut x: f64 = 0.37;
        for i in 0..400 {
            // Points over the plate, including ones on vertices and edges.
            x = (x * 7.31 + 0.123).fract();
            let y = ((i as f64) * 0.618).fract();
            let p = Point2::new(-2.0 + 104.0 * x, -2.0 + 64.0 * y);
            for r in [2.5, 3.0, 8.0] {
                let a = measure(&f.circle_in(p, r));
                let b = measure(&slow(p, r, &region));
                assert!((a - b).abs() < 1e-9, "{p:?} r {r}: {a} vs {b}");
            }
        }
        // Exactly through vertices of the island rectangle.
        for p in [Point2::new(70.0, 30.0), Point2::new(73.0, 25.0)] {
            let a = measure(&f.circle_in(p, 3.0));
            let b = measure(&slow(p, 3.0, &region));
            assert!((a - b).abs() < 1e-9, "{p:?}: {a} vs {b}");
        }
    }

    #[test]
    fn clears_a_rectangle_at_the_target_engagement() {
        let material = Region2::rect(0.0, 0.0, 40.0, 30.0);
        let centres = material.offset(-3.0);
        let k0 = Region2::circle(20.0, 15.0, 4.5).intersect(&material);
        let p = Params::new(3.0, 0.9);
        let c = clear(&material, &centres, k0, Point2::new(20.0, 15.0), &p);
        assert_eq!(c.stats.stop, "no start point", "{:?}", c.stats);
        // What the tool can reach is cleared, apart from slivers the final
        // contour takes.
        let reach = centres.offset(3.0);
        let left = reach.subtract(&c.cleared).area();
        assert!(left < 0.02 * reach.area(), "{left} of {}", reach.area());
        assert!(c.stats.rms_err < 20f64.to_radians(), "{:?}", c.stats);
    }
}
