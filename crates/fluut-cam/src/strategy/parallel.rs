// SPDX-License-Identifier: MIT OR Apache-2.0

//! `parallel` (D09 §4.3, D15 §5): passes in XY at an angle, following the
//! surface in Z by drop queries (D07). Finishing type.
//!
//! Each pass is a line across the region. Along it, CL points come from
//! adaptive sampling (D08 §4.2). A point is cut when its contact is on a
//! drive face, its slope is within the limits and its tip is not above
//! `top`; the cut-off points between cut and uncut stretches are found by
//! bisection. Uncut stretches and the gaps between passes are linked over
//! (D08 §8.2).

use crate::chain::chain_pieces;
use crate::cutter::Cutter;
use crate::drive::{curve_pieces, faces, nest};
use crate::geometry::{BodyGeom, face_z_range};
use crate::layout::{layout_even, layout_fixed};
use crate::moves::{Mv, Out, finish};
use crate::region::{Inside, Region2};
use crate::scallop::{FaceCurvature, face_curvature, is_mesh, scallop_stepover};
use crate::surface::{FaceSets, Hit, SurfaceQuery};
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, prepare, tag};
use fluut_geom::{Point2, Point3, UnitVec3, Vec2, Vec3};
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{
    BoundarySide, Containment, Lead, LeadKind, ParallelDirection, PassLayout, ResolvedHeights,
    RetractPolicy, resolve,
};
use fluut_model::tools::ToolShape;
use fluut_model::{Diagnostic, Sel};
use fluut_path::{PassKind, ToolpathResult};
use std::collections::BTreeSet;
use std::f64::consts::FRAC_PI_2;

#[derive(Debug)]
pub struct Parallel;

/// Cut-off points are found to this distance along the pass, mm.
const CUTOFF: f64 = 1e-7;
/// A tip this far above `top` counts as above it, mm.
const ABOVE_TOP: f64 = 1e-6;
/// Samples per lead for the gouge check.
const LEAD_SAMPLES: usize = 32;

impl ToolpathType for Parallel {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Parallel
    }

    fn role(&self) -> Role {
        Role::Finish
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        let tp = input.toolpath;
        let fail = |d: Vec<Diagnostic>| CamError::Invalid(tag(d, tp));
        let r0 = resolve(&tp.params, input.tool, input.material);
        let g = input.geometry;
        let geo = &r0.geometry;
        let chord = r0.passes.machining_tolerance.max(1e-4) / 4.0;
        // Face sets (D15 §2.3.2): empty drive faces = every face.
        let excluded = faces(g, &geo.excluded_faces, &[], "geometry.excluded_faces")
            .map_err(|d| fail(vec![d]))?;
        let drive = if geo.drive_faces.is_empty() {
            let all: Vec<Sel> = g
                .bodies
                .iter()
                .filter(|b| matches!(b.geom, BodyGeom::Brep(_)))
                .map(|b| Sel::Body(b.id.clone()))
                .collect();
            faces(g, &all, &geo.excluded_faces, "geometry.drive_faces")
        } else {
            faces(
                g,
                &geo.drive_faces,
                &geo.excluded_faces,
                "geometry.drive_faces",
            )
        }
        .map_err(|d| fail(vec![d]))?;
        let check = match &geo.check_faces {
            None => None,
            Some(c) => Some(
                faces(g, c, &geo.excluded_faces, "geometry.check_faces")
                    .map_err(|d| fail(vec![d]))?,
            ),
        };
        let ids = |v: &[(&fluut_topo::BrepBody, u32, String)]| -> BTreeSet<String> {
            v.iter().map(|x| x.2.clone()).collect()
        };
        let sets = FaceSets {
            drive: ids(&drive),
            check: check.as_deref().map(ids),
            excluded: ids(&excluded),
        };
        let mut sel: Option<(f64, f64)> = None;
        for (b, f, _) in &drive {
            let z = face_z_range(b, *f, chord);
            sel = Some(match sel {
                None => (z.lo, z.hi),
                Some((lo, hi)) => (lo.min(z.lo), hi.max(z.hi)),
            });
        }
        let prep = prepare(input, sel).map_err(|e| match e {
            CamError::Invalid(d) => fail(d),
            e => e,
        })?;
        let mut diags = prep.warnings;
        let p = &prep.params;
        let h = prep.heights;
        let Some(pp) = p.parallel().cloned() else {
            return Err(CamError::Unavailable("not a parallel toolpath".into()));
        };
        let tol = p.passes.machining_tolerance;
        let tool = input.tool;
        let cutter = match tool.shape {
            ToolShape::Flat | ToolShape::Ball | ToolShape::Bull => {
                Cutter::new(tool.diameter, tool.corner_radius())
            }
            ToolShape::V | ToolShape::Taper => None,
        };
        let Some(cutter) = cutter else {
            diags.push(
                Diagnostic::error(
                    "cam.parallel.tool_shape",
                    "parallel needs a flat, ball or bull-nose tool",
                )
                .at("tool"),
            );
            return Err(fail(diags));
        };
        if drive.is_empty() {
            diags.push(
                Diagnostic::error("cam.geometry.drive.empty", "there are no drive faces")
                    .at("geometry.drive_faces"),
            );
            return Err(fail(diags));
        }
        let stl = p.passes.stock_to_leave;
        let Some(q) = SurfaceQuery::new(g, &sets, cutter, stl) else {
            diags.push(
                Diagnostic::error(
                    "param.passes.stock_to_leave.range",
                    "the stock to leave is at or below the lower bound for this tool",
                )
                .at("passes.stock_to_leave"),
            );
            return Err(fail(diags));
        };
        // Curvature of the drive faces (scallop stepover, rest warning).
        let mut curv = FaceCurvature::default();
        let mut mesh = false;
        for b in &g.bodies {
            let BodyGeom::Brep(body) = &b.geom else {
                continue;
            };
            let mine: Vec<u32> = drive
                .iter()
                .filter(|x| std::ptr::eq(x.0, body))
                .map(|x| x.1)
                .collect();
            if mine.is_empty() {
                continue;
            }
            mesh |= is_mesh(body);
            let qg = crate::cutter::QueryGeometry::with_faces(
                body,
                &crate::cutter::BuildOptions::default(),
                |f| mine.contains(&f),
            );
            for &f in &mine {
                curv = curv.max(face_curvature(&qg, f));
            }
        }
        if curv.concave > 0.0 && 1.0 / curv.concave <= tool.radius() {
            diags.push(Diagnostic::warning(
                "cam.parallel.scallop.concave",
                format!(
                    "a concave region of radius {:.3} mm is not larger than the tool radius \
                     {}: it is left as rest material",
                    1.0 / curv.concave,
                    tool.radius()
                ),
            ));
        }
        let stepover = match pp.scallop_height {
            Some(sh) => {
                if mesh {
                    diags.push(
                        Diagnostic::warning(
                            "param.parallel.drive_faces.mesh_scallop",
                            "the drive faces are a mesh: curvature from triangles is zero \
                             inside facets, so the stepover is the flat-area value",
                        )
                        .at("passes.scallop_height"),
                    );
                }
                scallop_stepover(sh, tool.corner_radius(), curv.convex)
            }
            None => pp.stepover.unwrap_or(0.0),
        };
        if stepover.is_nan() || stepover <= 0.0 || stepover > tool.diameter {
            diags.push(
                Diagnostic::error(
                    "param.passes.stepover.max",
                    format!(
                        "stepover {stepover} is not above 0 and at most the tool diameter {}",
                        tool.diameter
                    ),
                )
                .at("passes.stepover"),
            );
            return Err(fail(diags));
        }
        // Region: the drive faces' box, limited by the boundary.
        let Some((lo, hi)) = q.drive_box() else {
            diags.push(Diagnostic::error(
                "cam.geometry.drive.empty",
                "the drive faces have no extent",
            ));
            return Err(fail(diags));
        };
        let mut region = Region2::rect(lo.x(), lo.y(), hi.x(), hi.y());
        if let Some(b) = &p.geometry.boundary {
            let join = g.tol_model().max(1e-5);
            let bc = chain_pieces(
                curve_pieces(g, &b.curves, chord, "geometry.boundary.curves")
                    .map_err(|d| fail(vec![d]))?,
                join,
            );
            if bc.iter().any(|c| !c.path.closed) {
                diags.push(
                    Diagnostic::error(
                        "param.geometry.boundary.open",
                        "a boundary chain is not closed",
                    )
                    .at("geometry.boundary.curves"),
                );
                return Err(fail(diags));
            }
            let br = nest(bc.iter().map(|c| c.ccw_loop()).collect());
            let grow = match b.containment {
                Containment::Inside => -tool.radius(),
                Containment::Centre => 0.0,
                Containment::Outside => tool.radius(),
            };
            region = match b.side {
                BoundarySide::Inside => region.intersect(&br.offset(grow)),
                BoundarySide::Outside => region.subtract(&br.offset(-grow)),
            };
        }
        let ang = pp.pass_angle_deg.to_radians();
        let (sn, cs) = libm::sincos(ang);
        let u = Vec2::new(cs, sn);
        let n = Vec2::new(-sn, cs);
        let pl = Sampler {
            q: &q,
            h: &h,
            min_slope: pp.min_slope_deg.to_radians(),
            max_slope: pp.max_slope_deg.to_radians(),
            tol,
            max_step: 0.5 * tool.radius(),
        };
        let mut runs: Vec<Run> = Vec::new();
        if !region.is_empty() {
            let (a, b) = region.support(n);
            let pos = match p.passes.pass_layout {
                PassLayout::Even => layout_even(a, b, stepover),
                PassLayout::FixedFromEdge => layout_fixed(a, b, stepover),
            };
            let mut k = 0usize;
            for off in pos {
                let o = Point2::new(n.x() * off, n.y() * off);
                let spans: Vec<(f64, f64)> = region
                    .clip_line(o, u)
                    .into_iter()
                    .filter(|(t0, t1)| t1 - t0 > CUTOFF)
                    .collect();
                if spans.is_empty() {
                    continue;
                }
                let forward = match pp.direction {
                    ParallelDirection::Zigzag => k.is_multiple_of(2),
                    ParallelDirection::Conventional => true,
                    // Climb: the next pass (+n) on the right of travel.
                    ParallelDirection::Climb => false,
                };
                k += 1;
                let mut pass_runs = Vec::new();
                for (si, (t0, t1)) in spans.into_iter().enumerate() {
                    for pts in pl.cut_runs(o, u, t0, t1) {
                        pass_runs.push(Run {
                            pts,
                            pass: k,
                            span: si,
                        });
                    }
                }
                if !forward {
                    pass_runs.reverse();
                    for r in &mut pass_runs {
                        r.pts.reverse();
                    }
                }
                runs.extend(pass_runs);
            }
        }
        if runs.is_empty() {
            diags.push(Diagnostic::warning(
                "cam.parallel.empty",
                "no part of the region is cut: check the slope limits, heights and boundary",
            ));
        }
        let lk = Linker {
            pl: &pl,
            region: &region,
            h: &h,
            retract: p.linking.retract,
            safety: p.linking.safety_distance,
            diameter: tool.diameter,
            lead_in: p.linking.lead_in,
            lead_out: p.linking.lead_out,
        };
        let (passes, dropped) = lk.link(&runs);
        if dropped {
            diags.push(Diagnostic::warning(
                "cam.parallel.lead.dropped",
                "a lead would gouge the model and was left out",
            ));
        }
        let passes = finish(
            passes,
            p.passes.output_arcs,
            p.passes.max_segment_length,
            tol,
        );
        let result = ToolpathResult::new(passes).map_err(CamError::Path)?;
        Ok(Generated {
            result,
            diagnostics: tag(diags, tp),
        })
    }
}

/// A cut stretch of one pass: CL points in travel order.
#[derive(Clone, Debug)]
struct Run {
    pts: Vec<Point3>,
    /// Pass number (1-based) and span of the region along it.
    pass: usize,
    span: usize,
}

/// A CL sample at parameter `t` along a drive line.
#[derive(Copy, Clone, Debug)]
struct Sample {
    t: f64,
    z: f64,
    cut: bool,
}

/// CL points along drive lines: drops, classification and adaptive
/// sampling.
struct Sampler<'a> {
    q: &'a SurfaceQuery<'a>,
    h: &'a ResolvedHeights,
    min_slope: f64,
    max_slope: f64,
    tol: f64,
    max_step: f64,
}

impl Sampler<'_> {
    /// CL tip at `p` (raised to `bottom`) and whether it is cut.
    fn cl(&self, p: Point2) -> (f64, bool) {
        let hit = self.q.drop(p.x(), p.y());
        let (z, slope, drive) = match hit {
            Some(Hit { tip, slope, drive }) if tip >= self.h.bottom => (tip, slope, drive),
            // Nothing under the tool, or below the bottom: at the bottom.
            _ => (self.h.bottom, 0.0, true),
        };
        // Slope limits are inclusive; rounding at 0° and 90° is forgiven.
        let eps = 1e-12;
        let cut = drive
            && slope >= self.min_slope - eps
            && slope <= self.max_slope + eps
            && z <= self.h.top + ABOVE_TOP;
        (z, cut)
    }

    fn sample(&self, o: Point2, u: Vec2, t: f64) -> Sample {
        let (z, cut) = self.cl(o + u * t);
        Sample { t, z, cut }
    }

    /// Samples of the line `o + t·u` over `[t0, t1]` in order: chords
    /// within `tol/4` of the drop and at most `max_step` long, with the
    /// cut-off points between cut and uncut stretches to [`CUTOFF`].
    fn samples(&self, o: Point2, u: Vec2, t0: f64, t1: f64) -> Vec<Sample> {
        let a = self.sample(o, u, t0);
        let b = self.sample(o, u, t1);
        let mut out = vec![a];
        let mut stack = vec![(a, b)];
        while let Some((a, b)) = stack.pop() {
            let len = b.t - a.t;
            if a.cut != b.cut {
                if len <= CUTOFF {
                    out.push(b);
                    continue;
                }
                let m = self.sample(o, u, 0.5 * (a.t + b.t));
                stack.push((m, b));
                stack.push((a, m));
                continue;
            }
            if len <= CUTOFF {
                out.push(b);
                continue;
            }
            let long = len > self.max_step;
            if !a.cut && !long {
                out.push(b);
                continue;
            }
            let m = self.sample(o, u, 0.5 * (a.t + b.t));
            let dev = (m.z - 0.5 * (a.z + b.z)).abs();
            // Stop splitting a vertical step (a flat tool over a wall).
            let split = long || m.cut != a.cut || (dev > 0.25 * self.tol && len > 1e-6);
            if split {
                stack.push((m, b));
                stack.push((a, m));
            } else {
                out.push(b);
            }
        }
        out
    }

    /// The cut stretches of `[t0, t1]` as CL polylines, collinear points
    /// merged within `tol/4`. Uncut gaps shorter than `tol` are bridged
    /// along the CL (rounding at a limit), and stretches shorter than
    /// `tol` are dropped.
    fn cut_runs(&self, o: Point2, u: Vec2, t0: f64, t1: f64) -> Vec<Vec<Point3>> {
        let pt = |s: &Sample| {
            let p = o + u * s.t;
            Point3::new(p.x(), p.y(), s.z)
        };
        let mut runs = Vec::new();
        let mut cur: Vec<Point3> = Vec::new();
        let mut last_cut = f64::NEG_INFINITY;
        let mut gap: Vec<Point3> = Vec::new();
        for s in self.samples(o, u, t0, t1) {
            if s.cut {
                if !gap.is_empty() && s.t - last_cut > self.tol {
                    runs.push(std::mem::take(&mut cur));
                } else {
                    cur.append(&mut gap);
                }
                gap.clear();
                cur.push(pt(&s));
                last_cut = s.t;
            } else if !cur.is_empty() {
                gap.push(pt(&s));
            }
        }
        if !cur.is_empty() {
            runs.push(cur);
        }
        runs.into_iter()
            .filter(|r| r.len() >= 2 && (r[r.len() - 1] - r[0]).length() > self.tol)
            .map(|r| merge(r, 0.25 * self.tol))
            .collect()
    }

    /// The surface-following link from `a` to `b`, if every point of it is
    /// cut.
    fn follow(&self, a: Point3, b: Point3) -> Option<Vec<Point3>> {
        let (pa, pb) = (Point2::new(a.x(), a.y()), Point2::new(b.x(), b.y()));
        let d = pb - pa;
        let len = d.length();
        if len <= CUTOFF {
            return Some(vec![a, b]);
        }
        let u = d * (1.0 / len);
        let s = self.samples(pa, u, 0.0, len);
        if !s.iter().all(|x| x.cut) {
            return None;
        }
        let mut pts: Vec<Point3> = s
            .iter()
            .map(|x| {
                let p = pa + u * x.t;
                Point3::new(p.x(), p.y(), x.z)
            })
            .collect();
        let k = pts.len() - 1;
        pts[0] = a;
        pts[k] = b;
        Some(merge(pts, 0.25 * self.tol))
    }
}

/// Drops points that lie within `tol` of the chord between their kept
/// neighbours (greedy).
fn merge(pts: Vec<Point3>, tol: f64) -> Vec<Point3> {
    if pts.len() <= 2 {
        return pts;
    }
    let mut out = vec![pts[0]];
    let mut i = 0;
    while i < pts.len() - 1 {
        let mut j = i + 1;
        while j + 1 < pts.len()
            && (i + 1..=j).all(|k| dist_to_seg(pts[k], pts[i], pts[j + 1]) <= tol)
        {
            j += 1;
        }
        out.push(pts[j]);
        i = j;
    }
    out
}

fn dist_to_seg(p: Point3, a: Point3, b: Point3) -> f64 {
    let d = b - a;
    let l2 = d.dot(d);
    let t = if l2 > 0.0 {
        ((p - a).dot(d) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + d * t - p).length()
}

/// Links cut runs (D08 §8): the approach from clearance, stay-down links
/// that follow the surface, lifts over uncut stretches, vertical leads,
/// and the final retract.
struct Linker<'a> {
    pl: &'a Sampler<'a>,
    region: &'a Region2,
    h: &'a ResolvedHeights,
    retract: RetractPolicy,
    safety: f64,
    diameter: f64,
    lead_in: Lead,
    lead_out: Lead,
}

impl Linker<'_> {
    fn link(&self, runs: &[Run]) -> (Vec<fluut_path::Pass>, bool) {
        let mut out = Out::new(self.pl.tol);
        let mut dropped = false;
        // stays[i]: the stay-down link into run i, if any.
        let stays: Vec<Option<Vec<Point3>>> = (0..runs.len())
            .map(|i| {
                i.checked_sub(1)
                    .and_then(|j| self.stay_down(&runs[j], &runs[i]))
            })
            .collect();
        for (i, run) in runs.iter().enumerate() {
            let start = run.pts[0];
            match (out.cur(), &stays[i]) {
                (Some(_), Some(path)) => {
                    for q in &path[1..] {
                        out.line(*q, Mv::Link);
                    }
                    out.pass(PassKind::Link, None);
                }
                (cur, _) => {
                    let lead = match self.lead(run, true) {
                        Some(l) if self.gouges(&l) => {
                            dropped = true;
                            None
                        }
                        l => l,
                    };
                    let entry = lead.as_ref().map_or(start, |l| l.start);
                    match cur {
                        None => {
                            out.start(Point3::new(entry.x(), entry.y(), self.h.clearance));
                        }
                        Some(e) => {
                            let hgt = self.link_height(e, entry);
                            out.line(Point3::new(e.x(), e.y(), hgt), Mv::Retract);
                            out.line(Point3::new(entry.x(), entry.y(), hgt), Mv::Rapid);
                            out.pass(PassKind::Link, None);
                        }
                    }
                    let z_now = out.cur().map_or(entry.z(), |c| c.z());
                    let rapid_to = (entry.z() + self.safety).max(z_now.min(self.h.feed));
                    if rapid_to < z_now {
                        out.line(Point3::new(entry.x(), entry.y(), rapid_to), Mv::Rapid);
                    }
                    if rapid_to > entry.z() {
                        out.line(entry, Mv::Plunge);
                    }
                    out.pass(PassKind::Ramp, None);
                    if let Some(l) = lead {
                        l.emit(&mut out, Mv::LeadIn);
                    }
                }
            }
            for q in &run.pts[1..] {
                out.line(*q, Mv::Cut);
            }
            // A lead-out unless the next run is joined by a stay-down link.
            let next_stays = stays.get(i + 1).is_some_and(Option::is_some);
            if !next_stays && let Some(l) = self.lead(run, false) {
                if self.gouges(&l) {
                    dropped = true;
                } else {
                    l.emit(&mut out, Mv::LeadOut);
                }
            }
            out.pass(PassKind::Cut, None);
        }
        if let Some(e) = out.cur() {
            out.line(Point3::new(e.x(), e.y(), self.h.clearance), Mv::Retract);
            out.pass(PassKind::Retract, None);
        }
        (out.passes, dropped)
    }

    /// The surface-following link from the end of `a` to the start of `b`,
    /// when the policy allows it: a different pass, a straight link inside
    /// the region no longer than allowed, and every point of it cut.
    fn stay_down(&self, a: &Run, b: &Run) -> Option<Vec<Point3>> {
        if a.pass == b.pass && a.span == b.span {
            return None;
        }
        let max_len = match self.retract {
            RetractPolicy::Full => return None,
            RetractPolicy::Minimum => self.diameter,
            RetractPolicy::StayDown => f64::INFINITY,
        };
        let (e, s) = (a.pts[a.pts.len() - 1], b.pts[0]);
        let (pe, ps) = (Point2::new(e.x(), e.y()), Point2::new(s.x(), s.y()));
        if (ps - pe).length() > max_len || !inside(self.region, pe, ps) {
            return None;
        }
        self.pl.follow(e, s)
    }

    /// Link height from `a` to `b` (D08 §8.2): `safety_distance` above the
    /// highest geometry the tool can meet, capped at `retract` for
    /// `minimum` and `stay_down`; `retract` for `full`. Never below the
    /// geometry bound or either end.
    fn link_height(&self, a: Point3, b: Point3) -> f64 {
        let bound = self
            .pl
            .q
            .link_bound(Point2::new(a.x(), a.y()), Point2::new(b.x(), b.y()))
            .max(a.z())
            .max(b.z());
        let need = bound + self.safety;
        let hgt = match self.retract {
            RetractPolicy::Full => self.h.retract,
            RetractPolicy::Minimum | RetractPolicy::StayDown => need.min(self.h.retract),
        };
        if hgt < bound { need } else { hgt }
    }

    /// The lead into (`into`) or out of a run, in the vertical plane of the
    /// pass and tangent to the run's first (last) segment: a straight
    /// extension of that segment, or a quarter arc above it after (before)
    /// an optional straight along the arc's end tangent.
    fn lead(&self, run: &Run, into: bool) -> Option<Lead3> {
        let lead = if into { &self.lead_in } else { &self.lead_out };
        let n = run.pts.len();
        let (p, q) = if into {
            (run.pts[0], run.pts[1])
        } else {
            (run.pts[n - 1], run.pts[n - 2])
        };
        // Direction of travel at the run end, and the plane's normal.
        let t = (if into { q - p } else { p - q }).normalized()?.get();
        let normal = t.cross(Vec3::new(0.0, 0.0, 1.0)).normalized()?;
        // Up in the plane of the pass, perpendicular to the travel.
        let m = normal.get().cross(t);
        match lead.kind {
            LeadKind::None => None,
            LeadKind::Linear if lead.length > 0.0 => Some(if into {
                Lead3::line(p - t * lead.length, p)
            } else {
                Lead3::line(p, p + t * lead.length)
            }),
            LeadKind::Linear => None,
            LeadKind::Arc => {
                let r = lead.radius;
                let c = p + m * r;
                let arc = (c, normal, FRAC_PI_2);
                let straight = (lead.length > 0.0).then_some(lead.length);
                Some(if into {
                    let s = c - t * r;
                    let mut l = Lead3 {
                        start: straight.map_or(s, |len| s + m * len),
                        pieces: Vec::new(),
                    };
                    if straight.is_some() {
                        l.pieces.push(Piece::Line(s));
                    }
                    l.pieces.push(Piece::Arc(p, arc));
                    l
                } else {
                    let e = c + t * r;
                    let mut l = Lead3 {
                        start: p,
                        pieces: vec![Piece::Arc(e, arc)],
                    };
                    if let Some(len) = straight {
                        l.pieces.push(Piece::Line(e + m * len));
                    }
                    l
                })
            }
        }
    }

    /// Whether any point of the lead is below the drop by more than the
    /// tolerance.
    fn gouges(&self, l: &Lead3) -> bool {
        l.points(LEAD_SAMPLES).iter().any(|p| {
            self.pl
                .q
                .drop(p.x(), p.y())
                .is_some_and(|h| h.tip > p.z() + 0.5 * self.pl.tol)
        })
    }
}

/// A vertical lead: lines and arcs from `start`.
#[derive(Clone, Debug)]
struct Lead3 {
    start: Point3,
    pieces: Vec<Piece>,
}

#[derive(Clone, Copy, Debug)]
enum Piece {
    Line(Point3),
    /// To the point, about `(centre, normal, sweep)`.
    Arc(Point3, (Point3, UnitVec3, f64)),
}

impl Lead3 {
    fn line(a: Point3, b: Point3) -> Lead3 {
        Lead3 {
            start: a,
            pieces: vec![Piece::Line(b)],
        }
    }

    fn emit(&self, out: &mut Out, mv: Mv) {
        for p in &self.pieces {
            match *p {
                Piece::Line(b) => out.line(b, mv),
                Piece::Arc(b, (c, n, sw)) => out.arc(b, c, n, sw, mv),
            }
        }
    }

    /// `k` points along each piece, ends included.
    fn points(&self, k: usize) -> Vec<Point3> {
        let mut out = vec![self.start];
        let mut a = self.start;
        for p in &self.pieces {
            match *p {
                Piece::Line(b) => {
                    out.extend((1..=k).map(|i| a.lerp(b, i as f64 / k as f64)));
                    a = b;
                }
                Piece::Arc(b, (c, n, sw)) => {
                    let r0 = a - c;
                    let w = n.get().cross(r0);
                    for i in 1..=k {
                        let (s, co) = libm::sincos(sw * i as f64 / k as f64);
                        out.push(c + r0 * co + w * s);
                    }
                    a = b;
                }
            }
        }
        out
    }
}

/// Whether the segment `p`–`q` lies in the closed region.
fn inside(region: &Region2, p: Point2, q: Point2) -> bool {
    if region.contains(p) == Inside::Out || region.contains(q) == Inside::Out {
        return false;
    }
    let d = q - p;
    if d.length() <= CUTOFF {
        return true;
    }
    region
        .clip_line(p, d)
        .iter()
        .any(|&(t0, t1)| t0 <= 1e-9 && t1 >= 1.0 - 1e-9)
}
