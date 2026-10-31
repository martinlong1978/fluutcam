// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ray cast against a convex set given by its support function (GJK ray cast,
//! G. van den Bergen, "Ray Casting against General Convex Objects", 2004).

use glam::DVec3;

const MAX_ITER: usize = 96;

/// Smallest `λ ∈ [0, lambda_max]` with `s + λ·r` within `eps` of the convex
/// set, or `None` if the ray misses it.
pub fn ray_cast(
    support: impl Fn(DVec3) -> DVec3,
    s: DVec3,
    r: DVec3,
    lambda_max: f64,
    eps: f64,
) -> Option<f64> {
    let mut lambda = 0.0;
    let mut x = s;
    let mut pts = [DVec3::ZERO; 4];
    let mut n = 0;
    let mut v = x - support(r);
    for _ in 0..MAX_ITER {
        if v.length_squared() <= eps * eps {
            return Some(lambda);
        }
        let p = support(v);
        let w = x - p;
        let vw = v.dot(w);
        if vw > 0.0 {
            let vr = v.dot(r);
            if vr >= 0.0 {
                return None;
            }
            lambda -= vw / vr;
            if lambda > lambda_max {
                return None;
            }
            x = s + r * lambda;
        }
        pts[n] = p;
        n += 1;
        let mut y = [DVec3::ZERO; 4];
        for i in 0..n {
            y[i] = x - pts[i];
        }
        let (closest, keep) = closest_on_simplex(&y[..n]);
        v = closest;
        let mut m = 0;
        for i in 0..n {
            if keep & (1 << i) != 0 {
                pts[m] = pts[i];
                m += 1;
            }
        }
        n = m;
        if n == 4 {
            // Origin inside the tetrahedron.
            return Some(lambda);
        }
    }
    Some(lambda)
}

/// Point of the simplex `y` (1 to 4 points) closest to the origin, and the
/// bit mask of the vertices of the smallest face containing it.
fn closest_on_simplex(y: &[DVec3]) -> (DVec3, u8) {
    match y.len() {
        1 => (y[0], 1),
        2 => closest_on_segment(y[0], y[1]),
        3 => closest_on_triangle(y[0], y[1], y[2]),
        _ => closest_on_tetra(y[0], y[1], y[2], y[3]),
    }
}

fn closest_on_segment(a: DVec3, b: DVec3) -> (DVec3, u8) {
    let ab = b - a;
    let den = ab.length_squared();
    let t = if den > 0.0 { -a.dot(ab) / den } else { 0.0 };
    if t <= 0.0 {
        (a, 0b01)
    } else if t >= 1.0 {
        (b, 0b10)
    } else {
        (a + ab * t, 0b11)
    }
}

/// Closest point to the origin on triangle `abc` (C. Ericson, Real-Time
/// Collision Detection, §5.1.5).
fn closest_on_triangle(a: DVec3, b: DVec3, c: DVec3) -> (DVec3, u8) {
    let ab = b - a;
    let ac = c - a;
    let d1 = -ab.dot(a);
    let d2 = -ac.dot(a);
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, 0b001);
    }
    let d3 = -ab.dot(b);
    let d4 = -ac.dot(b);
    if d3 >= 0.0 && d4 <= d3 {
        return (b, 0b010);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (a + ab * v, 0b011);
    }
    let d5 = -ab.dot(c);
    let d6 = -ac.dot(c);
    if d6 >= 0.0 && d5 <= d6 {
        return (c, 0b100);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (a + ac * w, 0b101);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (b + (c - b) * w, 0b110);
    }
    let den = va + vb + vc;
    if den == 0.0 {
        // Degenerate (collinear) triangle: fall back to its edges.
        return [
            closest_on_segment(a, b),
            map(closest_on_segment(a, c), [0, 2]),
            map(closest_on_segment(b, c), [1, 2]),
        ]
        .into_iter()
        .min_by(|p, q| p.0.length_squared().total_cmp(&q.0.length_squared()))
        .unwrap_or((a, 1));
    }
    let v = vb / den;
    let w = vc / den;
    (a + ab * v + ac * w, 0b111)
}

/// Re-labels a sub-simplex mask: bit `i` of `r.1` becomes bit `to[i]`.
fn map(r: (DVec3, u8), to: [usize; 2]) -> (DVec3, u8) {
    let mut m = 0;
    for (i, &t) in to.iter().enumerate() {
        if r.1 & (1 << i) != 0 {
            m |= 1 << t;
        }
    }
    (r.0, m)
}

fn map3(r: (DVec3, u8), to: [usize; 3]) -> (DVec3, u8) {
    let mut m = 0;
    for (i, &t) in to.iter().enumerate() {
        if r.1 & (1 << i) != 0 {
            m |= 1 << t;
        }
    }
    (r.0, m)
}

fn closest_on_tetra(a: DVec3, b: DVec3, c: DVec3, d: DVec3) -> (DVec3, u8) {
    let p = [a, b, c, d];
    let faces = [[0, 1, 2, 3], [0, 1, 3, 2], [0, 2, 3, 1], [1, 2, 3, 0]];
    let vol = (b - a).dot((c - a).cross(d - a));
    let scale = (b - a).length() * (c - a).length() * (d - a).length();
    let degenerate = vol.abs() <= 1e-12 * scale;
    let mut best: Option<(DVec3, u8)> = None;
    for f in faces {
        let (fa, fb, fc, opp) = (p[f[0]], p[f[1]], p[f[2]], p[f[3]]);
        let n = (fb - fa).cross(fc - fa);
        let sign_o = -fa.dot(n);
        let sign_d = (opp - fa).dot(n);
        if degenerate || sign_o * sign_d < 0.0 {
            let r = map3(closest_on_triangle(fa, fb, fc), [f[0], f[1], f[2]]);
            if best.is_none_or(|b| r.0.length_squared() < b.0.length_squared()) {
                best = Some(r);
            }
        }
    }
    best.unwrap_or((DVec3::ZERO, 0b1111))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_cast_sphere() {
        let c = DVec3::new(5.0, 1.0, 0.0);
        let sup = |n: DVec3| c + n.normalize() * 2.0;
        let t = ray_cast(sup, DVec3::ZERO, DVec3::X, 100.0, 1e-9).unwrap();
        assert!((t - (5.0 - 3f64.sqrt())).abs() < 1e-6, "{t}");
        assert!(ray_cast(sup, DVec3::new(0.0, 3.5, 0.0), DVec3::X, 100.0, 1e-9).is_none());
        assert!(ray_cast(sup, DVec3::ZERO, -DVec3::X, 100.0, 1e-9).is_none());
        assert_eq!(ray_cast(sup, c, DVec3::X, 100.0, 1e-9), Some(0.0));
    }

    #[test]
    fn ray_cast_box() {
        let h = DVec3::new(1.0, 2.0, 3.0);
        let sup = |n: DVec3| {
            DVec3::new(
                if n.x >= 0.0 { h.x } else { -h.x },
                if n.y >= 0.0 { h.y } else { -h.y },
                if n.z >= 0.0 { h.z } else { -h.z },
            )
        };
        let t = ray_cast(sup, DVec3::new(-10.0, 0.5, 0.5), DVec3::X, 100.0, 1e-9).unwrap();
        assert!((t - 9.0).abs() < 1e-6, "{t}");
        let dir = DVec3::new(1.0, 1.0, 1.0).normalize();
        let t = ray_cast(sup, -dir * 20.0, dir, 100.0, 1e-9).unwrap();
        assert!((t - (20.0 - 3f64.sqrt())).abs() < 1e-6, "{t}");
    }

    #[test]
    fn closest_on_degenerate_tetra() {
        // Four coplanar points in z = 1: the closest point is (0, 0, 1).
        let (v, _) = closest_on_tetra(
            DVec3::new(-1.0, -1.0, 1.0),
            DVec3::new(1.0, -1.0, 1.0),
            DVec3::new(-1.0, 1.0, 1.0),
            DVec3::new(1.0, 1.0, 1.0),
        );
        assert!((v - DVec3::Z).length() < 1e-12);
    }
}
