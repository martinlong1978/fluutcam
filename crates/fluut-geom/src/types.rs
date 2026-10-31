// SPDX-License-Identifier: MIT OR Apache-2.0

//! Points and vectors (D02 §1).
//!
//! Points and vectors are distinct types so that transforms apply translation to
//! points only.

use glam::{DVec2, DVec3};
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// A position in 3-D space, in millimetres.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Point3(pub DVec3);

/// A displacement or direction in 3-D space.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Vec3(pub DVec3);

/// A position in 2-D space (UV parameter space or planar wire geometry).
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Point2(pub DVec2);

/// A displacement or direction in 2-D space.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Vec2(pub DVec2);

/// A vector of length 1 (within 1e-12).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct UnitVec3(Vec3);

impl Point3 {
    pub const ORIGIN: Point3 = Point3(DVec3::ZERO);

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Point3(DVec3::new(x, y, z))
    }
    pub fn x(self) -> f64 {
        self.0.x
    }
    pub fn y(self) -> f64 {
        self.0.y
    }
    pub fn z(self) -> f64 {
        self.0.z
    }
    pub fn distance(self, other: Point3) -> f64 {
        self.0.distance(other.0)
    }
    /// The vector from the origin to this point.
    pub fn to_vec(self) -> Vec3 {
        Vec3(self.0)
    }
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }
    /// The point `t` of the way from `self` to `other`.
    pub fn lerp(self, other: Point3, t: f64) -> Point3 {
        Point3(self.0.lerp(other.0, t))
    }
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3(DVec3::ZERO);

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3(DVec3::new(x, y, z))
    }
    pub fn x(self) -> f64 {
        self.0.x
    }
    pub fn y(self) -> f64 {
        self.0.y
    }
    pub fn z(self) -> f64 {
        self.0.z
    }
    pub fn dot(self, other: Vec3) -> f64 {
        self.0.dot(other.0)
    }
    pub fn cross(self, other: Vec3) -> Vec3 {
        Vec3(self.0.cross(other.0))
    }
    pub fn length(self) -> f64 {
        self.0.length()
    }
    pub fn length_squared(self) -> f64 {
        self.0.length_squared()
    }
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }
    /// The unit vector in this direction, or `None` for a zero or non-finite vector.
    pub fn normalized(self) -> Option<UnitVec3> {
        UnitVec3::new(self)
    }
}

impl UnitVec3 {
    pub const X: UnitVec3 = UnitVec3(Vec3::new(1.0, 0.0, 0.0));
    pub const Y: UnitVec3 = UnitVec3(Vec3::new(0.0, 1.0, 0.0));
    pub const Z: UnitVec3 = UnitVec3(Vec3::new(0.0, 0.0, 1.0));

    /// Normalizes `v`. Returns `None` if `v` is zero, too small to normalize, or not finite.
    pub fn new(v: Vec3) -> Option<UnitVec3> {
        let len = v.length();
        if !len.is_finite() || len < 1e-300 {
            return None;
        }
        Some(UnitVec3(Vec3(v.0 / len)))
    }
    pub fn get(self) -> Vec3 {
        self.0
    }
    /// Wraps a vector the caller knows to be unit length (e.g. the cross
    /// product of two orthonormal vectors). Not checked.
    pub(crate) fn from_unit_unchecked(v: Vec3) -> UnitVec3 {
        UnitVec3(v)
    }
    pub fn dot(self, other: Vec3) -> f64 {
        self.0.dot(other)
    }
}

impl Neg for UnitVec3 {
    type Output = UnitVec3;
    fn neg(self) -> UnitVec3 {
        UnitVec3(-self.0)
    }
}

impl Point2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Point2(DVec2::new(x, y))
    }
    pub fn x(self) -> f64 {
        self.0.x
    }
    pub fn y(self) -> f64 {
        self.0.y
    }
    pub fn distance(self, other: Point2) -> f64 {
        self.0.distance(other.0)
    }
}

impl Vec2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Vec2(DVec2::new(x, y))
    }
    pub fn x(self) -> f64 {
        self.0.x
    }
    pub fn y(self) -> f64 {
        self.0.y
    }
    pub fn dot(self, other: Vec2) -> f64 {
        self.0.dot(other.0)
    }
    /// The z component of the 3-D cross product.
    pub fn perp_dot(self, other: Vec2) -> f64 {
        self.0.perp_dot(other.0)
    }
    pub fn length(self) -> f64 {
        self.0.length()
    }
}

// --- operators -------------------------------------------------------------

impl Sub for Point3 {
    type Output = Vec3;
    fn sub(self, rhs: Point3) -> Vec3 {
        Vec3(self.0 - rhs.0)
    }
}
impl Add<Vec3> for Point3 {
    type Output = Point3;
    fn add(self, rhs: Vec3) -> Point3 {
        Point3(self.0 + rhs.0)
    }
}
impl Sub<Vec3> for Point3 {
    type Output = Point3;
    fn sub(self, rhs: Vec3) -> Point3 {
        Point3(self.0 - rhs.0)
    }
}
impl AddAssign<Vec3> for Point3 {
    fn add_assign(&mut self, rhs: Vec3) {
        self.0 += rhs.0;
    }
}
impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, rhs: Vec3) -> Vec3 {
        Vec3(self.0 + rhs.0)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, rhs: Vec3) -> Vec3 {
        Vec3(self.0 - rhs.0)
    }
}
impl AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Vec3) {
        self.0 += rhs.0;
    }
}
impl SubAssign for Vec3 {
    fn sub_assign(&mut self, rhs: Vec3) {
        self.0 -= rhs.0;
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3(-self.0)
    }
}
impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, rhs: f64) -> Vec3 {
        Vec3(self.0 * rhs)
    }
}
impl Mul<Vec3> for f64 {
    type Output = Vec3;
    fn mul(self, rhs: Vec3) -> Vec3 {
        Vec3(self * rhs.0)
    }
}
impl Div<f64> for Vec3 {
    type Output = Vec3;
    fn div(self, rhs: f64) -> Vec3 {
        Vec3(self.0 / rhs)
    }
}
impl Sub for Point2 {
    type Output = Vec2;
    fn sub(self, rhs: Point2) -> Vec2 {
        Vec2(self.0 - rhs.0)
    }
}
impl Add<Vec2> for Point2 {
    type Output = Point2;
    fn add(self, rhs: Vec2) -> Point2 {
        Point2(self.0 + rhs.0)
    }
}
impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, rhs: Vec2) -> Vec2 {
        Vec2(self.0 + rhs.0)
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, rhs: Vec2) -> Vec2 {
        Vec2(self.0 - rhs.0)
    }
}
impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, rhs: f64) -> Vec2 {
        Vec2(self.0 * rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_vec_rejects_zero() {
        assert!(UnitVec3::new(Vec3::new(0.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn unit_vec_rejects_non_finite() {
        assert!(UnitVec3::new(Vec3::new(f64::NAN, 0.0, 0.0)).is_none());
        assert!(UnitVec3::new(Vec3::new(f64::INFINITY, 0.0, 0.0)).is_none());
    }

    #[test]
    fn unit_vec_has_length_one() {
        let u = UnitVec3::new(Vec3::new(3.0, 4.0, 12.0)).unwrap();
        assert!((u.get().length() - 1.0).abs() < 1e-15);
    }

    #[test]
    fn point_minus_point_is_vector() {
        let v = Point3::new(1.0, 2.0, 3.0) - Point3::new(1.0, 1.0, 1.0);
        assert_eq!(v, Vec3::new(0.0, 1.0, 2.0));
        assert_eq!(Point3::new(1.0, 1.0, 1.0) + v, Point3::new(1.0, 2.0, 3.0));
    }
}
