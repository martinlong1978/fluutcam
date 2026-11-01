// SPDX-License-Identifier: MIT OR Apache-2.0

//! Trim loops in the parameter plane (design D03 §3.1).
//!
//! This is the first piece of the trim cache: each loop of a face becomes a
//! polyline in `(u, v)` from its coedges' p-curves, and [`outer_loop`]
//! finds the loop that contains all the others. The edge-bucket grid and
//! point classification of D03 §3.2 come later.
//!
//! Two things need care on closed surfaces:
//! - **Seams.** A hole may lie across the seam, one period away from the
//!   outer loop. Test points are moved by whole periods into the window of
//!   the loop they are tested against.
//! - **Wrapping loops.** A circle round a cylinder ends one period from
//!   where it starts: in the plane it is an open curve and contains
//!   nothing. A face with such loops is a band, and one of its wrapping
//!   loops is the outer one.
//!
//! On a surface that is closed but not periodic (a closed B-spline), a
//! p-curve must stay inside the domain, so one that crosses the seam jumps
//! back by the domain length. The polyline undoes such jumps: it moves the
//! rest of the loop by whole periods wherever two samples are more than
//! half a period apart.
//!
//! Where consecutive p-curves do not meet in `(u, v)` (at a pole or apex,
//! where the surface collapses), the polyline joins them with a straight
//! segment, which runs along the collapsed side.

use crate::brep::BrepBody;
use crate::pcurve;
use fluut_geom::{Aabb3, Point2, Point3, Surface};

/// Samples per coedge in a loop polyline.
pub const SAMPLES: usize = 16;

/// A loop as a polyline in its face's `(u, v)` plane.
#[derive(Debug, Clone, PartialEq)]
pub struct UvLoop {
    /// In loop order; the last point joins the first unless `wraps`. Empty
    /// for a loop with no edge curve (a `VERTEX_LOOP`).
    pub points: Vec<Point2>,
    /// The loop ends a whole period (or more) from where it starts, not
    /// across a pole: it goes round the surface.
    pub wraps: bool,
}

/// How [`outer_loop`] chose the outer loop. Indices are into `Face::loops`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outer {
    /// The one closed loop that contains all the others in `(u, v)`.
    Contains(usize),
    /// The face has loops that wrap round the surface; the widest of them
    /// in 3-D.
    Wraps(usize),
    /// A p-curve is missing, or no loop (or more than one) contains all
    /// the others.
    Undecided,
}

/// Polyline of loop `l` of face `face`, or `None` if a coedge with an edge
/// curve has no p-curve.
pub fn loop_polyline(b: &BrepBody, face: u32, l: u32) -> Option<UvLoop> {
    let s = &b.surfaces[b.faces[face as usize].surface as usize];
    let per = pcurve::shift_periods(s, b.tol_model);
    let mut pieces: Vec<Vec<Point2>> = Vec::new();
    // Shift applied to the rest of the loop, from jumps inside p-curves.
    let mut off = (0.0, 0.0);
    for &c in &b.loops[l as usize].coedges {
        let ce = &b.coedges[c as usize];
        let e = &b.edges[ce.edge as usize];
        if e.curve.is_none() {
            // A collapsed edge (cone apex): the gap is joined below.
            continue;
        }
        let pc = b.pcurves.get(c as usize)?.as_ref()?;
        let mut piece: Vec<Point2> = Vec::with_capacity(SAMPLES + 1);
        for i in 0..=SAMPLES {
            let f = i as f64 / SAMPLES as f64;
            let q = pc.eval(e.range.at(if ce.forward { f } else { 1.0 - f }));
            if let Some(prev) = piece.last() {
                let (du, dv) = (q.x() + off.0 - prev.x(), q.y() + off.1 - prev.y());
                off.0 -= whole_periods(du, per.0);
                off.1 -= whole_periods(dv, per.1);
            }
            piece.push(Point2::new(q.x() + off.0, q.y() + off.1));
        }
        pieces.push(piece);
    }
    let n = pieces.len();
    let mut wraps = false;
    for i in 0..n {
        let a = *pieces[i].last().expect("SAMPLES + 1 points");
        let z = pieces[(i + 1) % n][0];
        wraps |= wrap_gap(s, per, a, z, b.tol_model);
    }
    let mut points: Vec<Point2> = Vec::with_capacity(n * SAMPLES);
    for p in pieces.into_iter().flatten() {
        if points.last().is_none_or(|q| q.distance(p) > 1e-12) {
            points.push(p);
        }
    }
    Some(UvLoop { points, wraps })
}

/// `d` rounded to whole periods `p` if it exceeds half of one, else 0.
fn whole_periods(d: f64, p: Option<f64>) -> f64 {
    match p {
        Some(p) if p > 0.0 && d.abs() > 0.5 * p => p * libm::round(d / p),
        _ => 0.0,
    }
}

/// Whether the jump from `a` to `z` is a whole number of periods (not
/// zero) along a closed direction, and not a joint across a pole: there
/// the straight segment maps to a single 3-D point.
fn wrap_gap(s: &Surface, per: (Option<f64>, Option<f64>), a: Point2, z: Point2, tol: f64) -> bool {
    let whole = |g: f64, p: Option<f64>| {
        p.is_some_and(|p| {
            let k = libm::round(g / p);
            k != 0.0 && (g - k * p).abs() <= 1e-6 * p
        })
    };
    if !(whole(z.x() - a.x(), per.0) || whole(z.y() - a.y(), per.1)) {
        return false;
    }
    let at = |f: f64| {
        let (u, v) = (a.x() + f * (z.x() - a.x()), a.y() + f * (z.y() - a.y()));
        eval_in_domain(s, per, u, v)
    };
    let pa = at(0.0);
    [0.25, 0.5, 0.75]
        .into_iter()
        .any(|f| at(f).distance(pa) > tol)
}

/// `S(u, v)`, with `(u, v)` first moved by whole periods into the domain
/// of a closed B-spline (whose evaluation clamps outside it).
fn eval_in_domain(s: &Surface, per: (Option<f64>, Option<f64>), u: f64, v: f64) -> Point3 {
    let Surface::BSpline(bs) = s else {
        return s.eval(u, v);
    };
    let d = bs.domain();
    let into = |x: f64, lo: f64, p: Option<f64>| match p {
        Some(p) if p > 0.0 => lo + (x - lo).rem_euclid(p),
        _ => x,
    };
    s.eval(into(u, d.u.lo, per.0), into(v, d.v.lo, per.1))
}

/// Winding number of the closed polyline `poly` about `p`.
pub fn winding(poly: &[Point2], p: Point2) -> i32 {
    let n = poly.len();
    let mut w = 0;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let side = (b.x() - a.x()) * (p.y() - a.y()) - (p.x() - a.x()) * (b.y() - a.y());
        if a.y() <= p.y() {
            if b.y() > p.y() && side > 0.0 {
                w += 1;
            }
        } else if b.y() <= p.y() && side < 0.0 {
            w -= 1;
        }
    }
    w
}

/// Diagonal of the 3-D bounding box of the edges of `coedges`.
pub fn loop_width(b: &BrepBody, coedges: &[u32]) -> f64 {
    let mut bx = Aabb3::empty();
    for &c in coedges {
        for p in b.edge_points(b.coedges[c as usize].edge, 1.0) {
            bx = bx.with_point(p);
        }
    }
    if bx.is_empty() {
        0.0
    } else {
        bx.max.distance(bx.min)
    }
}

/// The outer loop of `face`, judged in its `(u, v)` plane from the
/// p-curves (which must have been computed and aligned, see
/// [`pcurve::compute`]). The loops' `outer` flags are not read.
///
/// A loop contains another when most of the other's polyline points are
/// inside it, each moved by whole periods into the loop's window; so
/// points on or very near the boundary do not decide. Loops without edge
/// curves are ignored.
pub fn outer_loop(b: &BrepBody, face: u32) -> Outer {
    let f = &b.faces[face as usize];
    let Some(polys) = f
        .loops
        .iter()
        .map(|&l| loop_polyline(b, face, l))
        .collect::<Option<Vec<_>>>()
    else {
        return Outer::Undecided;
    };
    let used: Vec<usize> = (0..polys.len())
        .filter(|&i| !polys[i].points.is_empty())
        .collect();
    let widest = |is: &[usize]| {
        is.iter().copied().max_by(|&i, &j| {
            let w = |k: usize| loop_width(b, &b.loops[f.loops[k] as usize].coedges);
            w(i).total_cmp(&w(j))
        })
    };
    let wrapping: Vec<usize> = used.iter().copied().filter(|&i| polys[i].wraps).collect();
    if !wrapping.is_empty() {
        return widest(&wrapping).map_or(Outer::Undecided, Outer::Wraps);
    }
    let s = &b.surfaces[f.surface as usize];
    let per = pcurve::shift_periods(s, b.tol_model);
    let contains = |i: usize, j: usize| {
        let poly = &polys[i].points;
        let lo = |axis: usize| {
            poly.iter()
                .map(|p| if axis == 0 { p.x() } else { p.y() })
                .fold(f64::INFINITY, f64::min)
        };
        let (lo_u, lo_v) = (lo(0), lo(1));
        let into = |x: f64, lo: f64, p: Option<f64>| match p {
            Some(p) if p > 0.0 => x - p * libm::floor((x - lo) / p),
            _ => x,
        };
        let pts = &polys[j].points;
        let inside = pts
            .iter()
            .filter(|q| {
                let q = Point2::new(into(q.x(), lo_u, per.0), into(q.y(), lo_v, per.1));
                winding(poly, q) != 0
            })
            .count();
        2 * inside > pts.len()
    };
    let found: Vec<usize> = used
        .iter()
        .copied()
        .filter(|&i| polys[i].points.len() >= 3)
        .filter(|&i| used.iter().all(|&j| j == i || contains(i, j)))
        .collect();
    match found.as_slice() {
        [i] => Outer::Contains(*i),
        _ => Outer::Undecided,
    }
}

#[cfg(test)]
mod tests;
