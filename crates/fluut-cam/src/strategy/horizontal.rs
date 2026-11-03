// SPDX-License-Identifier: MIT OR Apache-2.0

//! `horizontal` (D09 §4.5, D15b §5.2): finishes the flat faces only.
//! Finishing type.
//!
//! Flats are the planar drive faces facing +Z with Z in [bottom, top];
//! flats at equal Z are cleared together. The area of a flat is its face
//! region offset by −R (the tool stays on the flat), limited to where the
//! tool can reach the flat's height without touching anything else: the
//! waterline just above the flat with the cutter grown by the stock to
//! leave ([`crate::waterline`]), which keeps R + stock from vertical walls
//! and is exact for sloped ones. Each area is cleared at the flat's Z plus
//! the axial stock with offset rings (outside in) or a raster, as
//! `pocket_2d`, and linked with the 2-D linker.

use super::contour::waterline_grow;
use crate::drive::{areas, floor_regions, is_horizontal_plane, nest};
use crate::follow::setup;
use crate::layout::{layout_even, layout_fixed};
use crate::link::{Cut, LinkCx, Turn, link};
use crate::moves::finish;
use crate::path2::Path2;
use crate::region::{Inside, Loop2, Region2, Seg2};
use crate::scallop::is_mesh;
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, tag};
use crate::waterline::Waterline;
use fluut_geom::{Point2, Surface, Vec2};
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{CutDirection, PassLayout, Pattern};
use fluut_model::{Diagnostic, Sel};
use fluut_path::ToolpathResult;

#[derive(Debug)]
pub struct Horizontal;

/// Safety cap on rings per area.
const MAX_RINGS: usize = 10_000;

impl ToolpathType for Horizontal {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Horizontal
    }

    fn role(&self) -> Role {
        Role::Finish
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        let mut st = setup(input, "horizontal")?;
        let Some(hz) = st.params.horizontal().cloned() else {
            return Err(CamError::Unavailable("not a horizontal toolpath".into()));
        };
        let g = input.geometry;
        let tol = st.params.passes.machining_tolerance;
        let h = st.heights;
        let tool = input.tool;
        let r = tool.radius();
        let stl = st.params.passes.stock_to_leave;
        let axial = st.params.passes.stock_to_leave_axial;
        let stepover = st.stepover(input, None, hz.stepover)?;
        if st.drive.iter().any(|d| is_mesh(d.0)) {
            st.diags.push(
                Diagnostic::warning(
                    "param.horizontal.drive_faces.mesh",
                    "the drive faces are a mesh: flats are found by the mesh heuristic",
                )
                .at("geometry.drive_faces"),
            );
        }
        // Flats: planar drive faces facing +Z within [bottom, top], grouped
        // by Z (highest first).
        let ztol = g.tol_model().max(1e-6);
        let mut flats: Vec<(f64, Vec<String>)> = Vec::new();
        for (b, f, id) in &st.drive {
            let face = &b.faces[*f as usize];
            let s = &b.surfaces[face.surface as usize];
            let Surface::Plane { frame } = s else {
                continue;
            };
            let up = frame.z.get().z() * if face.same_sense { 1.0 } else { -1.0 };
            if !is_horizontal_plane(s) || up <= 0.0 {
                continue;
            }
            // The top of the flat's edges, not its plane (`flat_z`): edges
            // a little above the plane (a model gap) would hold a flat
            // tool up within R of them, and the walls would cover the flat.
            let z = crate::geometry::face_z_range(b, *f, 1e-3).hi;
            if z < h.bottom - ztol || z > h.top + ztol {
                continue;
            }
            match flats.iter_mut().find(|(fz, _)| (fz - z).abs() <= ztol) {
                Some((_, ids)) => ids.push(id.clone()),
                None => flats.push((z, vec![id.clone()])),
            }
        }
        flats.sort_by(|a, b| b.0.total_cmp(&a.0));
        if flats.is_empty() {
            st.diags.push(Diagnostic::warning(
                "cam.horizontal.no_flats",
                "there are no flat faces in the selection between bottom and top",
            ));
        }
        let region = st.region(input, waterline_grow(&st, input))?;
        let wl = Waterline::new(&st.q, &region, tol, 0.5 * r, 0.5 * r);
        let climb = hz.direction == CutDirection::Climb;
        let turn = if climb { Turn::Left } else { Turn::Right };
        let orient = |p: Path2| if climb { p } else { p.reversed() };
        let mut safe: Vec<Region2> = Vec::new();
        let mut cuts: Vec<Cut> = Vec::new();
        let mut prev_end: Option<Point2> = None;
        let zs: Vec<f64> = flats.iter().map(|(z, _)| z + stl).collect();
        let walls = wl.levels(&zs);
        for ((z, ids), walls) in flats.iter().zip(walls) {
            let sels: Vec<Sel> = ids.iter().map(|i| Sel::Face(i.clone())).collect();
            let faces = floor_regions(g, &sels, &[], tol / 4.0)
                .map_err(|d| CamError::Invalid(tag(vec![d], input.toolpath)))?;
            let mut flat = Region2::empty();
            for (f, _) in faces {
                flat = if flat.is_empty() { f } else { flat.union(&f) };
            }
            // Where the tool reaches the flat's height (+ stock).
            let material = nest(
                walls
                    .iter()
                    .filter(|c| c.pts.len() >= 3)
                    .map(|c| polygon(c.pts.iter().map(|w| w.p)))
                    .collect(),
            );
            let reach = region.subtract(&material);
            let area = flat.offset(-r).intersect(&reach);
            let zc = z + axial;
            for a in areas(&area) {
                let ai = safe.len();
                let mut paths: Vec<Path2> = match hz.pattern {
                    Pattern::Offset => offset_rings(&a, stepover, tol)
                        .iter()
                        .flatten()
                        .map(|l| orient(Path2::from_loop(l)))
                        .collect(),
                    Pattern::Raster => raster(
                        &a,
                        hz.pass_angle_deg,
                        stepover,
                        st.params.passes.pass_layout,
                        tol,
                    )
                    .into_iter()
                    .chain(a.loops.iter().map(|l| orient(Path2::from_loop(l))))
                    .collect(),
                };
                for path in &mut paths {
                    if path.closed
                        && let Some(q) = prev_end
                    {
                        *path = path.rotated(path.closest(q).0, 1e-9);
                    }
                    prev_end = Some(path.end());
                    cuts.push(Cut {
                        area: Some(ai),
                        ..Cut::flat(path.clone(), zc, turn, zc)
                    });
                }
                safe.push(a);
            }
        }
        let p = &st.params;
        let mut diags = std::mem::take(&mut st.diags);
        let cx = LinkCx {
            heights: h,
            material_top: input.env.stock_top.max(h.top),
            linking: &p.linking,
            tool_diameter: tool.diameter,
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
            diagnostics: tag(diags, input.toolpath),
        })
    }
}

/// A closed polyline as a loop of lines.
fn polygon(pts: impl Iterator<Item = Point2>) -> Loop2 {
    let mut pts: Vec<(f64, f64)> = pts.map(|p| (p.x(), p.y())).collect();
    pts.dedup();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    let mut r = Region2::polygon(&pts);
    r.loops.pop().unwrap_or(Loop2 { verts: Vec::new() })
}

/// Zig-zag raster lines across `a` at `angle_deg` (D15 §4.1).
fn raster(a: &Region2, angle_deg: f64, s: f64, layout: PassLayout, tol: f64) -> Vec<Path2> {
    let (sn, cs) = libm::sincos(angle_deg.to_radians());
    let u = Vec2::new(cs, sn);
    let n = Vec2::new(-sn, cs);
    let (lo, hi) = a.support(n);
    let pos = match layout {
        PassLayout::Even => layout_even(lo, hi, s),
        PassLayout::FixedFromEdge => layout_fixed(lo, hi, s),
    };
    let mut out = Vec::new();
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
        out.extend(lines);
    }
    out
}

/// Offset rings of the tool-centre region `a`, outermost first, with the
/// half-step rings where a part runs out between two rings (as
/// `pocket_2d`, D09 §2.2).
fn offset_rings(a: &Region2, s: f64, tol: f64) -> Vec<Vec<Loop2>> {
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
        let extra: Vec<Loop2> = half
            .loops
            .iter()
            .filter(|l| {
                l.segs()
                    .any(|sg| left.contains(sg.point_at(0.5)) != Inside::Out)
            })
            .cloned()
            .collect();
        if !extra.is_empty() {
            out.push(extra);
        }
    }
    out
}
