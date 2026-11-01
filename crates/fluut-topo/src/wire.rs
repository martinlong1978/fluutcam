// SPDX-License-Identifier: MIT OR Apache-2.0

//! Wire bodies: curves without faces (spec 03 §3.2.1, from
//! `GEOMETRIC_CURVE_SET`, DXF and SVG).

use fluut_geom::{Affine, Curve3, Interval};

/// One curve of a wire body over its parameter range.
#[derive(Debug, Clone, PartialEq)]
pub struct WireCurve {
    pub curve: Curve3,
    pub range: Interval,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WireBody {
    pub name: String,
    pub tol_model: f64,
    pub curves: Vec<WireCurve>,
}

impl WireBody {
    /// The body moved by `a`; `None` if a curve cannot be transformed.
    /// Ranges follow [`Curve3::transformed_range`], so each curve keeps its
    /// end points.
    pub fn transformed(&self, a: &Affine) -> Option<WireBody> {
        let curves = self
            .curves
            .iter()
            .map(|c| {
                Some(WireCurve {
                    curve: c.curve.transformed(a)?,
                    range: c.curve.transformed_range(a, c.range),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(WireBody {
            name: self.name.clone(),
            tol_model: self.tol_model,
            curves,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluut_geom::{Point3, UnitVec3, Vec3};

    #[test]
    fn scaled_line_keeps_its_end_points() {
        let p0 = Point3::new(1.0, 1.0, 0.0);
        let w = WireBody {
            name: "w".into(),
            tol_model: 1e-4,
            curves: vec![WireCurve {
                curve: Curve3::line(p0, UnitVec3::new(Vec3::new(1.0, 1.0, 0.0)).unwrap()),
                range: Interval { lo: 0.0, hi: 2.0 },
            }],
        };
        let a = Affine::scale(25.4, 25.4, 1.0);
        let t = w.transformed(&a).unwrap();
        let c = &t.curves[0];
        let p1 = w.curves[0].curve.eval(2.0);
        assert!(c.curve.eval(c.range.lo).distance(a.apply_point(p0)) < 1e-12);
        assert!(c.curve.eval(c.range.hi).distance(a.apply_point(p1)) < 1e-12);
    }
}
