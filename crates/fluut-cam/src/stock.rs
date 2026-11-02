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
//!
//! A child Setup turned about the rotary axis gets the stock
//! [`LayeredStock::reframed`] (D08 §7, CH-15): never smaller than the
//! material it came from.

use crate::region::{Inside, Loop2, Region2, Vertex};
use crate::spike::pocket::{stadium, swept_path};
use fluut_geom::{Aabb3, Point2};
use fluut_model::tools::{Tool, ToolShape};
use fluut_path::{Motion, Segment, ToolpathResult, linearize};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::f64::consts::TAU;

/// Largest slab height when a stock is re-sliced at a new angle or from
/// a solid of revolution, mm †.
pub const RESLICE_STEP: f64 = 1.0;

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

    /// The key of the highest slab with material within `r` of `p`: no
    /// material under a tool of radius `r` at `p` lies above it. `None`
    /// when there is none, and for cylindrical stock.
    pub fn top_near(&self, p: Point2, r: f64) -> Option<f64> {
        if self.frame != StockFrame::Planar {
            return None;
        }
        self.levels
            .iter()
            .rev()
            .find(|(_, reg)| {
                !reg.is_empty() && (reg.contains(p) != Inside::Out || reg.boundary_distance(p) <= r)
            })
            .map(|(k, _)| k.0)
    }

    /// The stock of a child Setup whose work frame is this one turned by
    /// `turn` degrees about X (D08 §7, CH-15): planar, and never smaller
    /// than this stock. Half turns are exact; other angles re-slice the
    /// slabs as prisms at most [`RESLICE_STEP`] high, each child slab the
    /// union of boxes round the parts of the parent's slabs that turn into
    /// it. Cylindrical stock becomes its [`LayeredStock::revolved`]
    /// outline, which no turn changes.
    pub fn reframed(&self, turn: f64) -> LayeredStock {
        if self.frame == StockFrame::Cylindrical {
            return LayeredStock::revolved(&self.profile());
        }
        let (s, c) = sin_cos_deg(turn);
        if s == 0.0 && c == 1.0 {
            return self.clone();
        }
        let mut levels = BTreeMap::new();
        if s == 0.0 {
            // Slab (lo, k] turns into [-k, -lo), mirrored in Y.
            let mut lo = self.bottom;
            for (k, r) in &self.levels {
                levels.insert(Key(-lo), mirror_y(r));
                lo = k.0;
            }
            return LayeredStock {
                frame: StockFrame::Planar,
                levels,
                bottom: -self.top(),
            };
        }
        // Each parent slab: (z0, z1, region, outer loop boxes).
        let mut slabs = Vec::new();
        let mut lo = self.bottom;
        for (k, r) in &self.levels {
            if !r.is_empty() {
                slabs.push((lo, k.0, r, outer_boxes(r)));
            }
            lo = k.0;
        }
        let turned_z = |y: f64, z: f64| y * s + z * c;
        let (mut z_lo, mut z_hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (z0, z1, _, boxes) in &slabs {
            for (a, b) in boxes {
                for y in [a.y(), b.y()] {
                    for z in [*z0, *z1] {
                        z_lo = z_lo.min(turned_z(y, z));
                        z_hi = z_hi.max(turned_z(y, z));
                    }
                }
            }
        }
        if z_lo >= z_hi {
            return LayeredStock {
                frame: StockFrame::Planar,
                levels,
                bottom: 0.0,
            };
        }
        let n = ((z_hi - z_lo) / RESLICE_STEP).ceil().max(1.0) as usize;
        let h = (z_hi - z_lo) / n as f64;
        for i in 1..=n {
            let a = z_lo + (i - 1) as f64 * h;
            let b = if i == n { z_hi } else { z_lo + i as f64 * h };
            let mut parts = Vec::new();
            for (z0, z1, r, _) in &slabs {
                parts.extend(turned_part(r, (*z0, *z1), (a, b), (s, c)));
            }
            levels.insert(Key(b), union_all(parts));
        }
        LayeredStock {
            frame: StockFrame::Planar,
            levels,
            bottom: z_lo,
        }
    }

    /// Planar stock holding the solid of revolution about X made of the
    /// `pieces` `(x0, x1, radius)`, sliced at most [`RESLICE_STEP`] high:
    /// in each slab a piece is the box of its widest section there.
    pub fn revolved(pieces: &[(f64, f64, f64)]) -> LayeredStock {
        let rho = pieces.iter().map(|p| p.2).fold(0.0, f64::max);
        let mut levels = BTreeMap::new();
        if rho <= 0.0 {
            return LayeredStock {
                frame: StockFrame::Planar,
                levels,
                bottom: 0.0,
            };
        }
        let n = (2.0 * rho / RESLICE_STEP).ceil().max(1.0) as usize;
        let h = 2.0 * rho / n as f64;
        for i in 1..=n {
            let a = -rho + (i - 1) as f64 * h;
            let b = if i == n { rho } else { -rho + i as f64 * h };
            // The smallest |z| in the slab.
            let m = if a <= 0.0 && b >= 0.0 {
                0.0
            } else {
                a.abs().min(b.abs())
            };
            let parts = pieces
                .iter()
                .filter(|p| p.2 > m && p.1 > p.0)
                .map(|&(x0, x1, r)| {
                    let w = (r * r - m * m).sqrt();
                    Region2::rect(x0, -w, x1, w)
                })
                .collect();
            levels.insert(Key(b), union_all(parts));
        }
        LayeredStock {
            frame: StockFrame::Planar,
            levels,
            bottom: -rho,
        }
    }

    /// The outline of a cylindrical stock as pieces `(x0, x1, radius)`:
    /// the X range of each outer loop of each slab, at the slab's radius.
    fn profile(&self) -> Vec<(f64, f64, f64)> {
        self.levels
            .iter()
            .flat_map(|(k, r)| {
                outer_boxes(r)
                    .into_iter()
                    .map(move |(a, b)| (a.x(), b.x(), k.0))
            })
            .collect()
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

/// Sine and cosine of `deg` degrees, exact at multiples of 90°.
pub fn sin_cos_deg(deg: f64) -> (f64, f64) {
    let q = deg.rem_euclid(360.0);
    if q == 0.0 {
        (0.0, 1.0)
    } else if q == 90.0 {
        (1.0, 0.0)
    } else if q == 180.0 {
        (0.0, -1.0)
    } else if q == 270.0 {
        (-1.0, 0.0)
    } else {
        let t = deg.to_radians();
        (libm::sin(t), libm::cos(t))
    }
}

/// `r` mirrored in Y (y to -y), loops kept counter-clockwise outside.
fn mirror_y(r: &Region2) -> Region2 {
    Region2 {
        loops: r
            .loops
            .iter()
            .map(|l| {
                Loop2 {
                    verts: l
                        .verts
                        .iter()
                        .map(|v| Vertex::new(v.p.x(), -v.p.y(), -v.bulge))
                        .collect(),
                }
                .reversed()
            })
            .collect(),
    }
}

/// Bounding boxes of the outer (counter-clockwise) loops of `r`.
fn outer_boxes(r: &Region2) -> Vec<(Point2, Point2)> {
    r.loops
        .iter()
        .filter(|l| l.is_ccw())
        .map(|l| {
            Region2 {
                loops: vec![l.clone()],
            }
            .bbox()
        })
        .collect()
}

/// Rounding margin of the boxes of a re-sliced stock, mm.
const GROW: f64 = 1e-6;

/// Boxes in the child's XY round the material of the parent slab
/// (`z.0`, `z.1`] with region `r` that turns (sine and cosine `sc`) into
/// the child slab (`ab.0`, `ab.1`]. The parent points that land there lie
/// in the parallelogram P = {z0 <= z <= z1, a <= y sin + z cos <= b} of YZ;
/// `r` is cut to the Y range of P, and each outer loop of the cut gives the
/// box of its X range by the range of y' = y cos - z sin over P within the
/// Y range of the loop.
fn turned_part(r: &Region2, z: (f64, f64), ab: (f64, f64), sc: (f64, f64)) -> Vec<Region2> {
    let (s, c) = sc;
    // The corners of P as (y, z).
    let at = |t: f64, zz: f64| ((t - zz * c) / s, zz);
    let p = [at(ab.0, z.0), at(ab.1, z.0), at(ab.1, z.1), at(ab.0, z.1)];
    let (y0, y1) = p
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| {
            (a.min(q.0), b.max(q.0))
        });
    let (bmin, bmax) = r.bbox();
    if bmax.y() < y0 || bmin.y() > y1 {
        return Vec::new();
    }
    let cut = r.intersect(&Region2::rect(
        bmin.x() - 1.0,
        y0.max(bmin.y() - 1.0),
        bmax.x() + 1.0,
        y1.min(bmax.y() + 1.0),
    ));
    outer_boxes(&cut)
        .into_iter()
        .filter_map(|(a, b)| {
            let q = clip_y(&p, a.y(), b.y());
            let q = if q.is_empty() { p.to_vec() } else { q };
            let (u0, u1) = q
                .iter()
                .map(|(y, zz)| y * c - zz * s)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(u, v), w| {
                    (u.min(w), v.max(w))
                });
            (b.x() > a.x() && u1 > u0)
                .then(|| Region2::rect(a.x() - GROW, u0 - GROW, b.x() + GROW, u1 + GROW))
        })
        .collect()
}

/// The convex polygon `p` (points `(y, z)`) cut to `y0 <= y <= y1`.
fn clip_y(p: &[(f64, f64)], y0: f64, y1: f64) -> Vec<(f64, f64)> {
    fn half(pts: &[(f64, f64)], keep: impl Fn(f64) -> bool, edge: f64) -> Vec<(f64, f64)> {
        let n = pts.len();
        let mut out = Vec::new();
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            if keep(a.0) {
                out.push(a);
            }
            if keep(a.0) != keep(b.0) {
                let t = (edge - a.0) / (b.0 - a.0);
                out.push((edge, a.1 + t * (b.1 - a.1)));
            }
        }
        out
    }
    let q = half(p, |y| y >= y0, y0);
    half(&q, |y| y <= y1, y1)
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
