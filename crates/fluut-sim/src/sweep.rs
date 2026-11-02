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
    Side,
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
    /// lengths, and at least the height of the shaped bottom.
    pub fn new(tool: Tool, mut p0: Pose, mut p1: Pose) -> Sweep {
        let len = p0.length.max(p1.length).max(tool.profile_height());
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
        if self.tool.cone.is_some() {
            Method::Gjk
        } else if self.fixed_axis && e.dot(self.p0.axis).abs() >= 1.0 - 1e-12 {
            Method::Axial
        } else if self.tool.is_ball() {
            Method::Ball
        } else if self.fixed_axis && e.dot(self.p0.axis).abs() <= 1e-12 {
            Method::Side
        } else {
            Method::Gjk
        }
    }

    /// Parameter interval of the line `o + t·e` (`e` unit) inside the swept
    /// volume.
    pub fn ray(&self, o: DVec3, e: DVec3) -> Option<(f64, f64)> {
        let (ta, tb) = self.slab(o, e)?;
        match self.method(e) {
            Method::Axial if self.tool.is_ball() => self.ball_axial_ray(o, e),
            Method::Axial => self.axial_ray(o, e),
            Method::Ball => self.ball_ray(o, e),
            Method::Side => self.side_ray(o, e),
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

    /// Centres of the ball at the bottom and top of each pose's axis.
    fn ball_centres(&self) -> [DVec3; 4] {
        let r = self.tool.corner;
        [
            self.p0.tip + self.p0.axis * r,
            self.p0.tip + self.p0.axis * self.p0.length,
            self.p1.tip + self.p1.axis * r,
            self.p1.tip + self.p1.axis * self.p1.length,
        ]
    }

    /// Ball end mill: the hull of the four sphere centres grown by the ball
    /// radius. Most lines are decided by the bottom and top capsules alone;
    /// the rest by the whole hull.
    fn ball_ray(&self, o: DVec3, e: DVec3) -> Option<(f64, f64)> {
        if let Some(i) = self.ball_fast(o, e) {
            return i;
        }
        self.ball_ray_full(o, e)
    }

    /// Ball end mill: the line's entry and exit, each from the bottom capsule
    /// (`c0`–`c2`) or the top capsule (`c1`–`c3`), if they lie on the hull's
    /// boundary. A point of the bottom capsule with outward normal `n` does
    /// when `n·w ≤ 0` for both axes, because then the hull's support in `n` is
    /// at `c0` or `c2`; a point of the top capsule when `n·w ≥ 0`. A line
    /// farther than the radius from the bottom segment, in a direction `n`
    /// with `n·w ≤ 0`, misses. `None` if this cannot decide.
    fn ball_fast(&self, o: DVec3, e: DVec3) -> Option<Option<(f64, f64)>> {
        let r = self.tool.corner;
        let c = self.ball_centres();
        let (w0, w1) = (self.p0.axis, self.p1.axis);
        // Shortest vector from the bottom segment to the line.
        let perp = |v: DVec3| v - e * v.dot(e);
        let (pw, pd) = (perp(o - c[0]), perp(c[2] - c[0]));
        let dd = pd.length_squared();
        let l = if dd > 0.0 {
            (pw.dot(pd) / dd).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let v = pw - pd * l;
        if v.length_squared() > r * r {
            return (v.dot(w0) <= 0.0 && v.dot(w1) <= 0.0).then_some(None);
        }
        // Outward normal of the capsule `pq` at `x`, not normalised.
        let normal = |x: DVec3, p: DVec3, q: DVec3| {
            let d = q - p;
            let dd = d.length_squared();
            let l = if dd > 0.0 {
                ((x - p).dot(d) / dd).clamp(0.0, 1.0)
            } else {
                0.0
            };
            x - (p + d * l)
        };
        // The line's end `t` on capsule `pq`, if it is on the hull's boundary;
        // `s` is +1 for the top capsule, −1 for the bottom, `side` −1 for an
        // entry, +1 for an exit.
        let on_hull = |t: f64, p: DVec3, q: DVec3, s: f64, side: f64| {
            let n = normal(o + e * t, p, q);
            side * n.dot(e) > 0.0 && s * n.dot(w0) >= 0.0 && s * n.dot(w1) >= 0.0
        };
        let (b0, b1) = line_capsule(o, e, c[0], c[2], r)?;
        let top = std::cell::OnceCell::new();
        let top = || *top.get_or_init(|| line_capsule(o, e, c[1], c[3], r));
        let t0 = if on_hull(b0, c[0], c[2], -1.0, -1.0) {
            b0
        } else {
            let (a, _) = top()?;
            on_hull(a, c[1], c[3], 1.0, -1.0).then_some(a)?
        };
        let t1 = if on_hull(b1, c[0], c[2], -1.0, 1.0) {
            b1
        } else {
            let (_, b) = top()?;
            on_hull(b, c[1], c[3], 1.0, 1.0).then_some(b)?
        };
        Some((t0 <= t1).then_some((t0, t1)))
    }

    /// Ball end mill, by the whole hull. The line meets it in the hull of its
    /// intervals with the spheres, the edge cylinders and the triangles
    /// thickened by the radius. With a fixed axis the centres form a
    /// parallelogram, and its four sides and two triangles suffice.
    fn ball_ray_full(&self, o: DVec3, e: DVec3) -> Option<(f64, f64)> {
        const QUAD_EDGES: [(usize, usize); 4] = [(0, 1), (2, 3), (0, 2), (1, 3)];
        const QUAD_TRIS: [(usize, usize, usize); 2] = [(0, 1, 3), (0, 3, 2)];
        const EDGES: [(usize, usize); 6] = [(0, 1), (2, 3), (0, 2), (1, 3), (0, 3), (1, 2)];
        const TRIS: [(usize, usize, usize); 4] = [(0, 1, 3), (0, 3, 2), (0, 1, 2), (1, 2, 3)];
        let r = self.tool.corner;
        let c = self.ball_centres();
        let (edges, tris): (&[_], &[_]) = if self.fixed_axis {
            (&QUAD_EDGES, &QUAD_TRIS)
        } else {
            (&EDGES, &TRIS)
        };
        let mut acc = Acc::default();
        for p in c {
            acc.add(line_sphere(o, e, p, r));
        }
        for &(i, j) in edges {
            acc.add(line_cylinder(o, e, c[i], c[j], r));
        }
        for &(i, j, k) in tris {
            acc.add(line_slab_triangle(o, e, c[i], c[j], c[k], r));
        }
        acc.get()
    }

    /// Ball end mill, axis fixed, line along the axis. The swept volume is the
    /// capsule swept by the bottom ball, translated along the axis by up to
    /// the shank length; translating along the line shifts its interval.
    fn ball_axial_ray(&self, o: DVec3, e: DVec3) -> Option<(f64, f64)> {
        let r = self.tool.corner;
        let c = self.ball_centres();
        let (a, b) = line_capsule(o, e, c[0], c[2], r)?;
        let up = self.p0.length - r;
        if e.dot(self.p0.axis) > 0.0 {
            Some((a, b + up))
        } else {
            Some((a - up, b))
        }
    }

    /// Axis fixed, line across it. The line stays at one height `h` above the
    /// first tip. At `β` along the chord it is `h − β·Δh` above the tool's
    /// tip, where the tool's section is a disc whose centre moves along a
    /// line. On the straight flank the discs have radius `R` and sweep a 2-D
    /// capsule. In the corner bands of a bull nose the radius shrinks, and the
    /// entry and exit come from a 1-D convex search.
    fn side_ray(&self, o: DVec3, e: DVec3) -> Option<(f64, f64)> {
        let tool = &self.tool;
        let w = self.p0.axis;
        let h = (o - self.p0.tip).dot(w);
        let d = self.p1.tip - self.p0.tip;
        let dh = d.dot(w);
        let dp = d - w * dh;
        let (r, len) = (tool.corner, self.p0.length);
        let c0 = self.p0.tip + w * h;
        if dh == 0.0 {
            // Level chord: one radius, at height h.
            let x = if h < r {
                r - h
            } else if h > len {
                h - len
            } else {
                0.0
            };
            if h < 0.0 || x > r {
                return None;
            }
            let rho = tool.core() + (r * r - x * x).max(0.0).sqrt();
            return line_capsule(o, e, c0, c0 + dp, rho);
        }
        // β with the line between heights z0 and z1 above the tip.
        let band = |z0: f64, z1: f64| -> Option<(f64, f64)> {
            let (a, b) = ((h - z1) / dh, (h - z0) / dh);
            let (a, b) = (a.min(b).max(0.0), a.max(b).min(1.0));
            (a <= b).then_some((a, b))
        };
        let mut acc = Acc::default();
        if let Some((b0, b1)) = band(r, len) {
            acc.add(line_capsule(o, e, c0 + dp * b0, c0 + dp * b1, tool.radius));
        }
        if r > 0.0 {
            let n = w.cross(e);
            let m = c0 - o;
            let band_geo = CornerBand {
                a: (m.dot(e), dp.dot(e)),
                b: (m.dot(n), dp.dot(n)),
                core: tool.core(),
                r,
                scale: dp.length() + dh.abs(),
            };
            // Bottom corner below its centre, top corner above its centre.
            for (zc, z0, z1) in [(r, 0.0, r), (len, len, len + r)] {
                if let Some(range) = band(z0, z1) {
                    acc.add(band_geo.ray((h - zc, -dh), range));
                }
            }
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

/// One corner band of a bull nose seen from the side (see `Sweep::side_ray`).
/// At `β` along the chord, the section's centre is `a0 + a1·β` along the line
/// and `b0 + b1·β` across it.
struct CornerBand {
    a: (f64, f64),
    b: (f64, f64),
    core: f64,
    r: f64,
    /// Length of the chord, to set the search tolerance.
    scale: f64,
}

impl CornerBand {
    /// The line's interval over `β ∈ range`, where the line is `v0 + v1·β`
    /// above the corner centre.
    fn ray(&self, (v0, v1): (f64, f64), range: (f64, f64)) -> Option<(f64, f64)> {
        let (r, core) = (self.r, self.core);
        let (b0, b1) = self.b;
        // The section reaches the line where (b, v) lies in the 2-D capsule
        // of radius r around [−core, core] × {0}.
        let mut acc = Acc::default();
        for cx in [-core, core] {
            acc.add(quad_le0(b1 * b1 + v1 * v1, (b0 - cx) * b1 + v0 * v1, {
                let x = b0 - cx;
                x * x + v0 * v0 - r * r
            }));
        }
        acc.add(slab_1d(b0, b1, core).and_then(|(p, q)| {
            let (s, t) = slab_1d(v0, v1, r)?;
            let (p, q) = (p.max(s), q.min(t));
            (p <= q).then_some((p, q))
        }));
        let (lo, hi) = acc.get()?;
        let (lo, hi) = (lo.max(range.0), hi.min(range.1));
        if lo > hi {
            return None;
        }
        let (a0, a1) = self.a;
        let half = |beta: f64| {
            let v = v0 + v1 * beta;
            let rho = core + (r * r - v * v).max(0.0).sqrt();
            let b = b0 + b1 * beta;
            (rho * rho - b * b).max(0.0).sqrt()
        };
        let tol = 1e-8 / self.scale.max(1e-300);
        let t0 = convex_min(|beta| a0 + a1 * beta - half(beta), lo, hi, tol);
        let t1 = -convex_min(|beta| -(a0 + a1 * beta) - half(beta), lo, hi, tol);
        Some((t0, t1))
    }
}

/// `{x : a·x² + 2b·x + c ≤ 0}` for `a ≥ 0`, as an interval.
fn quad_le0(a: f64, b: f64, c: f64) -> Option<(f64, f64)> {
    if a == 0.0 {
        return (c <= 0.0).then_some((f64::NEG_INFINITY, f64::INFINITY));
    }
    let disc = b * b - a * c;
    if disc < 0.0 {
        return None;
    }
    let h = disc.sqrt();
    Some(((-b - h) / a, (-b + h) / a))
}

/// `{x : |p + q·x| ≤ m}` as an interval.
fn slab_1d(p: f64, q: f64, m: f64) -> Option<(f64, f64)> {
    if q == 0.0 {
        return (p.abs() <= m).then_some((f64::NEG_INFINITY, f64::INFINITY));
    }
    let (a, b) = ((-m - p) / q, (m - p) / q);
    Some((a.min(b), a.max(b)))
}

/// Minimum of a convex `f` over `[lo, hi]`, by golden-section search to
/// `tol` in the argument.
fn convex_min(f: impl Fn(f64) -> f64, lo: f64, hi: f64, tol: f64) -> f64 {
    const G: f64 = 0.618_033_988_749_894_8;
    let (mut a, mut b) = (lo, hi);
    let mut x1 = b - G * (b - a);
    let mut x2 = a + G * (b - a);
    let (mut f1, mut f2) = (f(x1), f(x2));
    for _ in 0..200 {
        if b - a <= tol {
            break;
        }
        if f1 <= f2 {
            b = x2;
            x2 = x1;
            f2 = f1;
            x1 = b - G * (b - a);
            f1 = f(x1);
        } else {
            a = x1;
            x1 = x2;
            f1 = f2;
            x2 = a + G * (b - a);
            f2 = f(x2);
        }
    }
    f1.min(f2).min(f(lo)).min(f(hi))
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
    acc.add(line_cylinder(o, e, p, q, r));
    acc.get()
}

/// The line inside the cylinder of radius `r` around segment `pq`, without
/// end caps.
fn line_cylinder(o: DVec3, e: DVec3, p: DVec3, q: DVec3, r: f64) -> Option<(f64, f64)> {
    let d = q - p;
    let dd = d.length_squared();
    if dd == 0.0 {
        return None;
    }
    // Infinite cylinder: |m⊥ + t·e⊥|² ≤ r², with ⊥ relative to d.
    let m = o - p;
    let md = m.dot(d);
    let nd = e.dot(d);
    let a = dd - nd * nd;
    let b = dd * m.dot(e) - nd * md;
    let c = dd * (m.length_squared() - r * r) - md * md;
    let (mut t0, mut t1) = if a <= 1e-15 * dd {
        if c > 0.0 {
            return None;
        }
        (f64::NEG_INFINITY, f64::INFINITY)
    } else {
        let disc = b * b - a * c;
        if disc < 0.0 {
            return None;
        }
        let h = disc.sqrt();
        ((-b - h) / a, (-b + h) / a)
    };
    // Clip to the segment: 0 ≤ (md + t·nd) / dd ≤ 1.
    if nd == 0.0 {
        if md < 0.0 || md > dd {
            return None;
        }
    } else {
        let sa = -md / nd;
        let sb = (dd - md) / nd;
        t0 = t0.max(sa.min(sb));
        t1 = t1.min(sa.max(sb));
    }
    (t0 <= t1).then_some((t0, t1))
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
