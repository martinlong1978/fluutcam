// SPDX-License-Identifier: MIT OR Apache-2.0

//! `chamfer_2d` (D09 §2.7, D15b §2.5): a V tool along horizontal edges, its
//! tip below the edge and its axis offset to the side away from the
//! material, so the cone cuts a chamfer of the set width.

use super::contour_2d::{offset_chain, selection_z};
use crate::chain::chain_pieces;
use crate::cutter::profile::Cutter;
use crate::drive::{curve_pieces, wall_edges};
use crate::link::{Cut, LinkCx, link};
use crate::moves::finish;
use crate::path2::Path2;
use crate::surface::{FaceSets, SurfaceQuery};
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, prepare, tag};
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{ResolvedHeights, Side, resolve};
use fluut_path::ToolpathResult;

#[derive(Debug)]
pub struct Chamfer2d;

/// Spacing of the tip-gouge probes along a pass, mm.
const PROBE_STEP: f64 = 1.0;

impl ToolpathType for Chamfer2d {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Chamfer2d
    }

    fn role(&self) -> Role {
        Role::Finish
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        let tp = input.toolpath;
        let fail = |d: Vec<Diagnostic>| CamError::Invalid(tag(d, tp));
        let r0 = resolve(&tp.params, input.tool, input.material);
        let tol = r0.passes.machining_tolerance.max(1e-4);
        let g = input.geometry;
        let mut pieces = curve_pieces(
            g,
            &r0.geometry.drive_curves,
            tol / 4.0,
            "geometry.drive_curves",
        )
        .map_err(|d| fail(vec![d]))?;
        pieces.extend(
            wall_edges(
                g,
                &r0.geometry.drive_faces,
                &r0.geometry.excluded_faces,
                tol / 4.0,
                true,
            )
            .map_err(|d| fail(vec![d]))?,
        );
        let chains = chain_pieces(pieces, g.tol_model().max(1e-5));
        let prep = prepare(input, selection_z(&chains)).map_err(|e| match e {
            CamError::Invalid(d) => fail(d),
            e => e,
        })?;
        let mut diags = prep.warnings;
        let p = &prep.params;
        let Some(c) = p.chamfer().cloned() else {
            return Err(CamError::Unavailable("not a chamfer_2d toolpath".into()));
        };
        let (Some(alpha), rt) = (input.tool.v_angle(), input.tool.tip_radius()) else {
            return Err(CamError::Unavailable("chamfer_2d needs a V tool".into()));
        };
        let (d, o) = c.depth_and_offset(alpha, rt);
        let top = prep.heights.top;
        let z = top - d;
        let h = ResolvedHeights {
            bottom: z,
            ..prep.heights
        };
        let mut errors = Vec::new();
        if chains.is_empty() {
            errors.push(Diagnostic::error(
                "cam.geometry.drive.empty",
                "the drive selection gives no edges to chamfer",
            ));
        }
        for ch in &chains {
            if ch.z.hi - ch.z.lo > tol {
                errors.push(Diagnostic::error(
                    "cam.chamfer_2d.edge.not_horizontal",
                    format!(
                        "a drive chain is not at one Z (it spans {} to {})",
                        ch.z.lo, ch.z.hi
                    ),
                ));
            }
            let side = side_of(c.side, ch.path.closed);
            let wrong = if ch.path.closed {
                matches!(side, Side::Left | Side::Right)
            } else {
                matches!(side, Side::Inside | Side::Outside)
            };
            if wrong {
                errors.push(
                    Diagnostic::error(
                        "param.chamfer_2d.side.closure",
                        "inside and outside need closed chains; left and right need open ones",
                    )
                    .at("passes.side"),
                );
            }
        }
        if !errors.is_empty() {
            diags.extend(errors);
            return Err(fail(diags));
        }
        let paths: Vec<_> = chains
            .iter()
            .flat_map(|ch| offset_chain(ch, side_of(c.side, ch.path.closed), o, c.direction))
            .collect();
        if tip_gouges(input, &paths, z, rt, tol) {
            diags.push(Diagnostic::error(
                "cam.chamfer_2d.tip_gouge",
                format!("the tool tip at {d} mm below the edge would touch the model below it"),
            ));
            return Err(fail(diags));
        }
        let mut cuts = Vec::new();
        let mut prev_end = None;
        for (path, turn) in paths {
            let mut path = path;
            if path.closed
                && let Some(q) = prev_end
            {
                path = path.rotated(path.closest(q).0, 1e-9);
            }
            prev_end = Some(path.end());
            cuts.push(Cut::flat(path, z, turn, top));
        }
        let cx = LinkCx {
            heights: h,
            material_top: input.env.stock_top.max(top),
            linking: &p.linking,
            tool_diameter: input.tool.diameter,
            tol,
            safe: &[],
        };
        let passes = link(&cuts, &cx, &mut diags);
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

fn side_of(side: Option<Side>, closed: bool) -> Side {
    side.unwrap_or(if closed { Side::Outside } else { Side::Right })
}

/// Whether a flat disc of the tip radius (a little smaller, so a tip just
/// touching a wall does not count) at Z `z` along `paths` meets the model:
/// the drop of that disc stops above `z` (D15b §2.5).
fn tip_gouges(
    input: &StrategyInput,
    paths: &[(Path2, crate::link::Turn)],
    z: f64,
    rt: f64,
    tol: f64,
) -> bool {
    let r = (rt - tol).max(0.5 * rt).max(1e-3);
    let Some(cutter) = Cutter::flat(2.0 * r) else {
        return false;
    };
    let Some(q) = SurfaceQuery::new(input.geometry, &FaceSets::default(), cutter, 0.0) else {
        return false;
    };
    for (path, _) in paths {
        let len = path.length();
        let n = (len / PROBE_STEP).ceil().max(1.0) as usize;
        for k in 0..=n {
            let pt = path.at(len * k as f64 / n as f64).0;
            if q.drop(pt.x(), pt.y()).is_some_and(|hit| hit.tip > z + tol) {
                return true;
            }
        }
    }
    false
}
