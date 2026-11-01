// SPDX-License-Identifier: MIT OR Apache-2.0

//! Planar paths of line and arc segments, measured by arc length.
//!
//! Consecutive segments share endpoints bitwise; every operation here keeps
//! that, so the 3-D moves built from a path are continuous (D08 §2).

use crate::region::{Loop2, Seg2, Vertex};
use fluut_geom::{Point2, Vec2};

/// Unit tangent of a segment at parameter `t` in [0, 1].
pub fn seg_tangent(s: &Seg2, t: f64) -> Vec2 {
    match s.arc() {
        None => unit(s.b - s.a),
        Some(g) => {
            let ang = g.start + g.sweep * t;
            let (sn, cs) = libm::sincos(ang);
            let d = Vec2::new(-sn, cs);
            if g.sweep >= 0.0 { d } else { d * -1.0 }
        }
    }
}

pub fn unit(v: Vec2) -> Vec2 {
    let l = v.length();
    if l > 0.0 { v * (1.0 / l) } else { v }
}

/// Left normal of a direction.
pub fn left(v: Vec2) -> Vec2 {
    Vec2::new(-v.y(), v.x())
}

/// The two pieces of a segment split at parameter `t` in (0, 1). The split
/// point is shared bitwise.
pub fn seg_split(s: &Seg2, t: f64) -> (Seg2, Seg2) {
    let p = s.point_at(t);
    let (b1, b2) = match s.arc() {
        None => (0.0, 0.0),
        Some(g) => (
            libm::tan(g.sweep * t / 4.0),
            libm::tan(g.sweep * (1.0 - t) / 4.0),
        ),
    };
    (
        Seg2 {
            a: s.a,
            b: p,
            bulge: b1,
        },
        Seg2 {
            a: p,
            b: s.b,
            bulge: b2,
        },
    )
}

/// Parameter in [0, 1] of the point of `s` nearest `p`.
pub fn seg_closest(s: &Seg2, p: Point2) -> f64 {
    match s.arc() {
        None => {
            let d = s.b - s.a;
            let l2 = d.dot(d);
            if l2 > 0.0 {
                ((p - s.a).dot(d) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            }
        }
        Some(g) => {
            let ang = libm::atan2(p.y() - g.centre.y(), p.x() - g.centre.x());
            let tau = std::f64::consts::TAU;
            let mut d = (ang - g.start).rem_euclid(tau);
            if g.sweep < 0.0 {
                d = (tau - d).rem_euclid(tau);
            }
            let t = d / g.sweep.abs();
            if t <= 1.0 {
                t
            } else {
                // Beyond the arc: the nearer end.
                let pa = p.distance(s.a);
                let pb = p.distance(s.b);
                if pa <= pb { 0.0 } else { 1.0 }
            }
        }
    }
}

pub fn seg_reversed(s: &Seg2) -> Seg2 {
    Seg2 {
        a: s.b,
        b: s.a,
        bulge: -s.bulge,
    }
}

/// A path of segments, open or closed.
#[derive(Debug, Clone, PartialEq)]
pub struct Path2 {
    pub segs: Vec<Seg2>,
    pub closed: bool,
}

impl Path2 {
    pub fn open(segs: Vec<Seg2>) -> Path2 {
        Path2 {
            segs,
            closed: false,
        }
    }

    pub fn from_loop(l: &Loop2) -> Path2 {
        Path2 {
            segs: l.segs().collect(),
            closed: true,
        }
    }

    pub fn to_loop(&self) -> Loop2 {
        Loop2 {
            verts: self
                .segs
                .iter()
                .map(|s| Vertex {
                    p: s.a,
                    bulge: s.bulge,
                })
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }

    pub fn length(&self) -> f64 {
        self.segs.iter().map(Seg2::length).sum()
    }

    pub fn start(&self) -> Point2 {
        self.segs[0].a
    }

    pub fn end(&self) -> Point2 {
        self.segs[self.segs.len() - 1].b
    }

    pub fn start_tangent(&self) -> Vec2 {
        seg_tangent(&self.segs[0], 0.0)
    }

    pub fn end_tangent(&self) -> Vec2 {
        seg_tangent(&self.segs[self.segs.len() - 1], 1.0)
    }

    pub fn reversed(&self) -> Path2 {
        Path2 {
            segs: self.segs.iter().rev().map(seg_reversed).collect(),
            closed: self.closed,
        }
    }

    /// Arc length of the point nearest `p`, and its distance.
    pub fn closest(&self, p: Point2) -> (f64, f64) {
        let mut best = (0.0, f64::INFINITY);
        let mut acc = 0.0;
        for s in &self.segs {
            let t = seg_closest(s, p);
            let d = s.point_at(t).distance(p);
            if d < best.1 {
                best = (acc + t * s.length(), d);
            }
            acc += s.length();
        }
        best
    }

    /// Point and unit tangent at arc length `at`.
    pub fn at(&self, at: f64) -> (Point2, Vec2) {
        let mut acc = 0.0;
        let last = self.segs.len().saturating_sub(1);
        for (i, s) in self.segs.iter().enumerate() {
            let l = s.length();
            if at <= acc + l || i == last {
                let t = if l > 0.0 {
                    ((at - acc) / l).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                return (s.point_at(t), seg_tangent(s, t));
            }
            acc += l;
        }
        (Point2::new(0.0, 0.0), Vec2::new(1.0, 0.0))
    }

    /// The path split at the given increasing arc lengths. Splits closer
    /// than `eps` to a segment end or to each other use the existing vertex.
    pub fn split_at(&self, cuts: &[f64], eps: f64) -> Path2 {
        let mut out = Vec::with_capacity(self.segs.len() + cuts.len());
        let mut acc = 0.0;
        let mut ci = 0;
        for s in &self.segs {
            let l = s.length();
            let mut cur = *s;
            let mut cur_from = acc;
            while ci < cuts.len() && cuts[ci] < acc + l - eps {
                let c = cuts[ci];
                ci += 1;
                if c <= cur_from + eps {
                    continue;
                }
                let rest = acc + l - cur_from;
                let t = (c - cur_from) / rest;
                let (x, y) = seg_split(&cur, t);
                out.push(x);
                cur = y;
                cur_from = c;
            }
            out.push(cur);
            acc += l;
        }
        Path2 {
            segs: out,
            closed: self.closed,
        }
    }

    /// A closed path starting at arc length `at` (split there if needed).
    pub fn rotated(&self, at: f64, eps: f64) -> Path2 {
        let total = self.length();
        let at = at.rem_euclid(total);
        if at <= eps || at >= total - eps {
            return self.clone();
        }
        let split = self.split_at(&[at], eps);
        let mut acc = 0.0;
        let mut k = 0;
        for (i, s) in split.segs.iter().enumerate() {
            if (acc - at).abs() <= eps {
                k = i;
                break;
            }
            acc += s.length();
        }
        let mut segs = split.segs[k..].to_vec();
        segs.extend_from_slice(&split.segs[..k]);
        Path2 { segs, closed: true }
    }

    /// The part between arc lengths `s0 < s1` of an open path (or of a
    /// closed path without wrapping).
    pub fn sub(&self, s0: f64, s1: f64, eps: f64) -> Path2 {
        let split = self.split_at(&[s0, s1], eps);
        let mut acc = 0.0;
        let mut segs = Vec::new();
        for s in &split.segs {
            let l = s.length();
            let mid = acc + 0.5 * l;
            if mid > s0 && mid < s1 {
                segs.push(*s);
            }
            acc += l;
        }
        Path2::open(segs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::Region2;

    fn square() -> Path2 {
        Path2::from_loop(&Region2::rect(0.0, 0.0, 10.0, 10.0).loops[0])
    }

    fn continuous(p: &Path2) -> bool {
        p.segs.windows(2).all(|w| w[0].b == w[1].a) && (!p.closed || p.end() == p.start())
    }

    #[test]
    fn split_and_rotate_keep_continuity() {
        let p = square();
        assert_eq!(p.length(), 40.0);
        let r = p.rotated(15.0, 1e-9);
        assert!(continuous(&r));
        assert_eq!(r.start(), Point2::new(10.0, 5.0));
        assert_eq!(r.segs.len(), 5);
        assert!((r.length() - 40.0).abs() < 1e-12);
        let s = p.split_at(&[5.0, 10.0, 25.0], 1e-9);
        assert_eq!(s.segs.len(), 6);
        assert!(continuous(&s));
        let sub = p.sub(5.0, 25.0, 1e-9);
        assert!((sub.length() - 20.0).abs() < 1e-12);
        assert_eq!(sub.start(), Point2::new(5.0, 0.0));
        assert_eq!(sub.end(), Point2::new(5.0, 10.0));
    }

    #[test]
    fn arcs_split_exactly() {
        let c = Path2::from_loop(&Region2::circle(0.0, 0.0, 5.0).loops[0]);
        let tau = std::f64::consts::TAU;
        assert!((c.length() - 5.0 * tau).abs() < 1e-12);
        let r = c.rotated(0.25 * 5.0 * tau, 1e-9);
        assert!(continuous(&r));
        assert!(r.start().distance(Point2::new(0.0, 5.0)) < 1e-12);
        assert!((r.length() - 5.0 * tau).abs() < 1e-9);
        let (s, d) = c.closest(Point2::new(0.0, -7.0));
        assert!((s - 0.75 * 5.0 * tau).abs() < 1e-9 && (d - 2.0).abs() < 1e-12);
        let (_, t) = c.at(0.0);
        assert!(t.distance_to(Vec2::new(0.0, 1.0)) < 1e-12);
    }

    #[test]
    fn reversed_and_tangents() {
        let p = square();
        let r = p.reversed();
        assert!(continuous(&r));
        assert_eq!(r.start(), p.start());
        assert_eq!(p.start_tangent(), Vec2::new(1.0, 0.0));
        assert_eq!(r.start_tangent(), Vec2::new(0.0, 1.0));
    }

    trait Dist {
        fn distance_to(self, o: Vec2) -> f64;
    }
    impl Dist for Vec2 {
        fn distance_to(self, o: Vec2) -> f64 {
            (self - o).length()
        }
    }
}
