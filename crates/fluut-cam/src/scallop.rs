// SPDX-License-Identifier: MIT OR Apache-2.0

//! Stepover from a scallop height (D09 §4.3, OQ-05) and the curvature of
//! faces it needs.

use crate::cutter::geom::QueryGeometry;
use fluut_geom::Surface;
use fluut_topo::BrepBody;

/// Largest curvatures of a face, seen from its outside (the side its
/// normal points to), in 1/mm. Convex is a dome, concave a bowl.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct FaceCurvature {
    pub convex: f64,
    pub concave: f64,
}

impl FaceCurvature {
    pub fn max(self, o: FaceCurvature) -> FaceCurvature {
        FaceCurvature {
            convex: self.convex.max(o.convex),
            concave: self.concave.max(o.concave),
        }
    }
}

/// Samples per parameter direction for curvature.
const GRID: usize = 9;

/// Curvature of face `f` of `q`: zero for planes, else the extremes of the
/// principal curvatures on a `GRID × GRID` sample of the face's parameter
/// box (samples inside the trim; all of them if none is inside).
pub fn face_curvature(q: &QueryGeometry, f: u32) -> FaceCurvature {
    let face = &q.body.faces[f as usize];
    let s = &q.body.surfaces[face.surface as usize];
    if matches!(s, Surface::Plane { .. }) {
        return FaceCurvature::default();
    }
    let Some(qf) = &q.faces[f as usize] else {
        return FaceCurvature::default();
    };
    let sign = if face.same_sense { 1.0 } else { -1.0 };
    let mut inside = FaceCurvature::default();
    let mut all = FaceCurvature::default();
    let mut any_inside = false;
    for i in 0..GRID {
        for j in 0..GRID {
            let u = qf.uv.u.at(i as f64 / (GRID - 1) as f64);
            let v = qf.uv.v.at(j as f64 / (GRID - 1) as f64);
            let Some(k) = s.curvatures(u, v) else {
                continue;
            };
            // Positive curvature bends towards the normal: concave seen
            // from the side the face normal points to.
            let (k1, k2) = (sign * k.k1, sign * k.k2);
            let c = FaceCurvature {
                convex: (-k1.min(k2)).max(0.0),
                concave: k1.max(k2).max(0.0),
            };
            all = all.max(c);
            if qf.trim.contains(u, v) {
                inside = inside.max(c);
                any_inside = true;
            }
        }
    }
    if any_inside { inside } else { all }
}

/// Whether every face of `b` is a planar triangle: a body read from a mesh.
pub fn is_mesh(b: &BrepBody) -> bool {
    !b.faces.is_empty()
        && b.faces.iter().all(|f| {
            matches!(b.surfaces[f.surface as usize], Surface::Plane { .. })
                && f.loops.len() == 1
                && b.loops[f.loops[0] as usize].coedges.len() == 3
        })
}

/// The stepover between contact points at which two passes of a corner of
/// radius `r` leave a cusp of height `h` on a sphere of convex curvature
/// `kappa` (0 for a plane).
///
/// Exact for a ball on a sphere: the corner centres lie on a sphere of
/// radius ρ + r (ρ = 1/κ) and the cusp at ρ + h; with α half the angle
/// between the contacts, `1 − cos α = κ²(2rh − h²) / (2(1 + κh)(1 + κr))`
/// and the stepover is the chord `2ρ sin α`. For small h this is
/// `sqrt(8hr / (1 + rκ))` (D09 §4.3). On level ground the XY stepover of
/// the CL points is this chord; on convex ground it is larger.
pub fn scallop_stepover(h: f64, r: f64, kappa: f64) -> f64 {
    let c1 = (2.0 * r * h - h * h) / (2.0 * (1.0 + kappa * h) * (1.0 + kappa * r));
    if kappa <= 0.0 {
        return 2.0 * (2.0 * c1).sqrt();
    }
    let x = (kappa * kappa * c1).min(1.0);
    let sin_a = (x * (2.0 - x)).sqrt();
    2.0 * sin_a / kappa
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cutter::geom::BuildOptions;
    use crate::cutter::synth::{Builder, cuboid};
    use fluut_geom::{Frame, Interval, Point3, UvBox, Vec3};

    fn iv(lo: f64, hi: f64) -> Interval {
        Interval { lo, hi }
    }

    fn frame(o: [f64; 3], z: [f64; 3], x: [f64; 3]) -> Frame {
        Frame::new(
            Point3::new(o[0], o[1], o[2]),
            Vec3::new(z[0], z[1], z[2]),
            Vec3::new(x[0], x[1], x[2]),
        )
        .unwrap()
    }

    fn one(s: Surface, uv: UvBox, same_sense: bool) -> FaceCurvature {
        let mut b = Builder::new("f");
        let f = b.patch(s, uv);
        let mut body = b.build();
        body.faces[f as usize].same_sense = same_sense;
        let q = QueryGeometry::new(&body, &BuildOptions::default());
        face_curvature(&q, f)
    }

    #[test]
    fn curvature_of_analytic_faces() {
        let plane = Surface::plane(frame([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
        let c = one(plane, UvBox::new(iv(0.0, 5.0), iv(0.0, 5.0)), true);
        assert_eq!(c, FaceCurvature::default());
        // Cylinder along X, radius 40: convex from outside, concave from
        // inside.
        let cyl =
            Surface::cylinder(frame([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), 40.0).unwrap();
        let uv = UvBox::new(iv(1.0, 2.0), iv(0.0, 60.0));
        let c = one(cyl.clone(), uv, true);
        assert!(
            (c.convex - 1.0 / 40.0).abs() < 1e-8 && c.concave.abs() < 1e-8,
            "{c:?}"
        );
        let c = one(cyl, uv, false);
        assert!(
            (c.concave - 1.0 / 40.0).abs() < 1e-8 && c.convex.abs() < 1e-8,
            "{c:?}"
        );
        let sph = Surface::sphere(frame([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), 25.0).unwrap();
        let c = one(sph, UvBox::new(iv(0.0, 1.0), iv(0.2, 1.0)), true);
        assert!((c.convex - 1.0 / 25.0).abs() < 1e-8, "{c:?}");
        // Torus: outer part convex in both directions; the tube (1/3) wins.
        let tor =
            Surface::torus(frame([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), 10.0, 3.0).unwrap();
        let c = one(tor, UvBox::new(iv(0.0, 1.0), iv(-0.5, 0.5)), true);
        assert!((c.convex - 1.0 / 3.0).abs() < 1e-9, "{c:?}");
    }

    #[test]
    fn a_box_is_not_a_mesh() {
        let mut b = Builder::new("box");
        cuboid(
            &mut b,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 1.0),
        );
        assert!(!is_mesh(&b.build()));
    }

    /// Cusp height of two circles of radius `r` touching a circle of radius
    /// `rho` at contacts a chord `s` apart, by intersecting the circles.
    fn cusp_numeric(r: f64, rho: f64, s: f64) -> f64 {
        let a = libm::asin(s / (2.0 * rho));
        // Centres at angles ±a on the circle of radius rho + r.
        let (cx, cy) = ((rho + r) * libm::sin(a), (rho + r) * libm::cos(a));
        // The intersection on the axis x = 0: (cx)² + (y − cy)² = r².
        let y = cy - (r * r - cx * cx).sqrt();
        y - rho
    }

    #[test]
    fn stepover_inverts_the_cusp() {
        for (h, r, kappa) in [
            (0.01, 3.0, 0.0),
            (0.01, 3.0, 1.0 / 40.0),
            (0.2, 3.0, 1.0 / 5.0),
            (0.05, 1.0, 1.0 / 2.0),
            (1e-4, 3.0, 1e-6),
        ] {
            let s = scallop_stepover(h, r, kappa);
            let got = if kappa > 0.0 {
                cusp_numeric(r, 1.0 / kappa, s)
            } else {
                r - (r * r - s * s / 4.0).sqrt()
            };
            assert!(
                (got - h).abs() < 1e-9 * h.max(1.0),
                "{h} {r} {kappa}: {got}"
            );
            // Matches the small-h form of D09 §4.3.
            let approx = (8.0 * h * r / (1.0 + r * kappa)).sqrt();
            assert!((s - approx).abs() / approx < 0.1, "{s} {approx}");
        }
        // The ball's own radius bounds it: h → r gives s → 2r on a plane.
        assert!((scallop_stepover(3.0, 3.0, 0.0) - 6.0).abs() < 1e-12);
    }
}
