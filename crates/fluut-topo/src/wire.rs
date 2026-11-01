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
    pub fn transformed(&self, a: &Affine) -> Option<WireBody> {
        let curves = self
            .curves
            .iter()
            .map(|c| {
                Some(WireCurve {
                    curve: c.curve.transformed(a)?,
                    range: c.range,
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
