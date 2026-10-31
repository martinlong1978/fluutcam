// SPDX-License-Identifier: MIT OR Apache-2.0

//! 2-D regions bounded by lines and arcs (design D08 §3).
//!
//! A [`Region2`] is a set of closed loops: outer loops counter-clockwise,
//! holes clockwise. Each loop is a list of vertices in the bulge form of
//! `cavalier_contours`: the segment from vertex `i` to vertex `i + 1` is a
//! line when the bulge is 0, else an arc with `bulge = tan(sweep / 4)`,
//! positive counter-clockwise.

use cavalier_contours::polyline::{PlineCreation, PlineSource, PlineSourceMut, Polyline};
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};
use fluut_geom::{Point2, Vec2};
use std::f64::consts::{PI, TAU};

/// Points closer than this to the boundary are [`Inside::On`] it, and
/// vertices closer than this are merged.
pub const EPS: f64 = 1e-9;

/// One vertex of a loop and the bulge of the segment that starts there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub p: Point2,
    pub bulge: f64,
}

impl Vertex {
    pub const fn new(x: f64, y: f64, bulge: f64) -> Self {
        Vertex {
            p: Point2::new(x, y),
            bulge,
        }
    }
}

/// A line (bulge 0) or arc segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seg2 {
    pub a: Point2,
    pub b: Point2,
    pub bulge: f64,
}

/// Centre, radius, start angle and signed sweep of an arc segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcGeom {
    pub centre: Point2,
    pub radius: f64,
    pub start: f64,
    pub sweep: f64,
}

impl Seg2 {
    pub fn is_line(&self) -> bool {
        self.bulge == 0.0
    }

    /// Arc geometry, or `None` for a line.
    pub fn arc(&self) -> Option<ArcGeom> {
        if self.is_line() {
            return None;
        }
        let b = self.bulge;
        let d = self.b - self.a;
        let half = d * 0.5;
        let m = self.a + half;
        // Left normal of the chord, scaled so that the centre lies at
        // m + n · (1 − b²) / (2b).
        let n = Vec2::new(-half.y(), half.x());
        let centre = m + n * ((1.0 - b * b) / (2.0 * b));
        let radius = d.length() * (1.0 + b * b) / (4.0 * b.abs());
        let start = libm::atan2(self.a.y() - centre.y(), self.a.x() - centre.x());
        Some(ArcGeom {
            centre,
            radius,
            start,
            sweep: 4.0 * libm::atan(b),
        })
    }

    /// Length along the segment.
    pub fn length(&self) -> f64 {
        match self.arc() {
            None => self.a.distance(self.b),
            Some(g) => g.radius * g.sweep.abs(),
        }
    }

    /// Point at parameter `t` in [0, 1] (proportional to length).
    pub fn point_at(&self, t: f64) -> Point2 {
        match self.arc() {
            None => self.a + (self.b - self.a) * t,
            Some(g) => g.point(g.start + g.sweep * t),
        }
    }

    /// The signed area between the segment and the origin, so that the sum
    /// over a closed loop is the loop's signed area.
    fn signed_area(&self) -> f64 {
        let chord = 0.5 * (self.a.x() * self.b.y() - self.b.x() * self.a.y());
        match self.arc() {
            None => chord,
            Some(g) => {
                let s = g.sweep;
                chord + 0.5 * g.radius * g.radius * (s - libm::sin(s))
            }
        }
    }

    /// Minimum and maximum of `p · dir` over the segment.
    fn support(&self, dir: Vec2) -> (f64, f64) {
        let pa = self.a.0.dot(dir.0);
        let pb = self.b.0.dot(dir.0);
        let (mut lo, mut hi) = (pa.min(pb), pa.max(pb));
        if let Some(g) = self.arc() {
            let len = dir.length();
            let phi = libm::atan2(dir.y(), dir.x());
            let c = g.centre.0.dot(dir.0);
            if g.contains_angle(phi) {
                hi = hi.max(c + g.radius * len);
            }
            if g.contains_angle(phi + PI) {
                lo = lo.min(c - g.radius * len);
            }
        }
        (lo, hi)
    }
}

impl ArcGeom {
    pub fn point(&self, angle: f64) -> Point2 {
        let (s, c) = libm::sincos(angle);
        Point2::new(
            self.centre.x() + self.radius * c,
            self.centre.y() + self.radius * s,
        )
    }

    /// Whether the direction `angle` from the centre lies on the arc.
    pub fn contains_angle(&self, angle: f64) -> bool {
        let d = (angle - self.start).rem_euclid(TAU);
        if self.sweep >= 0.0 {
            d <= self.sweep
        } else {
            d == 0.0 || d >= TAU + self.sweep
        }
    }
}

/// A closed loop of segments.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Loop2 {
    pub verts: Vec<Vertex>,
}

impl Loop2 {
    pub fn segs(&self) -> impl Iterator<Item = Seg2> + '_ {
        let n = self.verts.len();
        (0..n).map(move |i| Seg2 {
            a: self.verts[i].p,
            b: self.verts[(i + 1) % n].p,
            bulge: self.verts[i].bulge,
        })
    }

    /// Signed area: positive for counter-clockwise loops.
    pub fn area(&self) -> f64 {
        self.segs().map(|s| s.signed_area()).sum()
    }

    pub fn is_ccw(&self) -> bool {
        self.area() > 0.0
    }

    /// The same loop traversed the other way.
    pub fn reversed(&self) -> Loop2 {
        let n = self.verts.len();
        let verts = (0..n)
            .map(|i| {
                // Vertex i of the result starts the segment that was
                // segment (n − 1 − i) of the original, reversed.
                let j = (2 * n - 1 - i) % n;
                Vertex {
                    p: self.verts[(j + 1) % n].p,
                    bulge: -self.verts[j].bulge,
                }
            })
            .collect();
        Loop2 { verts }
    }

    pub fn length(&self) -> f64 {
        self.segs().map(|s| s.length()).sum()
    }

    pub(crate) fn to_pline(&self) -> Polyline<f64> {
        let mut pl = Polyline::with_capacity(self.verts.len(), true);
        for v in &self.verts {
            pl.add(v.p.x(), v.p.y(), v.bulge);
        }
        pl
    }

    pub(crate) fn from_pline<P: PlineSource<Num = f64>>(pl: &P) -> Loop2 {
        let verts = pl
            .iter_vertexes()
            .map(|v| Vertex::new(v.x, v.y, v.bulge))
            .collect();
        Loop2 { verts }
    }
}

/// Where a point lies relative to a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inside {
    In,
    On,
    Out,
}

/// A 2-D region: outer loops counter-clockwise, holes clockwise.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Region2 {
    pub loops: Vec<Loop2>,
}

impl Region2 {
    pub fn empty() -> Self {
        Region2::default()
    }

    pub fn is_empty(&self) -> bool {
        self.loops.is_empty()
    }

    /// The axis-aligned rectangle with corners `(x0, y0)` and `(x1, y1)`.
    pub fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Region2::polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
    }

    /// A polygon; the points may be in either order.
    pub fn polygon(pts: &[(f64, f64)]) -> Self {
        let l = Loop2 {
            verts: pts.iter().map(|&(x, y)| Vertex::new(x, y, 0.0)).collect(),
        };
        Region2::from_outer(l)
    }

    /// A disc, as two half-circle arcs.
    pub fn circle(cx: f64, cy: f64, r: f64) -> Self {
        Region2::from_outer(Loop2 {
            verts: vec![Vertex::new(cx + r, cy, 1.0), Vertex::new(cx - r, cy, 1.0)],
        })
    }

    fn from_outer(l: Loop2) -> Self {
        let l = if l.is_ccw() { l } else { l.reversed() };
        Region2 { loops: vec![l] }
    }

    /// This region with the outer loops of `hole` added as holes. The hole
    /// must lie inside this region and not touch any loop.
    pub fn with_hole(mut self, hole: &Region2) -> Self {
        for l in &hole.loops {
            let l = if l.is_ccw() { l.reversed() } else { l.clone() };
            self.loops.push(l);
        }
        self
    }

    pub fn area(&self) -> f64 {
        self.loops.iter().map(Loop2::area).sum()
    }

    /// Total length of all loops.
    pub fn perimeter(&self) -> f64 {
        self.loops.iter().map(Loop2::length).sum()
    }

    pub fn segs(&self) -> impl Iterator<Item = Seg2> + '_ {
        self.loops.iter().flat_map(Loop2::segs)
    }

    /// Distance from `p` to the nearest boundary point.
    pub fn boundary_distance(&self, p: Point2) -> f64 {
        self.segs()
            .map(|s| seg_distance(&s, p))
            .fold(f64::INFINITY, f64::min)
    }

    pub fn contains(&self, p: Point2) -> Inside {
        if self.boundary_distance(p) <= EPS {
            return Inside::On;
        }
        let v = cavalier_contours::core::math::Vector2::new(p.x(), p.y());
        let w: i32 = self
            .loops
            .iter()
            .map(|l| l.to_pline().winding_number(v))
            .sum();
        if w > 0 { Inside::In } else { Inside::Out }
    }

    /// Exact minimum and maximum of `p · dir` over the region.
    pub fn support(&self, dir: Vec2) -> (f64, f64) {
        self.segs()
            .map(|s| s.support(dir))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (c, d)| {
                (a.min(c), b.max(d))
            })
    }

    /// The region grown by `d` (shrunk when `d` is negative). Corners that
    /// grow become arcs of radius `d`; loops that shrink to nothing vanish,
    /// and loops that meet are joined.
    pub fn offset(&self, d: f64) -> Region2 {
        if self.is_empty() {
            return Region2::empty();
        }
        if d == 0.0 {
            return self.clone();
        }
        let shape = Shape::from_plines(self.loops.iter().map(Loop2::to_pline));
        // cavalier_contours offsets to the left of the loop direction, which
        // is inward for counter-clockwise loops.
        let out = shape.parallel_offset(-d, &ShapeOffsetOptions::new());
        let loops = out
            .ccw_plines
            .iter()
            .chain(&out.cw_plines)
            .map(|ip| Loop2::from_pline(&ip.polyline))
            .filter(|l| l.verts.len() >= 2 && l.area().abs() > EPS)
            .collect();
        Region2 { loops }
    }

    /// `(min, max)` corners of the bounding box.
    pub fn bbox(&self) -> (Point2, Point2) {
        let (x0, x1) = self.support(Vec2::new(1.0, 0.0));
        let (y0, y1) = self.support(Vec2::new(0.0, 1.0));
        (Point2::new(x0, y0), Point2::new(x1, y1))
    }
}

/// Distance from `p` to a segment.
pub fn seg_distance(s: &Seg2, p: Point2) -> f64 {
    match s.arc() {
        None => {
            let d = s.b - s.a;
            let l2 = d.dot(d);
            let t = if l2 > 0.0 {
                ((p - s.a).dot(d) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            p.distance(s.a + d * t)
        }
        Some(g) => {
            let ang = libm::atan2(p.y() - g.centre.y(), p.x() - g.centre.x());
            if g.contains_angle(ang) {
                (p.distance(g.centre) - g.radius).abs()
            } else {
                p.distance(s.a).min(p.distance(s.b))
            }
        }
    }
}

#[cfg(test)]
mod tests;
