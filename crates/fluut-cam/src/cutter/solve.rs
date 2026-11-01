// SPDX-License-Identifier: MIT OR Apache-2.0

//! Per-primitive drop solvers (D07 §4) for the tool axis +Z.
//!
//! Every result is a real point `p` of the primitive inside the footprint,
//! with `τ = p.z − h(d(p))`. So a solver can miss the contact (and the
//! caller gouges) but never place the tool too high.

use super::geom::{QueryFace, QueryGeometry, curve_box};
use super::profile::Cutter;
use fluut_geom::{Aabb3, Curve3, Interval, Point3, SurfDerivs, Surface, Tol, UvBox, Vec3};
use std::f64::consts::{FRAC_PI_2, TAU};

/// Where the tool touches.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ContactOn {
    Face { face: u32, u: f64, v: f64 },
    Edge { edge: u32, t: f64 },
    Vertex(u32),
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Contact {
    /// Height of the tool tip.
    pub tau: f64,
    pub point: Point3,
    pub on: ContactOn,
}

/// Seeds for the numeric face solver (CH-12). Candidates are a `grid ×
/// grid` sample of the primitive's parameter box, and target points in the
/// tool's footprint (the centre and `rings` rings of 8) projected vertically
/// onto the surface from the nearest sample. Newton runs from the `starts`
/// candidates with the highest `τ` (points outside the footprint ranked
/// down by their distance).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Seeding {
    pub grid: usize,
    pub rings: usize,
    pub starts: usize,
}

impl Default for Seeding {
    fn default() -> Self {
        Seeding {
            grid: 4,
            rings: 2,
            starts: 3,
        }
    }
}

/// Counters for the measurements of the spike.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct SolveStats {
    pub vertices: u64,
    pub edges: u64,
    pub faces_closed_form: u64,
    pub faces_numeric: u64,
    pub newton_runs: u64,
    pub newton_converged: u64,
}

/// The tool axis: a vertical line through `(x, y)`.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Axis {
    pub x: f64,
    pub y: f64,
}

impl Axis {
    pub fn lateral(&self, p: Point3) -> f64 {
        libm::hypot(p.x() - self.x, p.y() - self.y)
    }

    /// `τ` for touching `p`, if `p` is in the footprint (rounding on the
    /// rim forgiven).
    pub fn tau(&self, c: &Cutter, p: Point3) -> Option<f64> {
        let d = self.lateral(p);
        let r = c.radius();
        (d <= r * (1.0 + 1e-12)).then(|| p.z() - c.h(d.min(r)))
    }
}

/// Brent's method for the maximum of `f` on `[a, b]` (parabolic steps with
/// golden-section fallback). `None` values count as −∞.
pub fn maximize(f: impl Fn(f64) -> Option<f64>, a: f64, b: f64, tol: f64) -> (f64, f64) {
    let g = |t: f64| f(t).map_or(f64::INFINITY, |v| -v);
    const CG: f64 = 0.381_966_011_250_105_1;
    let (mut a, mut b) = (a.min(b), a.max(b));
    let mut x = a + CG * (b - a);
    let (mut w, mut v) = (x, x);
    let mut fx = g(x);
    let (mut fw, mut fv) = (fx, fx);
    let (mut d, mut e) = (0.0f64, 0.0f64);
    for _ in 0..100 {
        let xm = 0.5 * (a + b);
        let tol1 = tol * x.abs() + 1e-14;
        let tol2 = 2.0 * tol1;
        if (x - xm).abs() <= tol2 - 0.5 * (b - a) {
            break;
        }
        let mut golden = true;
        if e.abs() > tol1 && fx.is_finite() && fw.is_finite() && fv.is_finite() {
            let r = (x - w) * (fx - fv);
            let mut q = (x - v) * (fx - fw);
            let mut p = (x - v) * q - (x - w) * r;
            q = 2.0 * (q - r);
            if q > 0.0 {
                p = -p;
            }
            q = q.abs();
            let etemp = e;
            e = d;
            if p.abs() < (0.5 * q * etemp).abs() && p > q * (a - x) && p < q * (b - x) {
                d = p / q;
                let u = x + d;
                if u - a < tol2 || b - u < tol2 {
                    d = tol1.copysign(xm - x);
                }
                golden = false;
            }
        }
        if golden {
            e = if x >= xm { a - x } else { b - x };
            d = CG * e;
        }
        let u = if d.abs() >= tol1 {
            x + d
        } else {
            x + tol1.copysign(d)
        };
        let fu = g(u);
        if fu <= fx {
            if u >= x {
                a = x;
            } else {
                b = x;
            }
            (v, w, x) = (w, x, u);
            (fv, fw, fx) = (fw, fx, fu);
        } else {
            if u < x {
                a = u;
            } else {
                b = u;
            }
            if fu <= fw || w == x {
                (v, w) = (w, u);
                (fv, fw) = (fw, fu);
            } else if fu <= fv || v == x || v == w {
                v = u;
                fv = fu;
            }
        }
    }
    (x, -fx)
}

pub fn vertex(g: &QueryGeometry, i: u32, ax: Axis, c: &Cutter) -> Option<Contact> {
    let p = g.body.vertices[i as usize].p;
    Some(Contact {
        tau: ax.tau(c, p)?,
        point: p,
        on: ContactOn::Vertex(i),
    })
}

/// Parameter intervals of `curve` over `range` inside the footprint.
fn footprint_runs(curve: &Curve3, range: Interval, ax: Axis, c: &Cutter) -> Vec<Vec<f64>> {
    let rad = c.radius();
    let d = |t: f64| ax.lateral(curve.eval(t));
    let inside = |t: f64| d(t) <= rad;
    if let Curve3::Line { origin, dir } = curve {
        let (wx, wy) = (origin.x() - ax.x, origin.y() - ax.y);
        let (dx, dy) = (dir.get().x(), dir.get().y());
        let a = dx * dx + dy * dy;
        let b = 2.0 * (wx * dx + wy * dy);
        let cc = wx * wx + wy * wy - rad * rad;
        let (lo, hi) = if a < 1e-24 {
            if cc > 0.0 {
                return vec![];
            }
            (range.lo, range.hi)
        } else {
            let disc = b * b - 4.0 * a * cc;
            if disc < 0.0 {
                return vec![];
            }
            let s = disc.sqrt();
            let (t0, t1) = ((-b - s) / (2.0 * a), (-b + s) / (2.0 * a));
            (t0.max(range.lo), t1.min(range.hi))
        };
        if lo > hi {
            return vec![];
        }
        // Pull ends that rounding put just outside back in.
        let fix = |t: f64, toward: f64| {
            let mut t = t;
            for _ in 0..60 {
                if inside(t) {
                    break;
                }
                t += (toward - t) * 1e-9 + (toward - t).signum() * 1e-15;
            }
            t
        };
        let mid = 0.5 * (lo + hi);
        let (lo, hi) = (fix(lo, mid), fix(hi, mid));
        return if inside(lo) && inside(hi) {
            vec![vec![lo, mid, hi]]
        } else {
            vec![]
        };
    }
    // Samples at most a quarter of the tool radius apart.
    let breaks = curve.breaks(range);
    let mut ts: Vec<f64> = Vec::new();
    for w in breaks.windows(2) {
        // Pieces whose box is out of reach get one sample (it is outside).
        let b = curve_box(curve, Interval { lo: w[0], hi: w[1] });
        let dx = (b.min.x() - ax.x).max(ax.x - b.max.x()).max(0.0);
        let dy = (b.min.y() - ax.y).max(ax.y - b.max.y()).max(0.0);
        if libm::hypot(dx, dy) > rad {
            ts.push(w[0]);
            continue;
        }
        // The box diagonal bounds the chord; arcs add at most π/2 of it.
        let len = 1.6 * (b.max - b.min).length();
        let n = ((len / (0.25 * rad)).ceil() as usize).clamp(4, 4000);
        for i in 0..n {
            ts.push(w[0] + (w[1] - w[0]) * i as f64 / n as f64);
        }
    }
    ts.push(range.hi);
    // Near misses: a local minimum of d between samples may dip inside if
    // it is within a sample spacing of the rim.
    let ds: Vec<f64> = ts.iter().map(|&t| d(t)).collect();
    let mut extra = Vec::new();
    for i in 1..ts.len() - 1 {
        if ds[i] > rad
            && ds[i] <= ds[i - 1]
            && ds[i] <= ds[i + 1]
            && ds[i] < rad + curve.eval(ts[i - 1]).distance(curve.eval(ts[i + 1]))
        {
            let (t, neg) = maximize(|t| Some(-d(t)), ts[i - 1], ts[i + 1], 1e-10);
            if -neg <= rad {
                extra.push(t);
            }
        }
    }
    if !extra.is_empty() {
        ts.extend(extra);
        ts.sort_by(f64::total_cmp);
    }
    // The rim crossing between an inside point a and an outside point b:
    // Illinois false position on d − R, keeping the inside end.
    let bisect = |mut a: f64, mut b: f64| {
        let (mut fa, mut fb) = (d(a) - rad, d(b) - rad);
        let mut side = 0;
        for _ in 0..60 {
            let m = if fb != fa {
                (a * fb - b * fa) / (fb - fa)
            } else {
                0.5 * (a + b)
            };
            let m = if m.is_finite() && (m - a) * (m - b) < 0.0 {
                m
            } else {
                0.5 * (a + b)
            };
            if m == a || m == b || (a - b).abs() <= 1e-15 * (1.0 + a.abs()) {
                break;
            }
            let fm = d(m) - rad;
            if fm <= 0.0 {
                a = m;
                fa = fm;
                if side == -1 {
                    fb *= 0.5;
                }
                side = -1;
            } else {
                b = m;
                fb = fm;
                if side == 1 {
                    fa *= 0.5;
                }
                side = 1;
            }
            if fa.abs() < 1e-13 * rad {
                break;
            }
        }
        a
    };
    let mut runs = Vec::new();
    let mut cur: Vec<f64> = Vec::new();
    let mut prev: Option<(f64, bool)> = None;
    for &t in &ts {
        let ins = inside(t);
        match (prev, ins) {
            (Some((tp, false)), true) => cur.push(bisect(t, tp)),
            (Some((tp, true)), false) => {
                cur.push(bisect(tp, t));
                runs.push(std::mem::take(&mut cur));
            }
            _ => {}
        }
        if ins {
            cur.push(t);
        }
        prev = Some((t, ins));
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    runs
}

pub fn edge(g: &QueryGeometry, i: u32, ax: Axis, c: &Cutter) -> Option<Contact> {
    let e = &g.body.edges[i as usize];
    let curve = &g.body.curves[e.curve? as usize];
    let f = |t: f64| ax.tau(c, curve.eval(t));
    let mut best: Option<(f64, f64)> = None;
    let mut take = |t: f64, v: f64| {
        if best.is_none_or(|b| v > b.1) {
            best = Some((t, v));
        }
    };
    for run in footprint_runs(curve, e.range, ax, c) {
        let vals: Vec<Option<f64>> = run.iter().map(|&t| f(t)).collect();
        for k in 0..run.len() {
            let Some(vk) = vals[k] else { continue };
            take(run[k], vk);
            let up = |j: usize| vals[j].is_none_or(|x| x <= vk);
            let is_max = (k == 0 || up(k - 1)) && (k + 1 == run.len() || up(k + 1));
            if is_max && run.len() > 1 {
                let (a, b) = (run[k.saturating_sub(1)], run[(k + 1).min(run.len() - 1)]);
                let (t, v) = maximize(f, a, b, 1e-9);
                if v.is_finite() {
                    take(t, v);
                }
            }
        }
    }
    let (t, tau) = best?;
    Some(Contact {
        tau,
        point: curve.eval(t),
        on: ContactOn::Edge { edge: i, t },
    })
}

/// Parameters of a point on an analytic surface (closed-form closest point).
fn analytic_uv(s: &Surface, p: Point3) -> (f64, f64) {
    let iv = |lo, hi| Interval { lo, hi };
    let big = iv(-1e9, 1e9);
    let turn = iv(0.0, TAU);
    let b = match s {
        Surface::Sphere { .. } => UvBox::new(turn, iv(-FRAC_PI_2, FRAC_PI_2)),
        Surface::Plane { .. } => UvBox::new(big, big),
        Surface::Torus { .. } => UvBox::new(turn, turn),
        _ => UvBox::new(turn, big),
    };
    let c = s.closest(p, b, &Tol::for_machining(1e-7, 1e-4));
    (c.u, c.v)
}

/// Best valid candidate point of a face.
fn best_point(
    q: &QueryFace,
    face: u32,
    s: &Surface,
    ax: Axis,
    c: &Cutter,
    pts: impl IntoIterator<Item = Point3>,
) -> Option<Contact> {
    let mut best: Option<Contact> = None;
    for p in pts {
        let Some(tau) = ax.tau(c, p) else { continue };
        if best.is_some_and(|b| b.tau >= tau) {
            continue;
        }
        let (u, v) = analytic_uv(s, p);
        if q.trim.contains(u, v) {
            best = Some(Contact {
                tau,
                point: p,
                on: ContactOn::Face { face, u, v },
            });
        }
    }
    best
}

fn plane(q: &QueryFace, face: u32, s: &Surface, ax: Axis, c: &Cutter) -> Option<Contact> {
    let Surface::Plane { frame } = s else {
        return None;
    };
    let mut n = frame.z.get();
    if n.z() < 0.0 {
        n = -n;
    }
    let o = frame.origin;
    let sin = libm::hypot(n.x(), n.y());
    let (px, py) = if sin < 1e-12 {
        (ax.x, ax.y)
    } else if n.z() < 1e-9 {
        // A vertical face: contacts are on its edges.
        return None;
    } else {
        let d = c.contact_radius(sin).min(c.radius());
        (ax.x - d * n.x() / sin, ax.y - d * n.y() / sin)
    };
    let pz = o.z() - (n.x() * (px - o.x()) + n.y() * (py - o.y())) / n.z();
    best_point(q, face, s, ax, c, [Point3::new(px, py, pz)])
}

/// Ball on sphere or cylinder: the ball centre is on the offset surface
/// (radius ρ ± R) and on the axis; the contact is on the line from the
/// centre to the sphere centre or cylinder axis.
fn ball_on_quadric(q: &QueryFace, face: u32, s: &Surface, ax: Axis, c: &Cutter) -> Option<Contact> {
    let rad = c.radius();
    let mut pts: Vec<Point3> = Vec::new();
    match s {
        Surface::Sphere { frame, r } => {
            let o = frame.origin;
            let dxy2 = (ax.x - o.x()).powi(2) + (ax.y - o.y()).powi(2);
            for sr in [r + rad, (r - rad).abs()] {
                let h2 = sr * sr - dxy2;
                if h2 < 0.0 {
                    continue;
                }
                for sg in [1.0, -1.0] {
                    let cz = o.z() + sg * h2.sqrt();
                    let ctr = Point3::new(ax.x, ax.y, cz);
                    if let Some(u) = (ctr - o).normalized() {
                        pts.push(o + u.get() * *r);
                        pts.push(o - u.get() * *r);
                    }
                }
            }
        }
        Surface::Cylinder { frame, r } => {
            let (o, a) = (frame.origin, frame.z.get());
            let w0 = Vec3::new(ax.x - o.x(), ax.y - o.y(), -o.z());
            let wa = w0.dot(a);
            let qa = 1.0 - a.z() * a.z();
            if qa < 1e-12 {
                return None;
            }
            let qb = 2.0 * (w0.z() - wa * a.z());
            let qc = w0.length_squared() - wa * wa;
            for sr in [r + rad, (r - rad).abs()] {
                for z in fluut_geom::roots::quadratic(qa, qb, qc - sr * sr) {
                    let ctr = Point3::new(ax.x, ax.y, z);
                    let foot = o + a * (ctr - o).dot(a);
                    if let Some(u) = (ctr - foot).normalized() {
                        pts.push(foot + u.get() * *r);
                        pts.push(foot - u.get() * *r);
                    }
                }
            }
        }
        _ => return None,
    }
    best_point(q, face, s, ax, c, pts)
}

/// Upward unit normal and its parameter derivatives.
fn normal_and_derivs(d: &SurfDerivs) -> Option<(Vec3, Vec3, Vec3)> {
    let nn = d.su.cross(d.sv);
    let len = nn.length();
    if len < 1e-300 || !len.is_finite() {
        return None;
    }
    let sign = if nn.z() < 0.0 { -1.0 } else { 1.0 };
    let n = nn * (sign / len);
    let nu = d.suu.cross(d.sv) + d.su.cross(d.suv);
    let nv = d.suv.cross(d.sv) + d.su.cross(d.svv);
    let dn = |x: Vec3| (x * sign - n * n.dot(x * sign)) / len;
    Some((n, dn(nu), dn(nv)))
}

/// Contact map `G(u, v) = S_xy + (R − r)·n̂_xy + r·n_xy` (the axis position
/// at which the tool touches at `(u, v)`) and its Jacobian.
fn contact_map(s: &Surface, c: &Cutter, u: f64, v: f64) -> Option<([f64; 2], [[f64; 2]; 2])> {
    let d = s.derivs(u, v);
    let (n, nu, nv) = normal_and_derivs(&d)?;
    let (rf, r) = (c.flat_radius(), c.corner());
    let k = [n.x(), n.y()];
    let kl = libm::hypot(k[0], k[1]);
    let mut g = [d.p.x() + r * k[0], d.p.y() + r * k[1]];
    let mut j = [
        [d.su.x() + r * nu.x(), d.sv.x() + r * nv.x()],
        [d.su.y() + r * nu.y(), d.sv.y() + r * nv.y()],
    ];
    if rf > 0.0 {
        if kl < 1e-12 {
            return None;
        }
        let kh = [k[0] / kl, k[1] / kl];
        g[0] += rf * kh[0];
        g[1] += rf * kh[1];
        for (col, dk) in [(0, nu), (1, nv)] {
            let dk = [dk.x(), dk.y()];
            let along = kh[0] * dk[0] + kh[1] * dk[1];
            j[0][col] += rf * (dk[0] - kh[0] * along) / kl;
            j[1][col] += rf * (dk[1] - kh[1] * along) / kl;
        }
    }
    Some((g, j))
}

fn solve2(j: [[f64; 2]; 2], r: [f64; 2]) -> Option<[f64; 2]> {
    let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
    let scale = (j[0][0].abs() + j[0][1].abs()) * (j[1][0].abs() + j[1][1].abs());
    if det.abs() <= 1e-14 * scale || !det.is_finite() {
        return None;
    }
    Some([
        (j[1][1] * r[0] - j[0][1] * r[1]) / det,
        (j[0][0] * r[1] - j[1][0] * r[0]) / det,
    ])
}

/// Damped Newton on `G(u, v) = axis`, inside `bounds`.
fn newton_contact(
    s: &Surface,
    c: &Cutter,
    ax: Axis,
    start: (f64, f64),
    bounds: UvBox,
    tol: f64,
) -> Option<(f64, f64)> {
    let (mut u, mut v) = start;
    let resid =
        |u: f64, v: f64| contact_map(s, c, u, v).map(|(g, j)| ([g[0] - ax.x, g[1] - ax.y], j));
    let (mut r, mut j) = resid(u, v)?;
    for _ in 0..20 {
        let rn = libm::hypot(r[0], r[1]);
        if rn < tol {
            return Some((u, v));
        }
        let step = solve2(j, [-r[0], -r[1]])?;
        let mut lambda = 1.0;
        let mut moved = false;
        for _ in 0..12 {
            let (nu, nv) = bounds.clamp(u + lambda * step[0], v + lambda * step[1]);
            if let Some((r2, j2)) = resid(nu, nv)
                && libm::hypot(r2[0], r2[1]) < rn
            {
                (u, v, r, j) = (nu, nv, r2, j2);
                moved = true;
                break;
            }
            lambda *= 0.5;
        }
        if !moved {
            return None;
        }
    }
    None
}

/// Newton for a horizontal point under the flat bottom: a stationary
/// point of `z − ε·d²`. The small pull towards the axis picks the point
/// nearest the axis on a horizontal ridge (where `∇²z` is singular) and
/// changes `z` by far less than the solver tolerance.
fn newton_flat(
    s: &Surface,
    c: &Cutter,
    ax: Axis,
    start: (f64, f64),
    bounds: UvBox,
    tol: f64,
) -> Option<(f64, f64)> {
    let eps = 1e-5 / (c.flat_radius() * c.flat_radius());
    let (mut u, mut v) = start;
    for _ in 0..40 {
        let d = s.derivs(u, v);
        let w = [d.p.x() - ax.x, d.p.y() - ax.y];
        let dotw = |x: Vec3| w[0] * x.x() + w[1] * x.y();
        let dotxy = |a: Vec3, b: Vec3| a.x() * b.x() + a.y() * b.y();
        let gr = [
            d.su.z() - 2.0 * eps * dotw(d.su),
            d.sv.z() - 2.0 * eps * dotw(d.sv),
        ];
        let huv = d.suv.z() - 2.0 * eps * (dotxy(d.su, d.sv) + dotw(d.suv));
        let h = [
            [
                d.suu.z() - 2.0 * eps * (dotxy(d.su, d.su) + dotw(d.suu)),
                huv,
            ],
            [
                huv,
                d.svv.z() - 2.0 * eps * (dotxy(d.sv, d.sv) + dotw(d.svv)),
            ],
        ];
        let step = solve2(h, [-gr[0], -gr[1]])?;
        let (nu, nv) = bounds.clamp(u + step[0], v + step[1]);
        let moved = (d.su * (nu - u) + d.sv * (nv - v)).length();
        (u, v) = (nu, nv);
        if moved < tol {
            return Some((u, v));
        }
    }
    None
}

/// Newton on `S_xy(u, v) = (x, y)`: the surface point vertically above or
/// below a target.
fn vertical_project(
    s: &Surface,
    x: f64,
    y: f64,
    start: (f64, f64),
    bounds: UvBox,
) -> Option<(f64, f64)> {
    let (mut u, mut v) = start;
    for _ in 0..12 {
        let d = s.derivs(u, v);
        let r = [x - d.p.x(), y - d.p.y()];
        if libm::hypot(r[0], r[1]) < 1e-6 {
            return Some((u, v));
        }
        let j = [[d.su.x(), d.sv.x()], [d.su.y(), d.sv.y()]];
        let step = solve2(j, r)?;
        let scale = d.su.length().max(d.sv.length()).max(1e-300);
        // Keep steps within about a radian of the surface parametrisation.
        let len = (d.su * step[0] + d.sv * step[1]).length();
        let k = (scale * 2.0 / len.max(1e-300)).min(1.0);
        let (nu, nv) = bounds.clamp(u + k * step[0], v + k * step[1]);
        if (d.su * (nu - u) + d.sv * (nv - v)).length() < 1e-9 {
            // Stuck on the parameter box: the target is off the face.
            return Some((u, v));
        }
        (u, v) = (nu, nv);
    }
    Some((u, v))
}

#[allow(clippy::too_many_arguments)]
fn numeric(
    q: &QueryFace,
    face: u32,
    s: &Surface,
    uv: UvBox,
    bx: &Aabb3,
    ax: Axis,
    c: &Cutter,
    seeding: &Seeding,
    stats: &mut SolveStats,
) -> Option<Contact> {
    let n = seeding.grid.max(1);
    let mut grid: Vec<(Point3, f64, f64)> = Vec::with_capacity(n * n);
    for i in 0..n {
        for k in 0..n {
            let (u, v) = (
                uv.u.at((i as f64 + 0.5) / n as f64),
                uv.v.at((k as f64 + 0.5) / n as f64),
            );
            grid.push((s.eval(u, v), u, v));
        }
    }
    let rad = c.radius();
    let analytic = matches!(
        s,
        Surface::Plane { .. }
            | Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. }
    );
    let ztop = bx.max.z();
    let mut targets = vec![(ax.x, ax.y)];
    for ring in 1..=seeding.rings {
        let rr = rad * ring as f64 / seeding.rings as f64 * (1.0 - 1e-3);
        for k in 0..8 {
            let (sn, cs) = libm::sincos(k as f64 * std::f64::consts::FRAC_PI_4 + 0.3 * ring as f64);
            targets.push((ax.x + rr * cs, ax.y + rr * sn));
        }
    }
    // Rank by τ, penalising points outside the footprint.
    let estimate = |p: Point3| {
        let d = ax.lateral(p);
        p.z() - c.h(d.min(rad)) - 10.0 * (d - rad).max(0.0)
    };
    let mut seeds: Vec<(f64, f64, f64)> = grid.iter().map(|g| (estimate(g.0), g.1, g.2)).collect();
    for (tx, ty) in targets {
        if tx < bx.min.x() || tx > bx.max.x() || ty < bx.min.y() || ty > bx.max.y() {
            continue;
        }
        let start = if analytic {
            // Closed-form closest point to the target at the top of the
            // box (so the upper layer of a surface wins), moved into the
            // face's parameter box by whole periods.
            let (mut u, mut v) = analytic_uv(s, Point3::new(tx, ty, ztop));
            let (pu, pv) = s.periodicity();
            if let Some(p) = pu {
                u += p * ((q.uv.u.at(0.5) - u) / p).round();
            }
            if let Some(p) = pv {
                v += p * ((q.uv.v.at(0.5) - v) / p).round();
            }
            (u, v)
        } else {
            let top = Point3::new(tx, ty, ztop);
            grid.iter()
                .min_by(|a, b| a.0.distance(top).total_cmp(&b.0.distance(top)))
                .map(|g| (g.1, g.2))?
        };
        if let Some((u, v)) = vertical_project(s, tx, ty, start, q.uv) {
            seeds.push((estimate(s.eval(u, v)), u, v));
        }
    }
    seeds.sort_by(|a, b| b.0.total_cmp(&a.0));
    let tol = 1e-8;
    let mut best: Option<Contact> = None;
    let mut consider = |u: f64, v: f64, flat: bool| {
        let p = s.eval(u, v);
        let d = ax.lateral(p);
        let limit = if flat { c.flat_radius() } else { c.radius() };
        if d > limit + 1e-9 {
            return;
        }
        let tau = p.z() - c.h(d.min(c.radius()));
        if best.is_some_and(|b| b.tau >= tau) || !q.trim.contains(u, v) {
            return;
        }
        best = Some(Contact {
            tau,
            point: p,
            on: ContactOn::Face { face, u, v },
        });
    };
    for &(_, u, v) in seeds.iter().take(seeding.starts) {
        stats.newton_runs += 1;
        if let Some((u, v)) = newton_contact(s, c, ax, (u, v), q.uv, tol) {
            stats.newton_converged += 1;
            consider(u, v, false);
        }
        if c.flat_radius() > 0.0 {
            stats.newton_runs += 1;
            if let Some((u, v)) = newton_flat(s, c, ax, (u, v), q.uv, tol) {
                stats.newton_converged += 1;
                consider(u, v, true);
            }
        }
    }
    best
}

/// Highest contact on face primitive `(face, uv)`.
#[allow(clippy::too_many_arguments)]
pub fn face(
    g: &QueryGeometry,
    face: u32,
    uv: UvBox,
    bx: &Aabb3,
    ax: Axis,
    c: &Cutter,
    seeding: &Seeding,
    stats: &mut SolveStats,
) -> Option<Contact> {
    let q = g.faces[face as usize].as_ref()?;
    let s = &g.body.surfaces[g.body.faces[face as usize].surface as usize];
    let is_ball = c.flat_radius() == 0.0;
    match s {
        Surface::Plane { .. } => {
            stats.faces_closed_form += 1;
            plane(q, face, s, ax, c)
        }
        Surface::Sphere { .. } | Surface::Cylinder { .. } if is_ball => {
            stats.faces_closed_form += 1;
            ball_on_quadric(q, face, s, ax, c)
        }
        _ => {
            stats.faces_numeric += 1;
            numeric(q, face, s, uv, bx, ax, c, seeding, stats)
        }
    }
}
