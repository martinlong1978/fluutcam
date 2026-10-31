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

use super::{Inside, Loop2, Region2, Seg2, Vertex};
use cavalier_contours::polyline::{PlineSegIntr, PlineVertex, pline_seg_intr};
use fluut_geom::{Point2, Vec2};

/// Points closer than this are the same point when splitting and stitching.
const JOIN: f64 = 1e-7;
/// Distance of the side probe used to classify coincident pieces.
const PROBE: f64 = 1e-5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Op {
    Union,
    Intersect,
    Subtract,
}

pub(super) fn boolean(a: &Region2, b: &Region2, op: Op) -> Region2 {
    let sa: Vec<Seg2> = a.segs().collect();
    let sb: Vec<Seg2> = b.segs().collect();
    let mut cut_a = vec![Vec::new(); sa.len()];
    let mut cut_b = vec![Vec::new(); sb.len()];
    let boxes_b: Vec<_> = sb.iter().map(seg_box).collect();
    for (i, s) in sa.iter().enumerate() {
        let bx = seg_box(s);
        for (j, t) in sb.iter().enumerate() {
            if !overlap(&bx, &boxes_b[j]) {
                continue;
            }
            for p in intersections(s, t) {
                cut_a[i].push(p);
                cut_b[j].push(p);
            }
        }
    }
    let mut kept = Vec::new();
    for (s, cuts) in sa.iter().zip(&cut_a) {
        for piece in split(s, cuts) {
            let keep = matches!(
                (classify(&piece, b), op),
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
    for (s, cuts) in sb.iter().zip(&cut_b) {
        for piece in split(s, cuts) {
            // Coincident pieces were decided on the `a` side.
            match (classify(&piece, a), op) {
                (Class::Out, Op::Union) | (Class::In, Op::Intersect) => kept.push(piece),
                (Class::In, Op::Subtract) => kept.push(reverse(&piece)),
                _ => {}
            }
        }
    }
    stitch(kept)
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

fn classify(piece: &Seg2, other: &Region2) -> Class {
    let m = piece.point_at(0.5);
    if other.boundary_distance(m) > JOIN {
        return match other.contains(m) {
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

/// Joins directed pieces end to start into closed loops.
fn stitch(mut pieces: Vec<Seg2>) -> Region2 {
    let mut loops = Vec::new();
    while let Some(first) = pieces.pop() {
        let start = first.a;
        let mut verts = vec![Vertex {
            p: first.a,
            bulge: first.bulge,
        }];
        let mut end = first.b;
        while end.distance(start) > JOIN {
            let Some(k) = pieces.iter().position(|s| s.a.distance(end) <= JOIN) else {
                break;
            };
            let s = pieces.swap_remove(k);
            verts.push(Vertex {
                p: s.a,
                bulge: s.bulge,
            });
            end = s.b;
        }
        let l = Loop2 { verts };
        if end.distance(start) <= JOIN && l.area().abs() > JOIN {
            loops.push(l);
        }
    }
    Region2 { loops }
}

type Box2 = (f64, f64, f64, f64);

fn seg_box(s: &Seg2) -> Box2 {
    let (x0, x1) = s.support_x();
    let (y0, y1) = s.support_y();
    (x0 - JOIN, y0 - JOIN, x1 + JOIN, y1 + JOIN)
}

fn overlap(a: &Box2, b: &Box2) -> bool {
    a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3
}
