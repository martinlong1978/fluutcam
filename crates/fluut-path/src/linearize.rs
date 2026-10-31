// SPDX-License-Identifier: MIT OR Apache-2.0

//! Conversion of curved segments to lines (D08 §2), for posts without arc
//! support (D10 §4.2) and for display and simulation.

use crate::segment::{AxisPos, Segment, radial};
use fluut_geom::{Curve3, Interval, Point3, UnitVec3, Vec3};
use std::f64::consts::FRAC_PI_2;

/// Replaces arcs, helices and splines with lines whose chord height is at most
/// `tol`. Other segments are returned unchanged. The first line starts and the
/// last line ends bitwise at the original endpoints. Always returns at least one
/// segment.
pub fn linearize(seg: &Segment, tol: f64) -> Vec<Segment> {
    match seg {
        Segment::Arc {
            a,
            b,
            centre,
            normal,
            sweep,
            m,
        }
        | Segment::Helix {
            a,
            b,
            centre,
            normal,
            sweep,
            m,
        } => {
            let r = radial(a.xyz, *centre, *normal).length();
            let n = chord_count(r, *sweep, tol);
            let points = (1..n).map(|i| {
                let f = i as f64 / n as f64;
                helix_point(a, b, *centre, *normal, *sweep, f)
            });
            lines_through(*a, points, *b, *m)
        }
        Segment::Spline { a, b, curve, m } => {
            let d = curve.domain();
            let pts =
                Curve3::BSpline(curve.clone()).tessellate(Interval { lo: d.lo, hi: d.hi }, tol);
            let n = pts.len().saturating_sub(1).max(1);
            let inner = pts
                .iter()
                .skip(1)
                .take(n.saturating_sub(1))
                .enumerate()
                .map(|(i, &(_, p))| {
                    let f = (i + 1) as f64 / n as f64;
                    AxisPos {
                        xyz: p,
                        a: lerp_opt(a.a, b.a, f),
                        b: lerp_opt(a.b, b.b, f),
                    }
                });
            lines_through(*a, inner, *b, *m)
        }
        _ => vec![seg.clone()],
    }
}

/// Number of equal-angle chords so that the chord height stays within `tol`.
fn chord_count(r: f64, sweep: f64, tol: f64) -> usize {
    let step = if tol > 0.0 && tol < r {
        (2.0 * libm::acos(1.0 - tol / r)).min(FRAC_PI_2)
    } else {
        FRAC_PI_2
    };
    ((sweep.abs() / step).ceil() as usize).max(1)
}

/// Point at fraction `f` along an arc or helix from `a` to `b`.
fn helix_point(
    a: &AxisPos,
    b: &AxisPos,
    centre: Point3,
    normal: UnitVec3,
    sweep: f64,
    f: f64,
) -> AxisPos {
    let n = normal.get();
    let r0 = radial(a.xyz, centre, normal);
    let h0 = (a.xyz - centre).dot(n);
    let h1 = (b.xyz - centre).dot(n);
    let (s, c) = libm::sincos(sweep * f);
    // Rodrigues rotation of r0 (perpendicular to n) about n.
    let r: Vec3 = r0 * c + n.cross(r0) * s;
    AxisPos {
        xyz: centre + r + n * (h0 + (h1 - h0) * f),
        a: lerp_opt(a.a, b.a, f),
        b: lerp_opt(a.b, b.b, f),
    }
}

fn lerp_opt(x: Option<f64>, y: Option<f64>, f: f64) -> Option<f64> {
    match (x, y) {
        (Some(x), Some(y)) => Some(x + (y - x) * f),
        _ => x.or(y),
    }
}

fn lines_through(
    a: AxisPos,
    inner: impl Iterator<Item = AxisPos>,
    b: AxisPos,
    m: crate::MotionInfo,
) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut prev = a;
    for p in inner.chain(std::iter::once(b)) {
        out.push(Segment::Line { a: prev, b: p, m });
        prev = p;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::{FeedRef, Motion, MotionInfo};

    fn m() -> MotionInfo {
        MotionInfo::new(Motion::Cut, FeedRef::Cut, 0.01)
    }

    fn arc(r: f64, sweep: f64) -> Segment {
        let a = AxisPos::xyz(Point3::new(r, 0.0, 0.0));
        let (s, c) = libm::sincos(sweep);
        let b = AxisPos::xyz(Point3::new(r * c, r * s, 0.0));
        Segment::Arc {
            a,
            b,
            centre: Point3::ORIGIN,
            normal: UnitVec3::Z,
            sweep,
            m: m(),
        }
    }

    fn max_sagitta(original: &Segment, lines: &[Segment], r: f64) -> f64 {
        let _ = original;
        lines
            .iter()
            .map(|l| {
                let (a, b) = (l.start().unwrap().xyz, l.end().unwrap().xyz);
                let mid = a.lerp(b, 0.5);
                r - mid.to_vec().length()
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn linearize_arc_chord() {
        let s = arc(10.0, FRAC_PI_2);
        let lines = linearize(&s, 0.01);
        assert!(lines.len() > 1);
        assert!(max_sagitta(&s, &lines, 10.0) <= 0.01 + 1e-12);
        assert!(lines[0].start().unwrap().same_bits(s.start().unwrap()));
        assert!(
            lines
                .last()
                .unwrap()
                .end()
                .unwrap()
                .same_bits(s.end().unwrap())
        );
        for w in lines.windows(2) {
            assert!(w[0].end().unwrap().same_bits(w[1].start().unwrap()));
        }
    }

    #[test]
    fn linearize_tiny_and_huge() {
        for r in [1e-6, 1e5] {
            let s = arc(r, 1.0);
            let lines = linearize(&s, 0.001);
            assert!(!lines.is_empty());
            assert!(max_sagitta(&s, &lines, r) <= 0.001 + 1e-9 * r);
        }
    }

    #[test]
    fn linearize_full_circle_closes() {
        let a = AxisPos::xyz(Point3::new(1.0, 0.0, 0.0));
        let s = Segment::Arc {
            a,
            b: a,
            centre: Point3::ORIGIN,
            normal: UnitVec3::Z,
            sweep: -std::f64::consts::TAU,
            m: m(),
        };
        let lines = linearize(&s, 0.1);
        assert!(lines.len() >= 4);
        // Clockwise: first chord heads to negative y.
        assert!(lines[0].end().unwrap().xyz.y() < 0.0);
    }

    #[test]
    fn linearize_helix_descends_linearly() {
        let s = Segment::Helix {
            a: AxisPos::xyz(Point3::new(1.0, 0.0, 0.0)),
            b: AxisPos::xyz(Point3::new(1.0, 0.0, -4.0)),
            centre: Point3::ORIGIN,
            normal: UnitVec3::Z,
            sweep: std::f64::consts::TAU,
            m: m(),
        };
        let lines = linearize(&s, 0.01);
        let n = lines.len() as f64;
        for (i, l) in lines.iter().enumerate() {
            let want = -4.0 * (i + 1) as f64 / n;
            assert!((l.end().unwrap().xyz.z() - want).abs() < 1e-12);
        }
    }

    #[test]
    fn lines_pass_through() {
        let s = Segment::Line {
            a: AxisPos::xyz(Point3::ORIGIN),
            b: AxisPos::xyz(Point3::new(1.0, 0.0, 0.0)),
            m: m(),
        };
        assert_eq!(linearize(&s, 0.01), vec![s]);
    }
}
