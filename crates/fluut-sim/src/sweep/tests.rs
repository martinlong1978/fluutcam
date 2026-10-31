// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use glam::DVec3;

/// Deterministic pseudo-random numbers in [0, 1).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }

    fn unit(&mut self) -> DVec3 {
        loop {
            let v = DVec3::new(
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
            );
            if v.length_squared() > 0.01 && v.length_squared() <= 1.0 {
                return v.normalize();
            }
        }
    }
}

fn pose(tip: DVec3, axis: DVec3) -> Pose {
    Pose {
        tip,
        axis: axis.normalize(),
        length: 6.0,
    }
}

fn axial_sweep(tool: Tool, d: DVec3) -> Sweep {
    let a = DVec3::new(0.3, -0.2, 0.1);
    Sweep::new(tool, pose(a, DVec3::Z), pose(a + d, DVec3::Z))
}

/// Interval of the line inside the union of `n` poses interpolated along the
/// chord, by sampling `t` at step `dt`. Exact for fixed-axis chords up to the
/// sampling.
fn brute(s: &Sweep, o: DVec3, e: DVec3, dt: f64, n: usize) -> Option<(f64, f64)> {
    let (ta, tb) = s.slab(o, e)?;
    let mut out: Option<(f64, f64)> = None;
    let mut t = ta;
    while t <= tb {
        let p = o + e * t;
        let inside = (0..=n).any(|i| {
            let b = i as f64 / n as f64;
            let q = Pose {
                tip: s.p0.tip.lerp(s.p1.tip, b),
                axis: s.p0.axis,
                length: s.p0.length,
            };
            q.contains(&s.tool, p)
        });
        if inside {
            out = Some(out.map_or((t, t), |(a, _)| (a, t)));
        }
        t += dt;
    }
    out
}

fn close(a: Option<(f64, f64)>, b: Option<(f64, f64)>, tol: f64) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => (x.0 - y.0).abs() <= tol && (x.1 - y.1).abs() <= tol,
        // Grazing lines: one method may see a sliver the other misses.
        (Some(x), None) | (None, Some(x)) => x.1 - x.0 <= 2.0 * tol,
    }
}

#[test]
fn ball_along_line_is_capsule() {
    let s = axial_sweep(Tool::ball(1.0), DVec3::new(4.0, 0.0, 0.0));
    // Vertical line through the middle of the move, 0.6 off the centre line:
    // bottom at the sphere centre height minus sqrt(1 − 0.36).
    let o = DVec3::new(2.3, -0.2 + 0.6, -10.0);
    let (t0, t1) = s.ray(o, DVec3::Z).unwrap();
    assert!((t0 - (10.0 + 0.1 + 1.0 - 0.8)).abs() < 1e-12, "{t0}");
    assert!((t1 - (10.0 + 0.1 + 6.0 + 0.8)).abs() < 1e-12, "{t1}");
    // Horizontal line along the move at the centre height of the ball.
    let o = DVec3::new(-10.0, -0.2, 1.1);
    let (t0, t1) = s.ray(o, DVec3::X).unwrap();
    assert!(
        (t0 - 9.3).abs() < 1e-12 && (t1 - 15.3).abs() < 1e-12,
        "{t0} {t1}"
    );
}

#[test]
fn flat_plunge_is_cylinder() {
    let s = axial_sweep(Tool::flat(2.0), DVec3::new(0.0, 0.0, -3.0));
    assert_eq!(s.method(DVec3::Z), Method::Axial);
    let (t0, t1) = s.ray(DVec3::new(0.3 + 1.9, -0.2, -10.0), DVec3::Z).unwrap();
    assert!((t0 - (10.0 + 0.1 - 3.0)).abs() < 1e-12 && (t1 - (10.0 + 6.1)).abs() < 1e-12);
    assert!(
        s.ray(DVec3::new(0.3 + 2.1, -0.2, -10.0), DVec3::Z)
            .is_none()
    );
    // A sideways line through the plunge, by GJK.
    let (t0, t1) = s.ray(DVec3::new(-10.0, -0.2, 0.0), DVec3::X).unwrap();
    assert!(
        (t0 - 8.3).abs() < 2e-3 && (t1 - 12.3).abs() < 2e-3,
        "{t0} {t1}"
    );
}

#[test]
fn bull_vertical_depth_follows_profile() {
    let tool = Tool::bull(3.0, 1.0);
    let s = axial_sweep(tool, DVec3::ZERO);
    for rho in [0.0, 1.5, 2.0, 2.5, 2.9, 3.0] {
        let o = DVec3::new(0.3 + rho, -0.2, -5.0);
        let (t0, _) = s.ray(o, DVec3::Z).unwrap();
        let want = 5.1 + tool.bottom(rho).unwrap();
        assert!((t0 - want).abs() < 1e-12, "{rho}: {t0} vs {want}");
        // A downward line starting at the bottom point ends there.
        let (_, t1) = s.ray(DVec3::new(o.x, o.y, -5.0 + want), -DVec3::Z).unwrap();
        assert!(t1.abs() < 1e-12, "{t1}");
    }
}

#[test]
fn all_methods_match_brute_force() {
    let mut rng = Lcg(7);
    for tool in [Tool::flat(1.0), Tool::ball(1.0), Tool::bull(1.0, 0.4)] {
        for _ in 0..12 {
            let d = DVec3::new(
                rng.range(-2.0, 2.0),
                rng.range(-2.0, 2.0),
                rng.range(-1.0, 1.0),
            );
            let s = axial_sweep(tool, d);
            for k in 0..3 {
                let mut e = DVec3::ZERO;
                e[k] = 1.0;
                let mut o = DVec3::new(
                    rng.range(-1.5, 2.5),
                    rng.range(-1.5, 2.5),
                    rng.range(-0.5, 3.0),
                );
                o[k] = -10.0;
                let got = s.ray(o, e);
                let want = brute(&s, o, e, 0.004, 300);
                assert!(
                    close(got, want, 0.02),
                    "{tool:?} d={d} o={o} e={e}: {got:?} vs {want:?}"
                );
            }
        }
    }
}

#[test]
fn gjk_matches_closed_forms() {
    let mut rng = Lcg(11);
    for _ in 0..200 {
        let axis = rng.unit();
        let tip = DVec3::new(
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
        );
        let d = DVec3::new(
            rng.range(-3.0, 3.0),
            rng.range(-3.0, 3.0),
            rng.range(-3.0, 3.0),
        );
        // Rotary chords: the second axis may differ.
        let axis1 = if rng.next() < 0.5 {
            axis
        } else {
            (axis + rng.unit() * 0.2).normalize()
        };
        let s = Sweep::new(Tool::ball(1.0), pose(tip, axis), pose(tip + d, axis1));
        let e = rng.unit();
        let o = DVec3::new(
            rng.range(-2.0, 2.0),
            rng.range(-2.0, 2.0),
            rng.range(-2.0, 2.0),
        ) - e * 20.0;
        let exact = s.ray(o, e);
        let sup = |n: DVec3| s.support(n);
        let gjk = s.slab(o, e).and_then(|(ta, tb)| {
            let t0 = ray_cast(sup, o + e * ta, e, tb - ta, 1e-7)?;
            let t1 = ray_cast(sup, o + e * tb, -e, tb - ta, 1e-7)?;
            Some((ta + t0, tb - t1))
        });
        assert!(close(exact, gjk, 1e-3), "{exact:?} vs {gjk:?}");
    }
}

#[test]
fn axial_matches_gjk_for_bull() {
    let mut rng = Lcg(3);
    let tool = Tool::bull(2.0, 0.7);
    for _ in 0..300 {
        let d = DVec3::new(
            rng.range(-3.0, 3.0),
            rng.range(-3.0, 3.0),
            rng.range(-2.0, 2.0),
        );
        let s = axial_sweep(tool, d);
        let o = DVec3::new(rng.range(-3.0, 4.0), rng.range(-3.0, 4.0), -20.0);
        let axial = s.ray(o, DVec3::Z);
        let sup = |n: DVec3| s.support(n);
        let gjk = s.slab(o, DVec3::Z).and_then(|(ta, tb)| {
            let t0 = ray_cast(sup, o + DVec3::Z * ta, DVec3::Z, tb - ta, 1e-7)?;
            let t1 = ray_cast(sup, o + DVec3::Z * tb, -DVec3::Z, tb - ta, 1e-7)?;
            Some((ta + t0, tb - t1))
        });
        assert!(close(axial, gjk, 1e-3), "d={d} o={o}: {axial:?} vs {gjk:?}");
    }
}

#[test]
fn ball_axial_matches_general_ball() {
    let mut rng = Lcg(5);
    for _ in 0..300 {
        let d = DVec3::new(
            rng.range(-3.0, 3.0),
            rng.range(-3.0, 3.0),
            rng.range(-2.0, 2.0),
        );
        let s = axial_sweep(Tool::ball(1.2), d);
        let up = rng.next() < 0.5;
        let e = if up { DVec3::Z } else { -DVec3::Z };
        let o = DVec3::new(
            rng.range(-3.0, 4.0),
            rng.range(-3.0, 4.0),
            if up { -20.0 } else { 20.0 },
        );
        assert_eq!(s.method(e), Method::Axial);
        let a = s.ball_axial_ray(o, e);
        let b = s.ball_ray(o, e);
        assert!(close(a, b, 1e-9), "d={d} o={o}: {a:?} vs {b:?}");
    }
}

#[test]
fn side_matches_gjk() {
    let mut rng = Lcg(9);
    for tool in [Tool::flat(2.0), Tool::bull(2.0, 0.6)] {
        for _ in 0..300 {
            let d = DVec3::new(rng.range(-3.0, 3.0), rng.range(-3.0, 3.0), 0.0);
            let s = axial_sweep(tool, d);
            let e = if rng.next() < 0.5 {
                DVec3::X
            } else {
                -DVec3::Y
            };
            let mut o = DVec3::new(
                rng.range(-3.0, 4.0),
                rng.range(-3.0, 4.0),
                rng.range(-0.2, 7.5),
            );
            if e.x != 0.0 {
                o.x = -20.0;
            } else {
                o.y = 20.0;
            }
            assert_eq!(s.method(e), Method::Side);
            let side = s.ray(o, e);
            let sup = |n: DVec3| s.support(n);
            let gjk = s.slab(o, e).and_then(|(ta, tb)| {
                let t0 = ray_cast(sup, o + e * ta, e, tb - ta, 1e-7)?;
                let t1 = ray_cast(sup, o + e * tb, -e, tb - ta, 1e-7)?;
                Some((ta + t0, tb - t1))
            });
            assert!(
                close(side, gjk, 1e-3),
                "{tool:?} d={d} o={o}: {side:?} vs {gjk:?}"
            );
        }
    }
}
