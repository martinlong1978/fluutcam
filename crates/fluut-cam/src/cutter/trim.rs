// SPDX-License-Identifier: MIT OR Apache-2.0

//! Face trims in parameter space (spike version of D03 §3.1).
//!
//! The corpus has no p-curves (S1), so each loop is projected onto the
//! surface: edge points are tessellated, projected (closest point for the
//! first, then Gauss–Newton along the loop), unwrapped across seams and
//! filled in at poles. Points are classified even–odd with a ray along one
//! parameter direction ("b"); the other ("a") may be periodic.

use fluut_geom::{Interval, Point3, Surface, Tol, UvBox};
use fluut_topo::BrepBody;
use std::f64::consts::{FRAC_PI_2, TAU};

#[derive(Debug, Clone, PartialEq)]
pub enum TrimError {
    /// Loops wrap around both parameter directions (a full torus).
    WrapsBothWays,
    /// An odd number of loops wrap around the surface, but it has no pole
    /// on that side to close them.
    NoPole,
    /// The face has no loops and the surface is unbounded.
    Unbounded,
}

/// A face's trim loops as closed polylines in parameter space.
#[derive(Debug, Clone)]
pub struct Trim {
    /// Polylines in (a, b) coordinates; the last point closes the loop (one
    /// period away from the first if the loop wraps).
    loops: Vec<Vec<[f64; 2]>>,
    /// `(a, b) = (v, u)` instead of `(u, v)`.
    swap: bool,
    period_a: Option<f64>,
    period_b: Option<f64>,
    /// Lowest `b` on the loops, the start of the window `b` is wrapped into.
    b_lo: f64,
    /// Points above every loop (towards +b) are inside.
    top_inside: bool,
    /// No loops: the whole surface.
    all: bool,
    uv: UvBox,
}

/// Whether a parameter is free at a singular point (a pole or apex).
#[derive(Copy, Clone, PartialEq)]
enum Free {
    None,
    U,
    V,
}

/// Parameter box for closest-point projection.
fn search_box(s: &Surface, pts: &[Point3]) -> UvBox {
    let iv = |lo, hi| Interval { lo, hi };
    let big = iv(-1e7, 1e7);
    let turn = iv(0.0, TAU);
    match s {
        Surface::Plane { .. } => UvBox::new(big, big),
        Surface::Cylinder { .. } | Surface::Cone { .. } => UvBox::new(turn, big),
        Surface::Sphere { .. } => UvBox::new(turn, iv(-FRAC_PI_2, FRAC_PI_2)),
        Surface::Torus { .. } => UvBox::new(turn, turn),
        Surface::BSpline(b) => b.domain(),
        Surface::Revolution { profile, .. } => {
            UvBox::new(turn, profile.natural_range().unwrap_or(big))
        }
        Surface::Extrusion { profile, .. } => {
            let r = profile.natural_range();
            let p0 = profile.eval(r.map_or(0.0, |r| r.lo));
            let l = pts.iter().map(|p| p.distance(p0)).fold(1.0, f64::max) * 2.0;
            UvBox::new(r.unwrap_or(big), iv(-l, l))
        }
        Surface::Offset { basis, .. } => search_box(basis, pts),
    }
}

/// Gauss–Newton projection of `p` from `start`; `None` if it does not
/// converge to within `tol` of the closest point condition.
fn project_from(s: &Surface, p: Point3, start: (f64, f64), tol: f64) -> Option<(f64, f64)> {
    let (mut u, mut v) = start;
    for _ in 0..12 {
        let d = s.derivs(u, v);
        let r = p - d.p;
        let (a, b, c) = (d.su.dot(d.su), d.su.dot(d.sv), d.sv.dot(d.sv));
        let det = a * c - b * b;
        if det.is_nan() || det <= 1e-24 * (a * c).max(1e-300) {
            return None;
        }
        let (gu, gv) = (d.su.dot(r), d.sv.dot(r));
        let du = (c * gu - b * gv) / det;
        let dv = (a * gv - b * gu) / det;
        u += du;
        v += dv;
        if (d.su * du + d.sv * dv).length() < tol {
            let q = s.eval(u, v);
            let n = d.su.cross(d.sv);
            // At the closest point the residual is along the normal.
            let res = p - q;
            let tangential = res.length_squared() - sq(res.dot(n)) / n.length_squared();
            return (tangential.max(0.0).sqrt() < 10.0 * tol + 1e-9).then_some((u, v));
        }
    }
    None
}

fn sq(x: f64) -> f64 {
    x * x
}

/// Which parameter is free at `(u, v)`: one whose derivative vanishes.
fn free_param(s: &Surface, u: f64, v: f64) -> Free {
    let d = s.derivs(u, v);
    let (lu, lv) = (d.su.length(), d.sv.length());
    let scale = lu.max(lv).max(1e-300);
    if lu < 1e-7 * scale {
        Free::U
    } else if lv < 1e-7 * scale {
        Free::V
    } else {
        Free::None
    }
}

/// Points of a loop in travel order, without repeated joints.
fn loop_points(body: &BrepBody, l: u32, chord: f64) -> Vec<Point3> {
    let mut out: Vec<Point3> = Vec::new();
    for &c in &body.loops[l as usize].coedges {
        let ce = &body.coedges[c as usize];
        let mut pts = body.edge_points(ce.edge, chord);
        if !ce.forward {
            pts.reverse();
        }
        for p in pts {
            if out.last().is_none_or(|q| q.distance(p) > 1e-9) {
                out.push(p);
            }
        }
    }
    if out.len() > 1 && out[0].distance(out[out.len() - 1]) <= 1e-9 {
        out.pop();
    }
    out
}

/// Unwraps coordinate `k` of `pts` (skipping points where it is free) so
/// that neighbours differ by less than half a period, and returns the
/// loop's drift (0 or a multiple of the period). A pole passage (free point
/// or a jump of more than a quarter period) is a free joint; if the loop
/// does not close, the points after the last free joint are shifted to
/// close it.
fn unwrap(pts: &mut [[f64; 2]], free: &[bool], k: usize, period: f64) -> f64 {
    let n = pts.len();
    let mut last: Option<f64> = None;
    let mut joint: Option<usize> = None;
    let mut after_free = false;
    for i in 0..n {
        if free[i] {
            after_free = true;
            continue;
        }
        if let Some(l) = last {
            let x = pts[i][k];
            let y = x + period * ((l - x) / period).round();
            pts[i][k] = y;
            if after_free || (y - l).abs() > period / 4.0 {
                joint = Some(i);
            }
        }
        after_free = false;
        last = Some(pts[i][k]);
    }
    let first = (0..n).find(|&i| !free[i]);
    let (Some(first), Some(l)) = (first, last) else {
        return 0.0;
    };
    if free[0] || free[n - 1] {
        // The closing joint passes a pole: it is free, so the loop closes.
        return 0.0;
    }
    let x = pts[first][k];
    let drift = period * ((l - x) / period).round();
    if drift != 0.0
        && let Some(j) = joint
    {
        for p in &mut pts[j..] {
            p[k] -= drift;
        }
        return 0.0;
    }
    drift
}

/// The parameter value of a pole of `s` at the high (`top`) or low end of
/// coordinate `k` (0 = u, 1 = v), if it has one there.
fn pole(s: &Surface, k: usize, top: bool) -> Option<f64> {
    match (s, k) {
        (Surface::Sphere { .. }, 1) => Some(if top { FRAC_PI_2 } else { -FRAC_PI_2 }),
        (Surface::Cone { r, semi_angle, .. }, 1) => {
            let apex = -r / libm::tan(*semi_angle);
            // The apex lies below v = 0 for a positive half-angle.
            ((apex > 0.0) == top).then_some(apex)
        }
        (Surface::BSpline(b), _) => {
            let d = b.domain();
            let iv = if k == 0 { d.u } else { d.v };
            Some(if top { iv.hi } else { iv.lo })
        }
        (Surface::Revolution { profile, .. }, 1) => {
            let r = profile.natural_range()?;
            Some(if top { r.hi } else { r.lo })
        }
        (Surface::Offset { basis, .. }, _) => pole(basis, k, top),
        _ => None,
    }
}

impl Trim {
    /// The trim of face `f`, from edge points about `chord` apart.
    pub fn build(body: &BrepBody, f: u32, chord: f64) -> Result<Trim, TrimError> {
        let face = &body.faces[f as usize];
        let s = &body.surfaces[face.surface as usize];
        let (pu, pv) = s.periodicity();
        let gtol = Tol::for_machining(1e-7, 1e-4);
        let ptol = 1e-3 * chord.max(1e-6);
        let mut loops: Vec<Vec<[f64; 2]>> = Vec::new();
        let mut drifts: Vec<[f64; 2]> = Vec::new();
        for &l in &face.loops {
            let pts3 = loop_points(body, l, chord);
            if pts3.len() < 2 {
                // A vertex loop (cone apex): no area to bound.
                continue;
            }
            let sbox = search_box(s, &pts3);
            let mut uv: Vec<[f64; 2]> = Vec::with_capacity(pts3.len());
            let mut free: Vec<Free> = Vec::with_capacity(pts3.len());
            let mut prev: Option<(f64, f64)> = None;
            for &p in &pts3 {
                let r = prev
                    .and_then(|st| project_from(s, p, st, ptol))
                    .unwrap_or_else(|| {
                        let c = s.closest(p, sbox, &gtol);
                        (c.u, c.v)
                    });
                let fp = free_param(s, r.0, r.1);
                free.push(fp);
                uv.push([r.0, r.1]);
                // Do not seed the next point from a singular one.
                prev = (fp == Free::None).then_some(r);
            }
            let mut drift = [0.0, 0.0];
            for (k, per) in [(0, pu), (1, pv)] {
                if let Some(per) = per {
                    let fk: Vec<bool> = free
                        .iter()
                        .map(|&x| x == if k == 0 { Free::U } else { Free::V })
                        .collect();
                    drift[k] = unwrap(&mut uv, &fk, k, per);
                }
            }
            // Fill free parameters at singular points from their neighbours.
            let mut out: Vec<[f64; 2]> = Vec::with_capacity(uv.len() + 4);
            let n = uv.len();
            for i in 0..n {
                let k = match free[i] {
                    Free::None => {
                        out.push(uv[i]);
                        continue;
                    }
                    Free::U => 0,
                    Free::V => 1,
                };
                let nb = |step: isize| {
                    (1..n).find_map(|j| {
                        let m = (i as isize + step * j as isize).rem_euclid(n as isize) as usize;
                        (free[m] == Free::None).then_some(uv[m][k])
                    })
                };
                if let (Some(a), Some(b)) = (nb(-1), nb(1)) {
                    let mut p = uv[i];
                    p[k] = a;
                    out.push(p);
                    p[k] = b;
                    out.push(p);
                }
            }
            // Close explicitly: a wrapping loop ends one period from its start.
            if let Some(&first) = out.first() {
                out.push([first[0] + drift[0], first[1] + drift[1]]);
            }
            loops.push(out);
            drifts.push(drift);
        }
        let wraps = |k: usize| drifts.iter().filter(|d| d[k] != 0.0).count();
        let (wu, wv) = (wraps(0), wraps(1));
        if wu > 0 && wv > 0 {
            return Err(TrimError::WrapsBothWays);
        }
        let swap = wv > 0;
        let (ka, kb) = if swap { (1, 0) } else { (0, 1) };
        let (period_a, period_b) = if swap { (pv, pu) } else { (pu, pv) };
        if loops.is_empty() {
            let (lo_a, hi_a) = (pole(s, 0, false), pole(s, 0, true));
            let (lo_b, hi_b) = (pole(s, 1, false), pole(s, 1, true));
            let iu = match (pu, lo_a, hi_a) {
                (Some(p), _, _) => Interval { lo: 0.0, hi: p },
                (None, Some(lo), Some(hi)) => Interval { lo, hi },
                _ => return Err(TrimError::Unbounded),
            };
            let iv = match (pv, lo_b, hi_b) {
                (Some(p), _, _) => Interval { lo: 0.0, hi: p },
                (None, Some(lo), Some(hi)) => Interval { lo, hi },
                _ => return Err(TrimError::Unbounded),
            };
            return Ok(Trim {
                loops,
                swap: false,
                period_a: pu,
                period_b: pv,
                b_lo: 0.0,
                top_inside: true,
                all: true,
                uv: UvBox::new(iu, iv),
            });
        }
        let mut ab: Vec<Vec<[f64; 2]>> = loops
            .into_iter()
            .map(|l| l.into_iter().map(|p| [p[ka], p[kb]]).collect())
            .collect();
        // Bring the loops into one b window (they may have been projected
        // a period apart).
        if let Some(pb) = period_b {
            let ref_b = ab[0][0][1];
            for l in &mut ab {
                let shift = pb * ((ref_b - l[0][1]) / pb).round();
                for p in l.iter_mut() {
                    p[1] += shift;
                }
            }
        }
        if let Some(pa) = period_a {
            let ref_a = ab[0][0][0];
            for (l, d) in ab.iter_mut().zip(&drifts) {
                if d[ka] == 0.0 {
                    let shift = pa * ((ref_a - l[0][0]) / pa).round();
                    for p in l.iter_mut() {
                        p[0] += shift;
                    }
                }
            }
        }
        // Orientation of the topmost wrapping loop decides the region above.
        let sense = if face.same_sense { 1.0 } else { -1.0 } * if swap { -1.0 } else { 1.0 };
        let mut top: Option<(f64, f64)> = None;
        for (l, d) in ab.iter().zip(&drifts) {
            if d[ka] != 0.0 {
                let mean = l.iter().map(|p| p[1]).sum::<f64>() / l.len() as f64;
                if top.is_none_or(|t| mean > t.0) {
                    top = Some((mean, d[ka].signum() * sense));
                }
            }
        }
        let top_inside = top.is_some_and(|t| t.1 > 0.0);
        let wrapping = wu.max(wv);
        let (mut a_lo, mut a_hi, mut b_lo, mut b_hi) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for p in ab.iter().flatten() {
            a_lo = a_lo.min(p[0]);
            a_hi = a_hi.max(p[0]);
            b_lo = b_lo.min(p[1]);
            b_hi = b_hi.max(p[1]);
        }
        let (mut box_b_lo, mut box_b_hi) = (b_lo, b_hi);
        if top_inside {
            box_b_hi = pole(s, kb, true).ok_or(TrimError::NoPole)?;
        } else if wrapping % 2 == 1 {
            box_b_lo = pole(s, kb, false).ok_or(TrimError::NoPole)?;
        }
        let ia = Interval { lo: a_lo, hi: a_hi };
        let ib = Interval {
            lo: box_b_lo.min(b_lo),
            hi: box_b_hi.max(b_hi),
        };
        let uv = if swap {
            UvBox::new(ib, ia)
        } else {
            UvBox::new(ia, ib)
        };
        Ok(Trim {
            loops: ab,
            swap,
            period_a,
            period_b,
            b_lo,
            top_inside,
            all: false,
            uv,
        })
    }

    /// Parameter box containing the face (`u` may span a whole period).
    pub fn uv_box(&self) -> UvBox {
        self.uv
    }

    /// Whether `(u, v)` (any period) is inside the trimmed face. Points
    /// within about the projection chord of the boundary may go either way.
    pub fn contains(&self, u: f64, v: f64) -> bool {
        if self.all {
            return true;
        }
        let (a, mut b) = if self.swap { (v, u) } else { (u, v) };
        if let Some(pb) = self.period_b {
            b = self.b_lo + (b - self.b_lo).rem_euclid(pb);
        }
        let mut count = usize::from(self.top_inside);
        for l in &self.loops {
            for w in l.windows(2) {
                let (p0, p1) = (w[0], w[1]);
                let (lo, hi) = if p0[0] <= p1[0] {
                    (p0[0], p1[0])
                } else {
                    (p1[0], p0[0])
                };
                let mut cross = |x: f64| {
                    // Half-open in a, so a shared vertex counts once.
                    if (p0[0] <= x && x < p1[0]) || (p1[0] <= x && x < p0[0]) {
                        let t = (x - p0[0]) / (p1[0] - p0[0]);
                        if p0[1] + t * (p1[1] - p0[1]) > b {
                            count += 1;
                        }
                    }
                };
                match self.period_a {
                    None => cross(a),
                    Some(pa) => {
                        let k0 = ((lo - a) / pa).ceil() as i64;
                        let k1 = ((hi - a) / pa).floor() as i64;
                        for k in k0..=k1 {
                            cross(a + k as f64 * pa);
                        }
                    }
                }
            }
        }
        count % 2 == 1
    }
}

#[cfg(test)]
mod tests;
