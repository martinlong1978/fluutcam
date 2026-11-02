// SPDX-License-Identifier: MIT OR Apache-2.0

//! `face` (D09 §2.1, D15b §2.1): clears the stock top down to a flat Z
//! with straight passes that run `overlap` beyond the area.

use crate::drive::floor_regions;
use crate::layout::{layout_even, layout_fixed, levels};
use crate::link::{Cut, LinkCx, StockFloor, Turn, link_over};
use crate::moves::finish;
use crate::path2::Path2;
use crate::region::{Region2, Seg2};
use crate::strategy::boundary::{apply_boundary, boundary_region};
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, prepare, tag};
use fluut_geom::{Point2, Vec2};
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{ParallelDirection, PassLayout, resolve};
use fluut_path::ToolpathResult;

#[derive(Debug)]
pub struct Face;

impl ToolpathType for Face {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Face
    }

    fn role(&self) -> Role {
        Role::Rough
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        let tp = input.toolpath;
        let fail = |d: Vec<Diagnostic>| CamError::Invalid(tag(d, tp));
        let r0 = resolve(&tp.params, input.tool, input.material);
        let tol = r0.passes.machining_tolerance.max(1e-4);
        let g = input.geometry;
        let floors = floor_regions(
            g,
            &r0.geometry.drive_faces,
            &r0.geometry.excluded_faces,
            tol / 4.0,
        )
        .map_err(|d| fail(vec![d]))?;
        // The highest drive face sets `selection` (D15b §2.1).
        let sel = floors
            .iter()
            .map(|(_, z)| z.hi)
            .reduce(f64::max)
            .map(|z| (z, z));
        let prep = prepare(input, sel).map_err(|e| match e {
            CamError::Invalid(d) => fail(d),
            e => e,
        })?;
        let mut diags = prep.warnings;
        let p = &prep.params;
        let h = prep.heights;
        let Some(fp) = p.face().cloned() else {
            return Err(CamError::Unavailable("not a face toolpath".into()));
        };
        let boundary = boundary_region(input, p, tol).map_err(fail)?;
        // Area: the stock outline (decision 7) with the drive faces.
        let (lo, hi) = match input.stock {
            Some(b) => (
                Point2::new(b.min.x(), b.min.y()),
                Point2::new(b.max.x(), b.max.y()),
            ),
            None => {
                let b = g.bounds(tol);
                (
                    Point2::new(b.min.x(), b.min.y()),
                    Point2::new(b.max.x(), b.max.y()),
                )
            }
        };
        let mut area = Region2::rect(lo.x(), lo.y(), hi.x(), hi.y());
        for (f, _) in floors {
            area = area.union(&f);
        }
        let mut grown = area.offset(fp.overlap);
        if let (Some(b), Some(bp)) = (&boundary, &p.geometry.boundary) {
            grown = apply_boundary(&grown, b, bp, input.tool.radius());
        }
        let d = input.tool.diameter;
        let stepover = fp.stepover.unwrap_or(0.4 * d);
        let floor = h.bottom + p.passes.stock_to_leave_axial;
        let zs = match fp.final_step_down {
            Some(fs) if floor + fs < h.top => {
                let mut v = levels(h.top, floor + fs, fp.step_down, None);
                v.push(floor);
                v
            }
            _ => levels(h.top, floor, fp.step_down, None),
        };
        // Lines across the grown area at the pass angle.
        let ang = fp.pass_angle_deg.to_radians();
        let (sn, cs) = libm::sincos(ang);
        let u = Vec2::new(cs, sn);
        let n = Vec2::new(-sn, cs);
        let (a, b) = grown.support(n);
        let pos = match p.passes.pass_layout {
            PassLayout::Even => layout_even(a, b, stepover),
            PassLayout::FixedFromEdge => layout_fixed(a, b, stepover),
        };
        let mut lines: Vec<Path2> = Vec::new();
        for off in pos {
            let o = Point2::new(n.x() * off, n.y() * off);
            for (t0, t1) in grown.clip_line(o, u) {
                if t1 - t0 > tol {
                    lines.push(Path2::open(vec![Seg2 {
                        a: o + u * t0,
                        b: o + u * t1,
                        bulge: 0.0,
                    }]));
                }
            }
        }
        // Passes step towards +n, so the uncut material is on the left of
        // +u travel; climb keeps it on the right (spindle M3).
        let zigzag = fp.direction == ParallelDirection::Zigzag;
        let lines: Vec<Path2> = lines
            .into_iter()
            .enumerate()
            .map(|(i, l)| {
                let flip = match fp.direction {
                    ParallelDirection::Zigzag => i % 2 == 1,
                    ParallelDirection::Climb => true,
                    ParallelDirection::Conventional => false,
                };
                if flip { l.reversed() } else { l }
            })
            .collect();
        let safe = [grown.clone()];
        let mut cuts = Vec::new();
        for (li, &z) in zs.iter().enumerate() {
            let ramp_from = if li == 0 { h.top } else { zs[li - 1] };
            for (i, l) in lines.iter().enumerate() {
                // Zig-zag levels run back and forth across levels too.
                let path = if zigzag && li % 2 == 1 {
                    lines[lines.len() - 1 - i].reversed()
                } else {
                    l.clone()
                };
                cuts.push(Cut {
                    area: zigzag.then_some(0),
                    ..Cut::flat(path, z, Turn::Left, ramp_from)
                });
            }
        }
        if cuts.is_empty() {
            diags.push(Diagnostic::warning(
                "cam.geometry.drive.empty",
                "the area to face is empty",
            ));
        }
        let cx = LinkCx {
            heights: h,
            material_top: input.env.stock_top.max(h.top),
            linking: &p.linking,
            tool_diameter: d,
            tol,
            safe: &safe,
        };
        let passes = link_over(&cuts, &cx, StockFloor::of(input), &mut diags);
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
