// SPDX-License-Identifier: MIT OR Apache-2.0

//! Per-primitive drop solvers (D07 §4) for the tool axis +Z.
//!
//! Every result is a real point `p` of the primitive inside the footprint,
//! with `τ = p.z − h(d(p))`. So a solver can miss the contact (and the
//! caller gouges) but never place the tool too high.

use super::geom::{QueryFace, QueryGeometry, RIM_CHUNK, curve_box};
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
/// onto the surface from the nearest sample (projections that do not
/// converge are dropped). Newton runs from the `starts` candidates with the
/// highest `τ` (points outside the footprint ranked down by their distance).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Seeding {
    pub grid: usize,
    pub rings: usize,
    pub starts: usize,
    /// Search long curved edges only where the tool reaches them
    /// ([`edge_split`]): for tools far smaller than the edges.
    pub split_edges: bool,
}

impl Default for Seeding {
    fn default() -> Self {
        Seeding {
            grid: 4,
            rings: 2,
            starts: 3,
            split_edges: false,
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
    /// Face boundaries searched ([`rim`]).
    pub rims: u64,
    /// Patches solved only because another patch of their face may have
    /// missed a contact (see `drop`).
    pub rescues: u64,
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

/// As [`edge`], for a tool much smaller than the edge: the edge is halved
/// again and again, keeping the halves whose box reaches the footprint, so
/// that a long curved edge far from the tool is not sampled along its whole
/// length.
pub fn edge_split(g: &QueryGeometry, i: u32, ax: Axis, c: &Cutter) -> Option<Contact> {
    let e = &g.body.edges[i as usize];
    let curve = &g.body.curves[e.curve? as usize];
    let rad = c.radius();
    let mut best: Option<Contact> = None;
    let mut stack = vec![(e.range, 0u32)];
    while let Some((r, depth)) = stack.pop() {
        let b = curve_box(curve, r);
        let dx = (b.min.x() - ax.x).max(ax.x - b.max.x()).max(0.0);
        let dy = (b.min.y() - ax.y).max(ax.y - b.max.y()).max(0.0);
        if libm::hypot(dx, dy) > rad {
            continue;
        }
        if depth < 14 && (b.max - b.min).length() > 8.0 * rad {
            let m = 0.5 * (r.lo + r.hi);
            // Higher parameters first, so the lower are popped first.
            stack.push((Interval { lo: m, hi: r.hi }, depth + 1));
            stack.push((Interval { lo: r.lo, hi: m }, depth + 1));
            continue;
        }
        if let Some(x) = edge_in(g, i, r, ax, c)
            && best.is_none_or(|b| x.tau > b.tau)
        {
            best = Some(x);
        }
    }
    best
}

pub fn edge(g: &QueryGeometry, i: u32, ax: Axis, c: &Cutter) -> Option<Contact> {
    edge_in(g, i, g.body.edges[i as usize].range, ax, c)
}

fn edge_in(g: &QueryGeometry, i: u32, range: Interval, ax: Axis, c: &Cutter) -> Option<Contact> {
    let e = &g.body.edges[i as usize];
    let curve = &g.body.curves[e.curve? as usize];
    let f = |t: f64| ax.tau(c, curve.eval(t));
    let mut best: Option<(f64, f64)> = None;
    let mut take = |t: f64, v: f64| {
        if best.is_none_or(|b| v > b.1) {
            best = Some((t, v));
        }
    };
    for run in footprint_runs(curve, range, ax, c) {
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
pub(crate) fn analytic_uv(s: &Surface, p: Point3) -> (f64, f64) {
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

/// Best valid candidate point of a face. `trimmed` is raised to the τ of
/// better candidates outside the trim.
fn best_point(
    q: &QueryFace,
    face: u32,
    s: &Surface,
    ax: Axis,
    c: &Cutter,
    pts: impl IntoIterator<Item = Point3>,
    trimmed: &mut f64,
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
        } else {
            *trimmed = trimmed.max(tau);
        }
    }
    best
}

fn plane(
    q: &QueryFace,
    face: u32,
    s: &Surface,
    ax: Axis,
    c: &Cutter,
    trimmed: &mut f64,
) -> Option<Contact> {
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
    best_point(q, face, s, ax, c, [Point3::new(px, py, pz)], trimmed)
}

/// Ball on sphere or cylinder: the ball centre is on the offset surface
/// (radius ρ ± R) and on the axis; the contact is on the line from the
/// centre to the sphere centre or cylinder axis.
fn ball_on_quadric(
    q: &QueryFace,
    face: u32,
    s: &Surface,
    ax: Axis,
    c: &Cutter,
    trimmed: &mut f64,
) -> Option<Contact> {
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
    best_point(q, face, s, ax, c, pts, trimmed)
}

/// Flat and bull-nose tools on the outside of a sphere, in closed form:
/// the sphere grown by the corner radius r touches the disc of radius
/// R − r at height τ + r. Seam-proof where the numeric seeding is not
/// (M6). Candidates only: [`best_point`] keeps those inside the face.
fn bull_on_sphere(
    q: &QueryFace,
    face: u32,
    s: &Surface,
    ax: Axis,
    c: &Cutter,
    trimmed: &mut f64,
) -> Option<Contact> {
    let Surface::Sphere { frame, r: rho } = s else {
        return None;
    };
    let o = frame.origin;
    let (rf, r) = (c.flat_radius(), c.corner());
    let grown = rho + r;
    let (dx, dy) = (ax.x - o.x(), ax.y - o.y());
    let d = libm::hypot(dx, dy);
    let mut pts = vec![o + Vec3::new(0.0, 0.0, *rho)];
    let e = d - rf;
    if e > 0.0 && e <= grown {
        let h = (grown * grown - e * e).sqrt();
        let disc = Vec3::new(dx * e / d, dy * e / d, h);
        if let Some(u) = disc.normalized() {
            pts.push(o + u.get() * *rho);
        }
    }
    best_point(q, face, s, ax, c, pts, trimmed)
}

/// The upward unit normal of `su × sv`, its length and the sign applied.
fn up_normal(su: Vec3, sv: Vec3) -> Option<(Vec3, f64, f64)> {
    let nn = su.cross(sv);
    let len = nn.length();
    if len < 1e-300 || !len.is_finite() {
        return None;
    }
    let sign = if nn.z() < 0.0 { -1.0 } else { 1.0 };
    Some((nn * (sign / len), len, sign))
}

/// Upward unit normal and its parameter derivatives.
fn normal_and_derivs(d: &SurfDerivs) -> Option<(Vec3, Vec3, Vec3)> {
    let (n, len, sign) = up_normal(d.su, d.sv)?;
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
    let g = contact_point(d.p, n, c)?;
    let mut j = [
        [d.su.x() + r * nu.x(), d.sv.x() + r * nv.x()],
        [d.su.y() + r * nu.y(), d.sv.y() + r * nv.y()],
    ];
    if rf > 0.0 {
        let kh = [k[0] / kl, k[1] / kl];
        for (col, dk) in [(0, nu), (1, nv)] {
            let dk = [dk.x(), dk.y()];
            let along = kh[0] * dk[0] + kh[1] * dk[1];
            j[0][col] += rf * (dk[0] - kh[0] * along) / kl;
            j[1][col] += rf * (dk[1] - kh[1] * along) / kl;
        }
    }
    Some((g, j))
}

/// `G` at surface point `p` with upward normal `n` (as [`contact_map`]).
fn contact_point(p: Point3, n: Vec3, c: &Cutter) -> Option<[f64; 2]> {
    let (rf, r) = (c.flat_radius(), c.corner());
    let k = [n.x(), n.y()];
    let mut g = [p.x() + r * k[0], p.y() + r * k[1]];
    if rf > 0.0 {
        let kl = libm::hypot(k[0], k[1]);
        if kl < 1e-12 {
            return None;
        }
        g[0] += rf * (k[0] / kl);
        g[1] += rf * (k[1] / kl);
    }
    Some(g)
}

/// [`contact_map`] without the Jacobian: first derivatives only. The same
/// bits as its `G`, and `None` exactly where it is `None`.
fn contact_g(s: &Surface, c: &Cutter, u: f64, v: f64) -> Option<[f64; 2]> {
    let (p, su, sv) = s.derivs1(u, v);
    let (n, _, _) = up_normal(su, sv)?;
    contact_point(p, n, c)
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
    let split = matches!(s, Surface::BSpline(_));
    for _ in 0..20 {
        let rn = libm::hypot(r[0], r[1]);
        if rn < tol {
            // One more full step with the Jacobian in hand: the residual
            // goes from below `tol` to about its square, so a contact on
            // the footprint rim is not turned away for sitting a few 1e-9
            // outside it.
            if let Some(step) = solve2(j, [-r[0], -r[1]]) {
                return Some(bounds.clamp(u + step[0], v + step[1]));
            }
            return Some((u, v));
        }
        let step = solve2(j, [-r[0], -r[1]])?;
        let mut lambda = 1.0;
        let mut moved = false;
        for _ in 0..12 {
            let (nu, nv) = bounds.clamp(u + lambda * step[0], v + lambda * step[1]);
            // On a B-spline a trial point takes G only, from first
            // derivatives (most trials are rejected); the same bits.
            if split {
                if let Some(g) = contact_g(s, c, nu, nv)
                    && libm::hypot(g[0] - ax.x, g[1] - ax.y) < rn
                {
                    (r, j) = resid(nu, nv)?;
                    (u, v) = (nu, nv);
                    moved = true;
                    break;
                }
            } else if let Some((r2, j2)) = resid(nu, nv)
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
/// below a target. The flag says whether it converged; otherwise the point
/// is where Newton stopped (stuck on the parameter box: the target is off
/// the face, or the start was at a spot where `S_xy` does not move).
fn vertical_project(
    s: &Surface,
    x: f64,
    y: f64,
    start: (f64, f64),
    bounds: UvBox,
) -> Option<(f64, f64, bool)> {
    let (mut u, mut v) = start;
    for _ in 0..12 {
        let (p, su, sv) = s.derivs1(u, v);
        let r = [x - p.x(), y - p.y()];
        if libm::hypot(r[0], r[1]) < 1e-6 {
            return Some((u, v, true));
        }
        let j = [[su.x(), sv.x()], [su.y(), sv.y()]];
        let step = solve2(j, r)?;
        let scale = su.length().max(sv.length()).max(1e-300);
        // Keep steps within about a radian of the surface parametrisation.
        let len = (su * step[0] + sv * step[1]).length();
        let k = (scale * 2.0 / len.max(1e-300)).min(1.0);
        let (nu, nv) = bounds.clamp(u + k * step[0], v + k * step[1]);
        if (su * (nu - u) + sv * (nv - v)).length() < 1e-9 {
            return Some((u, v, false));
        }
        (u, v) = (nu, nv);
    }
    Some((u, v, false))
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
    trimmed: &mut f64,
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
        if analytic {
            // Closed-form closest points to the target at the top of the
            // box (so the upper layer of a surface wins), then lower down,
            // moved into the face's parameter box by whole periods. A start
            // on a spot where S_xy does not move (the side of a torus tube)
            // gets stuck, so the next start is tried.
            for z in [ztop, 0.5 * (ztop + bx.min.z()), bx.min.z()] {
                let (mut u, mut v) = analytic_uv(s, Point3::new(tx, ty, z));
                let (pu, pv) = s.periodicity();
                if let Some(p) = pu {
                    u += p * ((q.uv.u.at(0.5) - u) / p).round();
                }
                if let Some(p) = pv {
                    v += p * ((q.uv.v.at(0.5) - v) / p).round();
                }
                if let Some((u, v, true)) = vertical_project(s, tx, ty, (u, v), q.uv) {
                    seeds.push((estimate(s.eval(u, v)), u, v));
                    break;
                }
            }
        } else {
            let top = Point3::new(tx, ty, ztop);
            let start = grid
                .iter()
                .min_by(|a, b| a.0.distance(top).total_cmp(&b.0.distance(top)))
                .map(|g| (g.1, g.2))?;
            if let Some((u, v, true)) = vertical_project(s, tx, ty, start, q.uv) {
                seeds.push((estimate(s.eval(u, v)), u, v));
            }
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
        if best.is_some_and(|b| b.tau >= tau) {
            return;
        }
        if !q.trim.contains(u, v) {
            *trimmed = trimmed.max(tau);
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
    face_trimmed(g, face, uv, bx, ax, c, seeding, stats).0
}

/// As [`face`], with the highest τ of a stationary point the trim left out
/// (−∞ if none). Where it beats the contact, the face's highest point in
/// the footprint is on its boundary ([`rim`]).
#[allow(clippy::too_many_arguments)]
pub fn face_trimmed(
    g: &QueryGeometry,
    face: u32,
    uv: UvBox,
    bx: &Aabb3,
    ax: Axis,
    c: &Cutter,
    seeding: &Seeding,
    stats: &mut SolveStats,
) -> (Option<Contact>, f64) {
    let mut trimmed = f64::NEG_INFINITY;
    let Some(q) = g.faces[face as usize].as_ref() else {
        return (None, trimmed);
    };
    let t = &mut trimmed;
    let s = &g.body.surfaces[g.body.faces[face as usize].surface as usize];
    let is_ball = c.flat_radius() == 0.0;
    let r = match s {
        Surface::Plane { .. } => {
            stats.faces_closed_form += 1;
            plane(q, face, s, ax, c, t)
        }
        Surface::Sphere { .. } | Surface::Cylinder { .. } if is_ball => {
            stats.faces_closed_form += 1;
            ball_on_quadric(q, face, s, ax, c, t)
        }
        // Outside of a sphere: the closed form is the whole answer.
        Surface::Sphere { .. } if g.body.faces[face as usize].same_sense => {
            stats.faces_closed_form += 1;
            bull_on_sphere(q, face, s, ax, c, t)
        }
        Surface::Sphere { .. } => {
            stats.faces_closed_form += 1;
            let a = bull_on_sphere(q, face, s, ax, c, t);
            let b = numeric(q, face, s, uv, bx, ax, c, seeding, stats, t);
            match (a, b) {
                (Some(a), Some(b)) => Some(if b.tau > a.tau { b } else { a }),
                (a, b) => a.or(b),
            }
        }
        _ => {
            stats.faces_numeric += 1;
            numeric(q, face, s, uv, bx, ax, c, seeding, stats, t)
        }
    };
    (r, trimmed)
}

/// The highest point of face `face`'s own boundary (its trim loops on the
/// surface) in the footprint. It covers the boundary where the edge
/// solvers cannot: an edge curve a little off its face (a model gap) can
/// be out of reach while the face's boundary is not, and at the rim of a
/// ball on a near-vertical wall that misses the top of the wall. Only
/// contacts above `above` are searched for.
pub fn rim(g: &QueryGeometry, face: u32, ax: Axis, c: &Cutter, above: f64) -> Option<Contact> {
    let q = g.faces[face as usize].as_ref()?;
    let s = &g.body.surfaces[g.body.faces[face as usize].surface as usize];
    let rad = c.radius();
    let mut best: Option<Contact> = None;
    let mut floor = above;
    for lp in &q.rim {
        let pts = &lp.pts;
        let n = pts.len();
        if n < 2 {
            continue;
        }
        // Position t along the loop: segment ⌊t⌋, linear in (u, v).
        let at = |t: f64| {
            let i = (t.floor().max(0.0) as usize).min(n - 2);
            let f = t - i as f64;
            let (a, b) = (pts[i], pts[i + 1]);
            let (u, v) = (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f);
            (u, v, s.eval(u, v))
        };
        let tau_at = |t: f64| ax.tau(c, at(t).2);
        let lat = |t: f64| ax.lateral(at(t).2);
        // The footprint end of [inn, out] (inn inside, out outside).
        let rim_end = |mut inn: f64, mut out: f64| {
            for _ in 0..50 {
                let m = 0.5 * (inn + out);
                if lat(m) <= rad {
                    inn = m;
                } else {
                    out = m;
                }
            }
            inn
        };
        let mut take = |t: f64, floor: &mut f64| {
            let (u, v, p) = at(t);
            let Some(tau) = ax.tau(c, p) else { return };
            if tau > *floor {
                *floor = tau;
                best = Some(Contact {
                    tau,
                    point: p,
                    on: ContactOn::Face { face, u, v },
                });
            }
        };
        for (ci, bx) in lp.boxes.iter().enumerate() {
            let dx = (bx[0] - ax.x).max(ax.x - bx[2]).max(0.0);
            let dy = (bx[1] - ax.y).max(ax.y - bx[3]).max(0.0);
            if libm::hypot(dx, dy) > rad {
                continue;
            }
            let k0 = ci * RIM_CHUNK;
            for k in k0..(k0 + RIM_CHUNK).min(n - 1) {
                let (p0, p1) = (pts[k].2, pts[k + 1].2);
                // Bounds over the segment from its ends and its length: a
                // point s along it is within s of one end and L − s of the
                // other (the path on the surface is about the chord).
                let len = 1.01 * p0.distance(p1);
                let (d0, d1) = (ax.lateral(p0), ax.lateral(p1));
                let dmin = (0.5 * (d0 + d1 - len)).max(0.0);
                if dmin > rad {
                    continue;
                }
                let zmax = 0.5 * (p0.z() + p1.z() + len);
                if zmax - c.h(dmin) <= floor {
                    continue;
                }
                let (a, b) = (k as f64, (k + 1) as f64);
                let (lo, hi) = match (d0 <= rad, d1 <= rad) {
                    (true, true) => (a, b),
                    (true, false) => (a, rim_end(a, b)),
                    (false, true) => (rim_end(b, a), b),
                    (false, false) => {
                        let (t, neg) = maximize(|t| Some(-lat(t)), a, b, 1e-12);
                        if -neg > rad {
                            continue;
                        }
                        (rim_end(t, a), rim_end(t, b))
                    }
                };
                take(lo, &mut floor);
                take(hi, &mut floor);
                if hi > lo {
                    let (t, v) = maximize(tau_at, lo, hi, 1e-12);
                    if v.is_finite() {
                        take(t, &mut floor);
                    }
                }
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cutter::synth::bspline_square;

    #[test]
    fn trial_contact_points_match_the_contact_map() {
        // Newton's line search tests trial points with `contact_g` (first
        // derivatives only): it must give the bits of `contact_map`'s G,
        // and fail where it fails, or drops would change.
        let (s, _) = bspline_square(Point3::new(-3.0, 2.0, 1.0), 20.0, 7, |i, j| {
            libm::sin(i as f64) * 4.0 + libm::cos(1.7 * j as f64) * 3.0
        });
        let cutters = [
            Cutter::new(6.0, 3.0).unwrap(),
            Cutter::new(6.0, 1.0).unwrap(),
            Cutter::new(6.0, 0.0).unwrap(),
        ];
        let mut n = 0;
        for c in &cutters {
            for i in 0..=40 {
                for k in 0..=40 {
                    let (u, v) = (i as f64 / 40.0, k as f64 / 40.0);
                    let full = contact_map(&s, c, u, v).map(|(g, _)| g.map(f64::to_bits));
                    let g = contact_g(&s, c, u, v).map(|g| g.map(f64::to_bits));
                    assert_eq!(full, g, "{u} {v}");
                    n += usize::from(g.is_some());
                }
            }
        }
        assert!(n > 4000);
    }
}
