// SPDX-License-Identifier: MIT OR Apache-2.0

//! `scallop` (D09 §4.7, D15b §5.4): passes offset inward from the region
//! boundary over the surface at a constant 3-D stepover. Finishing type.
//!
//! Pass 0 is the region boundary, dropped onto the surface. Pass k+1 is
//! the region offset in XY by the running sum of steps, where the step
//! after pass k is s·cos θ, θ the steepest cut contact on passes k and
//! k+1: so the 3-D distance between passes stays within s (CH-18: one
//! conservative offset per pass, as `cavalier_contours` offsets
//! uniformly). A step reduced because the next pass is steeper is an
//! inserted pass (`cam.scallop.extra_passes`). The step is never below
//! s/10: on a vertical wall s·cos θ is zero.

use super::contour::link_runs;
use crate::follow::{LoopDrive, Run, Sampler, setup};
use crate::path2::Path2;
use crate::region::Region2;
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType};
use fluut_geom::{Point2, Point3};
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{CutDirection, OffsetOrder, RetractPolicy};

#[derive(Debug)]
pub struct Scallop;

/// Safety cap on passes.
const MAX_PASSES: usize = 10_000;
/// Re-checks of a step against the steeper next pass.
const STEP_TRIES: usize = 4;

impl ToolpathType for Scallop {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Scallop
    }

    fn role(&self) -> Role {
        Role::Finish
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        let mut st = setup(input, "scallop")?;
        let Some(sc) = st.params.scallop().cloned() else {
            return Err(CamError::Unavailable("not a scallop toolpath".into()));
        };
        let s = st.stepover(input, sc.scallop_height, sc.stepover)?;
        let region = st.region(input, 0.0)?;
        let h = st.heights;
        let pl = Sampler {
            q: &st.q,
            h: &h,
            min_slope: sc.min_slope_deg.to_radians(),
            max_slope: sc.max_slope_deg.to_radians(),
            tol: st.params.passes.machining_tolerance,
            max_step: 0.5 * input.tool.radius(),
        };
        let set = Loops {
            stepover: s,
            direction: sc.direction,
            cut_direction: sc.cut_direction,
            continuous: sc.continuous,
        };
        let (runs, extra) = set.runs(&pl, &region);
        let mut diags = std::mem::take(&mut st.diags);
        if extra {
            diags.push(Diagnostic::warning(
                "cam.scallop.extra_passes",
                "passes were inserted to keep the 3-D stepover",
            ));
        }
        if runs.is_empty() {
            diags.push(Diagnostic::warning(
                "cam.scallop.empty",
                "no part of the region is cut: check the slope limits, heights and boundary",
            ));
        }
        let retract = sc.continuous.then_some(RetractPolicy::StayDown);
        link_runs(&st, input, &region, &runs, diags, "scallop", None, retract)
    }
}

/// Scallop pass settings: `scallop`, or the shallow part of
/// `steep_and_shallow`.
#[derive(Clone, Debug)]
pub(crate) struct Loops {
    pub stepover: f64,
    pub direction: OffsetOrder,
    pub cut_direction: CutDirection,
    pub continuous: bool,
}

/// One pass: its loops' cut runs and the steepest cut contact.
struct Pass {
    /// Per loop.
    runs: Vec<Vec<Vec<Point3>>>,
    steepest: f64,
}

impl Loops {
    /// The cut runs of every pass in machining order, and whether passes
    /// were inserted.
    pub fn runs(&self, pl: &Sampler, region: &Region2) -> (Vec<Run>, bool) {
        let s = self.stepover;
        let step_of = |theta: f64| (s * libm::cos(theta)).max(0.1 * s);
        let mut passes: Vec<Pass> = Vec::new();
        let mut extra = false;
        let mut off = 0.0;
        let mut next: Option<Pass> = None;
        while passes.len() < MAX_PASSES {
            let cur = match next.take() {
                Some(p) => p,
                None => {
                    let reg = if off == 0.0 {
                        region.clone()
                    } else {
                        region.offset(-off)
                    };
                    if reg.is_empty() {
                        break;
                    }
                    self.pass(pl, &reg)
                }
            };
            let mut d = step_of(cur.steepest);
            for _ in 0..STEP_TRIES {
                let reg = region.offset(-(off + d));
                if reg.is_empty() {
                    break;
                }
                let p = self.pass(pl, &reg);
                let d2 = step_of(cur.steepest.max(p.steepest));
                if d2 >= d - 1e-12 {
                    next = Some(p);
                    break;
                }
                extra = true;
                d = d2;
            }
            passes.push(cur);
            off += d;
        }
        if self.direction == OffsetOrder::InsideOut {
            passes.reverse();
        }
        (self.order(passes), extra)
    }

    /// Samples the loops of the offset region `reg`, travelled so that the
    /// uncut side is on the right for climb (spindle M3).
    fn pass(&self, pl: &Sampler, reg: &Region2) -> Pass {
        // Region loops have the inside on the left. Outside in, the uncut
        // side is inside; inside out, it is outside.
        let inside_uncut = self.direction == OffsetOrder::OutsideIn;
        let climb = self.cut_direction == CutDirection::Climb;
        let flip = inside_uncut == climb;
        let mut steepest: f64 = 0.0;
        let mut runs = Vec::new();
        // Start each loop at its vertex nearest the region's lower left
        // corner: the offset library's start vertex is not translation
        // invariant.
        let (lo, _) = reg.bbox();
        for l in &reg.loops {
            let mut l = if flip { l.reversed() } else { l.clone() };
            if let Some(k) = (0..l.verts.len()).min_by(|&i, &j| {
                (l.verts[i].p - lo)
                    .length()
                    .total_cmp(&(l.verts[j].p - lo).length())
                    .then(i.cmp(&j))
            }) {
                l.verts.rotate_left(k);
            }
            let d = LoopDrive::new(&l);
            let (r, th) = pl.cut_runs_slope(&d, 0.0, d.length());
            steepest = steepest.max(th);
            runs.push(r);
        }
        Pass { runs, steepest }
    }

    /// Runs in order. Each closed loop starts at its point nearest the end
    /// of the previous one; `continuous` ends a fully cut loop one step
    /// early, so the stay-down link to the next loop blends across.
    fn order(&self, passes: Vec<Pass>) -> Vec<Run> {
        let mut out: Vec<Run> = Vec::new();
        let mut at: Option<Point2> = None;
        let mut id = 0;
        for p in passes {
            for rs in p.runs {
                id += 1;
                let full = rs.len() == 1 && rs[0].first() == rs[0].last() && rs[0].len() > 2;
                let mut rs = rs;
                if full && let Some(q) = at {
                    rs = vec![rotate(&rs[0], q)];
                }
                if full && self.continuous {
                    rs = vec![trim_end(&rs[0], self.stepover)];
                }
                for r in rs {
                    at = r.last().map(|e| Point2::new(e.x(), e.y()));
                    out.push(Run {
                        pts: r,
                        pass: id,
                        span: 0,
                        level: false,
                    });
                }
            }
        }
        out
    }
}

/// A closed CL polyline starting at its point nearest `q` in XY (the
/// nearest point of a segment, inserted).
fn rotate(r: &[Point3], q: Point2) -> Vec<Point3> {
    let xy: Vec<Point2> = r.iter().map(|p| Point2::new(p.x(), p.y())).collect();
    let path = Path2::open(
        xy.windows(2)
            .map(|w| crate::region::Seg2 {
                a: w[0],
                b: w[1],
                bulge: 0.0,
            })
            .collect(),
    );
    let (at, _) = path.closest(q);
    // Split at arc length `at`.
    let mut acc = 0.0;
    for i in 0..r.len() - 1 {
        let len = (xy[i + 1] - xy[i]).length();
        if at <= acc + len || i == r.len() - 2 {
            let f = if len > 0.0 {
                ((at - acc) / len).clamp(0.0, 1.0)
            } else {
                0.0
            };
            // At (within 1e-9 of) a vertex: start there, not at a sliver.
            let (head, tail) = if f * len <= 1e-9 {
                (r[i], i)
            } else if (1.0 - f) * len <= 1e-9 {
                (r[i + 1], i + 1)
            } else {
                (r[i].lerp(r[i + 1], f), i)
            };
            let mut v = vec![head];
            v.extend_from_slice(&r[tail + 1..]);
            v.extend_from_slice(&r[1..=tail]);
            if tail == 0 || v[v.len() - 1] != head {
                v.push(head);
            }
            v.dedup();
            return v;
        }
        acc += len;
    }
    r.to_vec()
}

/// A closed CL polyline without its last `len` (XY), kept when shorter.
fn trim_end(r: &[Point3], len: f64) -> Vec<Point3> {
    let total: f64 = r
        .windows(2)
        .map(|w| libm::hypot(w[1].x() - w[0].x(), w[1].y() - w[0].y()))
        .sum();
    if total <= 2.0 * len {
        return r.to_vec();
    }
    let keep = total - len;
    let mut acc = 0.0;
    let mut out = vec![r[0]];
    for w in r.windows(2) {
        let l = libm::hypot(w[1].x() - w[0].x(), w[1].y() - w[0].y());
        if acc + l >= keep {
            // Within 1e-9 of a vertex: end there, not after a sliver.
            if keep - acc > 1e-9 {
                if acc + l - keep <= 1e-9 {
                    out.push(w[1]);
                } else {
                    out.push(w[0].lerp(w[1], (keep - acc) / l));
                }
            }
            return out;
        }
        out.push(w[1]);
        acc += l;
    }
    out
}
