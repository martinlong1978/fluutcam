// SPDX-License-Identifier: MIT OR Apache-2.0

//! Relief stand-in: a 200 × 150 × 25 mm box finished by a Ø3 ball raster
//! along X, straight from the full stock.

use super::{polyline, pos};
use crate::stock::Stock;
use crate::tool::Tool;
use fluut_geom::Point3;
use fluut_path::Segment;
use std::f64::consts::TAU;

pub const STOCK: Stock = Stock::Box {
    min: Point3::new(0.0, 0.0, 0.0),
    max: Point3::new(200.0, 150.0, 25.0),
};

pub fn tool() -> Tool {
    Tool::ball(1.5)
}

/// Relief height: broad waves plus 4–5 mm detail that a Ø3 ball cannot fully
/// follow. Between 8.5 and 23.5 mm.
pub fn surface(x: f64, y: f64) -> f64 {
    16.0 + 5.0 * libm::sin(TAU * x / 70.0) * libm::cos(TAU * y / 55.0)
        + 1.5 * libm::cos(TAU * (x + y) / 23.0)
        + 1.0 * libm::sin(TAU * x / 4.0) * libm::sin(TAU * y / 5.0)
}

/// Zigzag raster along X: rows `stepover` apart from y = 0 to 150, the tip on
/// the surface every `step` mm from x = −2 to 202.
pub fn program(stepover: f64, step: f64) -> Vec<Segment> {
    let rows = (150.0 / stepover).round() as usize;
    let cols = (204.0 / step).round() as usize;
    let mut pts = Vec::with_capacity((rows + 1) * (cols + 1));
    for j in 0..=rows {
        let y = j as f64 * stepover;
        for c in 0..=cols {
            let i = if j % 2 == 0 { c } else { cols - c };
            let x = -2.0 + i as f64 * step;
            let z = surface(x.clamp(0.0, 200.0), y);
            pts.push(pos(x, y, z, None));
        }
    }
    polyline(&pts)
}

/// Roughing tool: Ø6 flat end mill.
pub fn rough_tool() -> Tool {
    Tool::flat(3.0)
}

/// Z-level roughing with the Ø6 flat, leaving about `leave` mm: levels every
/// 3 mm from 23 down, rows along X `stepover` apart, one Line per run where
/// the tool stays above the relief plus `leave` (checked at 25 points under the
/// tool), with retracts and rapids at z = 27 between runs.
pub fn roughing(stepover: f64, leave: f64) -> Vec<Segment> {
    let r = rough_tool().radius;
    let ring: Vec<(f64, f64)> = std::iter::once((0.0, 0.0))
        .chain((0..8).map(|i| {
            let (s, c) = libm::sincos(TAU * f64::from(i) / 8.0);
            (0.5 * r * c, 0.5 * r * s)
        }))
        .chain((0..16).map(|i| {
            let (s, c) = libm::sincos(TAU * f64::from(i) / 16.0);
            (r * c, r * s)
        }))
        .collect();
    let clear = |x: f64, y: f64, z: f64| {
        ring.iter().all(|&(dx, dy)| {
            surface((x + dx).clamp(0.0, 200.0), (y + dy).clamp(0.0, 150.0)) + leave <= z
        })
    };
    let safe = 27.0;
    let mut pts = vec![pos(-4.0, 0.0, safe, None)];
    let rows = (150.0 / stepover).ceil() as usize;
    for level in 0..5 {
        let z = 23.0 - 3.0 * f64::from(level);
        for j in 0..=rows {
            let y = (j as f64 * stepover).min(150.0);
            let mut run: Option<f64> = None;
            for c in 0..=416 {
                let x = -4.0 + 0.5 * f64::from(c);
                let ok = c < 416 && clear(x, y, z);
                match (ok, run) {
                    (true, None) => run = Some(x),
                    (false, Some(x0)) => {
                        let x1 = x - 0.5;
                        if x1 > x0 {
                            pts.push(pos(x0, y, safe, None));
                            pts.push(pos(x0, y, z, None));
                            pts.push(pos(x1, y, z, None));
                            pts.push(pos(x1, y, safe, None));
                        }
                        run = None;
                    }
                    _ => {}
                }
            }
        }
    }
    pts.dedup_by(|a, b| a.same_bits(b));
    polyline(&pts)
}
