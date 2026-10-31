// SPDX-License-Identifier: MIT OR Apache-2.0

//! S5 spike: synthetic stand-ins for the reference relief and rotary projects,
//! and an exact single-ray probe for error measurements. May be thrown away.

pub mod probe;
pub mod relief;
pub mod rotary;

use fluut_geom::Point3;
use fluut_path::{AxisPos, FeedRef, Motion, MotionInfo, Segment};

fn cut() -> MotionInfo {
    MotionInfo::new(Motion::Cut, FeedRef::Cut, 0.01)
}

fn pos(x: f64, y: f64, z: f64, a: Option<f64>) -> AxisPos {
    AxisPos {
        xyz: Point3::new(x, y, z),
        a,
        b: None,
    }
}

/// Lines through `points`, sharing endpoints bitwise.
fn polyline(points: &[AxisPos]) -> Vec<Segment> {
    points
        .windows(2)
        .map(|w| Segment::Line {
            a: w[0],
            b: w[1],
            m: cut(),
        })
        .collect()
}

#[cfg(test)]
mod tests;
