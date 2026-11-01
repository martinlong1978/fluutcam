// SPDX-License-Identifier: MIT OR Apache-2.0

//! 2-D curves (D02 §3.1): p-curves in a surface's (u, v) plane.

use crate::bounds::Interval;
use crate::bspline::{BSplineCurve3, BSplineError};
use crate::types::{Point2, Point3, Vec2};
use std::sync::Arc;

/// A B-spline or NURBS curve in the plane. Stored as a 3-D curve with
/// `z = 0`, so it shares the 3-D evaluation code.
#[derive(Clone, PartialEq, Debug)]
pub struct BSplineCurve2(BSplineCurve3);

impl BSplineCurve2 {
    /// Same rules as [`BSplineCurve3::new`].
    pub fn new(
        degree: u8,
        knots: Vec<f64>,
        poles: Vec<Point2>,
        weights: Option<Vec<f64>>,
    ) -> Result<BSplineCurve2, BSplineError> {
        let poles = poles
            .into_iter()
            .map(|p| Point3::new(p.x(), p.y(), 0.0))
            .collect();
        BSplineCurve3::new(degree, knots, poles, weights).map(BSplineCurve2)
    }

    pub fn degree(&self) -> u8 {
        self.0.degree()
    }
    pub fn knots(&self) -> &[f64] {
        self.0.knots()
    }
    pub fn poles(&self) -> Vec<Point2> {
        self.0
            .poles()
            .iter()
            .map(|p| Point2::new(p.x(), p.y()))
            .collect()
    }
    pub fn weights(&self) -> Option<&[f64]> {
        self.0.weights()
    }
    pub fn domain(&self) -> Interval {
        self.0.domain()
    }

    /// Point at `t`; `t` outside the domain is clamped to it.
    pub fn eval(&self, t: f64) -> Point2 {
        let p = self.0.eval(t);
        Point2::new(p.x(), p.y())
    }

    /// First derivative at `t`.
    pub fn deriv(&self, t: f64) -> Vec2 {
        let d = self.0.derivs(t, 1)[1];
        Vec2::new(d.x(), d.y())
    }
}

/// A curve in the plane.
#[derive(Clone, PartialEq, Debug)]
pub enum Curve2 {
    /// `C(t) = origin + t·dir`; `dir` need not be a unit vector.
    Line {
        origin: Point2,
        dir: Vec2,
    },
    /// `C(t) = centre + cos t·a + sin t·b`. Holds circles of either sense
    /// and ellipses; `a` and `b` need not be perpendicular.
    Ellipse {
        centre: Point2,
        a: Vec2,
        b: Vec2,
    },
    BSpline(Arc<BSplineCurve2>),
}

impl Curve2 {
    pub fn eval(&self, t: f64) -> Point2 {
        match self {
            Curve2::Line { origin, dir } => *origin + *dir * t,
            Curve2::Ellipse { centre, a, b } => {
                let (s, c) = libm::sincos(t);
                *centre + *a * c + *b * s
            }
            Curve2::BSpline(b) => b.eval(t),
        }
    }

    /// First derivative at `t`.
    pub fn deriv(&self, t: f64) -> Vec2 {
        match self {
            Curve2::Line { dir, .. } => *dir,
            Curve2::Ellipse { a, b, .. } => {
                let (s, c) = libm::sincos(t);
                *b * c - *a * s
            }
            Curve2::BSpline(b) => b.deriv(t),
        }
    }

    /// The curve moved by `d`.
    pub fn translated(&self, d: Vec2) -> Curve2 {
        match self {
            Curve2::Line { origin, dir } => Curve2::Line {
                origin: *origin + d,
                dir: *dir,
            },
            Curve2::Ellipse { centre, a, b } => Curve2::Ellipse {
                centre: *centre + d,
                a: *a,
                b: *b,
            },
            Curve2::BSpline(c) => {
                let poles = c.poles().into_iter().map(|p| p + d).collect();
                let k = c.knots().to_vec();
                let w = c.weights().map(<[f64]>::to_vec);
                // Same knots and weights: cannot fail.
                Curve2::BSpline(Arc::new(
                    BSplineCurve2::new(c.degree(), k, poles, w).expect("valid B-spline"),
                ))
            }
        }
    }

    /// The curve with parameter `s = a + b·t`: `C′(t) = C(a + b·t)`. `None`
    /// for an ellipse unless `b = ±1`, and when `b` is zero or not finite.
    pub fn reparametrised(&self, a: f64, b: f64) -> Option<Curve2> {
        if !(a.is_finite() && b.is_finite()) || b == 0.0 {
            return None;
        }
        Some(match self {
            Curve2::Line { origin, dir } => Curve2::Line {
                origin: *origin + *dir * a,
                dir: *dir * b,
            },
            Curve2::Ellipse {
                centre,
                a: ea,
                b: eb,
            } => {
                // cos(a ± t)·A + sin(a ± t)·B, expanded in cos t and sin t.
                let sign = if b == 1.0 {
                    1.0
                } else if b == -1.0 {
                    -1.0
                } else {
                    return None;
                };
                let (s, c) = libm::sincos(a);
                Curve2::Ellipse {
                    centre: *centre,
                    a: *ea * c + *eb * s,
                    b: (*eb * c - *ea * s) * sign,
                }
            }
            Curve2::BSpline(c) => {
                let mut knots: Vec<f64> = c.knots().iter().map(|k| (k - a) / b).collect();
                let mut poles = c.poles();
                let mut w = c.weights().map(<[f64]>::to_vec);
                if b < 0.0 {
                    knots.reverse();
                    poles.reverse();
                    if let Some(w) = &mut w {
                        w.reverse();
                    }
                }
                Curve2::BSpline(Arc::new(
                    BSplineCurve2::new(c.degree(), knots, poles, w).ok()?,
                ))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fd(c: &Curve2, t: f64) -> Vec2 {
        let h = 1e-6;
        (c.eval(t + h) - c.eval(t - h)) * (0.5 / h)
    }

    fn close(a: Vec2, b: Vec2, tol: f64) -> bool {
        (a - b).length() < tol
    }

    fn spline() -> Curve2 {
        Curve2::BSpline(Arc::new(
            BSplineCurve2::new(
                3,
                vec![0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0, 1.0],
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 2.0),
                    Point2::new(2.0, -1.0),
                    Point2::new(3.0, 1.0),
                    Point2::new(4.0, 0.5),
                ],
                Some(vec![1.0, 0.5, 2.0, 1.0, 1.0]),
            )
            .unwrap(),
        ))
    }

    fn all() -> Vec<Curve2> {
        vec![
            Curve2::Line {
                origin: Point2::new(1.0, 2.0),
                dir: Vec2::new(0.5, -3.0),
            },
            Curve2::Ellipse {
                centre: Point2::new(1.0, 2.0),
                a: Vec2::new(2.0, 0.0),
                b: Vec2::new(0.3, -1.0),
            },
            spline(),
        ]
    }

    #[test]
    fn derivatives_match_finite_differences() {
        for c in all() {
            for t in [0.1, 0.35, 0.5, 0.8] {
                assert!(close(c.deriv(t), fd(&c, t), 1e-6), "{c:?} at {t}");
            }
        }
    }

    #[test]
    fn spline_matches_the_wrapped_3d_curve() {
        let Curve2::BSpline(b) = spline() else {
            unreachable!()
        };
        let p = b.0.eval(0.3);
        assert_eq!(b.eval(0.3), Point2::new(p.x(), p.y()));
        assert_eq!(p.z(), 0.0);
    }

    #[test]
    fn translation_moves_every_point() {
        let d = Vec2::new(-0.25, 7.0);
        for c in all() {
            let m = c.translated(d);
            for t in [0.0, 0.3, 1.0] {
                assert!(close(m.eval(t) - c.eval(t), d, 1e-12), "{c:?}");
            }
        }
    }

    #[test]
    fn reparametrisation_is_composition() {
        for (a, b) in [(0.2, 1.0), (1.0, -1.0), (0.5, 2.0), (0.9, -0.5)] {
            for c in all() {
                let Some(r) = c.reparametrised(a, b) else {
                    assert!(matches!(c, Curve2::Ellipse { .. }) && b.abs() != 1.0);
                    continue;
                };
                for t in [0.0, 0.1, 0.2] {
                    let s = a + b * t;
                    if let Curve2::BSpline(sp) = &c
                        && !sp.domain().contains(s)
                    {
                        continue;
                    }
                    assert!(
                        r.eval(t).distance(c.eval(s)) < 1e-12,
                        "{c:?} a={a} b={b} t={t}"
                    );
                }
            }
        }
    }
}
