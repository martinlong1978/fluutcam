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

use crate::follow::{LineDrive, Linker, Run, Sampler, setup};
use crate::layout::{layout_even, layout_fixed};
use crate::link::StockFloor;
use crate::moves::finish;
use crate::region::Region2;
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, tag};
use fluut_geom::{Point2, Vec2};
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{ParallelDirection, PassLayout};
use fluut_path::ToolpathResult;
use rayon::prelude::*;

#[derive(Debug)]
pub struct Parallel;

impl ToolpathType for Parallel {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Parallel
    }

    fn role(&self) -> Role {
        Role::Finish
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        let tp = input.toolpath;
        let mut st = setup(input, "parallel")?;
        let Some(pp) = st.params.parallel().cloned() else {
            return Err(CamError::Unavailable("not a parallel toolpath".into()));
        };
        let tool = input.tool;
        // Curvature of the drive faces (scallop stepover, rest warning).
        let (curv, mesh) = st.curvature(input);
        if curv.concave > 0.0 && 1.0 / curv.concave <= tool.radius() {
            st.diags.push(Diagnostic::warning(
                "cam.parallel.scallop.concave",
                format!(
                    "a concave region of radius {:.3} mm is not larger than the tool radius \
                     {}: it is left as rest material",
                    1.0 / curv.concave,
                    tool.radius()
                ),
            ));
        }
        if pp.scallop_height.is_some() && mesh {
            st.diags.push(
                Diagnostic::warning(
                    "param.parallel.drive_faces.mesh_scallop",
                    "the drive faces are a mesh: curvature from triangles is zero \
                     inside facets, so the stepover is the flat-area value",
                )
                .at("passes.scallop_height"),
            );
        }
        let stepover = st.stepover(input, pp.scallop_height, pp.stepover)?;
        // Region: the drive faces' box, limited by the boundary.
        let region = st.region(input, 0.0)?;
        let p = &st.params;
        let h = st.heights;
        let tol = p.passes.machining_tolerance;
        let pl = Sampler {
            q: &st.q,
            h: &h,
            min_slope: pp.min_slope_deg.to_radians(),
            max_slope: pp.max_slope_deg.to_radians(),
            tol,
            max_step: 0.5 * tool.radius(),
        };
        let runs = parallel_runs(
            &pl,
            &region,
            pp.pass_angle_deg,
            stepover,
            p.passes.pass_layout,
            pp.direction,
        );
        let mut diags = std::mem::take(&mut st.diags);
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
            level_clear: None,
            material_top: input.env.stock_top.max(h.top),
            floor: StockFloor::of(input),
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

/// The cut runs of parallel passes across `region` (D09 §4.3): lines at
/// `angle_deg`, `stepover` apart by `layout`, in machining order and
/// travel direction.
pub(crate) fn parallel_runs(
    pl: &Sampler,
    region: &Region2,
    angle_deg: f64,
    stepover: f64,
    layout: PassLayout,
    direction: ParallelDirection,
) -> Vec<Run> {
    let ang = angle_deg.to_radians();
    let (sn, cs) = libm::sincos(ang);
    let u = Vec2::new(cs, sn);
    let n = Vec2::new(-sn, cs);
    if region.is_empty() {
        return Vec::new();
    }
    let (a, b) = region.support(n);
    let pos = match layout {
        PassLayout::Even => layout_even(a, b, stepover),
        PassLayout::FixedFromEdge => layout_fixed(a, b, stepover),
    };
    // The passes in order, with their spans and direction; their cut runs
    // are independent: sampled in parallel, joined in pass order.
    let mut k = 0usize;
    let mut todo = Vec::new();
    for off in pos {
        let o = Point2::new(n.x() * off, n.y() * off);
        let spans: Vec<(f64, f64)> = region
            .clip_line(o, u)
            .into_iter()
            .filter(|(t0, t1)| t1 - t0 > crate::follow::CUTOFF)
            .collect();
        if spans.is_empty() {
            continue;
        }
        let forward = match direction {
            ParallelDirection::Zigzag => k.is_multiple_of(2),
            ParallelDirection::Conventional => true,
            // Climb: the next pass (+n) on the right of travel.
            ParallelDirection::Climb => false,
        };
        k += 1;
        todo.push((o, spans, forward, k));
    }
    let per_pass: Vec<Vec<Run>> = todo
        .par_iter()
        .map(|(o, spans, forward, k)| {
            let mut pass_runs = Vec::new();
            let line = LineDrive { o: *o, u };
            for (si, &(t0, t1)) in spans.iter().enumerate() {
                for pts in pl.cut_runs(&line, t0, t1) {
                    pass_runs.push(Run {
                        pts,
                        pass: *k,
                        span: si,
                        level: false,
                    });
                }
            }
            if !forward {
                pass_runs.reverse();
                for r in &mut pass_runs {
                    r.pts.reverse();
                }
            }
            pass_runs
        })
        .collect();
    per_pass.into_iter().flatten().collect()
}
