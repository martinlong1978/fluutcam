// SPDX-License-Identifier: MIT OR Apache-2.0

//! `pocket_2d` (D09 §2.2, D15 §4): clears closed regions in Z steps with
//! offset rings or a zig-zag raster and a final contour.

use super::contour_2d::selection_z;
use crate::chain::chain_pieces;
use crate::drive::{areas, curve_pieces, floor_regions, nest};
use crate::layout::{layout_even, layout_fixed, levels};
use crate::link::{Cut, LinkCx, Turn, link};
use crate::moves::finish;
use crate::path2::Path2;
use crate::region::{Region2, Seg2};
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, prepare, tag};
use fluut_geom::{Point2, Vec2};
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{
    BoundarySide, Containment, CutDirection, OffsetOrder, Order, PassLayout, Pattern, resolve,
};
use fluut_path::ToolpathResult;

#[derive(Debug)]
pub struct Pocket2d;

/// The areas of one level: an index into the safe regions and the passes.
type LevelPlan = Vec<(usize, Vec<Path2>)>;

/// Safety cap on rings per level.
const MAX_RINGS: usize = 10_000;

impl ToolpathType for Pocket2d {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Pocket2d
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
        let join = g.tol_model().max(1e-5);
        let chains = chain_pieces(
            curve_pieces(
                g,
                &r0.geometry.drive_curves,
                tol / 4.0,
                "geometry.drive_curves",
            )
            .map_err(|d| fail(vec![d]))?,
            join,
        );
        let floors = floor_regions(
            g,
            &r0.geometry.drive_faces,
            &r0.geometry.excluded_faces,
            tol / 4.0,
        )
        .map_err(|d| fail(vec![d]))?;
        let mut sel = selection_z(&chains);
        for (_, z) in &floors {
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
        let Some(pk) = p.pocket().cloned() else {
            return Err(CamError::Unavailable("not a pocket_2d toolpath".into()));
        };
        let mut errors = Vec::new();
        for ch in &chains {
            if !ch.path.closed {
                errors.push(Diagnostic::error(
                    "cam.pocket_2d.region.open",
                    "a drive chain is not closed and cannot bound a pocket",
                ));
            } else if ch.self_intersects() {
                errors.push(Diagnostic::error(
                    "cam.geometry.chain_self_intersects",
                    "a drive chain intersects itself",
                ));
            }
        }
        let mut boundary = None;
        if let Some(b) = &p.geometry.boundary {
            let bc = chain_pieces(
                curve_pieces(g, &b.curves, tol / 4.0, "geometry.boundary.curves")
                    .map_err(|d| fail(vec![d]))?,
                join,
            );
            if bc.iter().any(|c| !c.path.closed) {
                errors.push(
                    Diagnostic::error(
                        "param.geometry.boundary.open",
                        "a boundary chain is not closed",
                    )
                    .at("geometry.boundary.curves"),
                );
            }
            boundary = Some(nest(
                bc.iter()
                    .filter(|c| c.path.closed)
                    .map(|c| c.ccw_loop())
                    .collect(),
            ));
        }
        if !errors.is_empty() {
            diags.extend(errors);
            return Err(fail(diags));
        }
        let mut clear = nest(chains.iter().map(|c| c.ccw_loop()).collect());
        for (f, _) in floors {
            clear = if clear.is_empty() { f } else { clear.union(&f) };
        }
        let r = input.tool.radius();
        let d = input.tool.diameter;
        let stl = p.passes.stock_to_leave;
        let mut centre = clear.offset(-(r + stl));
        if let (Some(b), Some(bp)) = (&boundary, &p.geometry.boundary) {
            let grow = match bp.containment {
                Containment::Inside => -r,
                Containment::Centre => 0.0,
                Containment::Outside => r,
            };
            centre = match bp.side {
                BoundarySide::Inside => centre.intersect(&b.offset(grow)),
                BoundarySide::Outside => centre.subtract(&b.offset(-grow)),
            };
        }
        for a in areas(&clear) {
            if a.offset(-(r + stl)).is_empty() {
                diags.push(Diagnostic::warning(
                    "cam.pocket_2d.region.unreachable",
                    "a region is narrower than the tool and is left uncut",
                ));
            }
        }
        let stepover = pk.stepover.unwrap_or(0.4 * d);
        let bottom = h.bottom + p.passes.stock_to_leave_axial;
        let zs = levels(h.top, bottom, pk.step_down, None);
        let climb = pk.direction == CutDirection::Climb;
        let turn = if climb { Turn::Left } else { Turn::Right };
        let orient = |path: Path2| if climb { path } else { path.reversed() };
        // The passes of one area at one level, before Z.
        let area_paths = |a: &Region2| -> Vec<Path2> {
            let mut v = Vec::new();
            match pk.pattern {
                Pattern::Offset => {
                    let rings = offset_rings(a, stepover, tol);
                    let mut rings: Vec<Vec<Path2>> = rings
                        .iter()
                        .map(|ls| ls.iter().map(|l| orient(Path2::from_loop(l))).collect())
                        .collect();
                    if pk.offset_order == OffsetOrder::InsideOut {
                        rings.reverse();
                    }
                    v.extend(rings.into_iter().flatten());
                }
                Pattern::Raster => {
                    let ang = pk.pass_angle_deg.to_radians();
                    let (sn, cs) = libm::sincos(ang);
                    let u = Vec2::new(cs, sn);
                    let n = Vec2::new(-sn, cs);
                    let (lo, hi) = a.support(n);
                    let pos = match p.passes.pass_layout {
                        PassLayout::Even => layout_even(lo, hi, stepover),
                        PassLayout::FixedFromEdge => layout_fixed(lo, hi, stepover),
                    };
                    let mut k = 0;
                    for off in pos {
                        let o = Point2::new(n.x() * off, n.y() * off);
                        let mut lines: Vec<Path2> = a
                            .clip_line(o, u)
                            .into_iter()
                            .filter(|(t0, t1)| t1 - t0 > tol)
                            .map(|(t0, t1)| {
                                Path2::open(vec![Seg2 {
                                    a: o + u * t0,
                                    b: o + u * t1,
                                    bulge: 0.0,
                                }])
                            })
                            .collect();
                        if k % 2 == 1 {
                            lines.reverse();
                            lines = lines.into_iter().map(|l| l.reversed()).collect();
                        }
                        if !lines.is_empty() {
                            k += 1;
                        }
                        v.extend(lines);
                    }
                    v.extend(a.loops.iter().map(|l| orient(Path2::from_loop(l))));
                }
            }
            v
        };
        // With rest machining each level clears only the tool-centre
        // positions within R of the stock left between it and the level
        // above (D15 §4); levels that meet the same stock share passes.
        let windows = match (p.geometry.rest_previous, input.in_process) {
            (true, Some(st)) => Some(
                st.windows(&zs)
                    .iter()
                    .map(|w| crate::stock::rest_material(w, &clear, tol))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        };
        let mut safe: Vec<Region2> = Vec::new();
        // Per level: indices into `safe` and the passes of each area.
        let mut plans: Vec<LevelPlan> = Vec::new();
        let mut seen: Vec<(Option<Region2>, LevelPlan)> = Vec::new();
        for li in 0..zs.len() {
            let w = windows.as_ref().map(|w| w[li].clone());
            if let Some((_, plan)) = seen.iter().find(|(k, _)| *k == w) {
                plans.push(plan.clone());
                continue;
            }
            let parts = match &w {
                None => areas(&centre),
                Some(w) => areas(&centre.intersect(&w.offset(r)))
                    .into_iter()
                    .filter(|a| a.area() > tol * tol)
                    .collect(),
            };
            let plan: LevelPlan = parts
                .iter()
                .map(|a| {
                    safe.push(a.clone());
                    (safe.len() - 1, area_paths(a))
                })
                .collect();
            seen.push((w, plan.clone()));
            plans.push(plan);
        }
        let mut cuts = Vec::new();
        let mut prev_end: Option<Point2> = None;
        let mut emit = |li: usize, ai: usize, cuts: &mut Vec<Cut>| {
            let z = zs[li];
            let ramp_from = if li == 0 { h.top } else { zs[li - 1] };
            let (si, paths) = &plans[li][ai];
            for path in paths {
                let mut path = path.clone();
                if path.closed
                    && let Some(q) = prev_end
                {
                    path = path.rotated(path.closest(q).0, 1e-9);
                }
                prev_end = Some(path.end());
                cuts.push(Cut {
                    area: Some(*si),
                    ..Cut::flat(path, z, turn, ramp_from)
                });
            }
        };
        // Areas change from level to level with rest machining, so
        // `by_area` keeps them only without it.
        match p.linking.order {
            Order::ByArea if windows.is_none() => {
                for ai in 0..plans.first().map_or(0, Vec::len) {
                    for li in 0..zs.len() {
                        emit(li, ai, &mut cuts);
                    }
                }
            }
            _ => {
                let counts: Vec<usize> = plans.iter().map(Vec::len).collect();
                for (li, n) in counts.into_iter().enumerate() {
                    for ai in 0..n {
                        emit(li, ai, &mut cuts);
                    }
                }
            }
        }
        let cx = LinkCx {
            heights: h,
            material_top: input.env.stock_top.max(h.top),
            linking: &p.linking,
            tool_diameter: d,
            tol,
            safe: &safe,
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

/// Offset rings of the tool-centre region `a`, outermost first: rings at
/// −k·s until the region is empty (D09 §2.2). Where a part of the region
/// runs out less than a stepover inside ring k (a thin band, or the
/// middle), the loops of the ring at −(k + ½)·s that pass through that part
/// are added after ring k, so no point is more than s/2 from a pass
/// (D08 §11 stepover test).
fn offset_rings(a: &Region2, s: f64, tol: f64) -> Vec<Vec<crate::region::Loop2>> {
    let mut levels = Vec::new();
    let mut cur = a.clone();
    while !cur.is_empty() && levels.len() < MAX_RINGS {
        levels.push(cur);
        cur = a.offset(-(levels.len() as f64) * s);
    }
    let mut out = Vec::new();
    for k in 0..levels.len() {
        out.push(levels[k].loops.clone());
        let half = levels[k].offset(-0.5 * s);
        if half.is_empty() {
            continue;
        }
        let left = match levels.get(k + 1) {
            Some(next) => half.subtract(&next.offset(0.5 * s + 1e-6)),
            None => half.clone(),
        };
        if left.area().abs() <= tol * s {
            continue;
        }
        let extra: Vec<_> = half
            .loops
            .iter()
            .filter(|l| {
                l.segs()
                    .any(|sg| left.contains(sg.point_at(0.5)) != crate::region::Inside::Out)
            })
            .cloned()
            .collect();
        if !extra.is_empty() {
            out.push(extra);
        }
    }
    out
}
