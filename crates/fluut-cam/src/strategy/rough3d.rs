// SPDX-License-Identifier: MIT OR Apache-2.0

//! 3-D roughing in Z levels (D15b §4, D09 §4.1–§4.2): `optimised_load_3d` and
//! `pocket_3d`.
//!
//! At each level z, from the top down:
//! - forbidden(z): the plane section of the solids at z −
//!   `stock_to_leave_axial` (taken [`DELTA`] above, CH-03), grown by R +
//!   `stock_to_leave`, joined with forbidden at every level above, so a
//!   part wider higher up is not cut from below;
//! - centres(z): the stock outline grown by R (cut by `boundary`), minus
//!   forbidden(z);
//! - material(z): the stock left above z within reach of centres(z);
//! - clear(z): centres(z) within R of material(z), cleared with the
//!   `optimised_load_2d` engine or the `pocket_2d` patterns.
//!
//! The stock is the Setup's (or with `rest_from` the in-process stock) and
//! is updated after each level, so `optimised_load_3d` fine levels (cut after the
//! main level below them) see only the steps left on shallow areas.

use super::boundary::{apply_boundary, boundary_region};
use super::optimised_load_2d::{AreaPlan, plan_area};
use super::pocket_2d::area_passes;
use crate::chain::Piece;
use crate::drive::{areas, faces, is_horizontal_plane};
use crate::geometry::{BodyGeom, face_z_range};
use crate::layout::levels;
use crate::link::{Cut, LinkCx, StockFloor, Turn, link_over};
use crate::moves::finish;
use crate::optimised_load::Params as OlParams;
use crate::path2::Path2;
use crate::region::Region2;
use crate::section::{Section, Sectioner};
use crate::spike::pocket::stadium;
use crate::stock::{LayeredStock, rest_material, union_all};
use crate::toolpath::{CamError, Generated, StrategyInput, prepare, tag};
use fluut_geom::Surface;
use fluut_model::Diagnostic;
use fluut_model::params::{Sel, ToolpathTypeId};
use fluut_model::resolve::{CutDirection, Pocket2d, ResolvedParams, resolve};
use fluut_path::ToolpathResult;
use std::collections::BTreeSet;

/// Sections are taken this far above their level (CH-03), mm.
pub const DELTA: f64 = 1e-4;

/// The clearing method of a level.
enum Method {
    OptimisedLoad { params: OlParams, fine: Option<f64> },
    Pocket { pk: Pocket2d, stepover: f64 },
}

/// One level to cut: its Z, the Z cleared above it (ramps start there),
/// and whether it is a fine level.
#[derive(Copy, Clone, Debug)]
struct Level {
    z: f64,
    from: f64,
    fine: bool,
}

pub fn generate(input: &StrategyInput) -> Result<Generated, CamError> {
    let tp = input.toolpath;
    let kind = tp.kind();
    let ty = kind.as_str();
    let fail = |d: Vec<Diagnostic>| CamError::Invalid(tag(d, tp));
    let r0 = resolve(&tp.params, input.tool, input.material);
    let g = input.geometry;
    let geo = &r0.geometry;
    let tol = r0.passes.machining_tolerance.max(1e-4);
    // Face sets (D15 §2.3.2): empty drive faces = every face.
    let excluded =
        faces(g, &geo.excluded_faces, &[], "geometry.excluded_faces").map_err(|d| fail(vec![d]))?;
    let all: Vec<Sel> = g
        .bodies
        .iter()
        .filter(|b| matches!(b.geom, BodyGeom::Brep(_)))
        .map(|b| Sel::Body(b.id.clone()))
        .collect();
    let drive = if geo.drive_faces.is_empty() {
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
    if let Some(c) = &geo.check_faces {
        faces(g, c, &geo.excluded_faces, "geometry.check_faces").map_err(|d| fail(vec![d]))?;
    }
    let drive_ids: BTreeSet<String> = drive.iter().map(|x| x.2.clone()).collect();
    let excluded_ids: BTreeSet<String> = excluded.iter().map(|x| x.2.clone()).collect();
    let mut sel: Option<(f64, f64)> = None;
    for (b, f, _) in &drive {
        let z = face_z_range(b, *f, tol / 4.0);
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
    let boundary = boundary_region(input, p, tol).map_err(fail)?;
    if drive.is_empty() {
        diags.push(Diagnostic::error(
            "cam.geometry.drive.empty",
            "the Setup has no solid faces to rough",
        ));
        return Err(fail(diags));
    }
    let Some(stock_box) = input.stock else {
        diags.push(Diagnostic::error(
            "cam.stock.missing",
            "3-D roughing needs the Setup stock",
        ));
        return Err(fail(diags));
    };
    let r = input.tool.radius();
    let d = input.tool.diameter;
    let stl = p.passes.stock_to_leave;
    let sta = p.passes.stock_to_leave_axial;
    let (method, step, flats) = method(p, kind, r, d)?;
    let climb = match &method {
        Method::OptimisedLoad { params, .. } => params.climb,
        Method::Pocket { pk, .. } => pk.direction == CutDirection::Climb,
    };
    let lv = layout(input, p, h.top, h.bottom, step, flats, &method, tol);
    // Starting stock (D15b §1.10).
    let mut stock = match (p.geometry.rest_previous, input.in_process) {
        (true, Some(s)) => s.clone(),
        _ => LayeredStock::planar(&stock_box),
    };
    let outline = Region2::rect(
        stock_box.min.x(),
        stock_box.min.y(),
        stock_box.max.x(),
        stock_box.max.y(),
    );
    let mut region = outline.offset(r);
    if let (Some(b), Some(bp)) = (&boundary, &p.geometry.boundary) {
        region = apply_boundary(&region, b, bp, r);
    }
    let sectioner = Sectioner::new(g, tol)
        .map_err(|m| fail(vec![Diagnostic::error(&format!("cam.{ty}.section"), m)]))?;
    // Forbidden regions, accumulated from the top down over every level.
    let mut heights: Vec<f64> = lv.iter().map(|l| l.z).collect();
    heights.sort_by(|a, b| b.total_cmp(a));
    heights.dedup();
    let mut forbidden: Vec<(f64, Region2)> = Vec::new();
    // The sections are joined first and grown once: joining grown
    // regions whose edges nearly coincide leaves notches.
    let mut acc_sec = Region2::empty();
    for &z in &heights {
        let mut s = sectioner
            .section(z - sta + DELTA)
            .map_err(|m| fail(vec![Diagnostic::error(&format!("cam.{ty}.section"), m)]))?;
        acc_sec = if acc_sec.is_empty() {
            s.region.clone()
        } else {
            acc_sec.union(&s.region)
        };
        s.region = acc_sec.clone();
        let f = grown(
            &s,
            r,
            stl,
            &drive_ids,
            &excluded_ids,
            p.geometry.check_faces.is_some(),
        );
        forbidden.push((z, f));
    }
    let forbidden_at = |z: f64| -> &Region2 {
        &forbidden
            .iter()
            .find(|(k, _)| *k == z)
            .expect("every level has a forbidden region")
            .1
    };
    let turn = if climb { Turn::Left } else { Turn::Right };
    let mut safe: Vec<Region2> = Vec::new();
    let mut cuts: Vec<Cut> = Vec::new();
    let (mut incomplete, mut fell_back, mut unreachable) = (false, false, false);
    for l in &lv {
        let centres = region.subtract(forbidden_at(l.z));
        let reach = centres.offset(r);
        let mut mat = rest_material(&stock.above(l.z), &reach, tol);
        if let (true, Method::OptimisedLoad { fine: Some(f), .. }) = (l.fine, &method) {
            // Only steps wider than the fine step-down (D15b §4.1).
            mat = mat.offset(-0.5 * f);
            if !mat.is_empty() {
                mat = mat.offset(0.5 * f).intersect(&reach);
            }
        }
        if matches!(method, Method::Pocket { .. }) && !l.fine {
            // Material that should go but the tool cannot reach.
            let section = sectioner
                .section(l.z - sta + DELTA)
                .map(|s| s.region.offset(stl.max(0.0)))
                .unwrap_or_default();
            let want = stock.above(l.z).intersect(&outline).subtract(&section);
            // Inside corners are left by any round tool; a part that holds
            // a disc of R/4 is a gap the tool cannot enter.
            let left = want.subtract(&reach).offset(-0.25 * r);
            if !left.is_empty() && left.area() > tol * tol {
                unreachable = true;
            }
        }
        if mat.is_empty() {
            continue;
        }
        let clear = centres.intersect(&mat.offset(r));
        let parts: Vec<Region2> = areas(&clear)
            .into_iter()
            .filter(|a| a.area() > tol * tol)
            .collect();
        let mut swept = Vec::new();
        for a in &parts {
            match &method {
                Method::OptimisedLoad { params, .. } => {
                    let out = plan_area(a, Some(&mat), params, &p.linking.ramp, tol, &mut safe);
                    incomplete |= out.incomplete;
                    fell_back |= out.fell_back;
                    swept.push(out.done.intersect(&mat));
                    emit_plan(&out.plan, l, turn, &mut cuts);
                }
                Method::Pocket { pk, stepover } => {
                    safe.push(a.clone());
                    let si = safe.len() - 1;
                    let mut prev_end = None;
                    for path in area_passes(a, pk, *stepover, p.passes.pass_layout, tol) {
                        let mut path = path;
                        if path.closed
                            && let Some(q) = prev_end
                        {
                            path = path.rotated(path.closest(q).0, 1e-9);
                        }
                        prev_end = Some(path.end());
                        cuts.push(Cut {
                            area: Some(si),
                            ..Cut::flat(path, l.z, turn, l.from)
                        });
                    }
                    swept.push(a.offset(r - tol));
                }
            }
        }
        stock.remove_swept(l.z, &union_all(swept));
    }
    if fell_back {
        diags.push(Diagnostic::warning(
            "cam.linking.ramp_fallback",
            "the helix does not fit in a region; the tool plunges there",
        ));
    }
    if incomplete {
        diags.push(Diagnostic::warning(
            &format!("cam.{ty}.incomplete"),
            "material above the tolerance is left after the path search stopped",
        ));
    }
    if unreachable {
        diags.push(Diagnostic::warning(
            &format!("cam.{ty}.region.unreachable"),
            "part of a level is narrower than the tool and is left uncut",
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

/// The clearing method, the step-down and whether flats add levels.
fn method(
    p: &ResolvedParams,
    kind: ToolpathTypeId,
    r: f64,
    d: f64,
) -> Result<(Method, Option<f64>, bool), CamError> {
    if let Some(a) = p.optimised_load_3d() {
        let params = OlParams {
            min_radius: a.min_cutting_radius,
            climb: a.direction == CutDirection::Climb,
            ..OlParams::new(r, a.optimal_load)
        };
        return Ok((
            Method::OptimisedLoad {
                params,
                fine: a.fine_step_down,
            },
            a.step_down,
            a.flat_detection,
        ));
    }
    if let Some(pk) = p.pocket_3d() {
        return Ok((
            Method::Pocket {
                pk: pk.pocket.clone(),
                stepover: pk.pocket.stepover.unwrap_or(0.4 * d),
            },
            pk.pocket.step_down,
            pk.flat_detection,
        ));
    }
    Err(CamError::Unavailable(format!(
        "{kind} is not a 3-D roughing type"
    )))
}

/// The levels in cutting order (D15b §4): even levels and flat levels top
/// down; fine levels between two main levels after the lower one.
#[allow(clippy::too_many_arguments)]
fn layout(
    input: &StrategyInput,
    p: &ResolvedParams,
    top: f64,
    bottom: f64,
    step: Option<f64>,
    flats: bool,
    method: &Method,
    tol: f64,
) -> Vec<Level> {
    let sta = p.passes.stock_to_leave_axial;
    let mut main = levels(top, bottom, step, None);
    if flats {
        for b in &input.geometry.bodies {
            let BodyGeom::Brep(body) = &b.geom else {
                continue;
            };
            for (fi, f) in body.faces.iter().enumerate() {
                let s = &body.surfaces[f.surface as usize];
                if !is_horizontal_plane(s) {
                    continue;
                }
                let Surface::Plane { frame } = s else {
                    continue;
                };
                // Upward-facing flats only: floors and shelves.
                let up = (frame.z.get().z() > 0.0) == f.same_sense;
                let z = face_z_range(body, fi as u32, tol).hi + sta;
                if up && z > bottom + 1e-9 && z < top - 1e-9 {
                    main.push(z);
                }
            }
        }
    }
    main.sort_by(|a, b| b.total_cmp(a));
    main.dedup_by(|a, b| (*a - *b).abs() <= 1e-9);
    let fine = match method {
        Method::OptimisedLoad { fine: Some(f), .. } => Some(*f),
        _ => None,
    };
    let mut out = Vec::new();
    for (i, &z) in main.iter().enumerate() {
        let from = if i == 0 { top } else { main[i - 1] };
        out.push(Level {
            z,
            from,
            fine: false,
        });
        if let (Some(f), true) = (fine, i > 0) {
            let upper = main[i - 1];
            for zf in levels(upper, z, Some(f), None) {
                if zf > z + 1e-9 {
                    out.push(Level {
                        z: zf,
                        from: upper,
                        fine: true,
                    });
                }
            }
        }
    }
    out
}

/// The section grown into the region the tool centre must avoid. Drive
/// faces get `stock_to_leave`; check faces none (D15b §9, decision 6).
fn grown(
    s: &Section,
    r: f64,
    stl: f64,
    drive: &BTreeSet<String>,
    excluded: &BTreeSet<String>,
    has_check: bool,
) -> Region2 {
    let all_drive = !has_check
        && s.pieces
            .iter()
            .all(|(_, id)| drive.contains(id) || excluded.contains(id));
    if all_drive || stl == 0.0 {
        return s.region.offset(r + stl);
    }
    let base = s.region.offset(r + stl.min(0.0));
    let (extra, which): (f64, bool) = if stl > 0.0 {
        (r + stl, true)
    } else {
        (r, false)
    };
    let bands: Vec<Region2> = s
        .pieces
        .iter()
        .filter(|(_, id)| drive.contains(id) == which)
        .map(|(pc, _)| swept_piece(pc, extra))
        .collect();
    base.union(&union_all(bands))
}

/// The region within `w` of a piece (arcs as chords, grown by their
/// sagitta so nothing within `w` is missed).
fn swept_piece(p: &Piece, w: f64) -> Region2 {
    let mut parts = Vec::new();
    for s in &p.segs {
        let path = Path2::open(vec![*s]);
        let n = (s.length() / (0.25 * w).max(1e-3)).ceil().clamp(1.0, 256.0) as usize;
        let len = path.length();
        let pts: Vec<_> = (0..=n)
            .map(|k| path.at(len * k as f64 / n as f64).0)
            .collect();
        for q in pts.windows(2) {
            parts.push(stadium(q[0], q[1], w * 1.01));
        }
    }
    union_all(parts)
}

/// Adds the cuts of one optimised-load area plan at level `l`.
fn emit_plan(plan: &AreaPlan, l: &Level, turn: Turn, cuts: &mut Vec<Cut>) {
    if let Some((path, s)) = &plan.entry {
        cuts.push(Cut {
            area: Some(*s),
            ..Cut::flat(path.clone(), l.z, turn, l.from)
        });
    }
    for (path, s) in &plan.passes {
        cuts.push(Cut {
            area: Some(*s),
            ..Cut::flat(path.clone(), l.z, turn, l.z)
        });
    }
    let mut prev_end = cuts.last().map(|c| c.path.end());
    for (path, s) in &plan.finish {
        let mut path = path.clone();
        if let Some(q) = prev_end {
            path = path.rotated(path.closest(q).0, 1e-9);
        }
        prev_end = Some(path.end());
        cuts.push(Cut {
            area: Some(*s),
            ..Cut::flat(path, l.z, turn, l.z)
        });
    }
}
