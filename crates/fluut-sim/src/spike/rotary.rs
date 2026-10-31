// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rotary stand-in: a Ø60 × 150 mm cylinder on the A axis, wrapped by a Ø6
//! ball in one spiral (X advances while A turns), as Rotary segments.

use super::{cut, pos};
use crate::stock::Stock;
use crate::tool::Tool;
use fluut_path::Segment;
use std::f64::consts::TAU;

pub const STOCK: Stock = Stock::Cylinder {
    x0: 0.0,
    x1: 150.0,
    radius: 30.0,
};

pub fn tool() -> Tool {
    Tool::ball(3.0)
}

/// Wrapped relief radius at `x` and angle `theta` (radians); 24 to 29 mm.
pub fn surface(x: f64, theta: f64) -> f64 {
    26.5 + 2.0 * libm::sin(TAU * x / 40.0) * libm::cos(3.0 * theta)
        + 0.5 * libm::cos(TAU * x / 13.0 + theta)
}

/// Spiral from x = −4 to at least 154 (whole turns) at `pitch` mm per turn, one Rotary segment per
/// `step_deg` of A, the tip on the surface.
pub fn program(pitch: f64, step_deg: f64) -> Vec<Segment> {
    let per_turn = (360.0 / step_deg).round() as usize;
    let turns = (158.0 / pitch).ceil() as usize;
    let n = per_turn * turns;
    let at = |i: usize| {
        let a = i as f64 * step_deg;
        let x = -4.0 + pitch * a / 360.0;
        let r = surface(x.clamp(0.0, 150.0), a.to_radians());
        pos(x, 0.0, r, Some(a))
    };
    let mut prev = at(0);
    let mut out = Vec::with_capacity(n);
    for i in 1..=n {
        let next = at(i);
        out.push(Segment::Rotary {
            a: prev,
            b: next,
            radius: prev.xyz.z(),
            m: cut(),
        });
        prev = next;
    }
    out
}
