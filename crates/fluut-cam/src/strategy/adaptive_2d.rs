// SPDX-License-Identifier: MIT OR Apache-2.0

//! `adaptive_2d` (D09 §2.3, D15b §2.2): clears closed regions in Z steps
//! with constant tool engagement, then runs a contour of the region at
//! each level. The search is in [`crate::adaptive`].

use super::boundary::{apply_boundary, boundary_region};
use super::contour_2d::selection_z;
use crate::adaptive::{self, Cleared, Params};
use crate::chain::chain_pieces;
use crate::drive::{areas, curve_pieces, floor_regions, nest};
use crate::layout::levels;
use crate::link::{Cut, LinkCx, Turn, link};
use crate::moves::finish;
use crate::path2::Path2;
use crate::region::{Inside, Region2, Seg2};
use crate::spike::pocket::{swept_loop, swept_path};
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, prepare, tag};
use fluut_geom::Point2;
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{CutDirection, Order, Ramp, RampKind, resolve};
use fluut_path::ToolpathResult;

#[derive(Debug)]
pub struct Adaptive2d;

/// One area of the tool-centre region at every level: the optional entry
/// circle (for a helix ramp), the passes and the final contour.
pub(crate) struct AreaPlan {
    pub entry: Option<(Path2, usize)>,
    pub passes: Vec<(Path2, usize)>,
    pub finish: Vec<(Path2, usize)>,
}

impl ToolpathType for Adaptive2d {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Adaptive2d
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
        let Some(ad) = p.adaptive().cloned() else {
            return Err(CamError::Unavailable("not an adaptive_2d toolpath".into()));
        };
        let mut errors = Vec::new();
        for ch in &chains {
            if !ch.path.closed {
                errors.push(Diagnostic::error(
                    "cam.pocket_2d.region.open",
                    "a drive chain is not closed and cannot bound a region",
                ));
            } else if ch.self_intersects() {
                errors.push(Diagnostic::error(
                    "cam.geometry.chain_self_intersects",
                    "a drive chain intersects itself",
                ));
            }
        }
        let boundary = match boundary_region(input, p, tol) {
            Ok(b) => b,
            Err(d) => {
                errors.extend(d);
                None
            }
        };
        if !errors.is_empty() {
            diags.extend(errors);
            return Err(fail(diags));
        }
        let mut region = nest(chains.iter().map(|c| c.ccw_loop()).collect());
        for (f, _) in floors {
            region = if region.is_empty() {
                f
            } else {
                region.union(&f)
            };
        }
        let r = input.tool.radius();
        let d = input.tool.diameter;
        let stl = p.passes.stock_to_leave;
        let mut centre = region.offset(-(r + stl));
        if let (Some(b), Some(bp)) = (&boundary, &p.geometry.boundary) {
            centre = apply_boundary(&centre, b, bp, r);
        }
        for a in areas(&region) {
            if a.offset(-(r + stl)).is_empty() {
                diags.push(Diagnostic::warning(
                    "cam.adaptive_2d.region.unreachable",
                    "a region is narrower than the tool and is left uncut",
                ));
            }
        }
        let climb = ad.direction == CutDirection::Climb;
        let params = Params {
            min_radius: ad.min_cutting_radius,
            climb,
            ..Params::new(r, ad.optimal_load)
        };
        let box_stock = input
            .stock
            .map(|b| Region2::rect(b.min.x(), b.min.y(), b.max.x(), b.max.y()));
        let bottom = h.bottom + p.passes.stock_to_leave_axial;
        let zs = levels(h.top, bottom, ad.step_down, None);
        // With rest machining a level meets only the stock left between it
        // and the level above; outside that the region counts as cleared,
        // as in an open pocket (D15b §2.2). Levels that meet the same stock
        // share one path search.
        let windows = match (p.geometry.rest_previous, input.in_process) {
            (true, Some(st)) => Some(
                st.windows(&zs)
                    .iter()
                    .map(|w| crate::stock::rest_material(w, &region, tol))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        };
        let mut safe: Vec<Region2> = Vec::new();
        let mut incomplete = false;
        let mut fell_back = false;
        let mut sets: Vec<(Option<Region2>, Vec<AreaPlan>)> = Vec::new();
        let mut level_set = Vec::new();
        for li in 0..zs.len() {
            let w = windows.as_ref().map(|w| w[li].clone());
            if let Some(i) = sets.iter().position(|(k, _)| *k == w) {
                level_set.push(i);
                continue;
            }
            let (stock, parts) = match &w {
                None => (box_stock.clone(), areas(&centre)),
                Some(w) => (
                    Some(w.clone()),
                    areas(&centre.intersect(&w.offset(r)))
                        .into_iter()
                        .filter(|a| a.area() > tol * tol)
                        .collect(),
                ),
            };
            let mut plans = Vec::new();
            for a in parts {
                let out = plan_area(&a, stock.as_ref(), &params, &p.linking.ramp, tol, &mut safe);
                incomplete |= out.incomplete;
                fell_back |= out.fell_back;
                plans.push(out.plan);
            }
            sets.push((w, plans));
            level_set.push(sets.len() - 1);
        }
        if fell_back {
            diags.push(Diagnostic::warning(
                "cam.linking.ramp_fallback",
                "the helix does not fit in a region; the tool plunges there",
            ));
        }
        if incomplete {
            diags.push(Diagnostic::warning(
                "cam.adaptive_2d.incomplete",
                "material above the tolerance is left after the path search stopped",
            ));
        }
        let turn = if climb { Turn::Left } else { Turn::Right };
        let mut cuts = Vec::new();
        let mut prev_end: Option<Point2> = None;
        let mut emit = |ai: usize, li: usize, cuts: &mut Vec<Cut>| {
            let z = zs[li];
            let ramp_from = if li == 0 { h.top } else { zs[li - 1] };
            let plan = &sets[level_set[li]].1[ai];
            if let Some((path, s)) = &plan.entry {
                cuts.push(Cut {
                    area: Some(*s),
                    ..Cut::flat(path.clone(), z, turn, ramp_from)
                });
            }
            // Passes start in cleared material: no ramp, except the first
            // one of an open pocket, which starts outside the stock.
            for (path, s) in &plan.passes {
                cuts.push(Cut {
                    area: Some(*s),
                    ..Cut::flat(path.clone(), z, turn, z)
                });
            }
            for (path, s) in &plan.finish {
                let mut path = path.clone();
                if let Some(q) = prev_end.or(cuts.last().map(|c| c.path.end())) {
                    path = path.rotated(path.closest(q).0, 1e-9);
                }
                prev_end = Some(path.end());
                cuts.push(Cut {
                    area: Some(*s),
                    ..Cut::flat(path, z, turn, z)
                });
            }
            prev_end = None;
        };
        // Areas change from level to level with rest machining, so
        // `by_area` keeps them only without it.
        let n_areas = |li: usize| sets[level_set[li]].1.len();
        match p.linking.order {
            Order::ByArea if windows.is_none() => {
                for ai in 0..zs.first().map_or(0, |_| n_areas(0)) {
                    for li in 0..zs.len() {
                        emit(ai, li, &mut cuts);
                    }
                }
            }
            _ => {
                for li in 0..zs.len() {
                    for ai in 0..n_areas(li) {
                        emit(ai, li, &mut cuts);
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

/// What [`plan_area`] gives for one area.
pub(crate) struct AreaOut {
    pub plan: AreaPlan,
    /// The material the plan sweeps.
    pub done: Region2,
    pub incomplete: bool,
    /// A helix entry did not fit.
    pub fell_back: bool,
}

/// Plans one area `a` of the tool-centre region: entry, passes, final
/// contour and sliver moves. Material outside `stock` counts as cleared;
/// where there is any, the first cut enters from there (an open pocket).
/// Safe regions for links are pushed to `safe`.
pub(crate) fn plan_area(
    a: &Region2,
    stock: Option<&Region2>,
    params: &Params,
    ramp: &Ramp,
    tol: f64,
    safe: &mut Vec<Region2>,
) -> AreaOut {
    let r = params.r;
    let climb = params.climb;
    let orient = |path: Path2| if climb { path } else { path.reversed() };
    let mut fell_back = false;
    let material = a.offset(r);
    // Open pocket: the region reaches outside the stock, so the
    // first cut enters from there, without a ramp (D15b §2.2).
    let outside = stock
        .map(|s| material.subtract(s))
        .filter(|o| o.area() > tol * material.perimeter());
    let deep = deepest(a, params.step);
    let mut entry = None;
    let k0 = match &outside {
        Some(o) => o.clone(),
        None => {
            let rad = match ramp.kind {
                RampKind::Helix => ramp.radius.min(a.boundary_distance(deep) - tol),
                _ => 0.0,
            };
            if rad > tol {
                let c = Region2::circle(deep.x(), deep.y(), rad);
                safe.push(Region2::circle(deep.x(), deep.y(), rad + 1e-6));
                entry = Some((orient(Path2::from_loop(&c.loops[0])), safe.len() - 1));
                Region2::circle(deep.x(), deep.y(), rad + r).intersect(&material)
            } else {
                if ramp.kind == RampKind::Helix {
                    fell_back = true;
                }
                Region2::circle(deep.x(), deep.y(), r).intersect(&material)
            }
        }
    };
    let Cleared {
        passes,
        cleared,
        stats,
    } = adaptive::clear(&material, a, k0, deep, params);
    let mut ps = Vec::new();
    for pass in passes {
        safe.push(pass.safe);
        let segs = pass
            .pts
            .windows(2)
            .map(|w| Seg2 {
                a: w[0],
                b: w[1],
                bulge: 0.0,
            })
            .collect();
        ps.push((Path2::open(segs), safe.len() - 1));
    }
    // Final contour at R + stock to leave, in `direction`.
    safe.push(adaptive::shrink(&cleared, r - 1e-6));
    let fin_safe = safe.len() - 1;
    let finish: Vec<(Path2, usize)> = a
        .loops
        .iter()
        .map(|l| (orient(Path2::from_loop(l)), fin_safe))
        .collect();
    let swept_finish = a
        .loops
        .iter()
        .map(|l| swept_loop(l, r))
        .fold(Region2::empty(), |acc, s| acc.union(&s));
    // Slivers left between passes: one short move from cleared
    // material to the tool position nearest each (D15b §2.2: the
    // path keeps φ ≤ φ*; these cut less).
    let mut left = material.subtract(&cleared).subtract(&swept_finish);
    let mut done = cleared.clone();
    for l in left.loops.iter().filter(|l| l.area() > tol * tol) {
        let n = l.verts.len() as f64;
        let (sx, sy) = l
            .verts
            .iter()
            .fold((0.0, 0.0), |(x, y), v| (x + v.p.x(), y + v.p.y()));
        let target = nearest_in(a, Point2::new(sx / n, sy / n));
        let inner = adaptive::shrink(&done, r - 1e-6);
        let Some(from) = nearest_on(&inner, target) else {
            continue;
        };
        if from.distance(target) <= tol {
            continue;
        }
        done = done.union(&swept_path(&[from, target], r));
        ps.push((
            Path2::open(vec![Seg2 {
                a: from,
                b: target,
                bulge: 0.0,
            }]),
            fin_safe,
        ));
    }
    left = left.subtract(&done);
    // Only material the tool reaches by more than the tolerance counts:
    // the edge of the reach is where offsets of different regions meet.
    let left = left.intersect(&a.offset(r - 2.0 * tol));
    let incomplete = stats.stop == "max passes" || !left.offset(-tol).is_empty();
    AreaOut {
        plan: AreaPlan {
            entry,
            passes: ps,
            finish,
        },
        done: done.union(&swept_finish),
        incomplete,
        fell_back,
    }
}

/// A point of `a` far from its boundary: the middle of the last non-empty
/// inward offset (S3 spike).
fn deepest(a: &Region2, step: f64) -> Point2 {
    let mut last = a.clone();
    loop {
        let next = last.offset(-step);
        if next.is_empty() {
            break;
        }
        last = next;
    }
    let Some(l) = last
        .loops
        .iter()
        .filter(|l| l.is_ccw())
        .max_by(|x, y| x.area().total_cmp(&y.area()))
    else {
        return a
            .loops
            .first()
            .map_or(Point2::new(0.0, 0.0), |l| l.verts[0].p);
    };
    let n = l.verts.len() as f64;
    let (sx, sy) = l
        .verts
        .iter()
        .fold((0.0, 0.0), |(x, y), v| (x + v.p.x(), y + v.p.y()));
    let c = Point2::new(sx / n, sy / n);
    if a.contains(c) == Inside::In {
        c
    } else {
        l.verts[0].p
    }
}

/// `q` if it is in `r`, else the nearest point of the boundary of `r`.
fn nearest_in(r: &Region2, q: Point2) -> Point2 {
    if r.contains(q) != Inside::Out {
        return q;
    }
    nearest_on(r, q).unwrap_or(q)
}

/// The point of the boundary of `r` nearest to `q`, or `q` when it is
/// inside; `None` for an empty region.
fn nearest_on(r: &Region2, q: Point2) -> Option<Point2> {
    if r.is_empty() {
        return None;
    }
    if r.contains(q) == Inside::In {
        return Some(q);
    }
    r.segs()
        .map(|s| s.point_at(crate::path2::seg_closest(&s, q)))
        .min_by(|a, b| a.distance(q).total_cmp(&b.distance(q)))
}
