// SPDX-License-Identifier: MIT OR Apache-2.0

//! Right-handed orthonormal frames (STEP `AXIS2_PLACEMENT_3D`, D02 §1).

use crate::types::{Point3, UnitVec3, Vec3};

/// A right-handed orthonormal frame. `y = z × x`.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Frame {
    pub origin: Point3,
    /// Axis direction.
    pub z: UnitVec3,
    /// Reference direction, perpendicular to `z`.
    pub x: UnitVec3,
}

impl Frame {
    /// Builds a frame from an axis and a reference direction, as STEP does: the
    /// reference direction is projected onto the plane normal to `z`.
    ///
    /// Returns `None` if `z` is zero or `x_ref` is parallel to `z` (relative 1e-12).
    pub fn new(origin: Point3, z: Vec3, x_ref: Vec3) -> Option<Frame> {
        let z = UnitVec3::new(z)?;
        let along = x_ref.dot(z.get());
        let x_perp = x_ref - z.get() * along;
        if x_perp.length() <= 1e-12 * x_ref.length() {
            return None;
        }
        let x = UnitVec3::new(x_perp)?;
        Some(Frame { origin, z, x })
    }

    /// The world XY frame at the origin.
    pub fn world_xy() -> Frame {
        Frame {
            origin: Point3::ORIGIN,
            z: UnitVec3::Z,
            x: UnitVec3::X,
        }
    }

    /// The third axis, `z × x`.
    pub fn y(&self) -> UnitVec3 {
        // z and x are orthonormal, so the cross product has unit length.
        UnitVec3::from_unit_unchecked(self.z.get().cross(self.x.get()))
    }

    /// Converts local coordinates (along the frame axes) to world.
    pub fn to_world(&self, local: Point3) -> Point3 {
        self.origin + self.vec_to_world(local.to_vec())
    }

    /// Converts a local vector to world.
    pub fn vec_to_world(&self, local: Vec3) -> Vec3 {
        self.x.get() * local.x() + self.y().get() * local.y() + self.z.get() * local.z()
    }

    /// Converts world coordinates to local.
    pub fn to_local(&self, world: Point3) -> Point3 {
        let d = world - self.origin;
        Point3::new(
            d.dot(self.x.get()),
            d.dot(self.y().get()),
            d.dot(self.z.get()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_orthonormal() {
        let f = Frame::new(
            Point3::ORIGIN,
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(1.0, 1.0, 0.0),
        )
        .unwrap();
        for v in [f.x.get(), f.y().get(), f.z.get()] {
            assert!((v.length() - 1.0).abs() < 1e-15);
        }
        assert!(f.x.dot(f.y().get()).abs() < 1e-15);
        assert!(f.x.dot(f.z.get()).abs() < 1e-15);
        assert!(f.y().dot(f.z.get()).abs() < 1e-15);
    }

    #[test]
    fn frame_projects_reference_direction() {
        let f = Frame::new(
            Point3::ORIGIN,
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 5.0),
        )
        .unwrap();
        assert_eq!(f.x, UnitVec3::X);
    }

    #[test]
    fn frame_rejects_parallel() {
        let o = Point3::ORIGIN;
        assert!(Frame::new(o, Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, -3.0)).is_none());
        assert!(Frame::new(o, Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn frame_round_trip() {
        let f = Frame::new(
            Point3::new(5.0, -2.0, 1.0),
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(-1.0, 0.5, 0.0),
        )
        .unwrap();
        let p = Point3::new(3.0, 7.0, -11.0);
        let q = f.to_local(f.to_world(p));
        assert!(p.distance(q) < 1e-12);
    }
}
