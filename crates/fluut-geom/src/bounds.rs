// SPDX-License-Identifier: MIT OR Apache-2.0

//! Axis-aligned boxes and parameter intervals (D02 §1).

use crate::types::{Point3, Vec3};
use glam::DVec3;

/// Axis-aligned bounding box. Empty when `min > max` on any axis.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Aabb3 {
    pub min: Point3,
    pub max: Point3,
}

impl Aabb3 {
    pub fn empty() -> Aabb3 {
        Aabb3 {
            min: Point3(DVec3::splat(f64::INFINITY)),
            max: Point3(DVec3::splat(f64::NEG_INFINITY)),
        }
    }

    pub fn from_points(points: &[Point3]) -> Aabb3 {
        points.iter().fold(Aabb3::empty(), |b, &p| b.with_point(p))
    }

    pub fn is_empty(&self) -> bool {
        self.min.x() > self.max.x() || self.min.y() > self.max.y() || self.min.z() > self.max.z()
    }

    pub fn with_point(&self, p: Point3) -> Aabb3 {
        Aabb3 {
            min: Point3(self.min.0.min(p.0)),
            max: Point3(self.max.0.max(p.0)),
        }
    }

    pub fn union(&self, other: &Aabb3) -> Aabb3 {
        Aabb3 {
            min: Point3(self.min.0.min(other.min.0)),
            max: Point3(self.max.0.max(other.max.0)),
        }
    }

    /// Grows the box by `d` on every side. An empty box stays empty.
    pub fn expand(&self, d: f64) -> Aabb3 {
        if self.is_empty() {
            return *self;
        }
        Aabb3 {
            min: self.min - Vec3(DVec3::splat(d)),
            max: self.max + Vec3(DVec3::splat(d)),
        }
    }

    pub fn contains(&self, p: Point3) -> bool {
        p.0.cmpge(self.min.0).all() && p.0.cmple(self.max.0).all()
    }

    /// Distance from `p` to the box (0 inside). Infinite for an empty box.
    pub fn distance(&self, p: Point3) -> f64 {
        if self.is_empty() {
            return f64::INFINITY;
        }
        let d = (self.min.0 - p.0).max(p.0 - self.max.0).max(DVec3::ZERO);
        d.length()
    }
}

/// Closed parameter interval `[lo, hi]`.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Interval {
    pub lo: f64,
    pub hi: f64,
}

impl Interval {
    /// Returns `None` if `lo > hi` or either end is NaN.
    pub fn new(lo: f64, hi: f64) -> Option<Interval> {
        (lo <= hi).then_some(Interval { lo, hi })
    }

    pub fn length(&self) -> f64 {
        self.hi - self.lo
    }

    pub fn contains(&self, t: f64) -> bool {
        self.lo <= t && t <= self.hi
    }

    pub fn clamp(&self, t: f64) -> f64 {
        t.clamp(self.lo, self.hi)
    }

    /// The parameter at fraction `f` of the interval.
    pub fn at(&self, f: f64) -> f64 {
        self.lo + (self.hi - self.lo) * f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aabb_union_contains() {
        let a = Aabb3::from_points(&[Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0)]);
        let b = Aabb3::from_points(&[Point3::new(2.0, -1.0, 0.5)]);
        let u = a.union(&b);
        assert!(u.contains(Point3::new(1.5, -0.5, 0.7)));
        assert!(!u.contains(Point3::new(1.5, -0.5, 1.7)));
    }

    #[test]
    fn empty_aabb() {
        let e = Aabb3::empty();
        assert!(e.is_empty());
        assert!(e.expand(1.0).is_empty());
        assert!(!e.contains(Point3::ORIGIN));
    }

    #[test]
    fn interval_rejects_inverted() {
        assert!(Interval::new(1.0, 0.0).is_none());
        assert!(Interval::new(f64::NAN, 0.0).is_none());
        assert!(Interval::new(0.0, 0.0).is_some());
    }
}
