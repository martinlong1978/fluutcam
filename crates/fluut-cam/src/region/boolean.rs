// SPDX-License-Identifier: MIT OR Apache-2.0

//! Booleans on multi-loop regions.
//!
//! `cavalier_contours` combines two single loops only, so regions with
//! several outer loops and holes are combined here by split and classify:
//! every segment of each region is split where it meets the other region,
//! each piece is kept or dropped by where its midpoint lies, and the kept
//! pieces are stitched into loops. Orientation carries through: pieces keep
//! their direction (or are reversed for the subtracted region), so outer
//! loops stay counter-clockwise and holes clockwise.
//!
//! Segment pairs and point queries go through a bounding-box index of each
//! region, so the work grows as (n + m) log(n + m) plus the number of pairs
//! that actually meet. Where several kept pieces start at one point (regions
//! that touch there), stitching takes the sharpest left turn, and loops that
//! still pass a point twice are split there, so every loop is simple.

use super::{Inside, Loop2, Region2, Seg2, Vertex, contains_in, seg_distance};
use cavalier_contours::polyline::{PlineSegIntr, PlineVertex, pline_seg_intr};
use cavalier_contours::static_aabb2d_index::{StaticAABB2DIndex, StaticAABB2DIndexBuilder};
use fluut_geom::{Point2, Vec2};
use std::f64::consts::TAU;

/// Points closer than this are the same point when splitting and stitching.
const JOIN: f64 = 1e-7;
/// Distance of the side probe used to classify coincident pieces.
const PROBE: f64 = 1e-5;
/// Directions closer than this (radians) are the same when stitching.
const ANGLE: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Op {
    Union,
    Intersect,
    Subtract,
}

pub(super) fn boolean(a: &Region2, b: &Region2, op: Op) -> Region2 {
    let ia = Indexed::new(a);
    let ib = Indexed::new(b);
    let mut cut_a = vec![Vec::new(); ia.segs.len()];
    let mut cut_b = vec![Vec::new(); ib.segs.len()];
    for (i, s) in ia.segs.iter().enumerate() {
        for j in ib.query(&ia.boxes[i]) {
            for p in intersections(s, &ib.segs[j]) {
                cut_a[i].push(p);
                cut_b[j].push(p);
            }
        }
    }
    // Vertices of one region that lie within JOIN of a segment of the other
    // cut it too, even where the intersection test misses them (nearly
    // collinear edges), so that the pieces on both sides line up.
    for (cuts, own, other) in [(&mut cut_a, &ia, &ib), (&mut cut_b, &ib, &ia)] {
        for v in &other.segs {
            let p = v.a;
            let near = (p.x() - JOIN, p.y() - JOIN, p.x() + JOIN, p.y() + JOIN);
            for i in own.query(&near) {
                if let Some(q) = foot(&own.segs[i], p) {
                    cuts[i].push(q);
                }
            }
        }
    }
    let mut kept = Vec::new();
    for (s, cuts) in ia.segs.iter().zip(&cut_a) {
        for piece in split(s, cuts) {
            let keep = matches!(
                (classify(&piece, &ib), op),
                (Class::Out, Op::Union | Op::Subtract)
                    | (Class::In, Op::Intersect)
                    | (Class::Same, Op::Union | Op::Intersect)
                    | (Class::Opposite, Op::Subtract)
            );
            if keep {
                kept.push(piece);
            }
        }
    }
    for (s, cuts) in ib.segs.iter().zip(&cut_b) {
        for piece in split(s, cuts) {
            // Coincident pieces were decided on the `a` side.
            match (classify(&piece, &ia), op) {
                (Class::Out, Op::Union) | (Class::In, Op::Intersect) => kept.push(piece),
                (Class::In, Op::Subtract) => kept.push(reverse(&piece)),
                _ => {}
            }
        }
    }
    stitch(kept)
}

/// A region's segments with a bounding-box index over them.
pub(crate) struct Indexed {
    pub(crate) segs: Vec<Seg2>,
    /// Segment boxes grown by [`JOIN`].
    boxes: Vec<Box2>,
    index: Option<StaticAABB2DIndex<f64>>,
}

impl Indexed {
    pub(crate) fn new(r: &Region2) -> Self {
        let segs: Vec<Seg2> = r.segs().collect();
        let boxes: Vec<Box2> = segs.iter().map(seg_box).collect();
        let index = if boxes.is_empty() {
            None
        } else {
            let mut builder = StaticAABB2DIndexBuilder::new(boxes.len());
            for b in &boxes {
                builder.add(b.0, b.1, b.2, b.3);
            }
            // Fails only when the number of boxes added differs from the count.
            Some(builder.build().expect("one box per segment"))
        };
        Indexed { segs, boxes, index }
    }

    /// Indices of the segments whose box meets `b`, in ascending order.
    pub(crate) fn query(&self, b: &Box2) -> Vec<usize> {
        let Some(index) = &self.index else {
            return Vec::new();
        };
        let mut found = index.query(b.0, b.1, b.2, b.3);
        found.sort_unstable();
        found
    }

    fn query_segs(&self, b: &Box2) -> impl Iterator<Item = Seg2> + Clone + '_ {
        self.query(b).into_iter().map(|i| self.segs[i])
    }

    /// Whether `p` lies within [`JOIN`] of the boundary.
    fn near_boundary(&self, p: Point2) -> bool {
        let b = (p.x() - JOIN, p.y() - JOIN, p.x() + JOIN, p.y() + JOIN);
        self.query_segs(&b).any(|s| seg_distance(&s, p) <= JOIN)
    }

    /// [`Region2::contains`], looking only at the segments that can change
    /// the answer: those near `p` and those that meet the ray to its right.
    pub(crate) fn contains(&self, p: Point2) -> Inside {
        let Some(index) = &self.index else {
            return Inside::Out;
        };
        let max_x = index.bounds().map_or(p.x(), |b| b.max_x);
        let ray = (
            p.x() - JOIN,
            p.y() - JOIN,
            max_x.max(p.x()) + JOIN,
            p.y() + JOIN,
        );
        contains_in(self.query_segs(&ray), p)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    In,
    Out,
    /// On the other region's boundary, with that region's interior on the
    /// same side as this piece's own interior.
    Same,
    Opposite,
}

fn classify(piece: &Seg2, other: &Indexed) -> Class {
    let m = piece.point_at(0.5);
    // A piece lies on the other boundary only when its ends do too; then
    // the other region was cut at both ends and has a matching piece. A
    // piece that only comes close (nearly collinear edges) is classified at
    // a point away from the boundary instead.
    let on = [m, piece.a, piece.b]
        .iter()
        .all(|&p| other.near_boundary(p));
    if !on {
        let p = [m, piece.point_at(0.25), piece.point_at(0.75)]
            .into_iter()
            .find(|&p| !other.near_boundary(p))
            .unwrap_or(m);
        return match other.contains(p) {
            Inside::In => Class::In,
            _ => Class::Out,
        };
    }
    // The piece's own interior is on its left.
    let q = m + left_normal(piece) * PROBE;
    if other.contains(q) == Inside::In {
        Class::Same
    } else {
        Class::Opposite
    }
}

fn left_normal(s: &Seg2) -> Vec2 {
    let n = match s.arc() {
        None => {
            let d = s.b - s.a;
            Vec2::new(-d.y(), d.x())
        }
        Some(g) => {
            let m = s.point_at(0.5);
            let towards_centre = g.centre - m;
            if g.sweep > 0.0 {
                towards_centre
            } else {
                towards_centre * -1.0
            }
        }
    };
    n * (1.0 / n.length())
}

fn reverse(s: &Seg2) -> Seg2 {
    Seg2 {
        a: s.b,
        b: s.a,
        bulge: -s.bulge,
    }
}

fn to_vertex(p: Point2, bulge: f64) -> PlineVertex<f64> {
    PlineVertex::new(p.x(), p.y(), bulge)
}

fn intersections(s: &Seg2, t: &Seg2) -> Vec<Point2> {
    let r = pline_seg_intr(
        to_vertex(s.a, s.bulge),
        to_vertex(s.b, 0.0),
        to_vertex(t.a, t.bulge),
        to_vertex(t.b, 0.0),
        1e-9,
    );
    let p = |v: cavalier_contours::core::math::Vector2<f64>| Point2::new(v.x, v.y);
    match r {
        PlineSegIntr::NoIntersect => vec![],
        PlineSegIntr::TangentIntersect { point } | PlineSegIntr::OneIntersect { point } => {
            vec![p(point)]
        }
        PlineSegIntr::TwoIntersects { point1, point2 }
        | PlineSegIntr::OverlappingLines { point1, point2 }
        | PlineSegIntr::OverlappingArcs { point1, point2 } => vec![p(point1), p(point2)],
    }
}

/// The point of the segment nearest to `p` when that is within [`JOIN`] and
/// not an end point.
fn foot(s: &Seg2, p: Point2) -> Option<Point2> {
    let q = match s.arc() {
        None => {
            let d = s.b - s.a;
            let t = (p - s.a).dot(d) / d.dot(d);
            if t <= 0.0 || t >= 1.0 {
                return None;
            }
            s.a + d * t
        }
        Some(g) => {
            let r = p - g.centre;
            let len = r.length();
            if len == 0.0 || !g.contains_angle(libm::atan2(r.y(), r.x())) {
                return None;
            }
            g.centre + r * (g.radius / len)
        }
    };
    (q.distance(p) <= JOIN).then_some(q)
}

/// Parameter in [0, 1] of a point on the segment.
fn param(s: &Seg2, p: Point2) -> f64 {
    match s.arc() {
        None => {
            let d = s.b - s.a;
            (p - s.a).dot(d) / d.dot(d)
        }
        Some(g) => {
            let ang = libm::atan2(p.y() - g.centre.y(), p.x() - g.centre.x());
            let mut d = ang - g.start;
            let tau = std::f64::consts::TAU;
            if g.sweep > 0.0 {
                d = d.rem_euclid(tau);
            } else {
                d = -((-d).rem_euclid(tau));
            }
            d / g.sweep
        }
    }
}

/// The segment split at the given points (which lie on it), in order.
fn split(s: &Seg2, cuts: &[Point2]) -> Vec<Seg2> {
    let mut ts: Vec<(f64, Point2)> = cuts
        .iter()
        .filter(|p| p.distance(s.a) > JOIN && p.distance(s.b) > JOIN)
        .map(|&p| (param(s, p), p))
        .filter(|(t, _)| *t > 0.0 && *t < 1.0)
        .collect();
    ts.sort_by(|x, y| x.0.total_cmp(&y.0));
    ts.dedup_by(|x, y| x.1.distance(y.1) <= JOIN);
    let mut out = Vec::with_capacity(ts.len() + 1);
    let (mut t0, mut p0) = (0.0, s.a);
    let sweep = s.arc().map(|g| g.sweep);
    for (t1, p1) in ts.into_iter().chain([(1.0, s.b)]) {
        let bulge = match sweep {
            None => 0.0,
            Some(sw) => libm::tan(sw * (t1 - t0) / 4.0),
        };
        out.push(Seg2 {
            a: p0,
            b: p1,
            bulge,
        });
        (t0, p0) = (t1, p1);
    }
    out
}

/// Unit tangent at the start of a segment and its signed curvature
/// (positive when it turns left), computed from the bulge without
/// trigonometry: the tangents make the angle sweep / 2 = 2·atan(bulge)
/// with the chord.
fn start_dir(s: &Seg2) -> (Vec2, f64) {
    let (chord, k, c, sn) = turn_parts(s);
    (rotate(chord, c, -sn), k)
}

/// Unit tangent at the end of a segment and its signed curvature.
fn end_dir(s: &Seg2) -> (Vec2, f64) {
    let (chord, k, c, sn) = turn_parts(s);
    (rotate(chord, c, sn), k)
}

/// Unit chord, curvature, and cos and sin of half the sweep.
fn turn_parts(s: &Seg2) -> (Vec2, f64, f64, f64) {
    let d = s.b - s.a;
    let len = d.length();
    let chord = d * (1.0 / len);
    let b = s.bulge;
    let q = 1.0 + b * b;
    (chord, 4.0 * b / (len * q), (1.0 - b * b) / q, 2.0 * b / q)
}

fn rotate(v: Vec2, c: f64, s: f64) -> Vec2 {
    Vec2::new(c * v.x() - s * v.y(), s * v.x() + c * v.y())
}

/// How far clockwise the piece `next` leaves a junction, measured from the
/// way back along the piece `inc` that arrived there, in [0, 2π]. The
/// smallest value is the sharpest left turn. Equal directions are ordered by
/// curvature: the piece that bends further left comes first.
fn turn_key(inc: &Seg2, next: &Seg2) -> (f64, f64) {
    let (t_in, k_in) = end_dir(inc);
    let (back, k_back) = (t_in * -1.0, -k_in);
    let (t, k) = start_dir(next);
    let mut cw = libm::atan2(-back.perp_dot(t), back.dot(t)).rem_euclid(TAU);
    if cw < ANGLE || cw > TAU - ANGLE {
        // Leaving the way it came in: just clockwise of the way back when it
        // bends further right than the way back does, else all the way round.
        cw = if k < k_back { 0.0 } else { TAU };
    }
    (cw, -k)
}

/// Whether key `x` comes before key `y`.
fn turns_before(x: (f64, f64), y: (f64, f64)) -> bool {
    if (x.0 - y.0).abs() > ANGLE {
        x.0 < y.0
    } else {
        x.1 < y.1
    }
}

/// Points sorted by x, for finding the points near a given one.
struct ByX {
    order: Vec<usize>,
    xs: Vec<f64>,
}

impl ByX {
    fn new(pts: &[Point2]) -> Self {
        let mut order: Vec<usize> = (0..pts.len()).collect();
        order.sort_by(|&i, &j| {
            let (p, q) = (pts[i], pts[j]);
            p.x().total_cmp(&q.x()).then(p.y().total_cmp(&q.y()))
        });
        let xs = order.iter().map(|&i| pts[i].x()).collect();
        ByX { order, xs }
    }

    /// Indices of the points within [`JOIN`] of `p`.
    fn near<'a>(&'a self, pts: &'a [Point2], p: Point2) -> impl Iterator<Item = usize> + 'a {
        let lo = self.xs.partition_point(|&x| x < p.x() - JOIN);
        self.order[lo..]
            .iter()
            .copied()
            .take_while(move |&i| pts[i].x() <= p.x() + JOIN)
            .filter(move |&i| pts[i].distance(p) <= JOIN)
    }
}

/// Joins directed pieces end to start into closed loops. At a point where
/// several pieces start, the loop takes the sharpest left turn: with the
/// interior on the left of every piece, this traces each piece of interior
/// on its own and never crosses another loop.
fn stitch(pieces: Vec<Seg2>) -> Region2 {
    let starts: Vec<Point2> = pieces.iter().map(|s| s.a).collect();
    let by_start = ByX::new(&starts);
    let mut used = vec![false; pieces.len()];
    let mut loops = Vec::new();
    for first in 0..pieces.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let mut verts = vec![Vertex {
            p: pieces[first].a,
            bulge: pieces[first].bulge,
        }];
        let mut cur = first;
        let closed = loop {
            let mut best: Option<(usize, (f64, f64))> = None;
            for k in by_start.near(&starts, pieces[cur].b) {
                // The first piece is still a candidate: taking it closes the loop.
                if used[k] && !(k == first && verts.len() > 1) {
                    continue;
                }
                let key = turn_key(&pieces[cur], &pieces[k]);
                if best.is_none_or(|(_, b)| turns_before(key, b)) {
                    best = Some((k, key));
                }
            }
            match best {
                None => break false,
                Some((k, _)) if k == first => break true,
                Some((k, _)) => {
                    used[k] = true;
                    verts.push(Vertex {
                        p: pieces[k].a,
                        bulge: pieces[k].bulge,
                    });
                    cur = k;
                }
            }
        };
        if closed {
            for verts in split_touching(verts) {
                let l = Loop2 { verts };
                if l.area().abs() > JOIN {
                    loops.push(l);
                }
            }
        }
    }
    Region2 { loops }
}

/// Splits a closed loop wherever it passes the same point twice.
fn split_touching(verts: Vec<Vertex>) -> Vec<Vec<Vertex>> {
    let mut out = Vec::new();
    let mut todo = vec![verts];
    while let Some(v) = todo.pop() {
        match repeated(&v) {
            None => out.push(v),
            Some((i, j)) => {
                let inner = v[i..j].to_vec();
                let outer = v[j..].iter().chain(&v[..i]).copied().collect();
                todo.push(outer);
                todo.push(inner);
            }
        }
    }
    out
}

/// The first pair `i < j` of vertices at the same point, if any.
fn repeated(v: &[Vertex]) -> Option<(usize, usize)> {
    let mut order: Vec<usize> = (0..v.len()).collect();
    order.sort_by(|&i, &j| v[i].p.x().total_cmp(&v[j].p.x()).then(i.cmp(&j)));
    let mut best: Option<(usize, usize)> = None;
    for (n, &i) in order.iter().enumerate() {
        for &j in &order[n + 1..] {
            if v[j].p.x() - v[i].p.x() > JOIN {
                break;
            }
            if v[i].p.distance(v[j].p) <= JOIN {
                let pair = (i.min(j), i.max(j));
                if best.is_none_or(|b| pair < b) {
                    best = Some(pair);
                }
            }
        }
    }
    best
}

pub(crate) type Box2 = (f64, f64, f64, f64);

fn seg_box(s: &Seg2) -> Box2 {
    let (x0, x1) = s.support_x();
    let (y0, y1) = s.support_y();
    (x0 - JOIN, y0 - JOIN, x1 + JOIN, y1 + JOIN)
}
