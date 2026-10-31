// SPDX-License-Identifier: MIT OR Apache-2.0

//! Segments: the motion primitives of a toolpath (D08 §2).

use fluut_geom::{BSplineCurve3, Interval, Point3, Tol, UnitVec3, Vec3};
use std::sync::Arc;

/// Persistent id of a model entity (face, body, …), stable across sessions (D01 §2.2).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PersistId(pub u128);

/// A machine position: tool tip in the work frame plus optional rotary angles
/// in degrees, as the machine sees them.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct AxisPos {
    pub xyz: Point3,
    pub a: Option<f64>,
    pub b: Option<f64>,
}

impl AxisPos {
    pub fn xyz(xyz: Point3) -> AxisPos {
        AxisPos {
            xyz,
            a: None,
            b: None,
        }
    }

    /// Bitwise equality (so `-0.0` differs from `0.0`), as the continuity
    /// invariant requires (D08 §2).
    pub fn same_bits(&self, other: &AxisPos) -> bool {
        let bits = |p: &AxisPos| {
            [
                p.xyz.x().to_bits(),
                p.xyz.y().to_bits(),
                p.xyz.z().to_bits(),
                p.a.map_or(u64::MAX, f64::to_bits),
                p.b.map_or(u64::MAX, f64::to_bits),
            ]
        };
        self.a.is_some() == other.a.is_some()
            && self.b.is_some() == other.b.is_some()
            && bits(self) == bits(other)
    }

    pub fn is_finite(&self) -> bool {
        self.xyz.is_finite()
            && self.a.is_none_or(f64::is_finite)
            && self.b.is_none_or(f64::is_finite)
    }
}

/// What a move does (D08 §9).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Motion {
    Rapid,
    Cut,
    Plunge,
    Ramp,
    LeadIn,
    LeadOut,
    Link,
    Retract,
}

/// Symbolic feed, resolved to a number at post time from the tool (D08 §9), so a
/// feed change needs no recalculation.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum FeedRef {
    Rapid,
    Cut,
    Plunge,
    Ramp,
    LeadIn,
    LeadOut,
    Transition,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct MotionInfo {
    pub motion: Motion,
    pub feed: FeedRef,
    /// Machining tolerance the segment was computed to, in mm.
    pub tol: f32,
    /// Model face the cutter touches, for display and diagnostics.
    pub face: Option<PersistId>,
}

impl MotionInfo {
    pub fn new(motion: Motion, feed: FeedRef, tol: f32) -> MotionInfo {
        MotionInfo {
            motion,
            feed,
            tol,
            face: None,
        }
    }
}

/// Drilling cycle kinds (spec 05 §5.5, D10 §5).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CycleKind {
    Drill,
    Spot,
    Peck,
    ChipBreak,
    Tap,
    Ream,
    Bore,
}

/// One drilling cycle at one hole (spec 03 `CycleSegment`).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct CycleParams {
    pub kind: CycleKind,
    /// Z of the hole top.
    pub top_z: f64,
    /// Z of the hole bottom.
    pub depth_z: f64,
    /// Z of the retract (R) plane.
    pub retract_z: f64,
    /// Peck depth, for peck and chip-break cycles.
    pub peck: Option<f64>,
    /// Dwell at the bottom, in seconds.
    pub dwell_s: Option<f64>,
}

/// Manual NC items (spec 03 `manual_nc`).
#[derive(Clone, PartialEq, Debug)]
pub enum MarkerKind {
    Stop,
    OptionalStop,
    Comment(String),
    Dwell(f64),
    PassThrough(String),
}

#[derive(Clone, PartialEq, Debug)]
pub enum Segment {
    Line {
        a: AxisPos,
        b: AxisPos,
        m: MotionInfo,
    },
    /// Circular arc about `centre` in the plane normal to `normal`. `sweep` is
    /// signed: positive is counter-clockwise seen from the tip of `normal`.
    /// A full circle has `a == b` and `|sweep| = 2π`.
    Arc {
        a: AxisPos,
        b: AxisPos,
        centre: Point3,
        normal: UnitVec3,
        sweep: f64,
        m: MotionInfo,
    },
    /// Arc plus linear motion along `normal` (from `a`'s height to `b`'s).
    Helix {
        a: AxisPos,
        b: AxisPos,
        centre: Point3,
        normal: UnitVec3,
        sweep: f64,
        m: MotionInfo,
    },
    /// Rotary-axis move at `radius` from the rotary axis (spec 04 §4.7.5).
    Rotary {
        a: AxisPos,
        b: AxisPos,
        radius: f64,
        m: MotionInfo,
    },
    /// Not emitted in v1 (OQ-07).
    Spline {
        a: AxisPos,
        b: AxisPos,
        curve: Arc<BSplineCurve3>,
        m: MotionInfo,
    },
    Cycle {
        at: AxisPos,
        cycle: CycleParams,
        m: MotionInfo,
    },
    Marker {
        kind: MarkerKind,
    },
}

impl Segment {
    pub fn start(&self) -> Option<&AxisPos> {
        match self {
            Segment::Line { a, .. }
            | Segment::Arc { a, .. }
            | Segment::Helix { a, .. }
            | Segment::Rotary { a, .. }
            | Segment::Spline { a, .. } => Some(a),
            Segment::Cycle { at, .. } => Some(at),
            Segment::Marker { .. } => None,
        }
    }

    pub fn end(&self) -> Option<&AxisPos> {
        match self {
            Segment::Line { b, .. }
            | Segment::Arc { b, .. }
            | Segment::Helix { b, .. }
            | Segment::Rotary { b, .. }
            | Segment::Spline { b, .. } => Some(b),
            Segment::Cycle { at, .. } => Some(at),
            Segment::Marker { .. } => None,
        }
    }

    pub fn motion(&self) -> Option<&MotionInfo> {
        match self {
            Segment::Line { m, .. }
            | Segment::Arc { m, .. }
            | Segment::Helix { m, .. }
            | Segment::Rotary { m, .. }
            | Segment::Spline { m, .. }
            | Segment::Cycle { m, .. } => Some(m),
            Segment::Marker { .. } => None,
        }
    }

    /// Tool-tip path length in mm. Rotary moves combine the arc travelled at
    /// `radius` with the linear axis change. Cycles and markers have length 0.
    pub fn length(&self) -> f64 {
        match self {
            Segment::Line { a, b, .. } => a.xyz.distance(b.xyz),
            Segment::Arc {
                a,
                centre,
                normal,
                sweep,
                ..
            } => radial(a.xyz, *centre, *normal).length() * sweep.abs(),
            Segment::Helix {
                a,
                b,
                centre,
                normal,
                sweep,
                ..
            } => {
                let r = radial(a.xyz, *centre, *normal).length();
                let dh = (b.xyz - a.xyz).dot(normal.get());
                libm::hypot(r * sweep, dh)
            }
            Segment::Rotary { a, b, radius, .. } => {
                let da = (b.a.unwrap_or(0.0) - a.a.unwrap_or(0.0)).to_radians();
                let db = (b.b.unwrap_or(0.0) - a.b.unwrap_or(0.0)).to_radians();
                libm::hypot(radius * libm::hypot(da, db), a.xyz.distance(b.xyz))
            }
            Segment::Spline { curve, m, .. } => {
                let tol = Tol::for_machining(0.0, f64::from(m.tol));
                let d = curve.domain();
                fluut_geom::Curve3::BSpline(curve.clone())
                    .length(Interval { lo: d.lo, hi: d.hi }, &tol)
            }
            Segment::Cycle { .. } | Segment::Marker { .. } => 0.0,
        }
    }
}

/// Component of `p − centre` perpendicular to `normal`.
pub(crate) fn radial(p: Point3, centre: Point3, normal: UnitVec3) -> Vec3 {
    let d = p - centre;
    d - normal.get() * d.dot(normal.get())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn m() -> MotionInfo {
        MotionInfo::new(Motion::Cut, FeedRef::Cut, 0.01)
    }

    #[test]
    fn same_bits_distinguishes_signed_zero() {
        let p = AxisPos::xyz(Point3::new(0.0, 1.0, 2.0));
        let q = AxisPos::xyz(Point3::new(-0.0, 1.0, 2.0));
        assert!(p.same_bits(&p));
        assert!(!p.same_bits(&q));
        let r = AxisPos { a: Some(0.0), ..p };
        assert!(!p.same_bits(&r));
    }

    #[test]
    fn full_circle_length() {
        let a = AxisPos::xyz(Point3::new(3.0, 0.0, 0.0));
        let s = Segment::Arc {
            a,
            b: a,
            centre: Point3::ORIGIN,
            normal: UnitVec3::Z,
            sweep: TAU,
            m: m(),
        };
        assert!((s.length() - 3.0 * TAU).abs() < 1e-12);
    }

    #[test]
    fn helix_length() {
        let s = Segment::Helix {
            a: AxisPos::xyz(Point3::new(1.0, 0.0, 0.0)),
            b: AxisPos::xyz(Point3::new(1.0, 0.0, -2.0)),
            centre: Point3::ORIGIN,
            normal: UnitVec3::Z,
            sweep: -TAU,
            m: m(),
        };
        assert!((s.length() - libm::hypot(TAU, 2.0)).abs() < 1e-12);
    }

    #[test]
    fn rotary_length() {
        let s = Segment::Rotary {
            a: AxisPos {
                a: Some(0.0),
                ..AxisPos::xyz(Point3::new(0.0, 0.0, 5.0))
            },
            b: AxisPos {
                a: Some(180.0),
                ..AxisPos::xyz(Point3::new(0.0, 0.0, 5.0))
            },
            radius: 5.0,
            m: m(),
        };
        assert!((s.length() - 5.0 * std::f64::consts::PI).abs() < 1e-12);
    }
}
