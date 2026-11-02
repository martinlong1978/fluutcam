// SPDX-License-Identifier: MIT OR Apache-2.0

//! `trace` (D09 §2.5, D15b §2.3): follows curves at the heights or at their
//! own Z, with an optional side offset.

use super::contour_2d::{offset_open, selection_z};
use crate::chain::chain_pieces;
use crate::drive::{curve_pieces, curve_points};
use crate::layout::levels;
use crate::link::{Cut, LinkCx, StockFloor, Turn, link_over};
use crate::moves::finish;
use crate::path2::Path2;
use crate::region::Region2;
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, prepare, tag};
use fluut_geom::{Point2, Point3};
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{DepthMode, Order, TraceDirection, resolve};
use fluut_path::ToolpathResult;

#[derive(Debug)]
pub struct Trace;

/// Spacing of the Z samples along a pass in `curve` mode, mm.
const Z_STEP: f64 = 2.0;

impl ToolpathType for Trace {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Trace
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
        let field = "geometry.drive_curves";
        let pieces = curve_pieces(g, &r0.geometry.drive_curves, tol / 4.0, field)
            .map_err(|d| fail(vec![d]))?;
        let chains = chain_pieces(pieces, g.tol_model().max(1e-5));
        let prep = prepare(input, selection_z(&chains)).map_err(|e| match e {
            CamError::Invalid(d) => fail(d),
            e => e,
        })?;
        let mut diags = prep.warnings;
        let p = &prep.params;
        let h = prep.heights;
        let Some(t) = p.trace().cloned() else {
            return Err(CamError::Unavailable("not a trace toolpath".into()));
        };
        if chains.is_empty() {
            diags.push(Diagnostic::error(
                "cam.geometry.drive.empty",
                "the drive selection gives no curves to trace",
            ));
            return Err(fail(diags));
        }
        let curve = t.depth_mode == DepthMode::Curve;
        let polylines = if curve {
            curve_points(g, &r0.geometry.drive_curves, tol, field).map_err(|d| fail(vec![d]))?
        } else {
            Vec::new()
        };
        // Passes per chain: the projected chain offset to the side.
        let mut per_chain: Vec<Vec<(Path2, Turn)>> = Vec::new();
        for ch in &chains {
            let d = t.side_offset;
            let mut v = Vec::new();
            if ch.path.closed {
                let ccw = ch.path.to_loop().is_ccw();
                let reg = Region2 {
                    loops: vec![ch.ccw_loop()],
                };
                let res = if d == 0.0 { reg } else { reg.offset(d) };
                // Outward of a counter-clockwise loop is to the right.
                let turn = if (d > 0.0) == ccw {
                    Turn::Right
                } else {
                    Turn::Left
                };
                for l in res.loops.iter().filter(|l| l.is_ccw()) {
                    let p = Path2::from_loop(l);
                    v.push((if ccw { p } else { p.reversed() }, turn));
                }
            } else {
                let turn = if d >= 0.0 { Turn::Left } else { Turn::Right };
                for p in offset_open(&ch.path, d) {
                    v.push((p, turn));
                }
            }
            if v.is_empty() {
                diags.push(Diagnostic::warning(
                    "cam.trace.offset_empty",
                    format!("a chain vanishes under the side offset {d} mm"),
                ));
            }
            if t.direction == TraceDirection::Reversed {
                v = v
                    .into_iter()
                    .map(|(p, turn)| (p.reversed(), flip(turn)))
                    .collect();
            }
            per_chain.push(v);
        }
        let steps = |top: f64, bottom: f64| {
            levels(
                top,
                bottom,
                t.step_down,
                t.depth_count.map(|n| n.max(1) as usize),
            )
        };
        // Heights mode: absolute Zs. Curve mode: offsets below the curve.
        let zs = if curve {
            steps(0.0, -t.curve_depth)
        } else {
            steps(h.top, h.bottom)
        };
        let mut below = false;
        let mut cuts = Vec::new();
        let mut prev_end: Option<Point2> = None;
        let mut emit = |ci: usize, li: usize, cuts: &mut Vec<Cut>| {
            for (path, turn) in &per_chain[ci] {
                let mut path = path.clone();
                let mut turn = *turn;
                if let Some(q) = prev_end
                    && t.direction == TraceDirection::Any
                {
                    if path.closed {
                        path = path.rotated(path.closest(q).0, 1e-9);
                    } else if path.end().distance(q) < path.start().distance(q) {
                        path = path.reversed();
                        turn = flip(turn);
                    }
                }
                prev_end = Some(path.end());
                let cut = if curve {
                    let off = zs[li];
                    let prev = if li == 0 { 0.0 } else { zs[li - 1] };
                    let profile = z_profile(&path, &polylines, off);
                    below |= profile.iter().any(|z| z.1 < input.env.stock_bottom - 1e-9);
                    let z0 = profile[0].1;
                    let lo = profile.iter().map(|z| z.1).fold(f64::INFINITY, f64::min);
                    let flat = profile.iter().all(|z| (z.1 - z0).abs() <= 1e-12);
                    Cut {
                        profile: if flat { Vec::new() } else { profile },
                        ..Cut::flat(path, if flat { z0 } else { lo }, turn, z0 - off + prev)
                    }
                } else {
                    let ramp_from = if li == 0 { h.top } else { zs[li - 1] };
                    Cut::flat(path, zs[li], turn, ramp_from)
                };
                cuts.push(cut);
            }
        };
        match p.linking.order {
            Order::ByLevel => {
                for li in 0..zs.len() {
                    for ci in 0..chains.len() {
                        emit(ci, li, &mut cuts);
                    }
                }
            }
            Order::ByArea => {
                for ci in 0..chains.len() {
                    for li in 0..zs.len() {
                        emit(ci, li, &mut cuts);
                    }
                }
            }
        }
        if below {
            diags.push(Diagnostic::warning(
                "cam.trace.below_stock",
                "the path goes below the stock bottom",
            ));
        }
        let top_z = cuts
            .iter()
            .flat_map(|c| c.profile.iter().map(|z| z.1).chain([c.z]))
            .fold(h.top, f64::max);
        let cx = LinkCx {
            heights: h,
            material_top: input.env.stock_top.max(top_z),
            linking: &p.linking,
            tool_diameter: input.tool.diameter,
            tol,
            safe: &[],
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

fn flip(t: Turn) -> Turn {
    match t {
        Turn::Left => Turn::Right,
        Turn::Right => Turn::Left,
    }
}

/// `(arc length, z)` along `path`: the Z of the nearest point (in XY) of
/// the curves, plus `off`, at every segment end and at most [`Z_STEP`]
/// apart.
fn z_profile(path: &Path2, curves: &[Vec<Point3>], off: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut acc = 0.0;
    let z = |q: Point2| nearest_z(curves, q) + off;
    out.push((0.0, z(path.start())));
    for s in &path.segs {
        let len = s.length();
        let n = (len / Z_STEP).ceil().max(1.0) as usize;
        for k in 1..=n {
            let f = k as f64 / n as f64;
            let q = if k == n { s.b } else { s.point_at(f) };
            out.push((acc + len * f, z(q)));
        }
        acc += len;
    }
    out
}

/// Z of the point of the polylines nearest to `q` in XY.
fn nearest_z(curves: &[Vec<Point3>], q: Point2) -> f64 {
    let mut best = (f64::INFINITY, 0.0);
    for c in curves {
        for w in c.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (ax, ay) = (b.x() - a.x(), b.y() - a.y());
            let l2 = ax * ax + ay * ay;
            let t = if l2 > 0.0 {
                (((q.x() - a.x()) * ax + (q.y() - a.y()) * ay) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (px, py) = (a.x() + ax * t, a.y() + ay * t);
            let d = (px - q.x()).powi(2) + (py - q.y()).powi(2);
            if d < best.0 {
                best = (d, a.z() + (b.z() - a.z()) * t);
            }
        }
        if let [p] = c.as_slice() {
            let d = (p.x() - q.x()).powi(2) + (p.y() - q.y()).powi(2);
            if d < best.0 {
                best = (d, p.z());
            }
        }
    }
    best.1
}
