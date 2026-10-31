// SPDX-License-Identifier: MIT OR Apache-2.0

//! The volume a tool sweeps over one chord, and its intersection with a line
//! (D11 §3).
//!
//! A chord goes from pose `p0` to pose `p1`. With equal axes the swept volume is
//! exactly `tool ⊕ segment`. When the axis turns (rotary chords) it is taken as
//! the convex hull of the two end poses; the motion code keeps the chords short
//! enough that this differs from the true sweep by less than the resolution.

use crate::gjk::ray_cast;
use crate::tool::{Pose, Tool};
use glam::DVec3;

/// GJK distance tolerance in mm.
pub const GJK_EPS: f64 = 1e-4;

/// Which method [`Sweep::ray`] used; for statistics.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Method {
    Ball,
    Axial,
    Gjk,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Sweep {
    pub tool: Tool,
    pub p0: Pose,
    pub p1: Pose,
    /// Both poses have the same axis (bitwise).
    pub fixed_axis: bool,
    pub min: DVec3,
    pub max: DVec3,
}

impl Sweep {
    /// The sweep from `p0` to `p1`. Both poses get the larger of the two
    /// lengths, and at least the corner radius.
    pub fn new(tool: Tool, mut p0: Pose, mut p1: Pose) -> Sweep {
        let len = p0.length.max(p1.length).max(tool.corner);
        p0.length = len;
        p1.length = len;
        let (a0, b0) = p0.aabb(&tool);
        let (a1, b1) = p1.aabb(&tool);
        Sweep {
            tool,
            p0,
            p1,
            fixed_axis: p0.axis == p1.axis,
            min: a0.min(a1),
            max: b0.max(b1),
        }
    }

    pub fn support(&self, n: DVec3) -> DVec3 {
        let a = self.p0.support(&self.tool, n);
        let b = self.p1.support(&self.tool, n);
        if b.dot(n) > a.dot(n) { b } else { a }
    }

    /// Parameter range of the line `o + t·e` (`e` unit) inside the box.
    pub fn slab(&self, o: DVec3, e: DVec3) -> Option<(f64, f64)> {
        let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
        for k in 0..3 {
            if e[k] == 0.0 {
                if o[k] < self.min[k] || o[k] > self.max[k] {
                    return None;
                }
            } else {
                let a = (self.min[k] - o[k]) / e[k];
                let b = (self.max[k] - o[k]) / e[k];
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
        }
        (t0 <= t1).then_some((t0, t1))
    }

    /// The method [`Sweep::ray`] uses for direction `e`.
    pub fn method(&self, e: DVec3) -> Method {
        if self.tool.is_ball() {
            Method::Ball
        } else if self.fixed_axis && e.dot(self.p0.axis).abs() >= 1.0 - 1e-12 {
            Method::Axial
        } else {
            Method::Gjk
        }
    }

    /// Parameter interval of the line `o + t·e` (`e` unit) inside the swept
    /// volume.
    pub fn ray(&self, o: DVec3, e: DVec3) -> Option<(f64, f64)> {
        let (ta, tb) = self.slab(o, e)?;
        match self.method(e) {
            Method::Ball => self.ball_ray(o, e),
            Method::Axial => self.axial_ray(o, e),
            Method::Gjk => {
                let sup = |n: DVec3| self.support(n);
                let len = tb - ta;
                let t0 = ray_cast(sup, o + e * ta, e, len, GJK_EPS)?;
                let t1 = ray_cast(sup, o + e * tb, -e, len, GJK_EPS)?;
                let (a, b) = (ta + t0, tb - t1);
                (a <= b).then_some((a, b))
            }
        }
    }

    /// Ball end mill: the hull of the four sphere centres (bottom and top of
    /// each pose's axis) grown by the ball radius. The line meets it in the
    /// hull of its intervals with the six edge capsules and the four
    /// triangles thickened by the radius.
    fn ball_ray(&self, o: DVec3, e: DVec3) -> Option<(f64, f64)> {
        let r = self.tool.corner;
        let c = [
            self.p0.tip + self.p0.axis * r,
            self.p0.tip + self.p0.axis * self.p0.length,
            self.p1.tip + self.p1.axis * r,
            self.p1.tip + self.p1.axis * self.p1.length,
        ];
        let mut acc = Acc::default();
        for (i, j) in [(0, 1), (2, 3), (0, 2), (1, 3), (0, 3), (1, 2)] {
            acc.add(line_capsule(o, e, c[i], c[j], r));
        }
        for (i, j, k) in [(0, 1, 3), (0, 3, 2), (0, 1, 2), (1, 2, 3)] {
            acc.add(line_slab_triangle(o, e, c[i], c[j], c[k], r));
        }
        acc.get()
    }

    /// Flat or bull nose, axis fixed, line along the axis: the lowest bottom
    /// and highest top over the chord, by 1-D convex minimisation.
    fn axial_ray(&self, o: DVec3, e: DVec3) -> Option<(f64, f64)> {
        let tool = &self.tool;
        let w = self.p0.axis;
        let s = e.dot(w).signum();
        let d = self.p1.tip - self.p0.tip;
        let dh = d.dot(w);
        let dp = d - w * dh;
        let c = o - self.p0.tip;
        let ch = c.dot(w);
        let cp = c - w * ch;
        // β with |cp − β·dp| ≤ R, within [0, 1].
        let dd = dp.length_squared();
        let (b0, b1) = if dd > 0.0 {
            let mid = cp.dot(dp) / dd;
            let q = (cp.length_squared() - tool.radius * tool.radius) / dd;
            let disc = mid * mid - q;
            if disc < 0.0 {
                return None;
            }
            let h = disc.sqrt();
            ((mid - h).max(0.0), (mid + h).min(1.0))
        } else if cp.length_squared() <= tool.radius * tool.radius {
            (0.0, 1.0)
        } else {
            return None;
        };
        if b0 > b1 {
            return None;
        }
        let low = min_profile(tool, cp, dp, dh, b0, b1);
        let high = self.p0.length + tool.corner - min_profile(tool, cp, dp, -dh, b0, b1);
        let (t0, t1) = ((low - ch) * s, (high - ch) * s);
        Some((t0.min(t1), t0.max(t1)))
    }
}

/// `min over β ∈ [b0, b1] of k·β + bottom(|cp − β·dp|)`; convex in β.
fn min_profile(tool: &Tool, cp: DVec3, dp: DVec3, k: f64, b0: f64, b1: f64) -> f64 {
    let rho = |b: f64| (cp - dp * b).length();
    let f = |b: f64| k * b + tool.bottom(rho(b)).unwrap_or(tool.corner);
    let dd = dp.length_squared();
    if tool.corner == 0.0 || dd == 0.0 {
        return f(b0).min(f(b1));
    }
    let slope = |b: f64| {
        let p = rho(b);
        let e = p - tool.core();
        if e <= 0.0 || p == 0.0 {
            return k;
        }
        let dp_db = (b * dd - cp.dot(dp)) / p;
        let r = tool.corner;
        let q = r * r - e * e;
        if q <= 0.0 {
            return k + f64::INFINITY * dp_db.signum();
        }
        k + e / q.sqrt() * dp_db
    };
    if slope(b0) >= 0.0 {
        return f(b0);
    }
    if slope(b1) <= 0.0 {
        return f(b1);
    }
    let (mut lo, mut hi) = (b0, b1);
    for _ in 0..60 {
        let m = 0.5 * (lo + hi);
        if slope(m) < 0.0 {
            lo = m;
        } else {
            hi = m;
        }
    }
    f(0.5 * (lo + hi))
}

/// Hull of intervals.
#[derive(Default)]
struct Acc(Option<(f64, f64)>);

impl Acc {
    fn add(&mut self, i: Option<(f64, f64)>) {
        if let Some((a, b)) = i {
            self.0 = Some(match self.0 {
                None => (a, b),
                Some((x, y)) => (x.min(a), y.max(b)),
            });
        }
    }

    fn get(self) -> Option<(f64, f64)> {
        self.0
    }
}

/// The line `o + t·e` (`e` unit) inside the sphere of radius `r` at `c`.
fn line_sphere(o: DVec3, e: DVec3, c: DVec3, r: f64) -> Option<(f64, f64)> {
    let m = o - c;
    let b = m.dot(e);
    let disc = b * b - (m.length_squared() - r * r);
    if disc < 0.0 {
        return None;
    }
    let h = disc.sqrt();
    Some((-b - h, -b + h))
}

/// The line inside the capsule of radius `r` around segment `pq`.
fn line_capsule(o: DVec3, e: DVec3, p: DVec3, q: DVec3, r: f64) -> Option<(f64, f64)> {
    let mut acc = Acc::default();
    acc.add(line_sphere(o, e, p, r));
    acc.add(line_sphere(o, e, q, r));
    let d = q - p;
    let dd = d.length_squared();
    if dd == 0.0 {
        return acc.get();
    }
    // Infinite cylinder: |m⊥ + t·e⊥|² ≤ r², with ⊥ relative to d.
    let m = o - p;
    let md = m.dot(d);
    let nd = e.dot(d);
    let a = dd - nd * nd;
    let b = dd * m.dot(e) - nd * md;
    let c = dd * (m.length_squared() - r * r) - md * md;
    let cyl = if a <= 1e-15 * dd {
        (c <= 0.0).then_some((f64::NEG_INFINITY, f64::INFINITY))
    } else {
        let disc = b * b - a * c;
        (disc >= 0.0).then(|| {
            let h = disc.sqrt();
            ((-b - h) / a, (-b + h) / a)
        })
    };
    // Clip to the segment: 0 ≤ (md + t·nd) / dd ≤ 1.
    if let Some((mut t0, mut t1)) = cyl {
        if nd == 0.0 {
            if md < 0.0 || md > dd {
                return acc.get();
            }
        } else {
            let sa = -md / nd;
            let sb = (dd - md) / nd;
            t0 = t0.max(sa.min(sb));
            t1 = t1.min(sa.max(sb));
        }
        if t0 <= t1 {
            acc.add(Some((t0, t1)));
        }
    }
    acc.get()
}

/// The line inside triangle `abc` thickened by `r` along its normal.
fn line_slab_triangle(
    o: DVec3,
    e: DVec3,
    a: DVec3,
    b: DVec3,
    c: DVec3,
    r: f64,
) -> Option<(f64, f64)> {
    let n = (b - a).cross(c - a);
    let nl = n.length();
    let scale = (b - a).length_squared().max((c - a).length_squared());
    if nl <= 1e-12 * scale {
        return None;
    }
    let n = n / nl;
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    // Each constraint: (x − p)·m ≥ 0 at x = o + t·e.
    let mut clip = |p: DVec3, m: DVec3| -> bool {
        let num = (o - p).dot(m);
        let den = e.dot(m);
        if den == 0.0 {
            return num >= 0.0;
        }
        let t = -num / den;
        if den > 0.0 {
            t0 = t0.max(t);
        } else {
            t1 = t1.min(t);
        }
        true
    };
    let ok = clip(a - n * r, n)
        && clip(a + n * r, -n)
        && [(a, b, c), (b, c, a), (c, a, b)].iter().all(|&(p, q, s)| {
            let mut m = n.cross(q - p);
            if m.dot(s - p) < 0.0 {
                m = -m;
            }
            clip(p, m)
        });
    (ok && t0 <= t1).then_some((t0, t1))
}

#[cfg(test)]
mod tests;
