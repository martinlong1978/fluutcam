// SPDX-License-Identifier: MIT OR Apache-2.0

//! Initial stock shapes, in the stock frame (D11 §2).

use fluut_geom::{Aabb3, Point3};
use glam::DVec3;

/// The raw stock before machining.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Stock {
    /// Axis-aligned box.
    Box { min: Point3, max: Point3 },
    /// Cylinder on the X axis (the rotary axis), from `x0` to `x1`.
    Cylinder { x0: f64, x1: f64, radius: f64 },
}

impl Stock {
    pub fn bbox(&self) -> Aabb3 {
        match *self {
            Stock::Box { min, max } => Aabb3 { min, max },
            Stock::Cylinder { x0, x1, radius } => Aabb3 {
                min: Point3::new(x0, -radius, -radius),
                max: Point3::new(x1, radius, radius),
            },
        }
    }

    /// Largest `(q − p)·dir` over the points `q` of the stock. A tool whose
    /// axis is `dir` and whose tip is at `p` cannot touch the stock above
    /// this height.
    pub fn extent_along(&self, p: DVec3, dir: DVec3) -> f64 {
        match *self {
            Stock::Box { min, max } => {
                let pick = |k: usize| {
                    if dir[k] > 0.0 {
                        max.0[k] * dir[k]
                    } else {
                        min.0[k] * dir[k]
                    }
                };
                pick(0) + pick(1) + pick(2) - p.dot(dir)
            }
            Stock::Cylinder { x0, x1, radius } => {
                let along = (x0 * dir.x).max(x1 * dir.x);
                along + radius * (dir.y * dir.y + dir.z * dir.z).sqrt() - p.dot(dir)
            }
        }
    }

    /// The material span of the line through `p` along axis `k`, as offsets
    /// from the bounding-box minimum along that axis. `None` if the line
    /// misses the stock.
    pub fn chord(&self, p: DVec3, k: usize) -> Option<(f64, f64)> {
        let b = self.bbox();
        let (lo, hi) = (b.min.0, b.max.0);
        let (u, v) = ((k + 1) % 3, (k + 2) % 3);
        match *self {
            Stock::Box { .. } => {
                let inside = |a: usize| p[a] >= lo[a] && p[a] <= hi[a];
                (inside(u) && inside(v) && lo[k] < hi[k]).then_some((0.0, hi[k] - lo[k]))
            }
            Stock::Cylinder { x0, x1, radius } => {
                if k == 0 {
                    let r2 = p.y * p.y + p.z * p.z;
                    (r2 <= radius * radius && x0 < x1).then_some((0.0, x1 - x0))
                } else {
                    // Line along Y or Z: x must lie on the cylinder, the other
                    // transverse coordinate fixes the half chord.
                    let other = if k == 1 { p.z } else { p.y };
                    let h2 = radius * radius - other * other;
                    (p.x >= x0 && p.x <= x1 && h2 > 0.0).then(|| {
                        let h = h2.sqrt();
                        (radius - h, radius + h)
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extent_of_box_and_cylinder() {
        let b = Stock::Box {
            min: Point3::new(0.0, 0.0, 0.0),
            max: Point3::new(10.0, 20.0, 5.0),
        };
        assert_eq!(b.extent_along(DVec3::new(1.0, 1.0, 2.0), DVec3::Z), 3.0);
        assert_eq!(b.extent_along(DVec3::new(1.0, 1.0, 2.0), -DVec3::X), 1.0);
        let c = Stock::Cylinder {
            x0: 0.0,
            x1: 100.0,
            radius: 30.0,
        };
        // Tip at radius 25 under the tool, axis radial.
        assert!((c.extent_along(DVec3::new(50.0, 0.0, 25.0), DVec3::Z) - 5.0).abs() < 1e-12);
        let d = DVec3::new(0.0, 1.0, 1.0).normalize();
        let p = d * 20.0;
        assert!((c.extent_along(p, d) - 10.0).abs() < 1e-12);
    }

    #[test]
    fn chords_of_cylinder() {
        let c = Stock::Cylinder {
            x0: 0.0,
            x1: 100.0,
            radius: 5.0,
        };
        assert_eq!(c.chord(DVec3::new(-1.0, 3.0, 4.0), 0), Some((0.0, 100.0)));
        assert_eq!(c.chord(DVec3::new(0.0, 3.0, 4.1), 0), None);
        let (a, b) = c.chord(DVec3::new(10.0, 0.0, 3.0), 1).unwrap();
        assert!((a - 1.0).abs() < 1e-12 && (b - 9.0).abs() < 1e-12);
        assert_eq!(c.chord(DVec3::new(101.0, 0.0, 0.0), 2), None);
    }
}
