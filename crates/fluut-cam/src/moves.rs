// SPDX-License-Identifier: MIT OR Apache-2.0

//! Building `fluut-path` passes from planar paths and heights. The builder
//! keeps the current position, so every segment starts bitwise where the
//! previous one ended (D08 §2).

use crate::path2::Path2;
use crate::region::Seg2;
use fluut_geom::{Point2, Point3, UnitVec3};
use fluut_path::{
    AxisPos, CompSide, CycleParams, FeedRef, Motion, MotionInfo, Pass, PassKind, Segment,
};

/// Motion and feed of a move (D08 §9).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Mv {
    Rapid,
    Retract,
    Plunge,
    Ramp,
    LeadIn,
    LeadOut,
    Cut,
    Link,
}

impl Mv {
    fn info(self, tol: f32) -> MotionInfo {
        let (m, f) = match self {
            Mv::Rapid => (Motion::Rapid, FeedRef::Rapid),
            Mv::Retract => (Motion::Retract, FeedRef::Rapid),
            Mv::Plunge => (Motion::Plunge, FeedRef::Plunge),
            Mv::Ramp => (Motion::Ramp, FeedRef::Ramp),
            Mv::LeadIn => (Motion::LeadIn, FeedRef::LeadIn),
            Mv::LeadOut => (Motion::LeadOut, FeedRef::LeadOut),
            Mv::Cut => (Motion::Cut, FeedRef::Cut),
            Mv::Link => (Motion::Link, FeedRef::Transition),
        };
        MotionInfo::new(m, f, tol)
    }
}

#[derive(Debug)]
pub struct Out {
    tol: f32,
    cur: Option<AxisPos>,
    segs: Vec<Segment>,
    pub passes: Vec<Pass>,
}

impl Out {
    pub fn new(tol: f64) -> Out {
        Out {
            tol: tol as f32,
            cur: None,
            segs: Vec::new(),
            passes: Vec::new(),
        }
    }

    pub fn cur(&self) -> Option<Point3> {
        self.cur.map(|c| c.xyz)
    }

    /// Sets the position the next segment starts from.
    pub fn start(&mut self, p: Point3) {
        self.cur = Some(AxisPos::xyz(p));
    }

    pub fn line(&mut self, b: Point3, mv: Mv) {
        let Some(a) = self.cur else {
            self.start(b);
            return;
        };
        let b = AxisPos::xyz(b);
        if a.same_bits(&b) {
            return;
        }
        self.segs.push(Segment::Line {
            a,
            b,
            m: mv.info(self.tol),
        });
        self.cur = Some(b);
    }

    /// Moves along `s` from the current point (which is `s.a`) to `s.b`,
    /// with Z going linearly to `z1`: a line, an arc, or a helix.
    pub fn seg(&mut self, s: &Seg2, z1: f64, mv: Mv) {
        let Some(a) = self.cur else {
            return;
        };
        let b = Point3::new(s.b.x(), s.b.y(), z1);
        if s.is_line() {
            self.line(b, mv);
            return;
        }
        // Centre from the actual start, so start and end radii agree.
        let s = Seg2 {
            a: Point2::new(a.xyz.x(), a.xyz.y()),
            ..*s
        };
        let Some(g) = s.arc() else {
            self.line(b, mv);
            return;
        };
        let centre = Point3::new(g.centre.x(), g.centre.y(), a.xyz.z());
        let b = AxisPos::xyz(b);
        let m = mv.info(self.tol);
        let seg = if a.xyz.z() == z1 {
            Segment::Arc {
                a,
                b,
                centre,
                normal: UnitVec3::Z,
                sweep: g.sweep,
                m,
            }
        } else {
            Segment::Helix {
                a,
                b,
                centre,
                normal: UnitVec3::Z,
                sweep: g.sweep,
                m,
            }
        };
        self.segs.push(seg);
        self.cur = Some(b);
    }

    /// A full helix turn (or turns) about `centre`, ending above or below
    /// the current point at `z1`. `sweep` is signed.
    pub fn helix(&mut self, centre: Point2, sweep: f64, z1: f64, mv: Mv) {
        let Some(a) = self.cur else {
            return;
        };
        let b = AxisPos::xyz(Point3::new(a.xyz.x(), a.xyz.y(), z1));
        self.segs.push(Segment::Helix {
            a,
            b,
            centre: Point3::new(centre.x(), centre.y(), a.xyz.z()),
            normal: UnitVec3::Z,
            sweep,
            m: mv.info(self.tol),
        });
        self.cur = Some(b);
    }

    /// Follows `path` with Z going linearly in arc length from the current
    /// Z to `z1`.
    pub fn path_ramp(&mut self, path: &Path2, z1: f64, mv: Mv) {
        let Some(a) = self.cur else {
            return;
        };
        let z0 = a.xyz.z();
        let total = path.length();
        let mut acc = 0.0;
        let n = path.segs.len();
        for (i, s) in path.segs.iter().enumerate() {
            acc += s.length();
            let z = if i + 1 == n || total <= 0.0 {
                z1
            } else {
                z0 + (z1 - z0) * (acc / total)
            };
            self.seg(s, z, mv);
        }
    }

    /// An arc of `sweep` about `normal` through `centre`, from the current
    /// point to `b` (vertical leads of 3-D types).
    pub fn arc(&mut self, b: Point3, centre: Point3, normal: UnitVec3, sweep: f64, mv: Mv) {
        let Some(a) = self.cur else {
            return;
        };
        let b = AxisPos::xyz(b);
        self.segs.push(Segment::Arc {
            a,
            b,
            centre,
            normal,
            sweep,
            m: mv.info(self.tol),
        });
        self.cur = Some(b);
    }

    /// Follows `path` at constant Z.
    pub fn path(&mut self, path: &Path2, mv: Mv) {
        let Some(a) = self.cur else {
            return;
        };
        let z = a.xyz.z();
        for s in &path.segs {
            self.seg(s, z, mv);
        }
    }

    /// A drilling cycle at the current point (the R plane above the hole).
    pub fn cycle(&mut self, cycle: CycleParams) {
        let Some(at) = self.cur else {
            return;
        };
        self.segs.push(Segment::Cycle {
            at,
            cycle,
            m: Mv::Plunge.info(self.tol),
        });
    }

    /// Closes the segments so far into a pass.
    pub fn pass(&mut self, kind: PassKind, comp: Option<CompSide>) {
        if self.segs.is_empty() {
            return;
        }
        let mut p = Pass::new(kind, std::mem::take(&mut self.segs));
        p.comp = comp;
        self.passes.push(p);
    }
}

/// Output options of the Passes group (D15 §2.5): `output_arcs = false`
/// replaces arcs and helices with lines within `tol`; `max_len` splits
/// longer lines and arcs into equal pieces.
pub fn finish(passes: Vec<Pass>, output_arcs: bool, max_len: Option<f64>, tol: f64) -> Vec<Pass> {
    passes
        .into_iter()
        .map(|mut p| {
            let mut segs = Vec::with_capacity(p.segments.len());
            for s in p.segments {
                let pieces = if output_arcs {
                    vec![s]
                } else {
                    fluut_path::linearize(&s, tol)
                };
                for piece in pieces {
                    match max_len {
                        Some(m) if m > 0.0 && piece.length() > m => split(&piece, m, &mut segs),
                        _ => segs.push(piece),
                    }
                }
            }
            p.segments = segs;
            p
        })
        .collect()
}

fn split(s: &Segment, max: f64, out: &mut Vec<Segment>) {
    let n = (s.length() / max).ceil() as usize;
    match s {
        Segment::Line { a, b, m } => {
            let mut prev = *a;
            for k in 1..=n {
                let q = if k == n {
                    *b
                } else {
                    AxisPos::xyz(a.xyz.lerp(b.xyz, k as f64 / n as f64))
                };
                out.push(Segment::Line {
                    a: prev,
                    b: q,
                    m: *m,
                });
                prev = q;
            }
        }
        Segment::Arc {
            a,
            b,
            centre,
            normal,
            sweep,
            m,
        }
        | Segment::Helix {
            a,
            b,
            centre,
            normal,
            sweep,
            m,
        } if *normal == UnitVec3::Z => {
            let helix = matches!(s, Segment::Helix { .. });
            let (cx, cy) = (centre.x(), centre.y());
            let (dx, dy) = (a.xyz.x() - cx, a.xyz.y() - cy);
            let mut prev = *a;
            for k in 1..=n {
                let f = k as f64 / n as f64;
                let q = if k == n {
                    *b
                } else {
                    let (sn, cs) = libm::sincos(sweep * f);
                    AxisPos::xyz(Point3::new(
                        cx + dx * cs - dy * sn,
                        cy + dx * sn + dy * cs,
                        a.xyz.z() + (b.xyz.z() - a.xyz.z()) * f,
                    ))
                };
                let c = Point3::new(cx, cy, prev.xyz.z());
                let piece = if helix {
                    Segment::Helix {
                        a: prev,
                        b: q,
                        centre: c,
                        normal: *normal,
                        sweep: sweep / n as f64,
                        m: *m,
                    }
                } else {
                    Segment::Arc {
                        a: prev,
                        b: q,
                        centre: c,
                        normal: *normal,
                        sweep: sweep / n as f64,
                        m: *m,
                    }
                };
                out.push(piece);
                prev = q;
            }
        }
        _ => out.push(s.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::Region2;
    use fluut_path::ToolpathResult;

    #[test]
    fn paths_become_continuous_segments() {
        let circle = Path2::from_loop(&Region2::circle(0.0, 0.0, 5.0).loops[0]);
        let square = Path2::from_loop(&Region2::rect(0.0, 0.0, 10.0, 10.0).loops[0]);
        let mut o = Out::new(0.01);
        o.start(Point3::new(5.0, 0.0, 10.0));
        o.line(Point3::new(5.0, 0.0, 2.0), Mv::Rapid);
        o.path_ramp(&circle, -1.0, Mv::Ramp);
        o.path(&circle, Mv::Cut);
        o.pass(PassKind::Cut, None);
        o.line(Point3::new(0.0, 0.0, -1.0), Mv::Link);
        o.path(&square, Mv::Cut);
        o.pass(PassKind::Cut, None);
        let r = ToolpathResult::new(o.passes.clone()).unwrap();
        assert!(matches!(r.passes()[0].segments[1], Segment::Helix { .. }));
        assert!(matches!(r.passes()[0].segments[3], Segment::Arc { .. }));
        let lin = finish(o.passes.clone(), false, Some(3.0), 0.01);
        let r2 = ToolpathResult::new(lin).unwrap();
        assert!(r2.segments().all(|s| matches!(s, Segment::Line { .. })));
        assert!(r2.segments().all(|s| s.length() <= 3.0 + 1e-12));
        let split = finish(o.passes, true, Some(3.0), 0.01);
        let r3 = ToolpathResult::new(split).unwrap();
        assert!((r3.stats().cut_length - r.stats().cut_length).abs() < 1e-9);
    }
}
