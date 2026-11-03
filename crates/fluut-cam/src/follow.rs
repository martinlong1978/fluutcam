// SPDX-License-Identifier: MIT OR Apache-2.0

//! Building blocks of the 3-D finishing types that follow the surface
//! (D08 §4, §8; D09 §4): the shared setup (face sets, heights, the drop
//! geometry with the stock to leave, the region), adaptive CL sampling
//! along a drive curve with exact cut-off points, cut runs, and linking
//! with stay-down links, lifts and vertical leads.
//!
//! First written for `parallel` (M3); shared since M6.

use crate::chain::chain_pieces;
use crate::cutter::{BuildOptions, Cutter, QueryGeometry};
use crate::drive::{curve_pieces, faces, nest};
use crate::geometry::{BodyGeom, face_z_range};
use crate::link::{StockFloor, stock_rapid_floor};
use crate::moves::{Mv, Out};
use crate::region::{Inside, Loop2, Region2, Seg2};
use crate::scallop::{FaceCurvature, face_curvature, is_mesh, scallop_stepover};
use crate::stock::StockFrame;
use crate::surface::{FaceSets, Hit, SurfaceQuery};
use crate::toolpath::{CamError, StrategyInput, prepare, tag};
use fluut_geom::{Point2, Point3, UnitVec3, Vec2, Vec3};
use fluut_model::resolve::{
    BoundarySide, Containment, Lead, LeadKind, ResolvedHeights, ResolvedParams, RetractPolicy,
    resolve,
};
use fluut_model::tools::ToolShape;
use fluut_model::{Diagnostic, Sel};
use fluut_path::PassKind;
use fluut_topo::BrepBody;
use std::collections::BTreeSet;
use std::f64::consts::FRAC_PI_2;

/// Cut-off points are found to this distance along the pass, mm.
pub const CUTOFF: f64 = 1e-7;
/// A tip this far above `top` counts as above it, mm.
pub const ABOVE_TOP: f64 = 1e-6;
/// Samples per lead for the gouge check.
const LEAD_SAMPLES: usize = 32;

/// A drive face: body, face index, id.
pub type DriveFace<'a> = (&'a BrepBody, u32, String);

/// Everything a surface-following type needs before its drive pattern.
#[derive(Debug)]
pub struct Setup<'a> {
    pub params: ResolvedParams,
    pub heights: ResolvedHeights,
    /// Warnings so far.
    pub diags: Vec<Diagnostic>,
    /// The tool as a cutter (stock to leave not applied).
    pub cutter: Cutter,
    /// Drops with the stock to leave applied.
    pub q: SurfaceQuery<'a>,
    pub drive: Vec<DriveFace<'a>>,
}

/// Face sets, heights, validation, the tool shape and the drop geometry
/// of `ty` (D15 §5, D15b §5): empty drive faces mean every face. Errors
/// are tagged with the toolpath.
pub fn setup<'a>(input: &StrategyInput<'a>, ty: &str) -> Result<Setup<'a>, CamError> {
    let tp = input.toolpath;
    let fail = |d: Vec<Diagnostic>| CamError::Invalid(tag(d, tp));
    let r0 = resolve(&tp.params, input.tool, input.material);
    let g = input.geometry;
    let geo = &r0.geometry;
    let chord = r0.passes.machining_tolerance.max(1e-4) / 4.0;
    let excluded =
        faces(g, &geo.excluded_faces, &[], "geometry.excluded_faces").map_err(|d| fail(vec![d]))?;
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
            faces(g, c, &geo.excluded_faces, "geometry.check_faces").map_err(|d| fail(vec![d]))?,
        ),
    };
    let ids = |v: &[DriveFace]| -> BTreeSet<String> { v.iter().map(|x| x.2.clone()).collect() };
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
    let tool = input.tool;
    let cutter = match tool.shape {
        ToolShape::Flat | ToolShape::Ball | ToolShape::Bull => {
            Cutter::new(tool.diameter, tool.corner_radius())
        }
        ToolShape::V | ToolShape::Taper | ToolShape::Drill => None,
    };
    let Some(cutter) = cutter else {
        diags.push(
            Diagnostic::error(
                &format!("cam.{ty}.tool_shape"),
                format!("{ty} needs a flat, ball or bull-nose tool"),
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
    let stl = prep.params.passes.stock_to_leave;
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
    Ok(Setup {
        params: prep.params,
        heights: prep.heights,
        diags,
        cutter,
        q,
        drive,
    })
}

impl Setup<'_> {
    /// Worst curvature of the drive faces, and whether any drive body is a
    /// mesh (every face a planar triangle).
    pub fn curvature(&self, input: &StrategyInput) -> (FaceCurvature, bool) {
        let mut curv = FaceCurvature::default();
        let mut mesh = false;
        for b in &input.geometry.bodies {
            let BodyGeom::Brep(body) = &b.geom else {
                continue;
            };
            let mine: Vec<u32> = self
                .drive
                .iter()
                .filter(|x| std::ptr::eq(x.0, body))
                .map(|x| x.1)
                .collect();
            if mine.is_empty() {
                continue;
            }
            mesh |= is_mesh(body);
            let qg =
                QueryGeometry::with_faces(body, &BuildOptions::default(), |f| mine.contains(&f));
            for &f in &mine {
                curv = curv.max(face_curvature(&qg, f));
            }
        }
        (curv, mesh)
    }

    /// The stepover: from the scallop height on the worst convex
    /// curvature (D09 §4.3), else the given one. Pushes
    /// `param.passes.stepover.max` and fails when it is not in (0, D].
    pub fn stepover(
        &mut self,
        input: &StrategyInput,
        scallop_height: Option<f64>,
        stepover: Option<f64>,
    ) -> Result<f64, CamError> {
        let tool = input.tool;
        let s = match scallop_height {
            Some(sh) => {
                let (curv, _) = self.curvature(input);
                scallop_stepover(sh, tool.corner_radius(), curv.convex)
            }
            None => stepover.unwrap_or(0.0),
        };
        if s.is_nan() || s <= 0.0 || s > tool.diameter {
            self.diags.push(
                Diagnostic::error(
                    "param.passes.stepover.max",
                    format!(
                        "stepover {s} is not above 0 and at most the tool diameter {}",
                        tool.diameter
                    ),
                )
                .at("passes.stepover"),
            );
            return Err(self.fail(input));
        }
        Ok(s)
    }

    /// The diagnostics so far as an error.
    pub fn fail(&mut self, input: &StrategyInput) -> CamError {
        CamError::Invalid(tag(std::mem::take(&mut self.diags), input.toolpath))
    }

    /// The tool-centre region: the drive faces' XY box grown by `grow`,
    /// limited by the `boundary` (D15 §2.3.2). `None` (with an error
    /// pushed) when the drive faces have no extent or a boundary chain is
    /// open.
    pub fn region(&mut self, input: &StrategyInput, grow: f64) -> Result<Region2, CamError> {
        let g = input.geometry;
        let Some((lo, hi)) = self.q.drive_box() else {
            self.diags.push(Diagnostic::error(
                "cam.geometry.drive.empty",
                "the drive faces have no extent",
            ));
            return Err(self.fail(input));
        };
        let mut region = Region2::rect(lo.x() - grow, lo.y() - grow, hi.x() + grow, hi.y() + grow);
        if let Some(b) = self.params.geometry.boundary.clone() {
            let chord = self.params.passes.machining_tolerance.max(1e-4) / 4.0;
            let join = g.tol_model().max(1e-5);
            let pieces = match curve_pieces(g, &b.curves, chord, "geometry.boundary.curves") {
                Ok(p) => p,
                Err(d) => {
                    self.diags.push(d);
                    return Err(self.fail(input));
                }
            };
            let bc = chain_pieces(pieces, join);
            if bc.iter().any(|c| !c.path.closed) {
                self.diags.push(
                    Diagnostic::error(
                        "param.geometry.boundary.open",
                        "a boundary chain is not closed",
                    )
                    .at("geometry.boundary.curves"),
                );
                return Err(self.fail(input));
            }
            let br = nest(bc.iter().map(|c| c.ccw_loop()).collect());
            let r = input.tool.radius();
            let grow = match b.containment {
                Containment::Inside => -r,
                Containment::Centre => 0.0,
                Containment::Outside => r,
            };
            region = match b.side {
                BoundarySide::Inside => region.intersect(&br.offset(grow)),
                BoundarySide::Outside => region.subtract(&br.offset(-grow)),
            };
        }
        Ok(region)
    }
}

/// A planar drive curve, by parameter `t`.
pub trait Drive {
    fn at(&self, t: f64) -> Point2;
    /// XY length between two parameters (step and cut-off tests).
    fn len(&self, ta: f64, tb: f64) -> f64;
    /// Largest XY distance of the curve between `ta` and `tb` from its
    /// chord; 0 for lines.
    fn sagitta(&self, _ta: f64, _tb: f64) -> f64 {
        0.0
    }
    /// Parameters where the curve has a corner: always samples.
    fn breaks(&self) -> Vec<f64> {
        Vec::new()
    }
}

/// The line `o + t·u`, `u` a unit vector: `t` is the length.
#[derive(Copy, Clone, Debug)]
pub struct LineDrive {
    pub o: Point2,
    pub u: Vec2,
}

impl Drive for LineDrive {
    fn at(&self, t: f64) -> Point2 {
        self.o + self.u * t
    }

    fn len(&self, ta: f64, tb: f64) -> f64 {
        tb - ta
    }
}

/// A closed loop of lines and arcs, by length from its first vertex.
#[derive(Clone, Debug)]
pub struct LoopDrive {
    segs: Vec<Seg2>,
    /// Length at the start of each segment, and the total at the end.
    cum: Vec<f64>,
}

impl LoopDrive {
    pub fn new(l: &Loop2) -> LoopDrive {
        let segs: Vec<Seg2> = l.segs().collect();
        let mut cum = vec![0.0];
        for sg in &segs {
            cum.push(cum[cum.len() - 1] + sg.length());
        }
        LoopDrive { segs, cum }
    }

    pub fn length(&self) -> f64 {
        self.cum[self.cum.len() - 1]
    }
}

impl Drive for LoopDrive {
    fn at(&self, t: f64) -> Point2 {
        let n = self.segs.len();
        if n == 0 {
            return Point2::new(0.0, 0.0);
        }
        let i = self.cum[1..n].partition_point(|&c| c <= t).min(n - 1);
        let sg = &self.segs[i];
        let len = self.cum[i + 1] - self.cum[i];
        if len <= 0.0 {
            return sg.a;
        }
        // Segment ends exactly at the vertices.
        if t >= self.cum[i + 1] {
            return sg.b;
        }
        sg.point_at(((t - self.cum[i]) / len).clamp(0.0, 1.0))
    }

    fn len(&self, ta: f64, tb: f64) -> f64 {
        tb - ta
    }

    fn sagitta(&self, ta: f64, tb: f64) -> f64 {
        curve_sagitta(self, ta, tb)
    }

    fn breaks(&self) -> Vec<f64> {
        self.cum[1..self.cum.len() - 1].to_vec()
    }
}

/// Distance of the curve's middle point from the chord's middle: the
/// sagitta for arcs and smooth curves.
pub fn curve_sagitta(d: &impl Drive, ta: f64, tb: f64) -> f64 {
    let (a, b) = (d.at(ta), d.at(tb));
    let m = d.at(0.5 * (ta + tb));
    (m - (a + (b - a) * 0.5)).length()
}

/// A cut stretch: CL points in travel order.
#[derive(Clone, Debug)]
pub struct Run {
    pub pts: Vec<Point3>,
    /// Pass number (1-based) and span of the region along it. Runs of the
    /// same pass and span are never joined by a stay-down link.
    pub pass: usize,
    pub span: usize,
    /// A constant-Z run (waterline): stay-down links into or out of it are
    /// straight, not surface-following.
    pub level: bool,
}

/// A CL sample at parameter `t` along a drive curve.
#[derive(Copy, Clone, Debug)]
pub struct Sample {
    pub t: f64,
    pub z: f64,
    pub cut: bool,
    /// Slope of the contact (0 where nothing is under the tool).
    pub slope: f64,
}

/// CL points along drive curves: drops, classification and adaptive
/// sampling (D08 §4.2).
#[derive(Debug)]
pub struct Sampler<'a> {
    pub q: &'a SurfaceQuery<'a>,
    pub h: &'a ResolvedHeights,
    pub min_slope: f64,
    pub max_slope: f64,
    pub tol: f64,
    pub max_step: f64,
}

impl Sampler<'_> {
    /// CL tip at `p` (raised to `bottom`) and whether it is cut.
    pub fn cl(&self, p: Point2) -> (f64, bool) {
        let (z, cut, _) = self.cl_slope(p);
        (z, cut)
    }

    /// As [`Sampler::cl`], with the contact slope.
    pub fn cl_slope(&self, p: Point2) -> (f64, bool, f64) {
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
        (z, cut, slope)
    }

    fn sample(&self, d: &impl Drive, t: f64) -> Sample {
        let (z, cut, slope) = self.cl_slope(d.at(t));
        Sample { t, z, cut, slope }
    }

    /// Samples of `d` over `[t0, t1]` in order: chords within `tol/4` of
    /// the drop (and of a curved drive in XY, and not below the drop at
    /// their middle by more than `tol/4`) and at most `max_step` long, with
    /// the cut-off points between cut and uncut stretches to [`CUTOFF`].
    pub fn samples(&self, d: &impl Drive, t0: f64, t1: f64) -> Vec<Sample> {
        let bs: Vec<f64> = d
            .breaks()
            .into_iter()
            .filter(|&b| d.len(t0, b) > CUTOFF && d.len(b, t1) > CUTOFF)
            .collect();
        if bs.is_empty() {
            return self.samples_on(d, t0, t1);
        }
        let mut out: Vec<Sample> = Vec::new();
        let mut a = t0;
        for b in bs.into_iter().chain(std::iter::once(t1)) {
            let mut s = self.samples_on(d, a, b);
            if !out.is_empty() {
                s.remove(0);
            }
            out.extend(s);
            a = b;
        }
        out
    }

    fn samples_on(&self, d: &impl Drive, t0: f64, t1: f64) -> Vec<Sample> {
        let a = self.sample(d, t0);
        let b = self.sample(d, t1);
        let mut out = vec![a];
        let mut stack = vec![(a, b)];
        while let Some((a, b)) = stack.pop() {
            let len = d.len(a.t, b.t);
            if a.cut != b.cut {
                if len <= CUTOFF {
                    out.push(b);
                    continue;
                }
                let m = self.sample(d, 0.5 * (a.t + b.t));
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
            let m = self.sample(d, 0.5 * (a.t + b.t));
            let dev = (m.z - 0.5 * (a.z + b.z)).abs();
            // A curved drive: the chord leaves the curve sideways, so the
            // pattern error is the sagitta and the gouge is the drop at the
            // chord's middle above the chord.
            let curved = || {
                let sag = d.sagitta(a.t, b.t);
                sag > 0.25 * self.tol
                    || (sag > 0.0 && {
                        let cm = d.at(a.t) + (d.at(b.t) - d.at(a.t)) * 0.5;
                        self.cl(cm).0 - 0.5 * (a.z + b.z) > 0.25 * self.tol
                    })
            };
            // Stop splitting a vertical step (a flat tool over a wall).
            let split = long
                || m.cut != a.cut
                || (dev > 0.25 * self.tol && len > 1e-6)
                || (len > 1e-6 && curved());
            if split {
                stack.push((m, b));
                stack.push((a, m));
            } else {
                out.push(b);
            }
        }
        out
    }

    /// The cut stretches of `d` over `[t0, t1]` as CL polylines, collinear
    /// points merged within `tol/4`. Uncut gaps shorter than `tol` are
    /// bridged along the CL (rounding at a limit), and stretches shorter
    /// than `tol` are dropped.
    pub fn cut_runs(&self, d: &impl Drive, t0: f64, t1: f64) -> Vec<Vec<Point3>> {
        self.cut_runs_slope(d, t0, t1).0
    }

    /// As [`Sampler::cut_runs`], with the largest slope of the cut samples
    /// (0 when none is cut).
    pub fn cut_runs_slope(&self, d: &impl Drive, t0: f64, t1: f64) -> (Vec<Vec<Point3>>, f64) {
        let mut steepest: f64 = 0.0;
        let pt = |s: &Sample| {
            let p = d.at(s.t);
            Point3::new(p.x(), p.y(), s.z)
        };
        let mut runs = Vec::new();
        let mut cur: Vec<Point3> = Vec::new();
        let mut last_cut = f64::NEG_INFINITY;
        let mut gap: Vec<Point3> = Vec::new();
        for s in self.samples(d, t0, t1) {
            if s.cut {
                steepest = steepest.max(s.slope);
                if !gap.is_empty() && d.len(last_cut, s.t) > self.tol {
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
        // A closed run (a loop drive cut all round) starts where it ends.
        let closed = |r: &Vec<Point3>| {
            let (a, b) = (r[0], r[r.len() - 1]);
            libm::hypot(a.x() - b.x(), a.y() - b.y()) < 1e-9
                && r.windows(2).map(|w| (w[1] - w[0]).length()).sum::<f64>() > self.tol
        };
        let runs = runs
            .into_iter()
            .filter(|r| r.len() >= 2 && ((r[r.len() - 1] - r[0]).length() > self.tol || closed(r)))
            .map(|r| merge(r, 0.25 * self.tol))
            .collect();
        (runs, steepest)
    }

    /// The surface-following link from `a` to `b`, if every point of it is
    /// cut.
    pub fn follow(&self, a: Point3, b: Point3) -> Option<Vec<Point3>> {
        let (pa, pb) = (Point2::new(a.x(), a.y()), Point2::new(b.x(), b.y()));
        let d = pb - pa;
        let len = d.length();
        if len <= CUTOFF {
            return Some(vec![a, b]);
        }
        let u = d * (1.0 / len);
        let line = LineDrive { o: pa, u };
        let s = self.samples(&line, 0.0, len);
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
pub fn merge(pts: Vec<Point3>, tol: f64) -> Vec<Point3> {
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

pub fn dist_to_seg(p: Point3, a: Point3, b: Point3) -> f64 {
    let d = b - a;
    let l2 = d.dot(d);
    let t = if l2 > 0.0 {
        ((p - a).dot(d) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + d * t - p).length()
}

/// Links cut runs (D08 §8): the approach from clearance, stay-down links,
/// lifts over uncut stretches, vertical leads, and the final retract.
pub struct Linker<'a> {
    pub pl: &'a Sampler<'a>,
    pub region: &'a Region2,
    pub h: &'a ResolvedHeights,
    pub retract: RetractPolicy,
    pub safety: f64,
    pub diameter: f64,
    pub lead_in: Lead,
    pub lead_out: Lead,
    /// For level runs: whether the horizontal XY move from the first to the
    /// second point, at the first point's Z, stays clear of the material
    /// at that level. `None`: level runs never stay down.
    pub level_clear: Option<&'a dyn Fn(Point3, Point3) -> bool>,
    /// The top of the material: the stock top, or `top` if higher. Without
    /// the in-process stock a finishing toolpath cannot see what earlier
    /// toolpaths left, so rapids stay `safety` above it (D08 §8.2, the
    /// rapid floor of `link`).
    pub material_top: f64,
    /// The in-process stock, when the toolpath has it: lifts and descents
    /// then stay `safety` above the highest material within the tool's
    /// reach instead, never above what `material_top` gives.
    pub floor: Option<StockFloor<'a>>,
}

impl std::fmt::Debug for Linker<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Linker")
            .field("retract", &self.retract)
            .field("safety", &self.safety)
            .field("level_clear", &self.level_clear.is_some())
            .finish_non_exhaustive()
    }
}

impl Linker<'_> {
    /// The passes, and whether a lead was dropped because it would gouge.
    pub fn link(&self, runs: &[Run]) -> (Vec<fluut_path::Pass>, bool) {
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
                    // Rapid down to the rapid floor, then plunge.
                    let floor = stock_rapid_floor(
                        self.floor,
                        Point2::new(entry.x(), entry.y()),
                        self.h.feed,
                        self.safety,
                        self.h.feed.max(self.material_top + self.safety),
                    );
                    let rapid_to = (entry.z() + self.safety).max(z_now.min(floor));
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

    /// The stay-down link from the end of `a` to the start of `b`, when
    /// the policy allows it: a different pass, a straight link inside the
    /// region no longer than allowed, and either every point of the
    /// surface-following link cut (surface runs) or the straight 3-D move
    /// clear of the model (level runs).
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
        if a.level != b.level {
            return None;
        }
        if a.level {
            // Across at the upper level, then straight down: never up.
            let clear = self.level_clear?;
            if s.z() > e.z() + 1e-9 {
                return None;
            }
            let over = Point3::new(s.x(), s.y(), e.z());
            if !clear(e, over) {
                return None;
            }
            let mut path = vec![e];
            if (over - e).length() > CUTOFF {
                path.push(over);
            }
            if (s - over).length() > CUTOFF {
                path.push(s);
            }
            if path.len() == 1 {
                path.push(s);
            }
            return Some(path);
        }
        self.pl.follow(e, s)
    }

    /// Link height from `a` to `b` (D08 §8.2): `safety_distance` above the
    /// highest geometry the tool can meet and above the material top (the
    /// in-process material within reach, when known), capped at `retract` for `minimum` and `stay_down`; `retract` for
    /// `full`. Never below the geometry bound or either end.
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
            RetractPolicy::Minimum | RetractPolicy::StayDown => need
                .max(self.material_over(a, b) + self.safety)
                .min(self.h.retract),
        };
        if hgt < bound { need } else { hgt }
    }

    /// The highest material the tool meets on a link from `a` to `b`, lift
    /// and descent included: the in-process material within the tool's
    /// reach of the segment when known (and lower than `material_top`),
    /// else `material_top`. Negative infinity where it meets none.
    fn material_over(&self, a: Point3, b: Point3) -> f64 {
        let Some(f) = self.floor.filter(|f| f.stock.frame == StockFrame::Planar) else {
            return self.material_top;
        };
        let (pa, pb) = (Point2::new(a.x(), a.y()), Point2::new(b.x(), b.y()));
        f.stock
            .top_along(pa, pb, f.radius)
            .map_or(f64::NEG_INFINITY, |t| t.min(self.material_top))
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
pub fn inside(region: &Region2, p: Point2, q: Point2) -> bool {
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
