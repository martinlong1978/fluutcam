// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rigid and affine transforms (D02 §1).

use crate::types::{Point3, Vec3};
use glam::{DMat3, DQuat, DVec3};

/// Rotation followed by translation.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Rigid {
    rot: DQuat,
    trans: DVec3,
}

impl Rigid {
    pub fn identity() -> Rigid {
        Rigid {
            rot: DQuat::IDENTITY,
            trans: DVec3::ZERO,
        }
    }

    /// `rot` is normalized here.
    pub fn from_rotation_translation(rot: DQuat, trans: Vec3) -> Rigid {
        Rigid {
            rot: rot.normalize(),
            trans: trans.0,
        }
    }

    pub fn rotation(&self) -> DQuat {
        self.rot
    }

    pub fn translation(&self) -> Vec3 {
        Vec3(self.trans)
    }

    pub fn apply_point(&self, p: Point3) -> Point3 {
        Point3(self.rot * p.0 + self.trans)
    }

    pub fn apply_vec(&self, v: Vec3) -> Vec3 {
        Vec3(self.rot * v.0)
    }

    /// The transform that applies `self` first, then `next`.
    pub fn then(&self, next: &Rigid) -> Rigid {
        Rigid {
            rot: (next.rot * self.rot).normalize(),
            trans: next.rot * self.trans + next.trans,
        }
    }

    pub fn inverse(&self) -> Rigid {
        let inv = self.rot.inverse();
        Rigid {
            rot: inv,
            trans: -(inv * self.trans),
        }
    }
}

/// Linear map (rotation, scale, mirror) followed by translation.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Affine {
    m: DMat3,
    trans: DVec3,
}

impl Affine {
    pub fn identity() -> Affine {
        Affine {
            m: DMat3::IDENTITY,
            trans: DVec3::ZERO,
        }
    }

    pub fn from_rigid(r: &Rigid) -> Affine {
        Affine {
            m: DMat3::from_quat(r.rot),
            trans: r.trans,
        }
    }

    pub fn from_mat3_translation(m: DMat3, trans: Vec3) -> Affine {
        Affine { m, trans: trans.0 }
    }

    /// Scale about the origin. Negative factors mirror.
    pub fn scale(sx: f64, sy: f64, sz: f64) -> Affine {
        Affine {
            m: DMat3::from_diagonal(DVec3::new(sx, sy, sz)),
            trans: DVec3::ZERO,
        }
    }

    pub fn translation(t: Vec3) -> Affine {
        Affine {
            m: DMat3::IDENTITY,
            trans: t.0,
        }
    }

    pub fn matrix(&self) -> DMat3 {
        self.m
    }

    pub fn apply_point(&self, p: Point3) -> Point3 {
        Point3(self.m * p.0 + self.trans)
    }

    pub fn apply_vec(&self, v: Vec3) -> Vec3 {
        Vec3(self.m * v.0)
    }

    /// The transform that applies `self` first, then `next`.
    pub fn then(&self, next: &Affine) -> Affine {
        Affine {
            m: next.m * self.m,
            trans: next.m * self.trans + next.trans,
        }
    }

    pub fn determinant(&self) -> f64 {
        self.m.determinant()
    }

    /// True for mirrors (determinant < 0). Applying such a transform to a face
    /// flips its sense (D03 §2).
    pub fn reverses_orientation(&self) -> bool {
        self.determinant() < 0.0
    }

    /// If the linear part is a similarity (rotation × uniform scale, possibly
    /// mirrored), returns the scale factor `s > 0`. `rel_tol` bounds the relative
    /// deviation of `MᵀM` from `s²·I`.
    pub fn uniform_scale(&self, rel_tol: f64) -> Option<f64> {
        let mtm = self.m.transpose() * self.m;
        let s2 = (mtm.x_axis.x + mtm.y_axis.y + mtm.z_axis.z) / 3.0;
        if !(s2.is_finite() && s2 > 0.0) {
            return None;
        }
        let diff = mtm - DMat3::from_diagonal(DVec3::splat(s2));
        let max = diff
            .to_cols_array()
            .iter()
            .fold(0.0f64, |a, &b| a.max(b.abs()));
        (max <= rel_tol * s2).then(|| s2.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn some_rigid() -> Rigid {
        Rigid::from_rotation_translation(
            DQuat::from_axis_angle(DVec3::new(1.0, 2.0, 3.0).normalize(), 0.7),
            Vec3::new(4.0, -5.0, 6.0),
        )
    }

    #[test]
    fn rigid_then_inverse_is_identity() {
        let r = some_rigid();
        let id = r.then(&r.inverse());
        let p = Point3::new(1.0, 2.0, 3.0);
        assert!(id.apply_point(p).distance(p) < 1e-12);
    }

    #[test]
    fn affine_from_rigid_matches_rigid() {
        let r = some_rigid();
        let a = Affine::from_rigid(&r);
        let p = Point3::new(-3.0, 0.5, 9.0);
        assert!(a.apply_point(p).distance(r.apply_point(p)) < 1e-12);
        assert!(!a.reverses_orientation());
        assert!((a.uniform_scale(1e-12).unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn affine_mirror_reverses() {
        assert!(Affine::scale(-1.0, 1.0, 1.0).reverses_orientation());
        assert!(!Affine::scale(2.0, 2.0, 2.0).reverses_orientation());
    }

    #[test]
    fn non_uniform_scale_has_no_uniform_factor() {
        assert!(Affine::scale(1.0, 2.0, 1.0).uniform_scale(1e-12).is_none());
        assert_eq!(
            Affine::scale(-2.0, 2.0, 2.0).uniform_scale(1e-12),
            Some(2.0)
        );
    }

    #[test]
    fn affine_then_order() {
        // Scale by 2, then translate by +1 in x: (1,0,0) -> (3,0,0).
        let a = Affine::scale(2.0, 2.0, 2.0).then(&Affine::translation(Vec3::new(1.0, 0.0, 0.0)));
        assert_eq!(
            a.apply_point(Point3::new(1.0, 0.0, 0.0)),
            Point3::new(3.0, 0.0, 0.0)
        );
    }

    proptest! {
        #[test]
        fn rigid_preserves_distance(
            ax in -1.0f64..1.0, ay in -1.0f64..1.0, az in 0.1f64..1.0, angle in -6.3f64..6.3,
            tx in -100.0f64..100.0,
            p in prop::array::uniform3(-100.0f64..100.0),
            q in prop::array::uniform3(-100.0f64..100.0),
        ) {
            let r = Rigid::from_rotation_translation(
                DQuat::from_axis_angle(DVec3::new(ax, ay, az).normalize(), angle),
                Vec3::new(tx, 0.0, 0.0),
            );
            let p = Point3(DVec3::from_array(p));
            let q = Point3(DVec3::from_array(q));
            let d0 = p.distance(q);
            let d1 = r.apply_point(p).distance(r.apply_point(q));
            prop_assert!((d0 - d1).abs() < 1e-10);
        }
    }
}
