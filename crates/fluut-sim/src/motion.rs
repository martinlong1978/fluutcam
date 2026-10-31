// SPDX-License-Identifier: MIT OR Apache-2.0

//! From toolpath segments to chords in the stock frame (D11 §3).
//!
//! For rotary Setups the rotary axis is the X axis of the stock frame and
//! `p_W = R_x(A)·p_S` (spec 03 §3.9.3), so a tool at machine position
//! `xyz, A` has its tip at `R_x(−A)·xyz` and its axis along `R_x(−A)·Z`.

use crate::stock::Stock;
use crate::sweep::Sweep;
use crate::tool::{Pose, Tool};
use fluut_path::{AxisPos, Segment, linearize};
use glam::DVec3;

/// Splits segments into chords.
#[derive(Copy, Clone, Debug)]
pub struct Chorder {
    pub tool: Tool,
    pub stock: Stock,
    /// Rotary Setup: A angles turn the part.
    pub rotary: bool,
    /// Largest sagitta of a chord, in mm.
    pub tol: f64,
    /// Largest distance of stock material from the rotary axis.
    pub r_out: f64,
    /// Extra shank length past the stock, in mm.
    pub margin: f64,
}

impl Chorder {
    /// Chords with sagitta at most `res / 4` (D11 §3).
    pub fn new(tool: Tool, stock: Stock, rotary: bool, res: f64) -> Chorder {
        let b = stock.bbox();
        let r_out = [b.min.0, b.max.0]
            .iter()
            .flat_map(|p| [p.y.abs(), p.z.abs()])
            .fold(0.0_f64, f64::max)
            * std::f64::consts::SQRT_2;
        let r_out = match stock {
            Stock::Cylinder { radius, .. } => radius,
            Stock::Box { .. } => r_out,
        };
        Chorder {
            tool,
            stock,
            rotary,
            tol: res / 4.0,
            r_out,
            margin: res,
        }
    }

    /// All chords of `segments`, in order. Chords that cannot touch the stock
    /// are dropped.
    pub fn chords(&self, segments: &[Segment]) -> Vec<Sweep> {
        let mut out = Vec::new();
        for s in segments {
            self.segment(s, &mut out);
        }
        out
    }

    pub fn segment(&self, seg: &Segment, out: &mut Vec<Sweep>) {
        match seg {
            Segment::Line { a, b, .. } | Segment::Rotary { a, b, .. } => self.line(a, b, out),
            Segment::Arc { .. } | Segment::Helix { .. } | Segment::Spline { .. } => {
                for l in linearize(seg, self.tol) {
                    if let (Some(a), Some(b)) = (l.start(), l.end()) {
                        self.line(a, b, out);
                    }
                }
            }
            Segment::Cycle { at, cycle, .. } => {
                let top = AxisPos {
                    xyz: fluut_geom::Point3::new(at.xyz.x(), at.xyz.y(), cycle.retract_z),
                    ..*at
                };
                let bottom = AxisPos {
                    xyz: fluut_geom::Point3::new(at.xyz.x(), at.xyz.y(), cycle.depth_z),
                    ..*at
                };
                self.line(&top, &bottom, out);
            }
            Segment::Marker { .. } => {}
        }
    }

    fn angle(&self, p: &AxisPos) -> f64 {
        if self.rotary {
            p.a.unwrap_or(0.0).to_radians()
        } else {
            0.0
        }
    }

    /// A straight move in machine coordinates; split when A changes.
    fn line(&self, a: &AxisPos, b: &AxisPos, out: &mut Vec<Sweep>) {
        let (aa, ab) = (self.angle(a), self.angle(b));
        let n = if aa == ab {
            1
        } else {
            let step = if self.tol < self.r_out {
                2.0 * libm::acos(1.0 - self.tol / self.r_out)
            } else {
                std::f64::consts::FRAC_PI_2
            };
            ((ab - aa).abs() / step).ceil().max(1.0) as usize
        };
        let (pa, pb) = (a.xyz.0, b.xyz.0);
        let at = |i: usize| -> (DVec3, f64) {
            if i == 0 {
                (pa, aa)
            } else if i == n {
                (pb, ab)
            } else {
                let f = i as f64 / n as f64;
                (pa.lerp(pb, f), aa + (ab - aa) * f)
            }
        };
        let mut prev = self.pose(at(0));
        for i in 1..=n {
            let next = self.pose(at(i));
            let s = Sweep::new(self.tool, prev, next);
            if self.touches_stock(&s) {
                out.push(s);
            }
            prev = next;
        }
    }

    fn pose(&self, (xyz, a): (DVec3, f64)) -> Pose {
        let (tip, axis) = if a == 0.0 {
            (xyz, DVec3::Z)
        } else {
            let (s, c) = libm::sincos(a);
            (
                DVec3::new(xyz.x, xyz.y * c + xyz.z * s, -xyz.y * s + xyz.z * c),
                DVec3::new(0.0, s, c),
            )
        };
        let length = self.stock.extent_along(tip, axis).max(0.0) + self.margin;
        Pose { tip, axis, length }
    }

    fn touches_stock(&self, s: &Sweep) -> bool {
        let b = self.stock.bbox();
        s.min.cmple(b.max.0).all() && s.max.cmpge(b.min.0).all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluut_geom::Point3;
    use fluut_path::{FeedRef, Motion, MotionInfo};

    fn m() -> MotionInfo {
        MotionInfo::new(Motion::Cut, FeedRef::Cut, 0.01)
    }

    fn pos(x: f64, y: f64, z: f64, a: f64) -> AxisPos {
        AxisPos {
            xyz: Point3::new(x, y, z),
            a: Some(a),
            b: None,
        }
    }

    #[test]
    fn rotary_turn_splits_and_rotates() {
        let stock = Stock::Cylinder {
            x0: 0.0,
            x1: 20.0,
            radius: 10.0,
        };
        let c = Chorder::new(Tool::ball(1.0), stock, true, 0.1);
        let seg = Segment::Rotary {
            a: pos(10.0, 0.0, 8.0, 0.0),
            b: pos(10.0, 0.0, 8.0, 90.0),
            radius: 8.0,
            m: m(),
        };
        let chords = c.chords(&[seg]);
        // Sagitta at r = 10 within 0.025 mm: about 2·acos(1 − 0.0025) per chord.
        let step = 2.0 * libm::acos(1.0 - 0.025 / 10.0);
        assert_eq!(
            chords.len(),
            (std::f64::consts::FRAC_PI_2 / step).ceil() as usize
        );
        let last = chords.last().unwrap().p1;
        // A = 90°: the tool on top of W sits on +Y of the stock, pointing +Y.
        assert!((last.tip - DVec3::new(10.0, 8.0, 0.0)).length() < 1e-12);
        assert!((last.axis - DVec3::Y).length() < 1e-12);
        // Shank reaches the stock surface plus the margin.
        assert!((last.length - 2.1).abs() < 1e-12);
        for w in chords.windows(2) {
            assert_eq!(w[0].p1, w[1].p0);
        }
    }

    #[test]
    fn three_axis_ignores_a_and_drops_far_moves() {
        let stock = Stock::Box {
            min: Point3::new(0.0, 0.0, 0.0),
            max: Point3::new(10.0, 10.0, 5.0),
        };
        let c = Chorder::new(Tool::flat(1.0), stock, false, 0.1);
        let near = Segment::Line {
            a: pos(1.0, 1.0, 4.0, 0.0),
            b: pos(5.0, 1.0, 4.0, 30.0),
            m: m(),
        };
        let far = Segment::Line {
            a: pos(1.0, 1.0, 9.0, 0.0),
            b: pos(5.0, 1.0, 9.0, 0.0),
            m: m(),
        };
        let chords = c.chords(&[near, far]);
        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].p1.axis, DVec3::Z);
        assert!((chords[0].p0.length - 1.1).abs() < 1e-12);
    }
}
