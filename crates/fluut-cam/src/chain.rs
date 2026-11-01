// SPDX-License-Identifier: MIT OR Apache-2.0

//! Drive chains: selected curves projected along the tool axis onto the XY
//! plane of W and joined end to end (D15 §2.3.1).

use crate::path2::{Path2, seg_reversed};
use crate::region::{Loop2, Seg2};
use cavalier_contours::polyline::{PlineSegIntr, PlineVertex, pline_seg_intr};
use fluut_geom::{Curve3, FitSeg, Interval, Point2, Point3, UnitVec3, fit_points};
use std::f64::consts::PI;

/// One projected curve: segments in XY and the Z range of the 3-D curve.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub segs: Vec<Seg2>,
    pub z: Interval,
}

/// Curves joined end to end.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub path: Path2,
    /// Z range of the source curves (the `selection` height reference).
    pub z: Interval,
}

fn xy(p: Point3) -> Point2 {
    Point2::new(p.x(), p.y())
}

/// Projects `c` over `range` onto XY. Lines stay lines; circles about the
/// Z axis become arcs; anything else is fitted with lines and arcs within
/// `tol`.
pub fn project_curve(c: &Curve3, range: Interval, tol: f64) -> Piece {
    let pts = c.tessellate(range, tol.max(1e-4));
    let z = pts.iter().fold(
        Interval {
            lo: f64::INFINITY,
            hi: f64::NEG_INFINITY,
        },
        |acc, (_, p)| Interval {
            lo: acc.lo.min(p.z()),
            hi: acc.hi.max(p.z()),
        },
    );
    let mut segs = Vec::new();
    match c {
        Curve3::Line { .. } => segs.push(Seg2 {
            a: xy(c.eval(range.lo)),
            b: xy(c.eval(range.hi)),
            bulge: 0.0,
        }),
        Curve3::Circle { frame, .. } if frame.z.get().z().abs() > 1.0 - 1e-12 => {
            let sign = frame.z.get().z().signum();
            let span = range.hi - range.lo;
            let n = ((span / PI) - 1e-12).ceil().max(1.0) as usize;
            let mut prev = xy(c.eval(range.lo));
            for k in 1..=n {
                let t = if k == n {
                    range.hi
                } else {
                    range.lo + span * k as f64 / n as f64
                };
                let p = xy(c.eval(t));
                segs.push(Seg2 {
                    a: prev,
                    b: p,
                    bulge: libm::tan(sign * span / n as f64 / 4.0),
                });
                prev = p;
            }
        }
        _ => {
            let flat: Vec<Point3> = pts
                .iter()
                .map(|(_, p)| Point3::new(p.x(), p.y(), 0.0))
                .collect();
            for f in fit_points(&flat, tol) {
                segs.push(match f {
                    FitSeg::Line { a, b } => Seg2 {
                        a: xy(a),
                        b: xy(b),
                        bulge: 0.0,
                    },
                    FitSeg::Arc {
                        a,
                        b,
                        normal,
                        sweep,
                        ..
                    } => Seg2 {
                        a: xy(a),
                        b: xy(b),
                        bulge: libm::tan(normal.get().z().signum() * sweep / 4.0),
                    },
                });
            }
            for i in 1..segs.len() {
                segs[i].a = segs[i - 1].b;
            }
        }
    }
    segs.retain(|s| s.a.distance(s.b) > 1e-12);
    Piece { segs, z }
}

fn reversed(segs: &[Seg2]) -> Vec<Seg2> {
    segs.iter().rev().map(seg_reversed).collect()
}

/// Joins pieces whose ends meet within `tol`, in selection order: each
/// chain grows at its end, then at its start. Joints are snapped so the
/// chain is continuous bitwise; a chain whose ends meet is closed.
pub fn chain_pieces(pieces: Vec<Piece>, tol: f64) -> Vec<Chain> {
    let mut pieces: Vec<Option<Piece>> = pieces
        .into_iter()
        .filter(|p| !p.segs.is_empty())
        .map(Some)
        .collect();
    let mut out = Vec::new();
    for i in 0..pieces.len() {
        let Some(first) = pieces[i].take() else {
            continue;
        };
        let mut segs = first.segs;
        let mut z = first.z;
        loop {
            let end = segs[segs.len() - 1].b;
            let start = segs[0].a;
            if segs.len() > 1 && end.distance(start) <= tol {
                break;
            }
            let mut grown = false;
            for slot in pieces.iter_mut() {
                let Some(p) = slot else { continue };
                let (ps, pe) = (p.segs[0].a, p.segs[p.segs.len() - 1].b);
                let add = if ps.distance(end) <= tol {
                    Some((p.segs.clone(), true))
                } else if pe.distance(end) <= tol {
                    Some((reversed(&p.segs), true))
                } else if pe.distance(start) <= tol {
                    Some((p.segs.clone(), false))
                } else if ps.distance(start) <= tol {
                    Some((reversed(&p.segs), false))
                } else {
                    None
                };
                if let Some((mut s, at_end)) = add {
                    z = Interval {
                        lo: z.lo.min(p.z.lo),
                        hi: z.hi.max(p.z.hi),
                    };
                    *slot = None;
                    if at_end {
                        s[0].a = end;
                        segs.extend(s);
                    } else {
                        let n = s.len();
                        s[n - 1].b = start;
                        s.extend(segs);
                        segs = s;
                    }
                    grown = true;
                    break;
                }
            }
            if !grown {
                break;
            }
        }
        let n = segs.len();
        let closed = segs[n - 1].b.distance(segs[0].a) <= tol && (n > 1 || segs[0].bulge != 0.0);
        if closed {
            segs[n - 1].b = segs[0].a;
        }
        out.push(Chain {
            path: Path2 { segs, closed },
            z,
        });
    }
    out
}

impl Chain {
    /// The chain as a loop, counter-clockwise. Closed chains only.
    pub fn ccw_loop(&self) -> Loop2 {
        let l = self.path.to_loop();
        if l.is_ccw() { l } else { l.reversed() }
    }

    /// Whether two non-adjacent segments meet.
    pub fn self_intersects(&self) -> bool {
        let s = &self.path.segs;
        let n = s.len();
        let v = |p: Point2, b: f64| PlineVertex::new(p.x(), p.y(), b);
        for i in 0..n {
            for j in i + 1..n {
                let adjacent = j == i + 1 || (self.path.closed && i == 0 && j == n - 1);
                let r = pline_seg_intr(
                    v(s[i].a, s[i].bulge),
                    v(s[i].b, 0.0),
                    v(s[j].a, s[j].bulge),
                    v(s[j].b, 0.0),
                    1e-9,
                );
                let meets = match r {
                    PlineSegIntr::NoIntersect => false,
                    PlineSegIntr::TangentIntersect { point: p }
                    | PlineSegIntr::OneIntersect { point: p } => {
                        !adjacent || !is_shared(&s[i], &s[j], Point2::new(p.x, p.y))
                    }
                    PlineSegIntr::TwoIntersects { point1, point2 } => {
                        !adjacent
                            || !is_shared(&s[i], &s[j], Point2::new(point1.x, point1.y))
                            || !is_shared(&s[i], &s[j], Point2::new(point2.x, point2.y))
                    }
                    _ => true,
                };
                if meets {
                    return true;
                }
            }
        }
        false
    }
}

/// Whether `p` is the joint of adjacent segments `a` and `b`.
fn is_shared(a: &Seg2, b: &Seg2, p: Point2) -> bool {
    let near = |q: Point2| q.distance(p) <= 1e-7;
    (near(a.b) && near(b.a)) || (near(a.a) && near(b.b))
}

/// Tool axis of W.
pub const Z: UnitVec3 = UnitVec3::Z;

#[cfg(test)]
mod tests {
    use super::*;
    use fluut_geom::{Frame, Vec3};

    fn line(a: [f64; 3], b: [f64; 3]) -> (Curve3, Interval) {
        let pa = Point3::new(a[0], a[1], a[2]);
        let pb = Point3::new(b[0], b[1], b[2]);
        let d = pb - pa;
        let c = Curve3::line(pa, UnitVec3::new(d).unwrap());
        (
            c,
            Interval {
                lo: 0.0,
                hi: d.length(),
            },
        )
    }

    fn pieces(cs: &[(Curve3, Interval)]) -> Vec<Piece> {
        cs.iter()
            .map(|(c, r)| project_curve(c, *r, 0.001))
            .collect()
    }

    #[test]
    fn out_of_order_and_reversed_curves_close() {
        let cs = [
            line([0.0, 0.0, 5.0], [10.0, 0.0, 5.0]),
            line([0.0, 10.0, 5.0], [10.0, 10.0, 5.0]),
            line([0.0, 0.0, 5.0], [0.0, 10.0, 5.0]),
            line([10.0, 0.0, 5.0], [10.0, 10.0, 5.0]),
        ];
        let ch = chain_pieces(pieces(&cs), 1e-6);
        assert_eq!(ch.len(), 1);
        assert!(ch[0].path.closed);
        assert_eq!(ch[0].path.segs.len(), 4);
        assert_eq!(ch[0].z, Interval { lo: 5.0, hi: 5.0 });
        assert!((ch[0].ccw_loop().area() - 100.0).abs() < 1e-12);
        assert!(!ch[0].self_intersects());
    }

    #[test]
    fn open_chain_and_gap() {
        let cs = [
            line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
            line([10.0, 0.0, 0.0], [10.0, 10.0, -2.0]),
            line([20.0, 0.0, 0.0], [30.0, 0.0, 0.0]),
        ];
        let ch = chain_pieces(pieces(&cs), 1e-6);
        assert_eq!(ch.len(), 2);
        assert!(!ch[0].path.closed);
        assert_eq!(ch[0].z, Interval { lo: -2.0, hi: 0.0 });
    }

    #[test]
    fn circles_about_z_become_arcs() {
        let f = Frame::new(
            Point3::new(1.0, 2.0, 3.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(1.0, 0.0, 0.0),
        )
        .unwrap();
        let c = Curve3::circle(f, 4.0).unwrap();
        let p = project_curve(
            &c,
            Interval {
                lo: 0.0,
                hi: std::f64::consts::TAU,
            },
            0.001,
        );
        assert_eq!(p.segs.len(), 2);
        let ch = chain_pieces(vec![p], 1e-6);
        assert!(ch[0].path.closed);
        // Clockwise seen from +Z: the frame's axis points down.
        assert!(ch[0].path.to_loop().area() < 0.0);
        assert!((ch[0].ccw_loop().area() - 16.0 * PI).abs() < 1e-9);
    }

    #[test]
    fn figure_eight_self_intersects() {
        let cs = [
            line([0.0, 0.0, 0.0], [10.0, 10.0, 0.0]),
            line([10.0, 10.0, 0.0], [10.0, 0.0, 0.0]),
            line([10.0, 0.0, 0.0], [0.0, 10.0, 0.0]),
            line([0.0, 10.0, 0.0], [0.0, 0.0, 0.0]),
        ];
        let ch = chain_pieces(pieces(&cs), 1e-6);
        assert!(ch[0].path.closed);
        assert!(ch[0].self_intersects());
    }
}
