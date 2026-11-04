// SPDX-License-Identifier: MIT OR Apache-2.0

//! Query geometry (D07 §2): a body's faces, edges and vertices as
//! primitives with conservative boxes in a best-first BVH (D03 §4).

use super::trim::{Trim, TrimError};
use fluut_geom::{Aabb3, Curve3, Interval, Point3, Surface, UvBox, Vec3};
use fluut_topo::{BrepBody, Bvh};
use std::borrow::Cow;

/// A leaf primitive of the query BVH.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Prim {
    /// Part of face `face` over parameter box `uv` (a whole analytic face,
    /// or one Bézier patch of a B-spline face).
    Face {
        face: u32,
        uv: UvBox,
    },
    Edge(u32),
    Vertex(u32),
}

/// A face prepared for contact queries.
#[derive(Debug, Clone)]
pub struct QueryFace {
    pub trim: Trim,
    /// Parameter box of the face, slightly grown, inside the surface domain.
    pub uv: UvBox,
    /// The trim loops on the surface: the face's own boundary.
    pub rim: Vec<Rim>,
}

/// Points per box of a [`Rim`].
pub const RIM_CHUNK: usize = 16;

/// One trim loop as surface points `(u, v, S(u, v))`, with the XY box of
/// each run of [`RIM_CHUNK`] segments.
#[derive(Debug, Clone)]
pub struct Rim {
    pub pts: Vec<(f64, f64, Point3)>,
    /// `[x_lo, y_lo, x_hi, y_hi]` of points `k·RIM_CHUNK ..= (k+1)·RIM_CHUNK`.
    pub boxes: Vec<[f64; 4]>,
}

impl Rim {
    fn new(s: &Surface, uv: &[(f64, f64)]) -> Rim {
        let pts: Vec<(f64, f64, Point3)> = uv.iter().map(|&(u, v)| (u, v, s.eval(u, v))).collect();
        let mut boxes = Vec::new();
        let mut k = 0;
        while k + 1 < pts.len() {
            let mut b = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for p in &pts[k..(k + RIM_CHUNK + 1).min(pts.len())] {
                b = [
                    b[0].min(p.2.x()),
                    b[1].min(p.2.y()),
                    b[2].max(p.2.x()),
                    b[3].max(p.2.y()),
                ];
            }
            boxes.push(b);
            k += RIM_CHUNK;
        }
        Rim { pts, boxes }
    }
}

/// Options for building the query geometry.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct BuildOptions {
    /// Chord of the trim polylines in mm; contacts this close to a face
    /// boundary may be classified either way (the edge contact covers them).
    pub trim_chord: f64,
    /// Slack allowed in sampled surface boxes before they are grown by the
    /// interpolation bound, in mm.
    pub box_slack: f64,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions {
            trim_chord: 1e-3,
            box_slack: 0.05,
        }
    }
}

#[derive(Debug)]
pub struct QueryGeometry<'a> {
    pub body: Cow<'a, BrepBody>,
    /// Per face; `None` when its trim could not be built (see `issues`).
    pub faces: Vec<Option<QueryFace>>,
    pub prims: Vec<Prim>,
    pub bvh: Bvh,
    /// The trim chord ([`BuildOptions::trim_chord`]).
    pub trim_chord: f64,
    /// Faces left out of the query, with the reason. Their edges and
    /// vertices are still queried.
    pub issues: Vec<(u32, TrimError)>,
}

fn iv(lo: f64, hi: f64) -> Interval {
    Interval { lo, hi }
}

/// Exact box of `o + a·cos t·x + b·sin t·y` over `range`.
fn ellipse_box(o: Point3, x: Vec3, y: Vec3, a: f64, b: f64, range: Interval) -> Aabb3 {
    let at = |t: f64| {
        let (s, c) = libm::sincos(t);
        o + x * (a * c) + y * (b * s)
    };
    let mut bx = Aabb3::from_points(&[at(range.lo), at(range.hi)]);
    let comps = |v: Vec3| [v.x(), v.y(), v.z()];
    let (xs, ys) = (comps(x), comps(y));
    for k in 0..3 {
        // Extremes of coordinate k where tan t = b·y_k / (a·x_k).
        let t0 = libm::atan2(b * ys[k], a * xs[k]);
        let first = ((range.lo - t0) / std::f64::consts::PI).ceil() as i64;
        let last = ((range.hi - t0) / std::f64::consts::PI).floor() as i64;
        for n in first..=last.min(first + 4) {
            bx = bx.with_point(at(t0 + n as f64 * std::f64::consts::PI));
        }
    }
    bx
}

/// A box containing `curve` over `range`.
pub fn curve_box(curve: &Curve3, range: Interval) -> Aabb3 {
    match curve {
        Curve3::Line { .. } => Aabb3::from_points(&[curve.eval(range.lo), curve.eval(range.hi)]),
        Curve3::Circle { frame, radius } => ellipse_box(
            frame.origin,
            frame.x.get(),
            frame.y().get(),
            *radius,
            *radius,
            range,
        ),
        Curve3::Ellipse { frame, a, b } => {
            ellipse_box(frame.origin, frame.x.get(), frame.y().get(), *a, *b, range)
        }
        Curve3::BSpline(b) => {
            // Poles of every span the range touches (convex hull property).
            let (k, poles, p) = (b.knots(), b.poles(), b.degree() as usize);
            let mut bx = Aabb3::empty();
            for i in p..poles.len() {
                if k[i + 1] > k[i] && k[i + 1] >= range.lo && k[i] <= range.hi {
                    for q in &poles[i - p..=i] {
                        bx = bx.with_point(*q);
                    }
                }
            }
            bx
        }
        Curve3::Offset { .. } => {
            // Spike: sampled, grown by the chord (not a proven bound).
            let chord = 1e-3;
            let pts: Vec<Point3> = curve
                .tessellate(range, chord)
                .into_iter()
                .map(|x| x.1)
                .collect();
            Aabb3::from_points(&pts).expand(10.0 * chord)
        }
    }
}

/// Bounds on `|Suu|` and `|Svv|` over `uv`, if known in closed form.
fn second_derivative_bounds(s: &Surface, uv: UvBox) -> Option<(f64, f64)> {
    Some(match s {
        Surface::Plane { .. } => (0.0, 0.0),
        Surface::Cylinder { r, .. } => (*r, 0.0),
        Surface::Cone { r, semi_angle, .. } => {
            let t = libm::tan(*semi_angle);
            ((r + uv.v.lo * t).abs().max((r + uv.v.hi * t).abs()), 0.0)
        }
        Surface::Sphere { r, .. } => (*r, *r),
        Surface::Torus { major, minor, .. } => (major + minor, *minor),
        _ => return None,
    })
}

/// A box containing `s` over `uv`: grid samples grown by the bilinear
/// interpolation error `(Δu²·|Suu| + Δv²·|Svv|) / 8`. Kinds without a
/// closed-form bound use the largest sampled second derivative times 2
/// (spike; not a proven bound).
fn sampled_box(s: &Surface, uv: UvBox, slack: f64) -> Aabb3 {
    let (muu, mvv) = second_derivative_bounds(s, uv).unwrap_or_else(|| {
        let (mut a, mut b) = (0.0f64, 0.0f64);
        for i in 0..=8 {
            for j in 0..=8 {
                let d = s.derivs(uv.u.at(i as f64 / 8.0), uv.v.at(j as f64 / 8.0));
                a = a.max(d.suu.length());
                b = b.max(d.svv.length());
            }
        }
        (2.0 * a, 2.0 * b)
    });
    let steps =
        |len: f64, m: f64| ((len * (m / (8.0 * slack)).sqrt()).ceil() as usize).clamp(1, 256);
    let (nu, nv) = (steps(uv.u.length(), muu), steps(uv.v.length(), mvv));
    let (du, dv) = (uv.u.length() / nu as f64, uv.v.length() / nv as f64);
    let grow = (du * du * muu + dv * dv * mvv) / 8.0;
    let mut bx = Aabb3::empty();
    for i in 0..=nu {
        for j in 0..=nv {
            bx =
                bx.with_point(s.eval(uv.u.at(i as f64 / nu as f64), uv.v.at(j as f64 / nv as f64)));
        }
    }
    bx.expand(grow)
}

/// A box containing `s` over `uv`.
pub fn surface_box(s: &Surface, uv: UvBox, slack: f64) -> Aabb3 {
    match s {
        Surface::Extrusion { profile, dir } => {
            let b = curve_box(profile, uv.u);
            let (lo, hi) = (dir.get() * uv.v.lo, dir.get() * uv.v.hi);
            Aabb3 {
                min: b.min + lo,
                max: b.max + lo,
            }
            .union(&Aabb3 {
                min: b.min + hi,
                max: b.max + hi,
            })
        }
        _ => sampled_box(s, uv, slack),
    }
}

fn intersect(a: Interval, b: Interval) -> Option<Interval> {
    Interval::new(a.lo.max(b.lo), a.hi.min(b.hi))
}

impl<'a> QueryGeometry<'a> {
    pub fn new(body: &'a BrepBody, opts: &BuildOptions) -> QueryGeometry<'a> {
        QueryGeometry::with_faces(body, opts, |_| true)
    }

    /// The query geometry of the faces for which `keep` is true, with the
    /// edges and vertices that bound them. Other faces are `None` in
    /// `faces`, without an issue.
    pub fn with_faces(
        body: &'a BrepBody,
        opts: &BuildOptions,
        keep: impl Fn(u32) -> bool,
    ) -> QueryGeometry<'a> {
        let mut prims = Vec::new();
        let mut boxes = Vec::new();
        let mut faces = Vec::with_capacity(body.faces.len());
        let mut issues = Vec::new();
        let mut edge_used = vec![false; body.edges.len()];
        for (fi, f) in body.faces.iter().enumerate() {
            if !keep(fi as u32) {
                continue;
            }
            for &l in &f.loops {
                for &c in &body.loops[l as usize].coedges {
                    edge_used[body.coedges[c as usize].edge as usize] = true;
                }
            }
        }
        let mut vertex_used = vec![false; body.vertices.len()];
        for (ei, e) in body.edges.iter().enumerate() {
            if edge_used[ei] {
                vertex_used[e.v0 as usize] = true;
                vertex_used[e.v1 as usize] = true;
            }
        }
        for (fi, f) in body.faces.iter().enumerate() {
            let fi = fi as u32;
            if !keep(fi) {
                faces.push(None);
                continue;
            }
            let s = &body.surfaces[f.surface as usize];
            let trim = match Trim::build(body, fi, opts.trim_chord) {
                Ok(t) => t,
                Err(e) => {
                    issues.push((fi, e));
                    faces.push(None);
                    continue;
                }
            };
            // Grow the box: the trim polylines cut corners by up to a chord.
            let t = trim.uv_box();
            let gu = 1e-3 * t.u.length() + 1e-9;
            let gv = 1e-3 * t.v.length() + 1e-9;
            let mut uv = UvBox::new(iv(t.u.lo - gu, t.u.hi + gu), iv(t.v.lo - gv, t.v.hi + gv));
            if let Surface::Sphere { .. } = s {
                uv.v = intersect(
                    uv.v,
                    iv(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2),
                )
                .unwrap_or(uv.v);
            }
            match s {
                Surface::BSpline(b) => {
                    let d = b.domain();
                    // A face box beyond the domain (periodic surface across
                    // its seam) takes the whole domain.
                    let fu = intersect(uv.u, d.u)
                        .filter(|_| uv.u.hi <= d.u.hi + gu && uv.u.lo >= d.u.lo - gu)
                        .unwrap_or(d.u);
                    let fv = intersect(uv.v, d.v)
                        .filter(|_| uv.v.hi <= d.v.hi + gv && uv.v.lo >= d.v.lo - gv)
                        .unwrap_or(d.v);
                    uv = UvBox::new(fu, fv);
                    for (pb, bx) in b.patches() {
                        if let (Some(u), Some(v)) = (intersect(pb.u, uv.u), intersect(pb.v, uv.v))
                            && u.length() > 0.0
                            && v.length() > 0.0
                        {
                            prims.push(Prim::Face {
                                face: fi,
                                uv: UvBox::new(u, v),
                            });
                            boxes.push(*bx);
                        }
                    }
                }
                _ => {
                    prims.push(Prim::Face { face: fi, uv });
                    boxes.push(surface_box(s, uv, opts.box_slack).expand(opts.trim_chord));
                }
            }
            let rim = trim.loops_uv().iter().map(|l| Rim::new(s, l)).collect();
            faces.push(Some(QueryFace { trim, uv, rim }));
        }
        for (ei, e) in body.edges.iter().enumerate() {
            let Some(c) = e.curve else { continue };
            if !edge_used[ei] {
                continue;
            }
            prims.push(Prim::Edge(ei as u32));
            boxes.push(curve_box(&body.curves[c as usize], e.range));
        }
        for (vi, v) in body.vertices.iter().enumerate() {
            if !vertex_used[vi] {
                continue;
            }
            prims.push(Prim::Vertex(vi as u32));
            boxes.push(Aabb3::from_points(&[v.p]));
        }
        QueryGeometry {
            body: Cow::Borrowed(body),
            faces,
            bvh: Bvh::new(&boxes),
            trim_chord: opts.trim_chord,
            prims,
            issues,
        }
    }

    /// The same geometry with its own copy of the body, for a query that
    /// outlives the body it was built from.
    pub fn into_owned(self) -> QueryGeometry<'static> {
        QueryGeometry {
            body: Cow::Owned(self.body.into_owned()),
            faces: self.faces,
            prims: self.prims,
            bvh: self.bvh,
            trim_chord: self.trim_chord,
            issues: self.issues,
        }
    }

    /// Box of primitive `i`.
    pub fn prim_box(&self, i: u32) -> &Aabb3 {
        self.bvh.item_box(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cutter::synth::{Builder, bspline_square, tiled};
    use fluut_geom::{BSplineCurve3, Frame};
    use std::sync::Arc;

    fn contains_all(b: &Aabb3, pts: impl IntoIterator<Item = Point3>) {
        for p in pts {
            assert!(b.expand(1e-9).contains(p), "{p:?} outside {b:?}");
        }
    }

    #[test]
    fn arc_boxes_are_exact_and_contain_the_arc() {
        let f = Frame::new(
            Point3::new(1.0, 2.0, 3.0),
            Vec3::new(0.3, -0.2, 1.0),
            Vec3::new(1.0, 1.0, 0.0),
        )
        .unwrap();
        let c = Curve3::circle(f, 2.0).unwrap();
        for range in [iv(0.0, 6.2), iv(-1.0, 0.5), iv(2.0, 2.1)] {
            let b = curve_box(&c, range);
            let pts: Vec<Point3> = (0..=1000)
                .map(|i| c.eval(range.at(i as f64 / 1000.0)))
                .collect();
            contains_all(&b, pts.iter().copied());
            let tight = Aabb3::from_points(&pts);
            assert!((b.max - tight.max).length() < 1e-4 && (b.min - tight.min).length() < 1e-4);
        }
    }

    #[test]
    fn bspline_curve_box_contains_the_range() {
        let poles: Vec<Point3> = (0..7)
            .map(|i| Point3::new(i as f64, libm::sin(i as f64), 0.3 * i as f64))
            .collect();
        let k = vec![0.0, 0.0, 0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0];
        let c = Curve3::BSpline(Arc::new(BSplineCurve3::new(3, k, poles, None).unwrap()));
        let r = iv(0.3, 0.6);
        let b = curve_box(&c, r);
        contains_all(&b, (0..=500).map(|i| c.eval(r.at(i as f64 / 500.0))));
        assert!(b.max.x() < 6.0, "only spans touching the range");
    }

    #[test]
    fn surface_boxes_contain_dense_samples() {
        let f = Frame::new(
            Point3::new(0.5, -1.0, 2.0),
            Vec3::new(0.2, 0.4, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
        )
        .unwrap();
        let cases = [
            (
                Surface::cylinder(f, 3.0).unwrap(),
                UvBox::new(iv(0.1, 2.9), iv(-1.0, 2.0)),
            ),
            (
                Surface::cone(f, 1.0, 0.5).unwrap(),
                UvBox::new(iv(-1.0, 4.0), iv(0.0, 3.0)),
            ),
            (
                Surface::sphere(f, 2.0).unwrap(),
                UvBox::new(iv(0.3, 5.0), iv(-1.2, 1.4)),
            ),
            (
                Surface::torus(f, 4.0, 1.0).unwrap(),
                UvBox::new(iv(0.0, 3.0), iv(-2.0, 2.5)),
            ),
        ];
        for (s, uv) in cases {
            let b = surface_box(&s, uv, 0.05);
            contains_all(
                &b,
                (0..=200).flat_map(|i| {
                    let s = &s;
                    (0..=200)
                        .map(move |j| s.eval(uv.u.at(i as f64 / 200.0), uv.v.at(j as f64 / 200.0)))
                }),
            );
        }
    }

    #[test]
    fn query_geometry_of_a_tile_model() {
        let body = tiled(2, false);
        let g = QueryGeometry::new(&body, &BuildOptions::default());
        assert!(g.issues.is_empty(), "{:?}", g.issues);
        assert_eq!(body.faces.len(), 32);
        // Every face's primitive boxes contain dense samples of the trimmed face.
        for (i, p) in g.prims.iter().enumerate() {
            let Prim::Face { face, uv } = *p else {
                continue;
            };
            let q = g.faces[face as usize].as_ref().unwrap();
            let s = &body.surfaces[body.faces[face as usize].surface as usize];
            let b = g.prim_box(i as u32);
            for a in 0..=60 {
                for c in 0..=60 {
                    let (u, v) = (uv.u.at(a as f64 / 60.0), uv.v.at(c as f64 / 60.0));
                    if q.trim.contains(u, v) {
                        contains_all(b, [s.eval(u, v)]);
                    }
                }
            }
        }
    }

    #[test]
    fn bspline_face_is_split_into_patches() {
        let mut b = Builder::new("bump");
        let (s, bd) = bspline_square(Point3::ORIGIN, 10.0, 6, |i, j| ((i * j) % 3) as f64);
        b.bspline_patch(s, bd);
        let body = b.build();
        let g = QueryGeometry::new(&body, &BuildOptions::default());
        let n = g
            .prims
            .iter()
            .filter(|p| matches!(p, Prim::Face { .. }))
            .count();
        assert_eq!(n, 9);
    }
}
