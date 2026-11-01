// SPDX-License-Identifier: MIT OR Apache-2.0

//! Radial drops and rotary output for `rotary` Setups (spec 04 §4.7, D09
//! §5).
//!
//! In the work frame W the rotary axis is X. The tool stands at machine
//! (x, 0, r) with A = θ and touches the part point (x, r·sin θ, r·cos θ):
//! the part turned by R_x(θ) brings that direction to +Z. The radial drop
//! at (x, θ) is therefore the vertical S2 drop at (x, 0) on the bodies
//! turned by θ. Turned query geometries are built once per angle and kept.

use crate::cutter::Cutter;
use crate::geometry::SetupGeometry;
use crate::moves::Mv;
use crate::surface::{FaceSets, SurfaceQuery};
use fluut_geom::{Affine, Point3, Surface, Vec3};
use fluut_path::{AxisPos, Pass, PassKind, Segment};
use glam::DMat3;
use std::f64::consts::TAU;

/// R_x(θ): turns the direction (0, sin θ, cos θ) to +Z.
pub fn turn_x(theta: f64) -> Affine {
    let (s, c) = libm::sincos(theta);
    let m = DMat3::from_cols(
        glam::DVec3::new(1.0, 0.0, 0.0),
        glam::DVec3::new(0.0, c, s),
        glam::DVec3::new(0.0, -s, c),
    );
    Affine::from_mat3_translation(m, Vec3::ZERO)
}

/// Number of equal angle steps over a turn so that a chord at radius `r`
/// stays within `tol` (spec 04 §4.7.4: r·(1 − cos(Δθ/2)) ≤ tol/2).
/// Callers refine between samples where the drop leaves the chord.
pub fn grid_count(r: f64, tol: f64) -> usize {
    let r = r.max(tol);
    let d = 2.0 * libm::acos((1.0 - 0.5 * tol / r).clamp(-1.0, 1.0));
    ((TAU / d.max(1e-6)).ceil() as usize).clamp(16, 20_000)
}

/// The bodies turned to a list of angles.
#[derive(Debug)]
pub struct Turned {
    pub angles: Vec<f64>,
    pub geoms: Vec<SetupGeometry>,
}

impl Turned {
    pub fn new(g: &SetupGeometry, angles: Vec<f64>) -> Result<Turned, String> {
        let geoms = angles
            .iter()
            .map(|&t| SetupGeometry::transformed(&g.bodies, &turn_x(t)))
            .collect::<Result<_, _>>()?;
        Ok(Turned { angles, geoms })
    }

    /// `n` equal steps over a turn, from θ = 0.
    pub fn grid(g: &SetupGeometry, n: usize) -> Result<Turned, String> {
        Turned::new(g, (0..n).map(|k| TAU * k as f64 / n as f64).collect())
    }
}

/// Radial drop queries on turned geometry.
#[derive(Debug)]
pub struct Radial<'a> {
    qs: Vec<SurfaceQuery<'a>>,
    /// The untouched bodies, for drops at angles off the grid.
    base: Option<&'a SetupGeometry>,
    sets: FaceSets,
    cutter: Cutter,
    stl: f64,
}

impl<'a> Radial<'a> {
    pub fn new(t: &'a Turned, sets: &FaceSets, cutter: Cutter, stl: f64) -> Option<Radial<'a>> {
        let qs = t
            .geoms
            .iter()
            .map(|g| SurfaceQuery::new(g, sets, cutter, stl))
            .collect::<Option<_>>()?;
        Some(Radial {
            qs,
            base: None,
            sets: sets.clone(),
            cutter,
            stl,
        })
    }

    /// Allows [`Radial::drop_at`] on the bodies `g` (untouched).
    pub fn with_base(mut self, g: &'a SetupGeometry) -> Radial<'a> {
        self.base = Some(g);
        self
    }

    /// The CL radius at `x` and any angle `theta` (radians); builds the
    /// turned query for this one drop. `None` without a base.
    pub fn drop_at(&self, theta: f64, x: f64) -> Option<f64> {
        let g = SetupGeometry::transformed(&self.base?.bodies, &turn_x(theta)).ok()?;
        let q = SurfaceQuery::new(&g, &self.sets, self.cutter, self.stl)?;
        q.drop(x, 0.0).map(|h| h.tip)
    }

    pub fn len(&self) -> usize {
        self.qs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.qs.is_empty()
    }

    /// The CL radius (tool tip) at `x` on angle `k`, with the stock to
    /// leave; `None` when nothing is under the tool.
    pub fn drop(&self, k: usize, x: f64) -> Option<f64> {
        self.qs[k % self.qs.len()].drop(x, 0.0).map(|h| h.tip)
    }
}

/// Radius and X extent of a face from edge samples. Planes normal to X
/// (shaft ends) give none; planes parallel to X add their distance to the
/// axis (D15b §1.3).
pub fn face_extent(b: &fluut_topo::BrepBody, f: u32, tol: f64) -> Option<(f64, f64, f64, f64)> {
    let face = &b.faces[f as usize];
    let s = &b.surfaces[face.surface as usize];
    if let Surface::Plane { frame } = s
        && frame.z.get().x().abs() > 1.0 - 1e-9
    {
        return None;
    }
    let bb = b.face_box(f, tol);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &l in &face.loops {
        for &c in &b.loops[l as usize].coedges {
            for p in b.edge_points(b.coedges[c as usize].edge, tol) {
                let r = libm::hypot(p.y(), p.z());
                lo = lo.min(r);
                hi = hi.max(r);
            }
        }
    }
    if let Surface::Plane { frame } = s
        && frame.z.get().x().abs() < 1e-9
    {
        let o = frame.origin;
        let n = frame.z.get();
        lo = lo.min((o.y() * n.y() + o.z() * n.z()).abs());
    }
    (lo <= hi).then_some((lo, hi, bb.min.x(), bb.max.x()))
}

/// The radius range of the faces of `g` (D15b §1.3 `model_top`,
/// `model_bottom`), from [`face_extent`].
pub fn radius_range(g: &SetupGeometry, tol: f64) -> Option<(f64, f64)> {
    let mut out: Option<(f64, f64)> = None;
    for b in &g.bodies {
        let crate::geometry::BodyGeom::Brep(body) = &b.geom else {
            continue;
        };
        for f in 0..body.faces.len() as u32 {
            if let Some((lo, hi, _, _)) = face_extent(body, f, tol) {
                out = Some(out.map_or((lo, hi), |(a, c)| (a.min(lo), c.max(hi))));
            }
        }
    }
    out
}

/// A rotary position: X, CL radius and A in degrees.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct RPos {
    pub x: f64,
    pub r: f64,
    pub a: f64,
}

impl RPos {
    pub fn new(x: f64, r: f64, a: f64) -> RPos {
        RPos { x, r, a }
    }

    fn axis(self) -> AxisPos {
        AxisPos {
            xyz: Point3::new(self.x, 0.0, self.r),
            a: Some(self.a),
            b: None,
        }
    }
}

/// Radii the toolpath moves at between cuts (D15b §1.3: heights are radii).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct RLinks {
    pub clearance: f64,
    pub retract: f64,
    pub feed: f64,
    /// Lift to the clearance between cuts (`retract = "full"`).
    pub full: bool,
}

/// Builds rotary passes: cuts at fixed A as lines, A changes as rotary
/// segments with their CL radius (spec 04 §4.7.5).
#[derive(Debug)]
pub struct RotOut {
    tol: f32,
    links: RLinks,
    passes: Vec<Pass>,
    segs: Vec<Segment>,
    kind: PassKind,
    cur: Option<AxisPos>,
}

/// Radii within this are the same (the coaxial fast path, spec 04 §4.7.3).
const SAME_R: f64 = 1e-7;

impl RotOut {
    pub fn new(tol: f64, links: RLinks) -> RotOut {
        RotOut {
            tol: tol as f32,
            links,
            passes: Vec::new(),
            segs: Vec::new(),
            kind: PassKind::Link,
            cur: None,
        }
    }

    pub fn cur(&self) -> Option<RPos> {
        self.cur.map(|p| RPos {
            x: p.xyz.x(),
            r: p.xyz.z(),
            a: p.a.unwrap_or(0.0),
        })
    }

    fn pass(&mut self, kind: PassKind) {
        if kind != self.kind && !self.segs.is_empty() {
            let segs = std::mem::take(&mut self.segs);
            self.passes.push(Pass::new(self.kind, segs));
        }
        self.kind = kind;
    }

    /// A move to `p`: a line when A is fixed, else a rotary segment at the
    /// larger radius.
    pub fn to(&mut self, p: RPos, mv: Mv) {
        let b = p.axis();
        let Some(a) = self.cur else {
            self.cur = Some(b);
            return;
        };
        if a.same_bits(&b) {
            return;
        }
        let m = mv.info(self.tol);
        let seg = if a.a == b.a {
            Segment::Line { a, b, m }
        } else {
            Segment::Rotary {
                a,
                b,
                radius: a.xyz.z().max(b.xyz.z()),
                m,
            }
        };
        self.segs.push(seg);
        self.cur = Some(b);
    }

    /// Moves to the start of a cut at `p` from wherever the tool is:
    /// up to the retract radius (or the clearance), across, down to the
    /// feed radius, then a plunge.
    pub fn approach(&mut self, p: RPos) {
        self.pass(PassKind::Link);
        let l = self.links;
        match self.cur() {
            None => {
                self.to(
                    RPos {
                        r: l.clearance,
                        ..p
                    },
                    Mv::Rapid,
                );
            }
            Some(c) => {
                let up = if l.full {
                    l.clearance
                } else {
                    l.retract.max(c.r)
                };
                if up > c.r {
                    self.to(RPos { r: up, ..c }, Mv::Retract);
                }
                let across = if l.full {
                    l.clearance
                } else {
                    l.retract.max(p.r)
                };
                self.to(
                    RPos {
                        x: p.x,
                        r: across.max(up),
                        a: p.a,
                    },
                    Mv::Rapid,
                );
            }
        }
        let feed = l.feed.max(p.r);
        if self.cur().is_some_and(|c| c.r > feed) {
            self.to(RPos { r: feed, ..p }, Mv::Rapid);
        }
        self.to(p, Mv::Plunge);
    }

    /// A cut through `pts` (the tool is at `pts[0]`). Runs of samples at
    /// one radius whose X is linear in A become one rotary segment.
    pub fn cut(&mut self, pts: &[RPos]) {
        self.pass(PassKind::Cut);
        let mut i = 0;
        while i + 1 < pts.len() {
            let p0 = pts[i];
            let mut j = i + 1;
            if pts[j].a != p0.a {
                let rate = (pts[j].x - p0.x) / (pts[j].a - p0.a);
                while j + 1 < pts.len() {
                    let q = pts[j + 1];
                    let same_r = (q.r - p0.r).abs() <= SAME_R;
                    let monotone = (q.a - pts[j].a) * (pts[j].a - p0.a) > 0.0;
                    let linear = (p0.x + rate * (q.a - p0.a) - q.x).abs() <= 1e-9;
                    if !(same_r && monotone && linear && (pts[j].r - p0.r).abs() <= SAME_R) {
                        break;
                    }
                    j += 1;
                }
            }
            let end = pts[j];
            let end = if (end.r - p0.r).abs() <= SAME_R {
                RPos { r: p0.r, ..end }
            } else {
                end
            };
            self.to(end, Mv::Cut);
            i = j;
        }
    }

    /// A move along the cut that stays down (a ring link).
    pub fn link_down(&mut self, p: RPos) {
        self.pass(PassKind::Cut);
        self.to(p, Mv::Link);
    }

    /// A radial descent while A turns: from the current position to `p`.
    pub fn ramp(&mut self, p: RPos) {
        self.pass(PassKind::Ramp);
        self.to(p, Mv::Ramp);
    }

    /// Up to the clearance and the passes.
    pub fn finish(mut self) -> Vec<Pass> {
        if let Some(c) = self.cur() {
            self.pass(PassKind::Retract);
            if c.r < self.links.clearance {
                self.to(
                    RPos {
                        r: self.links.clearance,
                        ..c
                    },
                    Mv::Retract,
                );
            }
        }
        self.pass(PassKind::Link);
        self.passes
    }
}
