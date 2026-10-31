// SPDX-License-Identifier: MIT OR Apache-2.0

//! Root finding (D02 §6).
//!
//! Closed forms give seeds; every seed is polished with Newton on the original
//! polynomial. Iterative solvers always work inside a bracket and report failure
//! instead of returning an unconverged value (S 04 §4.2.4).

use smallvec::SmallVec;
use std::f64::consts::PI;

/// Maximum Newton iterations before [`RootError::NotConverged`] (D02 §6).
pub const MAX_ITERATIONS: usize = 50;

#[derive(Copy, Clone, PartialEq, Debug, thiserror::Error)]
pub enum RootError {
    #[error("function has the same sign at both bracket ends")]
    NoSignChange,
    #[error("root not converged after {MAX_ITERATIONS} iterations (best {best})")]
    NotConverged { best: f64 },
}

/// Real roots of `a·x² + b·x + c = 0`, ascending. A double root is returned once.
/// `a == 0` is solved as linear; `a == b == 0` has no roots.
pub fn quadratic(a: f64, b: f64, c: f64) -> SmallVec<[f64; 2]> {
    let mut out = SmallVec::new();
    if a == 0.0 {
        if b != 0.0 {
            out.push(-c / b);
        }
        return out;
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return out;
    }
    if disc == 0.0 {
        out.push(-b / (2.0 * a));
        return out;
    }
    // Stable form: avoids cancellation between -b and sqrt(disc).
    let q = -0.5 * (b + b.signum() * disc.sqrt());
    let (r1, r2) = if q == 0.0 {
        // b == 0 and c == 0 cannot reach here (disc would be 0), so c/a ≤ 0.
        let r = (-c / a).sqrt();
        (-r, r)
    } else {
        (q / a, c / q)
    };
    out.push(r1.min(r2));
    out.push(r1.max(r2));
    out
}

/// Real roots of `a·x³ + b·x² + c·x + d = 0`, ascending, with double roots
/// returned once. Falls back to [`quadratic`] when `a == 0`.
pub fn cubic(a: f64, b: f64, c: f64, d: f64) -> SmallVec<[f64; 3]> {
    if a == 0.0 {
        return quadratic(b, c, d).into_iter().collect();
    }
    let f = |x: f64| ((a * x + b) * x + c) * x + d;
    let df = |x: f64| (3.0 * a * x + 2.0 * b) * x + c;
    // Size of the terms of f at x, for relative "is zero" tests.
    let scale = |x: f64| {
        let ax = x.abs();
        ((a.abs() * ax + b.abs()) * ax + c.abs()) * ax + d.abs()
    };

    let (bn, cn, dn) = (b / a, c / a, d / a);
    let shift = bn / 3.0;
    let p = cn - bn * bn / 3.0;
    let q = 2.0 * bn * bn * bn / 27.0 - bn * cn / 3.0 + dn;
    let half_q = q / 2.0;
    let third_p = p / 3.0;
    let delta = half_q * half_q + third_p * third_p * third_p;

    let mut roots: SmallVec<[f64; 3]> = SmallVec::new();
    if delta > 0.0 {
        let s = delta.sqrt();
        let y = libm::cbrt(-half_q + s) + libm::cbrt(-half_q - s);
        roots.push(y - shift);
    } else if p == 0.0 {
        // delta <= 0 and p == 0 imply q == 0: triple root.
        roots.push(-shift);
    } else {
        let m = 2.0 * (-third_p).sqrt();
        let arg = (3.0 * q / (p * m)).clamp(-1.0, 1.0);
        let theta = libm::acos(arg) / 3.0;
        for k in 0..3 {
            roots.push(m * libm::cos(theta - 2.0 * PI * k as f64 / 3.0) - shift);
        }
    }

    // Polish each seed on the original polynomial.
    for r in roots.iter_mut() {
        for _ in 0..2 {
            let d1 = df(*r);
            if d1 == 0.0 {
                break;
            }
            let next = *r - f(*r) / d1;
            if next.is_finite() && f(next).abs() <= f(*r).abs() {
                *r = next;
            }
        }
    }

    // Derivative test: a critical point where f vanishes is a double root. It
    // replaces nearby seeds (which a double root makes inaccurate) and is added
    // if the closed form missed it.
    for xc in quadratic(3.0 * a, 2.0 * b, c) {
        if f(xc).abs() <= 1e-14 * scale(xc) {
            roots.retain(|r| (*r - xc).abs() > 1e-6 * (1.0 + xc.abs()));
            roots.push(xc);
        }
    }

    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|r2, r1| (*r2 - *r1).abs() <= 1e-12 * (1.0 + r1.abs()));
    roots
}

/// Finds a root of `f` in `[lo, hi]`, where `f(x)` returns `(value, derivative)`.
///
/// Newton steps are used while they stay inside the bracket and shrink faster
/// than bisection would (step less than half the previous step, as in
/// `rtsafe`); otherwise the step is a bisection. Converged when a step or the
/// bracket is shorter than `tol_x`. A root at either end is returned as is.
///
/// Simple roots converge quadratically. Multiple roots converge linearly with
/// at least one bracket halving per two iterations, so a tight `tol_x` on a
/// wide bracket can return [`RootError::NotConverged`].
pub fn newton_bracketed(
    f: impl Fn(f64) -> (f64, f64),
    mut lo: f64,
    mut hi: f64,
    tol_x: f64,
) -> Result<f64, RootError> {
    let (flo, _) = f(lo);
    if flo == 0.0 {
        return Ok(lo);
    }
    let (fhi, _) = f(hi);
    if fhi == 0.0 {
        return Ok(hi);
    }
    if flo.signum() == fhi.signum() || flo.is_nan() || fhi.is_nan() {
        return Err(RootError::NoSignChange);
    }
    let lo_sign = flo.signum();

    let mut x = 0.5 * (lo + hi);
    let mut last_step = hi - lo;
    for _ in 0..MAX_ITERATIONS {
        let (fx, dfx) = f(x);
        if fx == 0.0 {
            return Ok(x);
        }
        if fx.signum() == lo_sign {
            lo = x;
        } else {
            hi = x;
        }
        let newton = x - fx / dfx;
        let use_newton = newton.is_finite()
            && newton > lo
            && newton < hi
            && (newton - x).abs() < 0.5 * last_step;
        let next = if use_newton { newton } else { 0.5 * (lo + hi) };
        last_step = (next - x).abs();
        if (next - x).abs() < tol_x || hi - lo < tol_x {
            return Ok(next);
        }
        x = next;
    }
    Err(RootError::NotConverged { best: x })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn quadratic_cancellation() {
        let r = quadratic(1.0, -1e8, 1.0);
        assert_eq!(r.len(), 2);
        assert!(((r[0] - 1e-8) / 1e-8).abs() < 1e-15);
        assert!(((r[1] - 1e8) / 1e8).abs() < 1e-15);
    }

    #[test]
    fn quadratic_double_root() {
        assert_eq!(quadratic(1.0, -2.0, 1.0).as_slice(), &[1.0]);
    }

    #[test]
    fn quadratic_none() {
        assert!(quadratic(1.0, 0.0, 1.0).is_empty());
    }

    #[test]
    fn quadratic_symmetric() {
        assert_eq!(quadratic(1.0, 0.0, -4.0).as_slice(), &[-2.0, 2.0]);
    }

    #[test]
    fn quadratic_linear_fallback() {
        assert_eq!(quadratic(0.0, 2.0, -4.0).as_slice(), &[2.0]);
        assert!(quadratic(0.0, 0.0, 1.0).is_empty());
    }

    #[test]
    fn cubic_three_roots() {
        // (x-1)(x-2)(x-3) = x³ - 6x² + 11x - 6
        let r = cubic(1.0, -6.0, 11.0, -6.0);
        assert_eq!(r.len(), 3);
        for (got, want) in r.iter().zip([1.0, 2.0, 3.0]) {
            assert!((got - want).abs() < 1e-12, "{r:?}");
        }
    }

    #[test]
    fn cubic_double_root() {
        // (x-1)²(x+2) = x³ - 3x + 2
        let r = cubic(1.0, 0.0, -3.0, 2.0);
        assert_eq!(r.len(), 2, "{r:?}");
        assert!((r[0] + 2.0).abs() < 1e-12);
        assert!((r[1] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn cubic_shifted_double_root() {
        // (x-0.1)²(x-5): delta is not exactly zero in floating point.
        let (r1, r2) = (0.1, 5.0);
        let b = -(2.0 * r1 + r2);
        let c = r1 * r1 + 2.0 * r1 * r2;
        let d = -r1 * r1 * r2;
        let r = cubic(1.0, b, c, d);
        assert_eq!(r.len(), 2, "{r:?}");
        assert!((r[0] - 0.1).abs() < 1e-9);
        assert!((r[1] - 5.0).abs() < 1e-12);
    }

    #[test]
    fn cubic_triple_root() {
        // (x-2)³ = x³ - 6x² + 12x - 8
        let r = cubic(1.0, -6.0, 12.0, -8.0);
        assert_eq!(r.len(), 1, "{r:?}");
        assert!((r[0] - 2.0).abs() < 1e-5);
    }

    #[test]
    fn cubic_one_real_root() {
        // x³ + x + 1 has one real root near -0.6823278
        let r = cubic(1.0, 0.0, 1.0, 1.0);
        assert_eq!(r.len(), 1);
        assert!((r[0] + 0.682_327_803_828_019_3).abs() < 1e-14);
    }

    #[test]
    fn newton_endpoint_root() {
        assert_eq!(newton_bracketed(|x| (x, 1.0), 0.0, 1.0, 1e-12), Ok(0.0));
        assert_eq!(
            newton_bracketed(|x| (x - 1.0, 1.0), 0.0, 1.0, 1e-12),
            Ok(1.0)
        );
    }

    #[test]
    fn newton_no_sign_change() {
        assert_eq!(
            newton_bracketed(|x| (x * x + 1.0, 2.0 * x), -1.0, 1.0, 1e-12),
            Err(RootError::NoSignChange)
        );
    }

    #[test]
    fn newton_cos() {
        let r = newton_bracketed(|x| (libm::cos(x), -libm::sin(x)), 1.0, 2.0, 1e-15).unwrap();
        assert!((r - PI / 2.0).abs() < 1e-14);
    }

    #[test]
    fn newton_zero_derivative_falls_back_to_bisection() {
        // f = x³ has f'(0) = 0 at the root. A triple root makes Newton linear, so
        // convergence comes from the interleaved bisections (about one bracket
        // halving per two iterations).
        let r = newton_bracketed(|x| (x * x * x, 3.0 * x * x), -1.0, 2.0, 1e-6).unwrap();
        assert!(r.abs() < 1e-5);
    }

    proptest! {
        #[test]
        fn cubic_recovers_separated_roots(
            a in -10.0f64..10.0, gap1 in 0.5f64..10.0, gap2 in 0.5f64..10.0,
            k in prop_oneof![-10.0f64..-0.1, 0.1f64..10.0],
        ) {
            // Root accuracy is about ε·|terms| / |f'(r)|; these ranges keep that
            // below 1e-8 (closer roots are covered by the double-root tests).
            let (r1, r2, r3) = (a, a + gap1, a + gap1 + gap2);
            let b = -k * (r1 + r2 + r3);
            let c = k * (r1 * r2 + r1 * r3 + r2 * r3);
            let d = -k * r1 * r2 * r3;
            let r = cubic(k, b, c, d);
            prop_assert_eq!(r.len(), 3, "{:?}", r);
            for (got, want) in r.iter().zip([r1, r2, r3]) {
                prop_assert!((got - want).abs() < 1e-8 * (1.0 + want.abs()), "{:?}", r);
            }
        }
    }
}
