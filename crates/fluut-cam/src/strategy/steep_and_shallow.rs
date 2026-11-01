// SPDX-License-Identifier: MIT OR Apache-2.0

//! `steep_and_shallow` (D09 §4.8, D15b §5.5): contour passes on steep
//! areas and parallel or scallop passes on shallow areas, split by a slope
//! threshold. Finishing type.
//!
//! - **Steep:** `contour` with slope limits [threshold, 90°].
//! - **Shallow region:** the cut runs of a probe `parallel` at the shallow
//!   stepover (pass angle 0, slopes up to the threshold), each widened
//!   into a strip one stepover wide, unioned and grown by `overlap`.
//!   Along the probe passes it is exact; across them it is within the
//!   stepover.
//! - **Shallow:** `parallel` or `scallop` inside that region, with no slope
//!   limit.

use super::contour::{Levels, link_runs, waterline, waterline_grow};
use super::parallel::parallel_runs;
use super::scallop::Loops;
use crate::follow::{Run, Sampler, setup};
use crate::region::Region2;
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType};
use fluut_geom::Point3;
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{
    CutDirection, OffsetOrder, ParallelDirection, PassLayout, RegionOrder, ShallowPattern,
};
use std::f64::consts::FRAC_PI_2;

#[derive(Debug)]
pub struct SteepAndShallow;

impl ToolpathType for SteepAndShallow {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::SteepAndShallow
    }

    fn role(&self) -> Role {
        Role::Finish
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        let mut st = setup(input, "steep_and_shallow")?;
        let Some(ss) = st.params.steep_and_shallow().cloned() else {
            return Err(CamError::Unavailable(
                "not a steep_and_shallow toolpath".into(),
            ));
        };
        let s = st.stepover(input, ss.shallow.scallop_height, ss.shallow.stepover)?;
        let theta = ss.threshold_deg.to_radians();
        let base = st.region(input, 0.0)?;
        let wide = st.region(input, waterline_grow(&st, input))?;
        let h = st.heights;
        let tol = st.params.passes.machining_tolerance;
        let max_step = 0.5 * input.tool.radius();
        // Steep: waterlines on contacts at least as steep as the threshold.
        let steep = Levels {
            step_down: ss.steep.step_down,
            scallop_height: ss.steep.scallop_height,
            min_slope: theta,
            max_slope: FRAC_PI_2,
            direction: ss.steep.direction,
            order: st.params.linking.order,
        };
        let wl = waterline(&st, input, &wide);
        let lr = steep.runs(&st, input, &wl);
        // Shallow region from a probe parallel.
        let probe = Sampler {
            q: &st.q,
            h: &h,
            min_slope: 0.0,
            max_slope: theta,
            tol,
            max_step,
        };
        let strips: Vec<Region2> = parallel_runs(
            &probe,
            &base,
            0.0,
            s,
            PassLayout::Even,
            ParallelDirection::Conventional,
        )
        .iter()
        .filter_map(|r| strip(&r.pts, 0.5 * s))
        .collect();
        let shallow_region = union_all(strips).offset(ss.overlap).intersect(&base);
        let pl = Sampler {
            q: &st.q,
            h: &h,
            min_slope: 0.0,
            max_slope: FRAC_PI_2,
            tol,
            max_step,
        };
        let mut shallow: Vec<Run> = match ss.shallow.pattern {
            ShallowPattern::Parallel => parallel_runs(
                &pl,
                &shallow_region,
                ss.shallow.pass_angle_deg,
                s,
                st.params.passes.pass_layout,
                ss.shallow.direction,
            ),
            ShallowPattern::Scallop => {
                Loops {
                    stepover: s,
                    direction: OffsetOrder::OutsideIn,
                    cut_direction: match ss.shallow.direction {
                        ParallelDirection::Conventional => CutDirection::Conventional,
                        _ => CutDirection::Climb,
                    },
                    continuous: false,
                }
                .runs(&pl, &shallow_region)
                .0
            }
        };
        // Pass numbers apart from the steep ones.
        let base_id = lr.runs.iter().map(|r| r.pass).max().unwrap_or(0) + 1;
        for r in &mut shallow {
            r.pass += base_id;
        }
        let runs: Vec<Run> = match ss.region_order {
            RegionOrder::SteepFirst => lr.runs.into_iter().chain(shallow).collect(),
            RegionOrder::ShallowFirst => shallow.into_iter().chain(lr.runs).collect(),
        };
        let mut diags = std::mem::take(&mut st.diags);
        if runs.is_empty() {
            diags.push(Diagnostic::warning(
                "cam.steep_and_shallow.empty",
                "no part of the region is cut: check the threshold, heights and boundary",
            ));
        }
        let outlines = lr.outlines;
        let clear = |a: Point3, b: Point3| outlines.clear(&st.q, a, b);
        link_runs(
            &st,
            input,
            &wide,
            &runs,
            diags,
            "steep_and_shallow",
            Some(&clear),
            None,
        )
    }
}

/// The XY strip of half-width `w` around a pass along X (its CL points
/// share a Y), from its first to its last point.
fn strip(pts: &[Point3], w: f64) -> Option<Region2> {
    let (mut x0, mut x1) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in pts {
        x0 = x0.min(p.x());
        x1 = x1.max(p.x());
    }
    let y = pts.first()?.y();
    (x1 > x0).then(|| Region2::rect(x0, y - w, x1, y + w))
}

/// The union of many regions, pairwise so each boolean stays small.
fn union_all(mut v: Vec<Region2>) -> Region2 {
    if v.is_empty() {
        return Region2::empty();
    }
    while v.len() > 1 {
        let mut next = Vec::with_capacity(v.len().div_ceil(2));
        let mut it = v.into_iter();
        while let Some(a) = it.next() {
            next.push(match it.next() {
                Some(b) => a.union(&b),
                None => a,
            });
        }
        v = next;
    }
    v.pop().unwrap_or_else(Region2::empty)
}
