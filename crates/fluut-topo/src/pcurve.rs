// SPDX-License-Identifier: MIT OR Apache-2.0

//! P-curves by projection (design D04 §5.2, CH-08).
//!
//! A p-curve maps the parameter `t` of its edge's 3-D curve, over
//! `Edge::range`, to `(u, v)` on the face's surface, so `S(pc(t)) = C(t)`.
//! Where the image is known exactly (a curve on a plane, a coaxial circle
//! on a surface of revolution, a generator line of a cylinder or cone, a
//! meridian of a sphere or torus) the p-curve is exact. Otherwise the edge
//! is sampled, each sample projected with the closest-point query and the
//! samples joined by a C¹ cubic B-spline whose pieces are split until they
//! map within `tol_model / 2` of the projected points.
//!
//! Periodic directions are unwrapped twice: along an edge, each sample is
//! taken within half a period of the previous one; around a loop, each
//! p-curve is shifted by whole periods to start where the previous one
//! ended ([`align_face`]). Where the surface collapses in one direction (a
//! sphere pole, a cone apex, a collapsed B-spline side), that coordinate
//! is meaningless and comes from the neighbouring samples.

use crate::brep::BrepBody;
use fluut_geom::{
    BSplineCurve2, Curve2, Curve3, Frame, Interval, Point2, Point3, Surface, SurfaceKind, Tol,
    UvBox, Vec2, Vec3,
};
use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;

/// Points per edge at which a p-curve is checked against the 3-D curve.
pub const CHECK_POINTS: usize = 16;
/// Most samples added to one fitted p-curve.
const MAX_SAMPLES: usize = 2048;
/// Deepest splitting of one initial piece.
const MAX_DEPTH: u32 = 24;
/// Angle (radians) below which directions count as parallel or
/// perpendicular for the exact cases.
const ANG: f64 = 1e-9;
/// Largest parameter range searched on an unbounded surface, in mm.
const BIG: f64 = 1e5;

/// A coedge whose p-curve is further than `tol_model` from its edge.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub face: u32,
    pub coedge: u32,
    pub kind: SurfaceKind,
    /// Largest `|S(pc(t)) − C(t)|` at the check points.
    pub dev: f64,
    /// The 3-D edge itself is further than `tol_model` from the surface, so
    /// no p-curve can be within tolerance.
    pub off_surface: bool,
    /// No p-curve at all could be computed.
    pub missing: bool,
}

/// What [`compute`] did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    /// Coedges of faces with a 3-D edge curve.
    pub coedges: usize,
    /// P-curves already present (read from the file), checked but kept.
    pub given: usize,
    /// Computed in closed form.
    pub exact: usize,
    /// Computed by sampling and fitting.
    pub sampled: usize,
    /// Coedges whose p-curve is within `tol_model` at every check point.
    pub within_tol: usize,
    /// Largest deviation over all coedges.
    pub max_dev: f64,
    pub failures: Vec<Failure>,
}

/// Fills `b.pcurves` for every coedge of every face that has none yet,
/// aligns each face's p-curves across seams and checks them all.
pub fn compute(b: &mut BrepBody) -> Stats {
    let n = b.coedges.len();
    b.pcurves.resize(n, None);
    let given: Vec<bool> = b.pcurves.iter().map(Option::is_some).collect();
    let tol_model = b.tol_model;
    let mut st = Stats::default();
    // A seam edge appears twice on one face: project it once.
    let mut cache: BTreeMap<(u32, u32), Option<(Curve2, bool)>> = BTreeMap::new();
    for fi in 0..b.faces.len() {
        let face = &b.faces[fi];
        let s = &b.surfaces[face.surface as usize];
        let mut any_new = false;
        for &l in &face.loops {
            for &c in &b.loops[l as usize].coedges {
                let edge = b.coedges[c as usize].edge;
                let e = &b.edges[edge as usize];
                let Some(curve) = e.curve else { continue };
                st.coedges += 1;
                if given[c as usize] {
                    st.given += 1;
                    continue;
                }
                let r = cache
                    .entry((edge, face.surface))
                    .or_insert_with(|| project(s, &b.curves[curve as usize], e.range, tol_model))
                    .clone();
                if let Some((pc, exact)) = r {
                    if exact {
                        st.exact += 1;
                    } else {
                        st.sampled += 1;
                    }
                    b.pcurves[c as usize] = Some(pc);
                    any_new = true;
                }
            }
        }
        if any_new {
            align_face(b, fi as u32, &given);
        }
        let face = &b.faces[fi];
        let s = &b.surfaces[face.surface as usize];
        for &l in &face.loops {
            for &c in &b.loops[l as usize].coedges {
                let e = &b.edges[b.coedges[c as usize].edge as usize];
                let Some(curve) = e.curve else { continue };
                let curve = &b.curves[curve as usize];
                let fail = |dev, off_surface, missing| Failure {
                    face: fi as u32,
                    coedge: c,
                    kind: s.kind(),
                    dev,
                    off_surface,
                    missing,
                };
                let Some(pc) = &b.pcurves[c as usize] else {
                    st.failures.push(fail(f64::INFINITY, false, true));
                    continue;
                };
                let dev = deviation(s, curve, e.range, pc);
                st.max_dev = st.max_dev.max(dev);
                if dev <= tol_model {
                    st.within_tol += 1;
                } else {
                    let off = off_surface(s, curve, e.range, pc, tol_model);
                    st.failures.push(fail(dev, off > tol_model, false));
                }
            }
        }
    }
    st
}

/// Largest `|S(pc(t)) − C(t)|` at [`CHECK_POINTS`] + 1 evenly spaced
/// parameters of `range`.
pub fn deviation(s: &Surface, c: &Curve3, range: Interval, pc: &Curve2) -> f64 {
    (0..=CHECK_POINTS)
        .map(|i| {
            let t = range.at(i as f64 / CHECK_POINTS as f64);
            let q = pc.eval(t);
            s.eval(q.x(), q.y()).distance(c.eval(t))
        })
        .fold(0.0, f64::max)
}

/// Largest distance from the 3-D curve to the surface at the check points.
fn off_surface(s: &Surface, c: &Curve3, range: Interval, pc: &Curve2, tol_model: f64) -> f64 {
    let Some(pj) = Projector::new(s, tol_model) else {
        return f64::INFINITY;
    };
    (0..=CHECK_POINTS)
        .map(|i| {
            let t = range.at(i as f64 / CHECK_POINTS as f64);
            let p = c.eval(t);
            let q = pc.eval(t);
            let (u, v) = pj.point(p, Some((q.x(), q.y())));
            s.eval(u, v).distance(p)
        })
        .fold(0.0, f64::max)
}

/// P-curve of `c` over `range` on `s`, and whether it is exact.
pub fn project(s: &Surface, c: &Curve3, range: Interval, tol_model: f64) -> Option<(Curve2, bool)> {
    if let Some(pc) = exact(s, c, range) {
        return Some((pc, true));
    }
    sampled(s, c, range, tol_model).map(|pc| (pc, false))
}

/// Parameter of `pc` whose image on `s` is closest to `p`, searched near
/// `hint`: the whole period of an ellipse around it, the domain of a
/// B-spline, and for a line the projection of `p`'s `(u, v)` (unwrapped
/// near `pc(hint)`) onto it.
pub fn locate(s: &Surface, pc: &Curve2, p: Point3, hint: f64, tol_model: f64) -> Option<f64> {
    let f = |x: f64| {
        let q = pc.eval(x);
        s.eval(q.x(), q.y()).distance(p)
    };
    let (lo, hi) = match pc {
        Curve2::Line { origin, dir } => {
            let pj = Projector::new(s, tol_model)?;
            let h = pc.eval(hint);
            let (u, v) = pj.point(p, Some((h.x(), h.y())));
            let d2 = dir.dot(*dir);
            if d2.is_nan() || d2 <= 0.0 {
                return None;
            }
            let x = (Point2::new(u, v) - *origin).dot(*dir) / d2;
            return x.is_finite().then_some(x);
        }
        Curve2::Ellipse { .. } => (hint - PI, hint + PI),
        Curve2::BSpline(b) => (b.domain().lo, b.domain().hi),
    };
    if !(lo.is_finite() && hi.is_finite()) {
        return None;
    }
    // Best of a coarse scan, then golden-section search around it.
    let n = 64;
    let step = (hi - lo) / n as f64;
    let best = (0..=n)
        .map(|i| lo + step * i as f64)
        .min_by(|&a, &b| f(a).total_cmp(&f(b)))?;
    let (mut a, mut b) = ((best - step).max(lo), (best + step).min(hi));
    let g = 0.5 * (libm::sqrt(5.0) - 1.0);
    for _ in 0..80 {
        let (x1, x2) = (b - g * (b - a), a + g * (b - a));
        if f(x1) <= f(x2) {
            b = x2;
        } else {
            a = x1;
        }
    }
    Some(0.5 * (a + b))
}

// --- exact cases -------------------------------------------------------------

fn angle_of(frame: &Frame, v: Vec3) -> f64 {
    libm::atan2(v.dot(frame.y().get()), v.dot(frame.x.get()))
}

/// The line through `(t0, a)` and `(t1, b)`.
fn line_through(t0: f64, a: Point2, t1: f64, b: Point2) -> Option<Curve2> {
    let h = t1 - t0;
    if h.is_nan() || h <= 0.0 {
        return None;
    }
    let dir = (b - a) * (1.0 / h);
    Some(Curve2::Line {
        origin: a + dir * -t0,
        dir,
    })
}

/// `v` of the closest point on an analytic surface of revolution.
fn analytic_v(s: &Surface, frame: &Frame, p: Point3) -> Option<f64> {
    let l = frame.to_local(p);
    let radial = libm::hypot(l.x(), l.y());
    match s {
        Surface::Cylinder { .. } => Some(l.z()),
        Surface::Cone { r, semi_angle, .. } => {
            let t = libm::tan(*semi_angle);
            Some(((radial - r) * t + l.z()) / (1.0 + t * t))
        }
        Surface::Sphere { .. } => Some(libm::atan2(l.z(), radial)),
        Surface::Torus { major, .. } => Some(libm::atan2(l.z(), radial - major)),
        _ => None,
    }
}

fn exact(s: &Surface, c: &Curve3, range: Interval) -> Option<Curve2> {
    match s {
        Surface::Plane { frame } => on_plane(frame, c),
        Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } => {
            generator(s, frame, c, range).or_else(|| coaxial(s, frame, c, range))
        }
        Surface::Sphere { frame, .. } => {
            coaxial(s, frame, c, range).or_else(|| sphere_meridian(frame, c, range))
        }
        Surface::Torus { frame, major, .. } => {
            coaxial(s, frame, c, range).or_else(|| torus_meridian(frame, *major, c))
        }
        _ => None,
    }
}

/// Orthogonal projection onto a plane is affine, so every curve kind except
/// offsets has an exact image of the same kind.
fn on_plane(frame: &Frame, c: &Curve3) -> Option<Curve2> {
    let (o, x, y) = (frame.origin, frame.x.get(), frame.y().get());
    let pt = |p: Point3| Point2::new((p - o).dot(x), (p - o).dot(y));
    let v2 = |v: Vec3| Vec2::new(v.dot(x), v.dot(y));
    match c {
        Curve3::Line { origin, dir } => Some(Curve2::Line {
            origin: pt(*origin),
            dir: v2(dir.get()),
        }),
        Curve3::Circle { frame: f, radius } => Some(Curve2::Ellipse {
            centre: pt(f.origin),
            a: v2(f.x.get() * *radius),
            b: v2(f.y().get() * *radius),
        }),
        Curve3::Ellipse { frame: f, a, b } => Some(Curve2::Ellipse {
            centre: pt(f.origin),
            a: v2(f.x.get() * *a),
            b: v2(f.y().get() * *b),
        }),
        Curve3::BSpline(bs) => {
            let poles = bs.poles().iter().map(|&p| pt(p)).collect();
            let w = bs.weights().map(<[f64]>::to_vec);
            BSplineCurve2::new(bs.degree(), bs.knots().to_vec(), poles, w)
                .ok()
                .map(|b| Curve2::BSpline(Arc::new(b)))
        }
        Curve3::Offset { .. } => None,
    }
}

/// A line in a plane through the axis of a cylinder or cone, on one side
/// of the axis: `u` is constant and `v` linear in `t`.
fn generator(s: &Surface, frame: &Frame, c: &Curve3, range: Interval) -> Option<Curve2> {
    let Curve3::Line { dir, .. } = c else {
        return None;
    };
    let z = frame.z.get();
    let (p0, p1) = (c.eval(range.lo), c.eval(range.hi));
    let radial = |p: Point3| {
        let w = p - frame.origin;
        w - z * w.dot(z)
    };
    let (r0, r1) = (radial(p0), radial(p1));
    let rr = if r0.length() >= r1.length() { r0 } else { r1 };
    let e = rr.normalized()?.get();
    let tiny = 1e-12 * (1.0 + rr.length());
    if dir.get().dot(z.cross(e)).abs() > ANG || r0.dot(e) < -tiny || r1.dot(e) < -tiny {
        return None;
    }
    let u = angle_of(frame, e);
    let a = Point2::new(u, analytic_v(s, frame, p0)?);
    let b = Point2::new(u, analytic_v(s, frame, p1)?);
    line_through(range.lo, a, range.hi, b)
}

/// A circle about the surface's axis: `u` follows `t`, `v` is constant.
fn coaxial(s: &Surface, frame: &Frame, c: &Curve3, range: Interval) -> Option<Curve2> {
    let Curve3::Circle { frame: cf, radius } = c else {
        return None;
    };
    let z = frame.z.get();
    let n = cf.z.get();
    if n.cross(z).length() > ANG {
        return None;
    }
    let w = cf.origin - frame.origin;
    if (w - z * w.dot(z)).length() > 1e-9 * (1.0 + radius) {
        return None;
    }
    let sign = if n.dot(z) > 0.0 { 1.0 } else { -1.0 };
    let u0 = angle_of(frame, cf.x.get());
    let v = analytic_v(s, frame, c.eval(range.lo))?;
    Some(Curve2::Line {
        origin: Point2::new(u0, v),
        dir: Vec2::new(sign, 0.0),
    })
}

/// `(φ0, σ)` with `C(t) − centre ∝ cos(σt + φ0)·e + sin(σt + φ0)·z` for a
/// circle in the plane spanned by `e` and `z`.
fn meridian_angle(cf: &Frame, e: Vec3, z: Vec3) -> (f64, f64) {
    let (x, y) = (cf.x.get(), cf.y().get());
    let phi0 = libm::atan2(x.dot(z), x.dot(e));
    let (sn, cs) = libm::sincos(phi0);
    let sigma = if y.dot(e * -sn + z * cs) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    (phi0, sigma)
}

/// A great circle through the poles, if the range stays on one side of the
/// axis: `u` is constant and `v` (latitude) is `±t` plus a constant.
fn sphere_meridian(frame: &Frame, c: &Curve3, range: Interval) -> Option<Curve2> {
    let Curve3::Circle { frame: cf, radius } = c else {
        return None;
    };
    if cf.origin.distance(frame.origin) > 1e-9 * (1.0 + radius) {
        return None;
    }
    let z = frame.z.get();
    let n = cf.z.get();
    if n.dot(z).abs() > ANG {
        return None;
    }
    let e = z.cross(n).normalized()?.get();
    let (phi0, sigma) = meridian_angle(cf, e, z);
    // φ(t) = σt + c0, with φ at mid-range in (−π, π].
    let mid = sigma * range.at(0.5) + phi0;
    let c0 = phi0 - TAU * libm::round(mid / TAU);
    let mid = sigma * range.at(0.5) + c0;
    let (lo, hi) = {
        let (a, b) = (sigma * range.lo + c0, sigma * range.hi + c0);
        (a.min(b), b.max(a))
    };
    let eps = 1e-9;
    if libm::cos(mid) >= 0.0 {
        // Near side: v = φ.
        if lo < -FRAC_PI_2 - eps || hi > FRAC_PI_2 + eps {
            return None;
        }
        Some(Curve2::Line {
            origin: Point2::new(angle_of(frame, e), c0),
            dir: Vec2::new(0.0, sigma),
        })
    } else {
        // Far side: u = angle(−e), v = ±π − φ.
        let k = if mid > 0.0 { PI } else { -PI };
        let (vlo, vhi) = (k - hi, k - lo);
        if vlo < -FRAC_PI_2 - eps || vhi > FRAC_PI_2 + eps {
            return None;
        }
        Some(Curve2::Line {
            origin: Point2::new(angle_of(frame, e * -1.0), k - c0),
            dir: Vec2::new(0.0, -sigma),
        })
    }
}

/// A circle of the tube about the core circle: `u` constant, `v = ±t + c`.
fn torus_meridian(frame: &Frame, major: f64, c: &Curve3) -> Option<Curve2> {
    let Curve3::Circle { frame: cf, radius } = c else {
        return None;
    };
    let z = frame.z.get();
    let w = cf.origin - frame.origin;
    let scale = 1e-9 * (1.0 + major + radius);
    let radial = w - z * w.dot(z);
    if w.dot(z).abs() > scale || (radial.length() - major).abs() > scale {
        return None;
    }
    let e = radial.normalized()?.get();
    let n = cf.z.get();
    if n.dot(z).abs() > ANG || n.dot(e).abs() > ANG {
        return None;
    }
    let (phi0, sigma) = meridian_angle(cf, e, z);
    Some(Curve2::Line {
        origin: Point2::new(angle_of(frame, e), phi0),
        dir: Vec2::new(0.0, sigma),
    })
}

// --- sampled case ------------------------------------------------------------

/// Closest-point projection onto one surface, with unwrapping.
struct Projector<'a> {
    s: &'a Surface,
    sbox: UvBox,
    /// Shift periods: true periods, or the domain length of a B-spline that
    /// is closed without being periodic.
    per: (Option<f64>, Option<f64>),
    /// Domains of directions that are closed but not periodic: shifts must
    /// keep parameters inside them.
    bounded: (Option<Interval>, Option<Interval>),
    /// Parameter spans used to judge degeneracy.
    span: (f64, f64),
    tol: Tol,
    tol_model: f64,
}

/// Search box for the closest-point query on a surface.
fn search_box(s: &Surface) -> Option<UvBox> {
    let big = Interval::new(-BIG, BIG)?;
    let turn = Interval::new(0.0, TAU)?;
    Some(match s {
        Surface::BSpline(b) => b.domain(),
        Surface::Extrusion { profile, .. } => {
            UvBox::new(profile.natural_range().unwrap_or(big), big)
        }
        Surface::Revolution { profile, .. } => {
            UvBox::new(turn, profile.natural_range().unwrap_or(big))
        }
        Surface::Offset { basis, .. } => search_box(basis)?,
        Surface::Sphere { .. } => UvBox::new(turn, Interval::new(-FRAC_PI_2, FRAC_PI_2)?),
        Surface::Cylinder { .. } | Surface::Cone { .. } => UvBox::new(turn, big),
        Surface::Torus { .. } => UvBox::new(turn, turn),
        Surface::Plane { .. } => UvBox::new(big, big),
    })
}

/// Shift periods of a surface (see [`Projector::per`]).
pub fn shift_periods(s: &Surface, tol_model: f64) -> (Option<f64>, Option<f64>) {
    let (mut pu, mut pv) = s.periodicity();
    if let Surface::BSpline(b) = s {
        let d = b.domain();
        let closed = |along_u: bool| {
            (0..=4).all(|i| {
                let f = i as f64 / 4.0;
                let (a, z) = if along_u {
                    let v = d.v.at(f);
                    (s.eval(d.u.lo, v), s.eval(d.u.hi, v))
                } else {
                    let u = d.u.at(f);
                    (s.eval(u, d.v.lo), s.eval(u, d.v.hi))
                };
                a.distance(z) <= tol_model
            })
        };
        if pu.is_none() && closed(true) {
            pu = Some(d.u.length());
        }
        if pv.is_none() && closed(false) {
            pv = Some(d.v.length());
        }
    }
    (pu, pv)
}

fn unwrap(x: f64, near: f64, period: Option<f64>) -> f64 {
    match period {
        Some(p) if p > 0.0 => x + p * libm::round((near - x) / p),
        _ => x,
    }
}

impl<'a> Projector<'a> {
    fn new(s: &'a Surface, tol_model: f64) -> Option<Self> {
        let sbox = search_box(s)?;
        let per = shift_periods(s, tol_model);
        let span = (
            per.0.unwrap_or(sbox.u.length().min(1e3)),
            per.1.unwrap_or(sbox.v.length().min(1e3)),
        );
        let (pu, pv) = s.periodicity();
        let bounded = (
            (per.0.is_some() && pu.is_none()).then_some(sbox.u),
            (per.1.is_some() && pv.is_none()).then_some(sbox.v),
        );
        Some(Projector {
            s,
            sbox,
            per,
            bounded,
            span,
            tol: Tol::for_machining(tol_model, (tol_model * 10.0).min(0.01)),
            tol_model,
        })
    }

    /// Closest `(u, v)` to `p`, within half a period of `near` in periodic
    /// directions.
    fn point(&self, p: Point3, near: Option<(f64, f64)>) -> (f64, f64) {
        let window = |c: f64| Interval {
            lo: c - PI,
            hi: c + PI,
        };
        let near = near.filter(|q| q.0.is_finite() && q.1.is_finite());
        let (u, v) = match self.s {
            Surface::Plane { .. }
            | Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. } => {
                // Closed form; the u (and torus v) window does the unwrapping.
                let (nu, nv) = near.unwrap_or((0.0, 0.0));
                let mut b = self.sbox;
                if self.per.0.is_some() {
                    b.u = window(nu);
                }
                if matches!(self.s, Surface::Torus { .. }) {
                    b.v = window(nv);
                }
                let r = self.s.closest(p, b, &self.tol);
                (r.u, r.v)
            }
            _ => {
                let r = self.s.closest(p, self.sbox, &self.tol);
                (r.u, r.v)
            }
        };
        match near {
            Some((nu, nv)) => (self.unwrap_axis(0, u, nu), self.unwrap_axis(1, v, nv)),
            None => (u, v),
        }
    }

    /// `x` moved by whole periods to within half a period of `near`, unless
    /// that leaves the domain of a closed, non-periodic direction.
    fn unwrap_axis(&self, axis: usize, x: f64, near: f64) -> f64 {
        let y = unwrap(x, near, self.per.get(axis));
        if self.inside(axis, y) { y } else { x }
    }

    /// Whether `x` is a valid parameter along `axis` (always, unless the
    /// direction is closed but not periodic).
    fn inside(&self, axis: usize, x: f64) -> bool {
        let b = if axis == 0 {
            self.bounded.0
        } else {
            self.bounded.1
        };
        b.is_none_or(|iv| {
            let slack = 1e-9 * iv.length();
            x >= iv.lo - slack && x <= iv.hi + slack
        })
    }

    /// Whether the surface collapses along u, v at `(u, v)`: moving across the
    /// whole span moves the point less than `tol_model`.
    fn degenerate(&self, u: f64, v: f64) -> (bool, bool) {
        let d = self.s.derivs(u, v);
        (
            d.su.length() * self.span.0 < self.tol_model,
            d.sv.length() * self.span.1 < self.tol_model,
        )
    }

    /// `(u′, v′)` with `Su·u′ + Sv·v′` closest to `dc`.
    fn tangent(&self, u: f64, v: f64, dc: Vec3) -> Option<(f64, f64)> {
        let d = self.s.derivs(u, v);
        let (a, b, c) = (d.su.dot(d.su), d.su.dot(d.sv), d.sv.dot(d.sv));
        let det = a * c - b * b;
        if det.is_nan() || det <= 1e-12 * a * c {
            return None;
        }
        let (ru, rv) = (d.su.dot(dc), d.sv.dot(dc));
        Some(((c * ru - b * rv) / det, (a * rv - b * ru) / det))
    }
}

#[derive(Debug, Clone, Copy)]
struct Sample {
    t: f64,
    q: (f64, f64),
    d: (f64, f64),
}

/// The cubic Hermite piece between two samples, at `t`.
fn hermite(a: &Sample, b: &Sample, t: f64) -> (f64, f64) {
    let h = b.t - a.t;
    let s = (t - a.t) / h;
    let (s2, s3) = (s * s, s * s * s);
    let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
    let h10 = s3 - 2.0 * s2 + s;
    let h01 = -2.0 * s3 + 3.0 * s2;
    let h11 = s3 - s2;
    let f = |i: usize| {
        let (pa, pb, da, db) = if i == 0 {
            (a.q.0, b.q.0, a.d.0, b.d.0)
        } else {
            (a.q.1, b.q.1, a.d.1, b.d.1)
        };
        h00 * pa + h10 * h * da + h01 * pb + h11 * h * db
    };
    (f(0), f(1))
}

fn sampled(s: &Surface, c: &Curve3, range: Interval, tol_model: f64) -> Option<Curve2> {
    if range.length().is_nan() || range.length() <= 0.0 {
        return None;
    }
    let pj = Projector::new(s, tol_model)?;
    let mut ts: Vec<f64> = Vec::new();
    for w in c.breaks(range).windows(2) {
        for k in 0..4 {
            ts.push(w[0] + (w[1] - w[0]) * k as f64 / 4.0);
        }
    }
    ts.push(range.hi);
    if ts.len() < 9 {
        ts = (0..=8).map(|i| range.at(i as f64 / 8.0)).collect();
    }
    // A range a few ulps long gives repeated parameters.
    ts.dedup_by(|b, a| *b <= *a);
    if ts.len() < 2 {
        return None;
    }
    // Project, unwrapping against the last sample that has both coordinates.
    let mut qs = Vec::with_capacity(ts.len());
    let mut degs = Vec::with_capacity(ts.len());
    let mut last: Option<(f64, f64)> = None;
    for &t in &ts {
        let (u, v) = pj.point(c.eval(t), last);
        let dg = pj.degenerate(u, v);
        if !dg.0 && !dg.1 {
            last = Some((u, v));
        }
        qs.push((u, v));
        degs.push(dg);
    }
    fill_degenerate(&ts, &mut qs, &degs);
    let mut samples: Vec<Sample> = Vec::with_capacity(ts.len());
    for i in 0..ts.len() {
        let (u, v) = qs[i];
        let d = if degs[i].0 || degs[i].1 {
            None
        } else {
            pj.tangent(u, v, c.derivs(ts[i], 1)[1])
        };
        let d = d.unwrap_or_else(|| {
            let (a, b) = (i.saturating_sub(1), (i + 1).min(ts.len() - 1));
            let h = ts[b] - ts[a];
            ((qs[b].0 - qs[a].0) / h, (qs[b].1 - qs[a].1) / h)
        });
        samples.push(Sample {
            t: ts[i],
            q: (u, v),
            d,
        });
    }
    // Split pieces until each midpoint maps close to its projection.
    let tol_fit = tol_model * 0.5;
    let mut budget = MAX_SAMPLES;
    let mut out = vec![samples[0]];
    for w in samples.windows(2) {
        refine(&pj, c, w[0], w[1], 0, tol_fit, &mut budget, &mut out);
    }
    hermite_spline(&out)
}

/// Replaces coordinates that are meaningless at a degenerate point by
/// linear interpolation in `t` between the nearest samples where they mean
/// something (or a copy of the one neighbour there is).
fn fill_degenerate(ts: &[f64], qs: &mut [(f64, f64)], degs: &[(bool, bool)]) {
    for axis in 0..2 {
        let bad = |i: usize| if axis == 0 { degs[i].0 } else { degs[i].1 };
        let get = |q: (f64, f64)| if axis == 0 { q.0 } else { q.1 };
        let n = qs.len();
        for i in 0..n {
            if !bad(i) {
                continue;
            }
            let before = (0..i).rev().find(|&j| !bad(j));
            let after = (i + 1..n).find(|&j| !bad(j));
            let x = match (before, after) {
                (Some(a), Some(b)) => {
                    let f = (ts[i] - ts[a]) / (ts[b] - ts[a]);
                    get(qs[a]) + (get(qs[b]) - get(qs[a])) * f
                }
                (Some(a), None) => get(qs[a]),
                (None, Some(b)) => get(qs[b]),
                (None, None) => continue,
            };
            if axis == 0 {
                qs[i].0 = x;
            } else {
                qs[i].1 = x;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn refine(
    pj: &Projector,
    c: &Curve3,
    a: Sample,
    b: Sample,
    depth: u32,
    tol_fit: f64,
    budget: &mut usize,
    out: &mut Vec<Sample>,
) {
    let tm = 0.5 * (a.t + b.t);
    if tm <= a.t || tm >= b.t || depth >= MAX_DEPTH || *budget == 0 {
        out.push(b);
        return;
    }
    let pred = hermite(&a, &b, tm);
    let (mut u, mut v) = pj.point(c.eval(tm), Some(pred));
    let dg = pj.degenerate(u, v);
    if dg.0 {
        u = pred.0;
    }
    if dg.1 {
        v = pred.1;
    }
    let err = pj.s.eval(pred.0, pred.1).distance(pj.s.eval(u, v));
    if err.is_nan() || err <= tol_fit {
        out.push(b);
        return;
    }
    *budget -= 1;
    let d = if dg.0 || dg.1 {
        None
    } else {
        pj.tangent(u, v, c.derivs(tm, 1)[1])
    };
    let d = d.unwrap_or_else(|| {
        let h = b.t - a.t;
        ((b.q.0 - a.q.0) / h, (b.q.1 - a.q.1) / h)
    });
    let m = Sample {
        t: tm,
        q: (u, v),
        d,
    };
    refine(pj, c, a, m, depth + 1, tol_fit, budget, out);
    refine(pj, c, m, b, depth + 1, tol_fit, budget, out);
}

/// The C¹ cubic B-spline through Hermite samples: double interior knots,
/// Bézier inner poles at a third of each piece.
fn hermite_spline(s: &[Sample]) -> Option<Curve2> {
    let n = s.len().checked_sub(1).filter(|&n| n > 0)?;
    let pt = |q: (f64, f64)| Point2::new(q.0, q.1);
    let mut knots = vec![s[0].t; 4];
    let mut poles = vec![pt(s[0].q)];
    for i in 0..n {
        let (a, b) = (&s[i], &s[i + 1]);
        let h3 = (b.t - a.t) / 3.0;
        poles.push(Point2::new(a.q.0 + h3 * a.d.0, a.q.1 + h3 * a.d.1));
        poles.push(Point2::new(b.q.0 - h3 * b.d.0, b.q.1 - h3 * b.d.1));
        if i + 1 < n {
            knots.extend([b.t, b.t]);
        }
    }
    poles.push(pt(s[n].q));
    knots.extend([s[n].t; 4]);
    BSplineCurve2::new(3, knots, poles, None)
        .ok()
        .map(|b| Curve2::BSpline(Arc::new(b)))
}

// --- seams -------------------------------------------------------------------

/// Shifts the p-curves of one face by whole periods so that each starts
/// where the previous coedge of its loop ends. Coedges flagged in `fixed`
/// (p-curves from the file) are not moved and anchor their loops.
///
/// A joint at a point where the surface is degenerate in a direction (pole,
/// apex) does not fix the shift in that direction; the coedge is aligned to
/// its other neighbour instead. A coedge fixed by neither neighbour goes
/// within half a period of its loop's start. The first loop starts in the
/// surface's base period window; later loops are moved next to it.
pub fn align_face(b: &mut BrepBody, face: u32, fixed: &[bool]) {
    let f = &b.faces[face as usize];
    let s = &b.surfaces[f.surface as usize];
    let Some(pj) = Projector::new(s, b.tol_model) else {
        return;
    };
    let per = pj.per;
    if per.0.is_none() && per.1.is_none() {
        return;
    }
    let mut loops = f.loops.clone();
    loops.sort_by_key(|&l| !b.loops[l as usize].outer);
    let mut reference: Option<(f64, f64)> = None;
    for l in loops {
        let cs: Vec<u32> = b.loops[l as usize]
            .coedges
            .iter()
            .copied()
            .filter(|&c| b.pcurves.get(c as usize).is_some_and(Option::is_some))
            .collect();
        if cs.is_empty() {
            continue;
        }
        let ends: Vec<((f64, f64), (f64, f64))> = cs
            .iter()
            .map(|&c| {
                let ce = &b.coedges[c as usize];
                let r = b.edges[ce.edge as usize].range;
                let pc = b.pcurves[c as usize].as_ref().expect("filtered");
                let (p0, p1) = (pc.eval(r.lo), pc.eval(r.hi));
                let (a, z) = if ce.forward { (p0, p1) } else { (p1, p0) };
                ((a.x(), a.y()), (z.x(), z.y()))
            })
            .collect();
        let n = cs.len();
        let any_fixed = cs.iter().any(|&c| fixed.get(c as usize) == Some(&true));
        let mut shifts = [vec![None; n], vec![None; n]];
        for (i, &c) in cs.iter().enumerate() {
            if fixed.get(c as usize) == Some(&true) {
                shifts[0][i] = Some(0.0);
                shifts[1][i] = Some(0.0);
            }
        }
        if !any_fixed {
            shifts[0][0] = Some(0.0);
            shifts[1][0] = Some(0.0);
        }
        let coord = |q: (f64, f64), axis: usize| if axis == 0 { q.0 } else { q.1 };
        for (axis, sh) in shifts.iter_mut().enumerate() {
            let Some(p) = per.get(axis) else {
                sh.fill(Some(0.0));
                continue;
            };
            // Propagate across joints that are not degenerate along `axis`.
            for _ in 0..n {
                let mut changed = false;
                for i in 0..n {
                    let j = (i + 1) % n;
                    let joint = ends[i].1;
                    let dg = pj.degenerate(joint.0, joint.1);
                    if (axis == 0 && dg.0) || (axis == 1 && dg.1) {
                        continue;
                    }
                    let (e, st) = (coord(ends[i].1, axis), coord(ends[j].0, axis));
                    match (sh[i], sh[j]) {
                        (Some(a), None) => {
                            sh[j] = Some(p * libm::round((e + a - st) / p));
                            changed = true;
                        }
                        (None, Some(bb)) => {
                            sh[i] = Some(p * libm::round((st + bb - e) / p));
                            changed = true;
                        }
                        _ => {}
                    }
                }
                if !changed {
                    break;
                }
            }
            let start = coord(ends[0].0, axis) + sh[0].unwrap_or(0.0);
            for i in 0..n {
                if sh[i].is_none() {
                    let mid = 0.5 * (coord(ends[i].0, axis) + coord(ends[i].1, axis));
                    sh[i] = Some(p * libm::round((start - mid) / p));
                }
                // On a closed, non-periodic surface the p-curve must stay in
                // the domain: a seam coedge goes to whichever side it fits.
                let fits = |d: f64| {
                    pj.inside(axis, coord(ends[i].0, axis) + d)
                        && pj.inside(axis, coord(ends[i].1, axis) + d)
                };
                if let Some(d) = sh[i]
                    && !fits(d)
                {
                    sh[i] = [0.0, p, -p].into_iter().find(|&d| fits(d)).or(Some(0.0));
                }
            }
        }
        let shift = |i: usize| (shifts[0][i].unwrap_or(0.0), shifts[1][i].unwrap_or(0.0));
        // Whole-loop shift into the window (unless the file anchors it).
        let mut whole = (0.0, 0.0);
        if !any_fixed {
            let mean = |axis: usize| {
                cs.iter()
                    .enumerate()
                    .map(|(i, _)| coord(ends[i].0, axis) + coord(shift(i), axis))
                    .sum::<f64>()
                    / n as f64
            };
            for axis in 0..2 {
                let Some(p) = per.get(axis) else { continue };
                if !pj.inside(axis, f64::INFINITY) {
                    // Closed, non-periodic: the domain is the window.
                    continue;
                }
                let w = match reference {
                    None => {
                        let lo = if axis == 0 {
                            pj.sbox.u.lo
                        } else {
                            pj.sbox.v.lo
                        };
                        let st = coord(ends[0].0, axis) + coord(shift(0), axis);
                        p * libm::ceil((lo - st) / p - 1e-9)
                    }
                    Some(r) => p * libm::round((coord(r, axis) - mean(axis)) / p),
                };
                if axis == 0 {
                    whole.0 = w;
                } else {
                    whole.1 = w;
                }
            }
        }
        // The first loop sets the reference for the others.
        if reference.is_none() {
            let m = |axis: usize| {
                cs.iter()
                    .enumerate()
                    .map(|(i, _)| {
                        coord(ends[i].0, axis)
                            + coord(shift(i), axis)
                            + if axis == 0 { whole.0 } else { whole.1 }
                    })
                    .sum::<f64>()
                    / n as f64
            };
            reference = Some((m(0), m(1)));
        }
        for (i, &c) in cs.iter().enumerate() {
            let (du, dv) = shift(i);
            let d = Vec2::new(du + whole.0, dv + whole.1);
            if d.x() != 0.0 || d.y() != 0.0 {
                let pc = b.pcurves[c as usize].as_ref().expect("filtered");
                b.pcurves[c as usize] = Some(pc.translated(d));
            }
        }
    }
}

trait PerAxis {
    fn get(&self, axis: usize) -> Option<f64>;
}

impl PerAxis for (Option<f64>, Option<f64>) {
    fn get(&self, axis: usize) -> Option<f64> {
        if axis == 0 { self.0 } else { self.1 }
    }
}

#[cfg(test)]
mod tests;
