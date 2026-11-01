// SPDX-License-Identifier: MIT OR Apache-2.0

//! Sites of the medial axis, their bisectors and the points equidistant
//! from three of them (CH-17).
//!
//! There are two kinds of site. A line has a unit direction and a length;
//! the region lies on its left. A circle has a centre, a radius (0 for a
//! reflex vertex), a start angle and a signed sweep: the region is inside
//! the circle when the sweep is positive (the boundary turns left) and
//! outside when it is negative. Each site has a parameter, the arc length
//! along a line or the angle turned along a circle, from 0 to [`Geom::len`].
//! The points whose foot lies within that range are the site's *cone*.

use fluut_geom::{Point2, Vec2};
use std::f64::consts::TAU;

/// The geometry of a site.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Geom {
    /// From `a` along the unit direction `t` for `len`.
    Line { a: Point2, t: Vec2, len: f64 },
    /// Centre `c`, radius `r`, from angle `start` through the signed
    /// `sweep`.
    Circle {
        c: Point2,
        r: f64,
        start: f64,
        sweep: f64,
    },
}

/// The unit vector at `angle`.
pub fn dir(angle: f64) -> Vec2 {
    let (s, c) = libm::sincos(angle);
    Vec2::new(c, s)
}

fn left(v: Vec2) -> Vec2 {
    Vec2::new(-v.y(), v.x())
}

fn angle_of(v: Vec2) -> f64 {
    libm::atan2(v.y(), v.x())
}

impl Geom {
    /// Length of the parameter range.
    pub fn len(&self) -> f64 {
        match *self {
            Geom::Line { len, .. } => len,
            Geom::Circle { sweep, .. } => sweep.abs(),
        }
    }

    /// −1 when the region is inside the circle, +1 when outside (and for
    /// lines, where it is not used).
    pub fn side(&self) -> f64 {
        match *self {
            Geom::Circle { sweep, .. } if sweep > 0.0 => -1.0,
            _ => 1.0,
        }
    }

    /// Whether this is an arc with the region inside its circle.
    pub fn is_concave(&self) -> bool {
        matches!(*self, Geom::Circle { r, sweep, .. } if sweep > 0.0 && r > 0.0)
    }

    fn angle(&self, u: f64) -> f64 {
        match *self {
            Geom::Circle { start, sweep, .. } => start + u * sweep.signum(),
            Geom::Line { .. } => 0.0,
        }
    }

    /// The boundary point at parameter `u`.
    pub fn foot(&self, u: f64) -> Point2 {
        match *self {
            Geom::Line { a, t, .. } => a + t * u,
            Geom::Circle { c, r, .. } => c + dir(self.angle(u)) * r,
        }
    }

    /// The unit normal at parameter `u`, into the region.
    pub fn normal(&self, u: f64) -> Vec2 {
        match *self {
            Geom::Line { t, .. } => left(t),
            Geom::Circle { .. } => dir(self.angle(u)) * self.side(),
        }
    }

    /// Signed distance from the supporting line or circle, positive on the
    /// region side.
    pub fn dist(&self, p: Point2) -> f64 {
        match *self {
            Geom::Line { a, t, .. } => (p - a).dot(left(t)),
            Geom::Circle { c, r, .. } => self.side() * (p.distance(c) - r),
        }
    }

    /// Parameter of the foot of `p` on the supporting line or circle. For a
    /// circle the angle is taken in the range centred on the arc, so points
    /// just before the start give small negative values.
    pub fn param(&self, p: Point2) -> f64 {
        match *self {
            Geom::Line { a, t, .. } => (p - a).dot(t),
            Geom::Circle {
                c, start, sweep, ..
            } => {
                let d = ((angle_of(p - c) - start) * sweep.signum()).rem_euclid(TAU);
                let len = sweep.abs();
                if d > len + 0.5 * (TAU - len) {
                    d - TAU
                } else {
                    d
                }
            }
        }
    }

    /// Parameter tolerance at `p` for a distance tolerance `tol`.
    pub fn ptol(&self, p: Point2, tol: f64) -> f64 {
        match *self {
            Geom::Line { .. } => tol,
            Geom::Circle { c, .. } => tol / p.distance(c).max(tol),
        }
    }

    /// Distance from `p` when its foot lies in the cone (within `tol`) and
    /// `p` is on the region side; `None` otherwise. At the centre of a
    /// concave arc every point of the arc is a foot.
    pub fn cone_dist(&self, p: Point2, tol: f64) -> Option<f64> {
        if let Geom::Circle { c, r, .. } = *self
            && p.distance(c) <= tol
        {
            return if self.is_concave() || r == 0.0 {
                Some(r)
            } else {
                None
            };
        }
        let u = self.param(p);
        let pt = self.ptol(p, tol);
        if u < -pt || u > self.len() + pt {
            return None;
        }
        let d = self.dist(p);
        (d >= -tol).then_some(d)
    }

    /// The point whose foot on this site is at parameter `u` and which is
    /// as far from `other` (signed) as from this site, and that distance.
    /// `None` when there is no such point at a distance of at least
    /// `−tol`.
    pub fn bisector_at(&self, other: &Geom, u: f64, tol: f64) -> Option<(Point2, f64)> {
        let f = self.foot(u);
        let n = self.normal(u);
        let rho = match *other {
            Geom::Line { a, t, .. } => {
                let nl = left(t);
                // 1 − n · nl for unit vectors, without cancellation.
                let dn = n - nl;
                let den = 0.5 * dn.dot(dn);
                if den <= 1e-30 {
                    return None;
                }
                (f - a).dot(nl) / den
            }
            Geom::Circle { c, r, .. } => {
                let s = other.side();
                let w = f - c;
                let wl = w.length();
                // w · n − r s, as (f − (c + r s n)) · n to avoid cancelling.
                let den = 2.0 * (f - (c + n * (r * s))).dot(n);
                if den.abs() <= 1e-300 {
                    return None;
                }
                let rho = (r - wl) * (r + wl) / den;
                if r + s * rho < -tol {
                    return None;
                }
                rho
            }
        };
        if !rho.is_finite() || rho < -tol {
            return None;
        }
        Some((f + n * rho, rho))
    }
}

/// One site equation in coordinates (x, y, ρ) relative to an origin:
/// `quad · (x² + y² − ρ²) + g · X + h = 0`, where `quad` is 0 for lines
/// and 1 for circles.
#[derive(Clone, Copy, Debug)]
struct Eq3 {
    quad: bool,
    g: [f64; 3],
    h: f64,
}

fn equation(s: &Geom, o: Point2) -> Eq3 {
    match *s {
        Geom::Line { a, t, .. } => {
            // n · X − ρ = n · (a − o)
            let n = left(t);
            Eq3 {
                quad: false,
                g: [n.x(), n.y(), -1.0],
                h: -(a - o).dot(n),
            }
        }
        Geom::Circle { c, r, .. } => {
            // |X − c'|² − (r + sρ)² = 0
            let cc = c - o;
            let rs = r * s.side();
            Eq3 {
                quad: true,
                g: [-2.0 * cc.x(), -2.0 * cc.y(), -2.0 * rs],
                h: cc.dot(cc) - r * r,
            }
        }
    }
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The Lorentz form x² + y² − ρ² of the circle equations.
fn quad3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] - a[2] * b[2]
}

/// Solves `m · x = b` by Gaussian elimination with partial pivoting;
/// `None` when `m` is singular relative to its size.
pub(crate) fn solve3(mut m: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    let scale = m
        .iter()
        .flat_map(|r| r.iter())
        .fold(0.0f64, |a, &v| a.max(v.abs()));
    if scale == 0.0 {
        return None;
    }
    for col in 0..3 {
        let piv = (col..3)
            .max_by(|&i, &j| m[i][col].abs().total_cmp(&m[j][col].abs()))
            .unwrap_or(col);
        if m[piv][col].abs() <= 1e-13 * scale {
            return None;
        }
        m.swap(col, piv);
        b.swap(col, piv);
        let pivot = m[col];
        for row in col + 1..3 {
            let f = m[row][col] / pivot[col];
            for (v, p) in m[row].iter_mut().zip(pivot).skip(col) {
                *v -= f * p;
            }
            b[row] -= f * b[col];
        }
    }
    let mut x = [0.0; 3];
    for row in (0..3).rev() {
        let mut v = b[row];
        for k in row + 1..3 {
            v -= m[row][k] * x[k];
        }
        x[row] = v / m[row][row];
    }
    Some(x)
}

/// Points at equal signed distance ρ ≥ −`tol` from three sites, treated as
/// whole lines and circles: up to two, polished, with residuals within
/// `tol`. Cones are not checked.
pub fn equidistant(s: [&Geom; 3], o: Point2, tol: f64) -> Vec<(Point2, f64)> {
    let eqs = s.map(|g| equation(g, o));
    let mut rows: Vec<([f64; 3], f64)> = Vec::with_capacity(3);
    let circle = eqs.iter().position(|e| e.quad);
    for (i, e) in eqs.iter().enumerate() {
        match circle {
            Some(k) if e.quad && i == k => {}
            Some(k) if e.quad => {
                let r = eqs[k];
                rows.push((
                    [e.g[0] - r.g[0], e.g[1] - r.g[1], e.g[2] - r.g[2]],
                    -(e.h - r.h),
                ));
            }
            _ => rows.push((e.g, -e.h)),
        }
    }
    let mut raw: Vec<[f64; 3]> = Vec::with_capacity(2);
    match circle {
        None => {
            let m = [rows[0].0, rows[1].0, rows[2].0];
            if let Some(x) = solve3(m, [rows[0].1, rows[1].1, rows[2].1]) {
                raw.push(x);
            }
        }
        Some(k) => {
            let (a0, b0) = rows[0];
            let (a1, b1) = rows[1];
            let d = cross3(a0, a1);
            let dn = dot3(d, d).sqrt();
            let n0 = dot3(a0, a0).sqrt();
            let n1 = dot3(a1, a1).sqrt();
            if dn <= 1e-12 * n0 * n1 || dn == 0.0 {
                return Vec::new();
            }
            // Particular solution in the span of the rows.
            let g00 = dot3(a0, a0);
            let g01 = dot3(a0, a1);
            let g11 = dot3(a1, a1);
            let det = g00 * g11 - g01 * g01;
            let l0 = (b0 * g11 - b1 * g01) / det;
            let l1 = (g00 * b1 - g01 * b0) / det;
            let x0 = [
                a0[0] * l0 + a1[0] * l1,
                a0[1] * l0 + a1[1] * l1,
                a0[2] * l0 + a1[2] * l1,
            ];
            let d = [d[0] / dn, d[1] / dn, d[2] / dn];
            let e = eqs[k];
            let qa = quad3(d, d);
            let qb = 2.0 * quad3(x0, d) + dot3(e.g, d);
            let qc = quad3(x0, x0) + dot3(e.g, x0) + e.h;
            let mut ts = Vec::with_capacity(2);
            let big = qb.abs().max((qa * qc).abs().sqrt());
            if qa.abs() <= 1e-12 * (1.0 + big) {
                if qb != 0.0 {
                    ts.push(-qc / qb);
                }
            } else {
                let mut disc = qb * qb - 4.0 * qa * qc;
                if disc < 0.0 && disc >= -1e-8 * (qb * qb + (4.0 * qa * qc).abs()) {
                    disc = 0.0;
                }
                if disc >= 0.0 {
                    let q = -0.5 * (qb + disc.sqrt().copysign(qb));
                    if q != 0.0 {
                        ts.push(q / qa);
                        ts.push(qc / q);
                    } else {
                        ts.push(0.0);
                    }
                }
            }
            for t in ts {
                raw.push([x0[0] + t * d[0], x0[1] + t * d[1], x0[2] + t * d[2]]);
            }
        }
    }
    let mut out: Vec<(Point2, f64)> = Vec::with_capacity(2);
    for x in raw {
        let p = Point2::new(o.x() + x[0], o.y() + x[1]);
        let (p, rho) = polish(s, p, x[2]);
        if !(rho.is_finite() && p.x().is_finite() && p.y().is_finite()) || rho < -tol {
            continue;
        }
        if s.iter().any(|g| (g.dist(p) - rho).abs() > tol) {
            continue;
        }
        if out.iter().any(|q| q.0.distance(p) <= tol) {
            continue;
        }
        out.push((p, rho));
    }
    out
}

/// Newton steps on `dist_i(p) = ρ`; keeps the best point found.
fn polish(s: [&Geom; 3], p: Point2, rho: f64) -> (Point2, f64) {
    let resid = |p: Point2, rho: f64| -> f64 {
        s.iter()
            .map(|g| (g.dist(p) - rho).abs())
            .fold(0.0, f64::max)
    };
    let mut best = (p, rho);
    let mut best_r = resid(p, rho);
    let (mut p, mut rho) = (p, rho);
    for _ in 0..3 {
        let mut m = [[0.0; 3]; 3];
        let mut b = [0.0; 3];
        for (i, g) in s.iter().enumerate() {
            let grad = match **g {
                Geom::Line { t, .. } => left(t),
                Geom::Circle { c, .. } => {
                    let w = p - c;
                    let l = w.length();
                    if l == 0.0 {
                        return best;
                    }
                    w * (g.side() / l)
                }
            };
            m[i] = [grad.x(), grad.y(), -1.0];
            b[i] = -(g.dist(p) - rho);
        }
        let Some(dx) = solve3(m, b) else {
            break;
        };
        p = Point2::new(p.x() + dx[0], p.y() + dx[1]);
        rho += dx[2];
        let r = resid(p, rho);
        if r < best_r {
            best = (p, rho);
            best_r = r;
        } else {
            break;
        }
    }
    best
}
