// SPDX-License-Identifier: MIT OR Apache-2.0

//! Lower-profile height function of flat, ball and bull-nose cutters (D07 §1.1).

/// A cutter with a straight side: flat (`corner = 0`), ball (`corner =
/// radius`) or bull-nose. Lengths in mm.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Cutter {
    radius: f64,
    corner: f64,
}

impl Cutter {
    /// `None` unless `0 ≤ corner ≤ diameter / 2` and both are finite.
    pub fn new(diameter: f64, corner: f64) -> Option<Cutter> {
        let radius = diameter / 2.0;
        (radius.is_finite() && radius > 0.0 && corner >= 0.0 && corner <= radius)
            .then_some(Cutter { radius, corner })
    }

    pub fn flat(diameter: f64) -> Option<Cutter> {
        Cutter::new(diameter, 0.0)
    }

    pub fn ball(diameter: f64) -> Option<Cutter> {
        Cutter::new(diameter, diameter / 2.0)
    }

    pub fn radius(&self) -> f64 {
        self.radius
    }

    pub fn corner(&self) -> f64 {
        self.corner
    }

    /// Radius of the flat bottom, `R − r`.
    pub fn flat_radius(&self) -> f64 {
        self.radius - self.corner
    }

    /// Height of the lower surface above the tip at lateral distance `d`;
    /// infinite outside the footprint.
    pub fn h(&self, d: f64) -> f64 {
        let e = d - self.flat_radius();
        if d > self.radius {
            f64::INFINITY
        } else if e <= 0.0 {
            0.0
        } else {
            let r = self.corner;
            r - (r * r - e * e).max(0.0).sqrt()
        }
    }

    /// `h′(d)`; infinite at the rim of a ball or bull-nose, 0 for a flat.
    pub fn dh(&self, d: f64) -> f64 {
        let e = d - self.flat_radius();
        if e <= 0.0 || self.corner == 0.0 {
            0.0
        } else {
            let s2 = self.corner * self.corner - e * e;
            if s2 <= 0.0 {
                f64::INFINITY
            } else {
                e / s2.sqrt()
            }
        }
    }

    /// `h″(d)`, taken from the corner side at `d = R − r`.
    pub fn ddh(&self, d: f64) -> f64 {
        let e = d - self.flat_radius();
        if e < 0.0 || self.corner == 0.0 {
            0.0
        } else {
            let r = self.corner;
            let s2 = r * r - e * e;
            if s2 <= 0.0 {
                f64::INFINITY
            } else {
                r * r / (s2 * s2.sqrt())
            }
        }
    }

    /// `h′(d) / d`, with its limit `h″(0)` at `d = 0`.
    pub fn dh_over_d(&self, d: f64) -> f64 {
        if d <= 0.0 {
            if self.flat_radius() == 0.0 {
                self.ddh(0.0)
            } else {
                0.0
            }
        } else {
            self.dh(d) / d
        }
    }

    /// Lateral distance of the contact with a surface whose normal makes
    /// angle θ with the axis, given `sin θ` (`h′(d) = tan θ`). For θ = 0 the
    /// contact can be anywhere on the flat bottom; this returns its rim.
    pub fn contact_radius(&self, sin_theta: f64) -> f64 {
        self.flat_radius() + self.corner * sin_theta.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_shapes() {
        assert!(Cutter::new(6.0, 3.5).is_none());
        assert!(Cutter::new(-6.0, 0.0).is_none());
        assert!(Cutter::new(f64::NAN, 0.0).is_none());
        assert!(Cutter::new(6.0, -1.0).is_none());
    }

    #[test]
    fn ball_profile() {
        let c = Cutter::ball(6.0).unwrap();
        assert_eq!(c.h(0.0), 0.0);
        assert!((c.h(3.0) - 3.0).abs() < 1e-12);
        // Sphere of radius 3 centred 3 above the tip.
        let d: f64 = 1.7;
        assert!((c.h(d) - (3.0 - (9.0 - d * d).sqrt())).abs() < 1e-12);
        assert_eq!(c.h(3.01), f64::INFINITY);
        assert!((c.dh_over_d(0.0) - 1.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn flat_profile() {
        let c = Cutter::flat(10.0).unwrap();
        for d in [0.0, 2.0, 5.0] {
            assert_eq!(c.h(d), 0.0);
            assert_eq!(c.dh(d), 0.0);
        }
        assert_eq!(c.h(5.0001), f64::INFINITY);
        assert_eq!(c.contact_radius(0.3), 5.0);
    }

    #[test]
    fn bull_profile_continuous() {
        let c = Cutter::new(10.0, 1.0).unwrap();
        assert_eq!(c.flat_radius(), 4.0);
        assert_eq!(c.h(4.0), 0.0);
        assert!(c.h(4.0 + 1e-9) < 1e-12);
        assert!((c.h(5.0) - 1.0).abs() < 1e-12);
        assert!(c.dh(4.0 + 1e-9) < 1e-8);
    }

    #[test]
    fn derivatives_match_differences() {
        for c in [Cutter::ball(6.0).unwrap(), Cutter::new(10.0, 2.0).unwrap()] {
            for d in [0.5, 1.5, 2.5, 3.5, 4.2, 4.8] {
                if d >= c.radius() - 0.05 {
                    continue;
                }
                let e = 1e-6;
                let fd = (c.h(d + e) - c.h(d - e)) / (2.0 * e);
                assert!((fd - c.dh(d)).abs() < 1e-6, "{c:?} {d}");
                let fdd = (c.dh(d + e) - c.dh(d - e)) / (2.0 * e);
                if (d - c.flat_radius()).abs() > 1e-3 {
                    assert!((fdd - c.ddh(d)).abs() < 1e-4, "{c:?} {d}");
                }
            }
        }
    }

    #[test]
    fn contact_radius_inverts_slope() {
        let c = Cutter::new(10.0, 2.0).unwrap();
        for theta in [0.1f64, 0.6, 1.2] {
            let d = c.contact_radius(libm::sin(theta));
            assert!((c.dh(d) - libm::tan(theta)).abs() < 1e-9);
        }
    }
}
