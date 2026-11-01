// SPDX-License-Identifier: MIT OR Apache-2.0

//! Curves under affine maps, without losing arcs.
//!
//! [`Curve3::transformed`] accepts only similarities for circles and
//! ellipses. Blocks and SVG transforms may scale unevenly or skew, which
//! turns a circle into an ellipse. Here a circle or ellipse is held as
//! `C(t) = o + a·cos t + b·sin t` with conjugate semi-diameters `a` and `b`,
//! which any affine map sends to another such form. The result is put back
//! on its principal axes with a shift of parameter.

use fluut_geom::{Affine, Curve3, Frame, Interval, Point3, UnitVec3, Vec3};
use fluut_topo::WireCurve;
use std::f64::consts::TAU;
use std::sync::Arc;

/// Relative difference of the semi-axes below which an ellipse is a circle.
const CIRCLE_TOL: f64 = 1e-9;

/// The segment from `a` to `b`, parametrised by length; `None` if it has
/// none.
pub fn line(a: Point3, b: Point3) -> Option<WireCurve> {
    let len = a.distance(b);
    if !(len.is_finite() && len > 0.0) {
        return None;
    }
    Some(WireCurve {
        curve: Curve3::line(a, UnitVec3::new(b - a)?),
        range: Interval { lo: 0.0, hi: len },
    })
}

/// The arc `o + a·cos t + b·sin t` for `t` in `range`, as a circle or an
/// ellipse on its principal axes. The range is shifted to start in
/// `[0, 2π)`. `None` if the arc is degenerate (axes parallel or zero) or not
/// finite.
pub fn arc(o: Point3, a: Vec3, b: Vec3, range: Interval) -> Option<WireCurve> {
    if !(o.is_finite() && a.is_finite() && b.is_finite())
        || !(range.lo.is_finite() && range.hi.is_finite() && range.hi > range.lo)
    {
        return None;
    }
    let (aa, bb, ab) = (a.dot(a), b.dot(b), a.dot(b));
    let scale = aa.max(bb);
    if scale.is_nan() || scale <= 0.0 {
        return None;
    }
    let circular = (aa - bb).abs() <= CIRCLE_TOL * scale && ab.abs() <= CIRCLE_TOL * scale;
    // Principal axes: |u(t)| is largest at 2·t0 = atan2(2 a·b, |a|² − |b|²).
    let t0 = if circular {
        0.0
    } else {
        0.5 * libm::atan2(2.0 * ab, aa - bb)
    };
    let (s0, c0) = libm::sincos(t0);
    let u = a * c0 + b * s0;
    let v = b * c0 - a * s0;
    let z = u.cross(v);
    if z.length().is_nan() || z.length() <= 1e-12 * scale {
        return None;
    }
    let frame = Frame::new(o, z, u)?;
    let (ra, rb) = (u.length(), v.length());
    let curve = if circular {
        Curve3::circle(frame, 0.5 * (ra + rb))?
    } else {
        Curve3::ellipse(frame, ra, rb)?
    };
    let shift = range.lo - t0;
    let turns = (shift / TAU).floor();
    let lo = shift - turns * TAU;
    Some(WireCurve {
        curve,
        range: Interval {
            lo,
            hi: lo + range.length(),
        },
    })
}

/// `c` mapped by `m`: same points, mapped. `None` if the result is
/// degenerate (for example a circle squashed to a line).
pub fn transform(c: &WireCurve, m: &Affine) -> Option<WireCurve> {
    match &c.curve {
        Curve3::Line { .. } => line(
            m.apply_point(c.curve.eval(c.range.lo)),
            m.apply_point(c.curve.eval(c.range.hi)),
        ),
        Curve3::Circle { frame, radius } => arc(
            m.apply_point(frame.origin),
            m.apply_vec(frame.x.get() * *radius),
            m.apply_vec(frame.y().get() * *radius),
            c.range,
        ),
        Curve3::Ellipse { frame, a, b } => arc(
            m.apply_point(frame.origin),
            m.apply_vec(frame.x.get() * *a),
            m.apply_vec(frame.y().get() * *b),
            c.range,
        ),
        Curve3::BSpline(s) => {
            let t = s.transformed(m);
            t.poles().iter().all(|p| p.is_finite()).then(|| WireCurve {
                curve: Curve3::BSpline(Arc::new(t)),
                range: c.range,
            })
        }
        Curve3::Offset { .. } => Some(WireCurve {
            curve: c.curve.transformed(m)?,
            range: c.range,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{DMat3, DVec3};

    /// Points of `c` at `n + 1` even steps of its range.
    fn samples(c: &WireCurve, n: usize) -> Vec<Point3> {
        (0..=n)
            .map(|i| c.curve.eval(c.range.at(i as f64 / n as f64)))
            .collect()
    }

    fn close(a: Point3, b: Point3) -> bool {
        a.distance(b) < 1e-9
    }

    fn quarter_arc() -> WireCurve {
        arc(
            Point3::new(1.0, 2.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(0.0, 3.0, 0.0),
            Interval {
                lo: 0.3,
                hi: 0.3 + std::f64::consts::FRAC_PI_2,
            },
        )
        .unwrap()
    }

    #[test]
    fn arc_on_principal_axes() {
        let c = quarter_arc();
        assert!(matches!(c.curve, Curve3::Circle { radius, .. } if (radius - 3.0).abs() < 1e-12));
        // Conjugate diameters of a tilted ellipse give the same points.
        let (a, b) = (Vec3::new(4.0, 1.0, 0.0), Vec3::new(1.0, 2.0, 0.5));
        let r = Interval { lo: -1.0, hi: 2.5 };
        let e = arc(Point3::ORIGIN, a, b, r).unwrap();
        assert!(e.range.lo >= 0.0 && e.range.lo < TAU);
        assert!(matches!(e.curve, Curve3::Ellipse { .. }));
        for i in 0..=20 {
            let t = r.at(i as f64 / 20.0);
            let want = Point3::ORIGIN + a * libm::cos(t) + b * libm::sin(t);
            let got = e.curve.eval(e.range.at(i as f64 / 20.0));
            assert!(close(got, want), "{i}: {got:?} {want:?}");
        }
    }

    #[test]
    fn transforms_keep_the_point_set() {
        let rot = DMat3::from_axis_angle(DVec3::new(0.3, -0.2, 1.0).normalize(), 0.9);
        let maps = [
            Affine::from_mat3_translation(rot, Vec3::new(5.0, -1.0, 2.0)),
            Affine::scale(-1.0, 1.0, 1.0),
            Affine::scale(2.0, 0.5, 1.0),
            // Skew x by y.
            Affine::from_mat3_translation(
                DMat3::from_cols(DVec3::X, DVec3::new(0.7, 1.0, 0.0), DVec3::Z),
                Vec3::ZERO,
            ),
            Affine::from_mat3_translation(rot, Vec3::ZERO).then(&Affine::scale(1.0, -3.0, 1.0)),
        ];
        let bspline = {
            let poles = vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 2.0, 0.0),
                Point3::new(3.0, -1.0, 1.0),
                Point3::new(4.0, 0.0, 0.0),
            ];
            let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
            WireCurve {
                curve: Curve3::BSpline(Arc::new(
                    fluut_geom::BSplineCurve3::new(3, knots, poles, None).unwrap(),
                )),
                range: Interval { lo: 0.0, hi: 1.0 },
            }
        };
        let curves = [
            quarter_arc(),
            line(Point3::new(1.0, 1.0, 1.0), Point3::new(4.0, -2.0, 0.0)).unwrap(),
            arc(
                Point3::new(-2.0, 0.0, 0.0),
                Vec3::new(4.0, 1.0, 0.0),
                Vec3::new(-0.5, 2.0, 0.0),
                Interval { lo: 0.0, hi: TAU },
            )
            .unwrap(),
            bspline,
        ];
        for (mi, m) in maps.iter().enumerate() {
            for (ci, c) in curves.iter().enumerate() {
                let t = transform(c, m).unwrap();
                let want: Vec<Point3> = samples(c, 16).iter().map(|&p| m.apply_point(p)).collect();
                let got = samples(&t, 16);
                for (g, w) in got.iter().zip(&want) {
                    assert!(close(*g, *w), "map {mi}, curve {ci}: {g:?} {w:?}");
                }
            }
        }
    }

    #[test]
    fn degenerate_maps_give_none() {
        let flat = Affine::scale(1.0, 0.0, 1.0);
        assert!(transform(&quarter_arc(), &flat).is_none());
        assert!(line(Point3::ORIGIN, Point3::ORIGIN).is_none());
        let nan = Interval {
            lo: 0.0,
            hi: f64::NAN,
        };
        assert!(
            arc(
                Point3::ORIGIN,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                nan
            )
            .is_none()
        );
    }
}
