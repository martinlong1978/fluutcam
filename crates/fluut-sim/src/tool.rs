// SPDX-License-Identifier: MIT OR Apache-2.0

//! Cutting tools and tool poses.
//!
//! A tool is the convex solid `disk(R − r) ⊕ axis segment ⊕ ball(r)`: a flat
//! end mill for `r = 0`, a ball end mill for `r = R`, a bull nose between. A
//! cone tool (V bit, drill point) is instead the hull of a tip disc and the
//! shank. The shank is straight and is made long enough to leave the stock,
//! so its top never touches material.

use glam::DVec3;
use std::f64::consts::FRAC_PI_2;

/// A cutter of radius `radius` with corner radius `corner`, or a cone.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Tool {
    pub radius: f64,
    pub corner: f64,
    /// Cone tools: the tip radius and the tangent of the half angle; the
    /// radius at height `h` is `min(R, tip + h·tan)`. `corner` is 0.
    pub cone: Option<(f64, f64)>,
}

impl Tool {
    pub fn flat(radius: f64) -> Tool {
        Tool::bull(radius, 0.0)
    }

    pub fn ball(radius: f64) -> Tool {
        Tool::bull(radius, radius)
    }

    pub fn bull(radius: f64, corner: f64) -> Tool {
        assert!(
            radius > 0.0 && (0.0..=radius).contains(&corner),
            "need 0 ≤ corner ≤ radius and radius > 0"
        );
        Tool {
            radius,
            corner,
            cone: None,
        }
    }

    /// A V bit or drill point: tip radius `tip`, half angle `half` in
    /// radians (0 < half < π/2).
    pub fn cone(radius: f64, tip: f64, half: f64) -> Tool {
        assert!(
            radius > 0.0 && (0.0..radius).contains(&tip) && half > 0.0 && half < FRAC_PI_2,
            "need 0 ≤ tip < radius and 0 < half angle < 90°"
        );
        Tool {
            radius,
            corner: 0.0,
            cone: Some((tip, libm::tan(half))),
        }
    }

    /// Height of the shaped bottom: the corner radius, or the cone's height.
    pub fn profile_height(&self) -> f64 {
        match self.cone {
            Some((tip, tan)) => (self.radius - tip) / tan,
            None => self.corner,
        }
    }

    /// Radius of the flat part of the bottom, `R − r`.
    pub fn core(&self) -> f64 {
        self.radius - self.corner
    }

    pub fn is_ball(&self) -> bool {
        self.cone.is_none() && self.corner == self.radius
    }

    /// Height of the bottom profile above the tip at radial distance `rho`;
    /// `None` outside the tool.
    pub fn bottom(&self, rho: f64) -> Option<f64> {
        if rho > self.radius {
            return None;
        }
        if let Some((tip, tan)) = self.cone {
            return Some(((rho - tip) / tan).max(0.0));
        }
        let e = rho - self.core();
        if e <= 0.0 {
            return Some(0.0);
        }
        let r = self.corner;
        Some(r - (r * r - e * e).max(0.0).sqrt())
    }
}

/// A tool position: tip, unit axis (from the tip toward the shank) and the
/// length of the straight part, measured from the tip.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Pose {
    pub tip: DVec3,
    pub axis: DVec3,
    pub length: f64,
}

impl Pose {
    /// The point of the tool farthest in direction `n`.
    pub fn support(&self, tool: &Tool, n: DVec3) -> DVec3 {
        let w = self.axis;
        let nh = n.dot(w);
        let np = n - w * nh;
        if let Some((tip, _)) = tool.cone {
            // The hull of the tip disc and the cylinder above the cone.
            let lp = np.length();
            let side = if lp > 0.0 { np / lp } else { DVec3::ZERO };
            let low = self.tip + side * tip;
            let h = if nh > 0.0 {
                self.length
            } else {
                tool.profile_height()
            };
            let high = self.tip + w * h + side * tool.radius;
            return if high.dot(n) > low.dot(n) { high } else { low };
        }
        let r = tool.corner;
        let mut p = self.tip + w * if nh > 0.0 { self.length } else { r };
        let lp = np.length();
        if lp > 0.0 {
            p += np * (tool.core() / lp);
        }
        let ln = n.length();
        if r > 0.0 && ln > 0.0 {
            p += n * (r / ln);
        }
        p
    }

    /// Whether `p` is inside the tool (for tests and probes).
    pub fn contains(&self, tool: &Tool, p: DVec3) -> bool {
        let d = p - self.tip;
        let h = d.dot(self.axis);
        let rho = (d - self.axis * h).length();
        if let Some((tip, tan)) = tool.cone {
            return (0.0..=self.length).contains(&h) && rho <= tool.radius.min(tip + h * tan);
        }
        let r = tool.corner;
        let e = (rho - tool.core()).max(0.0);
        let g = (r - h).max(h - self.length).max(0.0);
        e * e + g * g <= r * r
    }

    /// Axis-aligned box around the tool.
    pub fn aabb(&self, tool: &Tool) -> (DVec3, DVec3) {
        let top = self.tip + self.axis * (self.length + tool.corner);
        let w2 = self.axis * self.axis;
        let ext = (DVec3::ONE - w2).max(DVec3::ZERO);
        let ext = DVec3::new(ext.x.sqrt(), ext.y.sqrt(), ext.z.sqrt()) * tool.radius;
        (self.tip.min(top) - ext, self.tip.max(top) + ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_profiles() {
        assert_eq!(Tool::flat(3.0).bottom(2.9), Some(0.0));
        assert_eq!(Tool::flat(3.0).bottom(3.1), None);
        let b = Tool::ball(2.0).bottom(1.0).unwrap();
        assert!((b - (2.0 - 3f64.sqrt())).abs() < 1e-15);
        let v = Tool::cone(3.0, 0.5, std::f64::consts::FRAC_PI_4);
        assert_eq!(v.bottom(0.2), Some(0.0));
        assert!((v.bottom(2.0).unwrap() - 1.5).abs() < 1e-12);
        assert!((v.profile_height() - 2.5).abs() < 1e-12);
        let bull = Tool::bull(5.0, 1.0);
        assert_eq!(bull.bottom(4.0), Some(0.0));
        assert!((bull.bottom(5.0).unwrap() - 1.0).abs() < 1e-15);
    }

    #[test]
    fn support_and_contains_agree() {
        let pose = Pose {
            tip: DVec3::new(1.0, 2.0, 3.0),
            axis: DVec3::new(0.0, 0.6, 0.8),
            length: 10.0,
        };
        for tool in [
            Tool::flat(2.0),
            Tool::ball(2.0),
            Tool::bull(2.0, 0.5),
            Tool::cone(2.0, 0.1, 0.5),
        ] {
            for k in 0..50 {
                let a = f64::from(k) * 0.7;
                let n = DVec3::new(libm::cos(a), libm::sin(1.3 * a), libm::cos(0.4 * a) - 0.3);
                let s = pose.support(&tool, n);
                assert!(
                    pose.contains(&tool, s - n.normalize() * 1e-9),
                    "{tool:?} {n}"
                );
                assert!(
                    !pose.contains(&tool, s + n.normalize() * 1e-6),
                    "{tool:?} {n}"
                );
                let (lo, hi) = pose.aabb(&tool);
                assert!(s.cmpge(lo - 1e-9).all() && s.cmple(hi + 1e-9).all());
            }
        }
    }
}
