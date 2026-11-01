// SPDX-License-Identifier: MIT OR Apache-2.0

//! In-process stock as layered exact 2-D regions (D08 §7, spec 04
//! §4.12.6, OQ-11).
//!
//! The region at key `k` is the material in the slab from the next key
//! below (or the stock bottom) up to `k`. A cut with its tip at `z` clears
//! the slabs above `z`; a level inside a slab splits it, and the new lower
//! slab is a copy of the old one.
//!
//! - **Planar** stock: keys are Z values, regions are in XY of W.
//! - **Cylindrical** stock (rotary Setups): keys are radii, regions are in
//!   (x, θ) with θ in radians over [0, 2π): the unrolled cylinder (x, θ·r)
//!   scaled by 1/r, so one region serves every radius of its slab.
//!
//! Updates from a result never take away material that was not cut: arcs
//! become chords inside a radius reduced by the tolerance, ramps count at
//! their top, and a tool clears its full radius only above its corner or
//! cone height.

use crate::region::Region2;
use crate::spike::pocket::{stadium, swept_path};
use fluut_geom::{Aabb3, Point2};
use fluut_model::tools::{Tool, ToolShape};
use fluut_path::{Motion, Segment, ToolpathResult, linearize};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::f64::consts::TAU;

/// What the levels of a stock are.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum StockFrame {
    /// Z levels; regions in XY.
    Planar,
    /// Radii about the rotary axis (X); regions in (x, θ).
    Cylindrical,
}

/// A level key ordered by `total_cmp`.
#[derive(Copy, Clone, Debug)]
struct Key(f64);

impl PartialEq for Key {
    fn eq(&self, o: &Key) -> bool {
        self.0.total_cmp(&o.0) == Ordering::Equal
    }
}
impl Eq for Key {}
impl PartialOrd for Key {
    fn partial_cmp(&self, o: &Key) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Key {
    fn cmp(&self, o: &Key) -> Ordering {
        self.0.total_cmp(&o.0)
    }
}

/// Material left after earlier toolpaths (D08 §7).
#[derive(Clone, Debug, PartialEq)]
pub struct LayeredStock {
    pub frame: StockFrame,
    levels: BTreeMap<Key, Region2>,
    /// The stock bottom (planar) or the axis, 0 (cylindrical).
    pub bottom: f64,
}

/// How a tool sweeps: its radius and the height above the tip where it
/// reaches that radius.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Sweep {
    pub radius: f64,
    pub lift: f64,
}

impl Sweep {
    pub fn of(tool: &Tool) -> Sweep {
        let r = tool.radius();
        let lift = match tool.shape {
            ToolShape::Flat => 0.0,
            ToolShape::Ball | ToolShape::Bull => tool.corner_radius(),
            ToolShape::V | ToolShape::Taper => tool.cone_height().unwrap_or(0.0),
            ToolShape::Drill => r / libm::tan(0.5 * tool.point_angle().to_radians()),
        };
        Sweep {
            radius: r,
            lift: lift.max(0.0),
        }
    }
}

impl LayeredStock {
    /// A block: the XY rectangle of `b` from its bottom to its top.
    pub fn planar(b: &Aabb3) -> LayeredStock {
        let mut levels = BTreeMap::new();
        levels.insert(
            Key(b.max.z()),
            Region2::rect(b.min.x(), b.min.y(), b.max.x(), b.max.y()),
        );
        LayeredStock {
            frame: StockFrame::Planar,
            levels,
            bottom: b.min.z(),
        }
    }

    /// A cylinder of `radius` about X from `x0` to `x1`.
    pub fn cylindrical(radius: f64, x0: f64, x1: f64) -> LayeredStock {
        let mut levels = BTreeMap::new();
        levels.insert(Key(radius), Region2::rect(x0, 0.0, x1, TAU));
        LayeredStock {
            frame: StockFrame::Cylindrical,
            levels,
            bottom: 0.0,
        }
    }

    /// The highest key: the stock top or outer radius.
    pub fn top(&self) -> f64 {
        self.levels.keys().next_back().map_or(self.bottom, |k| k.0)
    }

    /// The level keys, from the bottom up.
    pub fn keys(&self) -> Vec<f64> {
        self.levels.keys().map(|k| k.0).collect()
    }

    /// The material at height `z`: the region of the slab holding `z`.
    pub fn at(&self, z: f64) -> Region2 {
        if z <= self.bottom {
            return Region2::empty();
        }
        self.levels
            .range(Key(z)..)
            .next()
            .map_or_else(Region2::empty, |(_, r)| r.clone())
    }

    /// The material anywhere above `z`: the union of the slabs above it.
    pub fn above(&self, z: f64) -> Region2 {
        let z = z.max(self.bottom);
        let mut parts: Vec<Region2> = self
            .levels
            .range((
                std::ops::Bound::Excluded(Key(z)),
                std::ops::Bound::Unbounded,
            ))
            .map(|(_, r)| r.clone())
            .filter(|r| !r.is_empty())
            .collect();
        parts.dedup();
        union_all(parts)
    }

    /// The material between `lo` and `hi`: the union of the slabs that
    /// reach into (`lo`, `hi`]. `hi` may be infinite.
    pub fn between(&self, lo: f64, hi: f64) -> Region2 {
        let lo = lo.max(self.bottom);
        let mut parts = Vec::new();
        for (k, r) in self.levels.range((
            std::ops::Bound::Excluded(Key(lo)),
            std::ops::Bound::Unbounded,
        )) {
            if !r.is_empty() && parts.last() != Some(r) {
                parts.push(r.clone());
            }
            if k.0 >= hi {
                break;
            }
        }
        union_all(parts)
    }

    /// The material each level of a toolpath meets: for level `i`, the
    /// slabs between it and level `i − 1` (everything above for the first).
    pub fn windows(&self, zs: &[f64]) -> Vec<Region2> {
        (0..zs.len())
            .map(|i| {
                let hi = if i == 0 { f64::INFINITY } else { zs[i - 1] };
                self.between(zs[i], hi)
            })
            .collect()
    }

    /// Splits the slab holding `z` at `z`; both halves keep its region.
    pub fn ensure_level(&mut self, z: f64) {
        if z <= self.bottom || self.levels.contains_key(&Key(z)) {
            return;
        }
        if let Some((_, r)) = self.levels.range(Key(z)..).next() {
            let r = r.clone();
            self.levels.insert(Key(z), r);
        }
    }

    /// Takes `swept` out of every slab above a cut with its tip at `z`.
    pub fn remove_swept(&mut self, z: f64, swept: &Region2) {
        if swept.is_empty() {
            return;
        }
        self.ensure_level(z);
        let lo = (
            std::ops::Bound::Excluded(Key(z)),
            std::ops::Bound::Unbounded,
        );
        for (_, r) in self.levels.range_mut(lo) {
            if !r.is_empty() {
                *r = r.subtract(swept);
            }
        }
    }

    /// Takes out what `result` cut with a tool that sweeps as `sweep`.
    /// `tol` is the chord tolerance for arcs.
    pub fn apply(&mut self, result: &ToolpathResult, sweep: Sweep, tol: f64) {
        let cuts = match self.frame {
            StockFrame::Planar => planar_cuts(result, sweep, tol),
            StockFrame::Cylindrical => cylindrical_cuts(result, sweep, tol),
        };
        // From the top down, so each cut sees fewer slabs to clip.
        for (k, parts) in cuts.into_iter().rev() {
            let swept = union_all(parts);
            self.remove_swept(k.0 + sweep.lift, &swept);
        }
    }
}

/// The stock in `window` that lies in the region to clear, shrunk by
/// twice `tol`. This drops the slivers [`LayeredStock::apply`] leaves
/// along every cut, and material a tool already reached to within the
/// tolerance; neither is stock to rough.
pub fn rest_material(window: &Region2, region: &Region2, tol: f64) -> Region2 {
    window.intersect(region).offset(-2.0 * tol)
}

/// Union of many regions as a balanced tree.
pub fn union_all(mut parts: Vec<Region2>) -> Region2 {
    if parts.is_empty() {
        return Region2::empty();
    }
    while parts.len() > 1 {
        parts = parts
            .chunks(2)
            .map(|c| match c {
                [a, b] => a.union(b),
                [a] => a.clone(),
                _ => unreachable!(),
            })
            .collect();
    }
    parts.pop().unwrap_or_default()
}

/// Whether a move can cut material.
fn cuts(seg: &Segment) -> bool {
    seg.motion().is_some_and(|m| m.motion != Motion::Rapid)
}

/// Swept regions of the planar moves of `result`, keyed by tip Z. Level
/// runs are joined into polylines; a sloped move counts at the lowest
/// level run at or above its top (or not at all).
fn planar_cuts(result: &ToolpathResult, sweep: Sweep, tol: f64) -> BTreeMap<Key, Vec<Region2>> {
    let r = (sweep.radius - tol).max(0.0);
    let mut level: BTreeMap<Key, Vec<Region2>> = BTreeMap::new();
    let mut sloped: Vec<(f64, Point2, Point2)> = Vec::new();
    let mut run: Vec<Point2> = Vec::new();
    let mut run_z = f64::NAN;
    let flush = |run: &mut Vec<Point2>, z: f64, level: &mut BTreeMap<Key, Vec<Region2>>| {
        if run.len() >= 2 {
            level.entry(Key(z)).or_default().push(swept_path(run, r));
        }
        run.clear();
    };
    for seg in result.segments() {
        if let Segment::Cycle { at, cycle, .. } = seg {
            flush(&mut run, run_z, &mut level);
            level
                .entry(Key(cycle.depth_z))
                .or_default()
                .push(Region2::circle(at.xyz.x(), at.xyz.y(), r));
            continue;
        }
        if !cuts(seg) {
            flush(&mut run, run_z, &mut level);
            continue;
        }
        for line in linearize(seg, tol) {
            let (Some(a), Some(b)) = (line.start(), line.end()) else {
                continue;
            };
            let (pa, pb) = (
                Point2::new(a.xyz.x(), a.xyz.y()),
                Point2::new(b.xyz.x(), b.xyz.y()),
            );
            let (za, zb) = (a.xyz.z(), b.xyz.z());
            if za == zb {
                if za != run_z || run.last() != Some(&pa) {
                    flush(&mut run, run_z, &mut level);
                    run_z = za;
                    run.push(pa);
                }
                run.push(pb);
            } else {
                flush(&mut run, run_z, &mut level);
                if pa != pb {
                    sloped.push((za.max(zb), pa, pb));
                }
            }
        }
    }
    flush(&mut run, run_z, &mut level);
    let keys: Vec<Key> = level.keys().copied().collect();
    for (top, a, b) in sloped {
        if let Some(k) = keys.iter().find(|k| k.0 >= top) {
            level.entry(*k).or_default().push(stadium(a, b, r));
        }
    }
    level
}

/// Swept strips of the rotary moves of `result`, keyed by CL radius.
fn cylindrical_cuts(
    result: &ToolpathResult,
    sweep: Sweep,
    tol: f64,
) -> BTreeMap<Key, Vec<Region2>> {
    let w = (sweep.radius - tol).max(0.0);
    let mut out: BTreeMap<Key, Vec<Region2>> = BTreeMap::new();
    for seg in result.segments() {
        let Segment::Rotary { a, b, radius, .. } = seg else {
            continue;
        };
        if !cuts(seg) || a.xyz.x() != b.xyz.x() {
            continue;
        }
        let (Some(t0), Some(t1)) = (a.a, b.a) else {
            continue;
        };
        let x = a.xyz.x();
        for (lo, hi) in wrap(t0.to_radians(), t1.to_radians()) {
            out.entry(Key(*radius))
                .or_default()
                .push(Region2::rect(x - w, lo, x + w, hi));
        }
    }
    out
}

/// The θ intervals in [0, 2π] covered by turning from `t0` to `t1`.
pub fn wrap(t0: f64, t1: f64) -> Vec<(f64, f64)> {
    let len = (t1 - t0).abs();
    if len >= TAU - 1e-12 {
        return vec![(0.0, TAU)];
    }
    if len == 0.0 {
        return Vec::new();
    }
    let s = t0.min(t1).rem_euclid(TAU);
    if s + len <= TAU {
        vec![(s, s + len)]
    } else {
        vec![(s, TAU), (0.0, s + len - TAU)]
    }
}

#[cfg(test)]
mod tests;
