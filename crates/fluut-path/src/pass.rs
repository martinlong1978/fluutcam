// SPDX-License-Identifier: MIT OR Apache-2.0

//! Passes, results and their invariants (D08 §2).

use crate::linearize::linearize;
use crate::segment::{AxisPos, Motion, Segment, radial};
use fluut_geom::Aabb3;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PassKind {
    Cut,
    Lead,
    Link,
    Ramp,
    Retract,
}

/// Side for cutter compensation in the controller (G41/G42; D09 §2.4).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CompSide {
    Left,
    Right,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Pass {
    pub kind: PassKind,
    pub segments: Vec<Segment>,
    /// Set when the controller compensates this pass (compensation "in
    /// control" or "wear"); the post fires the cutter-comp events around it.
    pub comp: Option<CompSide>,
}

/// An invariant violation. `pass` and `index` locate the offending segment.
#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum PathError {
    #[error("pass {pass} segment {index}: does not start where the previous segment ends")]
    Discontinuous { pass: usize, index: usize },
    #[error("pass {pass} segment {index}: non-finite value")]
    NonFinite { pass: usize, index: usize },
    #[error("pass {pass} segment {index}: arc start and end radii differ")]
    ArcRadiusMismatch { pass: usize, index: usize },
    #[error("pass {pass} segment {index}: zero-length move")]
    ZeroLength { pass: usize, index: usize },
}

/// Relative tolerance on arc start/end radius equality (D08 §2).
const ARC_RADIUS_REL_TOL: f64 = 1e-9;

impl Pass {
    pub fn new(kind: PassKind, segments: Vec<Segment>) -> Pass {
        Pass {
            kind,
            segments,
            comp: None,
        }
    }

    /// The pass with controller compensation on `side`.
    pub fn with_comp(mut self, side: CompSide) -> Pass {
        self.comp = Some(side);
        self
    }

    /// Checks the D08 §2 invariants within this pass. `pass` is used only to
    /// label errors.
    pub fn check(&self, pass: usize) -> Result<(), PathError> {
        let mut prev: Option<AxisPos> = None;
        for (index, seg) in self.segments.iter().enumerate() {
            check_segment(seg, pass, index)?;
            check_continuity(&mut prev, seg, pass, index)?;
        }
        Ok(())
    }
}

fn check_continuity(
    prev: &mut Option<AxisPos>,
    seg: &Segment,
    pass: usize,
    index: usize,
) -> Result<(), PathError> {
    if let Some(start) = seg.start() {
        if let Some(p) = prev
            && !p.same_bits(start)
        {
            return Err(PathError::Discontinuous { pass, index });
        }
        *prev = seg.end().copied();
    }
    Ok(())
}

fn check_segment(seg: &Segment, pass: usize, index: usize) -> Result<(), PathError> {
    let finite = seg.start().is_none_or(AxisPos::is_finite)
        && seg.end().is_none_or(AxisPos::is_finite)
        && match seg {
            Segment::Arc { centre, sweep, .. } | Segment::Helix { centre, sweep, .. } => {
                centre.is_finite() && sweep.is_finite()
            }
            Segment::Rotary { radius, .. } => radius.is_finite(),
            _ => true,
        };
    if !finite {
        return Err(PathError::NonFinite { pass, index });
    }
    match seg {
        Segment::Line { a, b, .. } if a.same_bits(b) => Err(PathError::ZeroLength { pass, index }),
        Segment::Arc {
            a,
            b,
            centre,
            normal,
            sweep,
            ..
        }
        | Segment::Helix {
            a,
            b,
            centre,
            normal,
            sweep,
            ..
        } => {
            let ra = radial(a.xyz, *centre, *normal).length();
            let rb = radial(b.xyz, *centre, *normal).length();
            if *sweep == 0.0 || ra == 0.0 {
                Err(PathError::ZeroLength { pass, index })
            } else if (ra - rb).abs() > ARC_RADIUS_REL_TOL * ra.max(rb) {
                Err(PathError::ArcRadiusMismatch { pass, index })
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

/// Summary figures for a result. Estimated time is added by `fluut-sim` (D11).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct PathStats {
    /// Length of all non-rapid moves, mm.
    pub cut_length: f64,
    /// Length of rapid moves, mm.
    pub rapid_length: f64,
    /// Bounds of the tool tip, including arc bulges to within 1 µm.
    pub bounds: Aabb3,
}

/// Toolpath result: passes in machining order (links included as passes).
///
/// `fingerprint` and `stock_out` (D08 §2) are attached by `fluut-cam` and
/// `fluut-core`, which own hashing and in-process stock.
#[derive(Clone, PartialEq, Debug)]
pub struct ToolpathResult {
    passes: Vec<Pass>,
    stats: PathStats,
}

/// Chord tolerance used only for the bounds in [`PathStats`].
const BOUNDS_CHORD: f64 = 1e-3;

impl ToolpathResult {
    /// Checks every invariant, including continuity across passes, and computes
    /// the statistics.
    pub fn new(passes: Vec<Pass>) -> Result<ToolpathResult, PathError> {
        let mut prev: Option<AxisPos> = None;
        let mut cut_length = 0.0;
        let mut rapid_length = 0.0;
        let mut bounds = Aabb3::empty();
        for (pi, pass) in passes.iter().enumerate() {
            for (si, seg) in pass.segments.iter().enumerate() {
                check_segment(seg, pi, si)?;
                check_continuity(&mut prev, seg, pi, si)?;
                let len = seg.length();
                match seg.motion().map(|m| m.motion) {
                    Some(Motion::Rapid) => rapid_length += len,
                    Some(_) => cut_length += len,
                    None => {}
                }
                for piece in linearize(seg, BOUNDS_CHORD) {
                    for p in piece.start().into_iter().chain(piece.end()) {
                        bounds = bounds.with_point(p.xyz);
                    }
                }
            }
        }
        Ok(ToolpathResult {
            passes,
            stats: PathStats {
                cut_length,
                rapid_length,
                bounds,
            },
        })
    }

    pub fn passes(&self) -> &[Pass] {
        &self.passes
    }

    pub fn stats(&self) -> &PathStats {
        &self.stats
    }

    /// Every segment in machining order.
    pub fn segments(&self) -> impl Iterator<Item = &Segment> {
        self.passes.iter().flat_map(|p| p.segments.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::{FeedRef, MotionInfo};
    use fluut_geom::{Point3, UnitVec3};

    fn pos(x: f64, y: f64, z: f64) -> AxisPos {
        AxisPos::xyz(Point3::new(x, y, z))
    }
    fn cut() -> MotionInfo {
        MotionInfo::new(Motion::Cut, FeedRef::Cut, 0.01)
    }
    fn rapid() -> MotionInfo {
        MotionInfo::new(Motion::Rapid, FeedRef::Rapid, 0.01)
    }
    fn line(a: AxisPos, b: AxisPos, m: MotionInfo) -> Segment {
        Segment::Line { a, b, m }
    }

    #[test]
    fn continuity_checked() {
        let p = Pass::new(
            PassKind::Cut,
            vec![
                line(pos(0., 0., 0.), pos(1., 0., 0.), cut()),
                line(pos(1., 0., 1e-12), pos(2., 0., 0.), cut()),
            ],
        );
        assert_eq!(
            p.check(3),
            Err(PathError::Discontinuous { pass: 3, index: 1 })
        );
    }

    #[test]
    fn markers_do_not_break_continuity() {
        let p = Pass::new(
            PassKind::Cut,
            vec![
                line(pos(0., 0., 0.), pos(1., 0., 0.), cut()),
                Segment::Marker {
                    kind: crate::MarkerKind::OptionalStop,
                },
                line(pos(1., 0., 0.), pos(2., 0., 0.), cut()),
            ],
        );
        assert_eq!(p.check(0), Ok(()));
    }

    #[test]
    fn continuity_checked_across_passes() {
        let r = ToolpathResult::new(vec![
            Pass::new(
                PassKind::Cut,
                vec![line(pos(0., 0., 0.), pos(1., 0., 0.), cut())],
            ),
            Pass::new(
                PassKind::Link,
                vec![line(pos(1., 0., 5.), pos(2., 0., 5.), rapid())],
            ),
        ]);
        assert_eq!(r, Err(PathError::Discontinuous { pass: 1, index: 0 }));
    }

    #[test]
    fn nan_rejected() {
        let p = Pass::new(
            PassKind::Cut,
            vec![line(pos(0., 0., 0.), pos(f64::NAN, 0., 0.), cut())],
        );
        assert_eq!(p.check(0), Err(PathError::NonFinite { pass: 0, index: 0 }));
    }

    #[test]
    fn arc_radius_mismatch() {
        let p = Pass::new(
            PassKind::Cut,
            vec![Segment::Arc {
                a: pos(1., 0., 0.),
                b: pos(0., 1.001, 0.),
                centre: Point3::ORIGIN,
                normal: UnitVec3::Z,
                sweep: std::f64::consts::FRAC_PI_2,
                m: cut(),
            }],
        );
        assert_eq!(
            p.check(0),
            Err(PathError::ArcRadiusMismatch { pass: 0, index: 0 })
        );
    }

    #[test]
    fn zero_length_line_rejected() {
        let p = Pass::new(
            PassKind::Cut,
            vec![line(pos(1., 2., 3.), pos(1., 2., 3.), cut())],
        );
        assert_eq!(p.check(0), Err(PathError::ZeroLength { pass: 0, index: 0 }));
    }

    #[test]
    fn full_circle_arc_valid() {
        let a = pos(2., 0., 0.);
        let p = Pass::new(
            PassKind::Cut,
            vec![Segment::Arc {
                a,
                b: a,
                centre: Point3::ORIGIN,
                normal: UnitVec3::Z,
                sweep: std::f64::consts::TAU,
                m: cut(),
            }],
        );
        assert_eq!(p.check(0), Ok(()));
        let r = ToolpathResult::new(vec![p]).unwrap();
        // Bounds include the far side of the circle, not just the endpoints.
        assert!((r.stats().bounds.min.x() + 2.0).abs() < 1e-3);
        assert!((r.stats().bounds.max.y() - 2.0).abs() < 1e-3);
    }

    #[test]
    fn stats_sum() {
        let r = ToolpathResult::new(vec![
            Pass::new(
                PassKind::Link,
                vec![line(pos(0., 0., 5.), pos(0., 0., 0.), rapid())],
            ),
            Pass::new(
                PassKind::Cut,
                vec![
                    line(pos(0., 0., 0.), pos(3., 0., 0.), cut()),
                    line(pos(3., 0., 0.), pos(3., 4., 0.), cut()),
                ],
            ),
        ])
        .unwrap();
        assert_eq!(r.stats().rapid_length, 5.0);
        assert_eq!(r.stats().cut_length, 7.0);
        assert_eq!(r.segments().count(), 3);
    }
}
