// SPDX-License-Identifier: MIT OR Apache-2.0

//! `contour_2d` (D09 §2.4, D15 §3): profiles along drive chains at one or
//! more depths, with finishing and spring passes, compensation and tabs.

use crate::chain::{Chain, chain_pieces};
use crate::drive::{curve_pieces, wall_pieces};
use crate::layout::levels;
use crate::link::{Cut, LinkCx, StockFloor, Turn, link_over};
use crate::moves::finish;
use crate::path2::Path2;
use crate::region::{Region2, RegionOp, Seg2, guard};
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, prepare, tag};
use cavalier_contours::polyline::{PlineCreation, PlineSource, PlineSourceMut, Polyline};
use fluut_geom::Point2;
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{
    Compensation, Contour2d as Params, CutDirection, Side, TabMode, resolve,
};
use fluut_path::{CompSide, ToolpathResult};

#[derive(Debug)]
pub struct Contour2d;

/// The selection Z range of chains, for the `selection` height reference.
pub(crate) fn selection_z(chains: &[Chain]) -> Option<(f64, f64)> {
    chains.iter().fold(None, |acc, c| {
        Some(match acc {
            None => (c.z.lo, c.z.hi),
            Some((lo, hi)) => (f64::min(lo, c.z.lo), f64::max(hi, c.z.hi)),
        })
    })
}

impl ToolpathType for Contour2d {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Contour2d
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
            wall_pieces(
                g,
                &r0.geometry.drive_faces,
                &r0.geometry.excluded_faces,
                tol / 4.0,
            )
            .map_err(|d| fail(vec![d]))?,
        );
        let chains = chain_pieces(pieces, g.tol_model().max(1e-5));
        let prep = prepare(input, selection_z(&chains)).map_err(|e| match e {
            CamError::Invalid(d) => fail(d),
            e => e,
        })?;
        let mut diags = prep.warnings;
        let Some(c) = prep.params.contour().cloned() else {
            return Err(CamError::Unavailable("not a contour_2d toolpath".into()));
        };
        let mut errors = Vec::new();
        if chains.is_empty() {
            errors.push(Diagnostic::error(
                "cam.geometry.drive.empty",
                "the drive selection gives no curves to profile",
            ));
        }
        for ch in &chains {
            if ch.self_intersects() {
                errors.push(Diagnostic::error(
                    "cam.geometry.chain_self_intersects",
                    "a drive chain intersects itself",
                ));
            }
            let side = side_of(&c, ch);
            let wrong = if ch.path.closed {
                matches!(side, Side::Left | Side::Right)
            } else {
                matches!(side, Side::Inside | Side::Outside)
            };
            if wrong {
                errors.push(
                    Diagnostic::error(
                        "param.contour_2d.side.closure",
                        "inside and outside need closed chains; left and right need open ones",
                    )
                    .at("passes.side"),
                );
            }
        }
        if c.start_point.is_some() && chains.iter().all(|ch| !ch.path.closed) {
            diags.push(
                Diagnostic::warning(
                    "param.contour_2d.start_point.open",
                    "start_point is ignored: every chain is open",
                )
                .at("passes.start_point"),
            );
        }
        if !errors.is_empty() {
            diags.extend(errors);
            return Err(fail(diags));
        }
        let p = &prep.params;
        let h = prep.heights;
        let d = input.tool.diameter;
        let r = input.tool.radius();
        let stl = p.passes.stock_to_leave;
        let base = match c.compensation {
            Compensation::Control => stl,
            _ => r + stl,
        };
        let bottom = h.bottom + p.passes.stock_to_leave_axial;
        let zs = levels(
            h.top,
            bottom,
            c.step_down,
            c.depth_count.map(|n| n.max(1) as usize),
        );
        let n_fin = c.finishing_pass_count.clamp(0, 10) as usize;
        let start_point = c.start_point.map(|q| input.point_xy(q));
        let tab_top = h.bottom + c.tabs.height;
        // Offset paths per chain and finishing index (0 = final offset).
        let mut per_chain = Vec::new();
        for ch in &chains {
            let side = side_of(&c, ch);
            let mut offs = Vec::new();
            for i in 0..=n_fin {
                let off = if side == Side::On {
                    0.0
                } else {
                    base + i as f64 * c.finishing_stepover
                };
                let paths = offset_chain(ch, side, off, c.direction);
                if paths.is_empty() {
                    diags.push(Diagnostic::warning(
                        "cam.contour_2d.offset_empty",
                        format!(
                            "an offset of {off} mm vanishes: the chain is too small for the tool"
                        ),
                    ));
                }
                offs.push(paths);
                if side == Side::On {
                    break;
                }
            }
            let tabs = tab_points(input, &c, ch, d, &mut diags);
            per_chain.push((offs, tabs));
        }
        let comp = |t: Turn| match c.compensation {
            Compensation::Computer => None,
            _ => Some(match t {
                Turn::Left => CompSide::Left,
                Turn::Right => CompSide::Right,
            }),
        };
        let last = zs.len() - 1;
        let tab_on = c.tabs.mode != TabMode::Off;
        let ramp_len = |z: f64| (tab_top - z) / libm::tan(c.tabs.ramp_angle_deg.to_radians());
        let mut cuts = Vec::new();
        let mut prev_end: Option<Point2> = None;
        let mut overlap = false;
        let mut emit = |ci: usize, li: usize, fin: &[usize], cuts: &mut Vec<Cut>| {
            let (offs, tabs) = &per_chain[ci];
            let z = zs[li];
            let ramp_from = if li == 0 { h.top } else { zs[li - 1] };
            for &i in fin {
                let Some(paths) = offs.get(i) else { continue };
                for (path, turn) in paths {
                    let mut path = path.clone();
                    if path.closed
                        && let Some(q) = start_point.or(prev_end)
                    {
                        path = path.rotated(path.closest(q).0, 1e-9);
                    }
                    let mut profile = Vec::new();
                    if tab_on && z < tab_top && !tabs.is_empty() {
                        let (pp, pr, ov) = tab_profile(
                            &path,
                            tabs,
                            z,
                            tab_top,
                            0.5 * (c.tabs.width + d),
                            ramp_len(z),
                        );
                        path = pp;
                        profile = pr;
                        overlap |= ov;
                    }
                    prev_end = Some(path.end());
                    cuts.push(Cut {
                        path,
                        z,
                        profile,
                        tool_side: *turn,
                        comp: comp(*turn),
                        ramp_from,
                        area: None,
                    });
                }
            }
        };
        let fin_at = |li: usize| -> Vec<usize> {
            if li == last || c.finish_every_level {
                (0..=n_fin).rev().collect()
            } else {
                vec![0]
            }
        };
        let spring = c.spring_pass_count.clamp(0, 5) as usize;
        match p.linking.order {
            fluut_model::resolve::Order::ByLevel => {
                for li in 0..zs.len() {
                    for ci in 0..chains.len() {
                        emit(ci, li, &fin_at(li), &mut cuts);
                    }
                }
                for ci in 0..chains.len() {
                    for _ in 0..spring {
                        emit(ci, last, &[0], &mut cuts);
                    }
                }
            }
            fluut_model::resolve::Order::ByArea => {
                for ci in 0..chains.len() {
                    for li in 0..zs.len() {
                        emit(ci, li, &fin_at(li), &mut cuts);
                    }
                    for _ in 0..spring {
                        emit(ci, last, &[0], &mut cuts);
                    }
                }
            }
        }
        if overlap {
            diags.push(Diagnostic::warning(
                "cam.contour_2d.tabs.overlap",
                "tabs and their ramps overlap; overlapping tabs were merged",
            ));
        }
        let cx = LinkCx {
            heights: h,
            material_top: input.env.stock_top.max(h.top),
            linking: &p.linking,
            tool_diameter: d,
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

/// The side for a chain: as set, else `outside` (closed) or `right` (open).
fn side_of(c: &Params, ch: &Chain) -> Side {
    c.side.unwrap_or(if ch.path.closed {
        Side::Outside
    } else {
        Side::Right
    })
}

/// Tool-centre paths for one chain at offset `d` to `side`, oriented for
/// `dir` (spindle M3: climb keeps the material on the right of travel), each
/// with the side the tool is on.
pub(crate) fn offset_chain(
    ch: &Chain,
    side: Side,
    d: f64,
    dir: CutDirection,
) -> Vec<(Path2, Turn)> {
    let climb = dir == CutDirection::Climb;
    let turn = if climb || side == Side::On {
        Turn::Left
    } else {
        Turn::Right
    };
    let mut out = Vec::new();
    if ch.path.closed {
        let reg = Region2 {
            loops: vec![ch.ccw_loop()],
        };
        let res = match side {
            Side::Inside => reg.offset(-d),
            Side::Outside => reg.offset(d),
            _ => reg,
        };
        let ccw = match side {
            Side::Inside => climb,
            Side::Outside => !climb,
            _ => true,
        };
        for l in res.loops.iter().filter(|l| l.is_ccw()) {
            let p = Path2::from_loop(l);
            out.push((if ccw { p } else { p.reversed() }, turn));
        }
    } else {
        let paths = match side {
            Side::Left => offset_open(&ch.path, d),
            Side::Right => offset_open(&ch.path, -d),
            _ => vec![ch.path.clone()],
        };
        let keep = match side {
            Side::Left => climb,
            Side::Right => !climb,
            _ => true,
        };
        for p in paths {
            out.push((if keep { p } else { p.reversed() }, turn));
        }
    }
    out
}

/// Open path offset to the left by `d` (right when negative).
pub(crate) fn offset_open(p: &Path2, d: f64) -> Vec<Path2> {
    if d == 0.0 {
        return vec![p.clone()];
    }
    let mut pl: Polyline<f64> = Polyline::with_capacity(p.segs.len() + 1, false);
    for s in &p.segs {
        pl.add(s.a.x(), s.a.y(), s.bulge);
    }
    pl.add(p.end().x(), p.end().y(), 0.0);
    guard(RegionOp::Offset, || pl.parallel_offset(d))
        .unwrap_or_else(|e| e.raise())
        .iter()
        .map(|o| {
            let v: Vec<_> = o.iter_vertexes().collect();
            let segs = v
                .windows(2)
                .map(|w| Seg2 {
                    a: Point2::new(w[0].x, w[0].y),
                    b: Point2::new(w[1].x, w[1].y),
                    bulge: w[0].bulge,
                })
                .filter(|s| s.a.distance(s.b) > 1e-12)
                .collect();
            Path2::open(segs)
        })
        .filter(|p| !p.is_empty())
        .collect()
}

/// Tab centres on a chain (D15 §3.2): evenly by count or distance, or the
/// stored points snapped to the chain.
fn tab_points(
    input: &StrategyInput,
    c: &Params,
    ch: &Chain,
    d: f64,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Point2> {
    let path = &ch.path;
    let len = path.length();
    let at = |s: f64| path.at(s).0;
    match c.tabs.mode {
        TabMode::Off => Vec::new(),
        TabMode::Count => {
            let n = c.tabs.count.max(1) as usize;
            (0..n)
                .map(|k| {
                    if path.closed {
                        at((k as f64 + 0.5) * len / n as f64)
                    } else {
                        at((k + 1) as f64 * len / (n + 1) as f64)
                    }
                })
                .collect()
        }
        TabMode::Distance => {
            let step = c.tabs.distance;
            let mut v = Vec::new();
            let mut s = 0.5 * step;
            while s < len && step > 0.0 {
                v.push(at(s));
                s += step;
            }
            v
        }
        TabMode::Points => c
            .tabs
            .points
            .iter()
            .filter_map(|&q| {
                let q = input.point_xy(q);
                let (s, dist) = path.closest(q);
                if dist > d {
                    diags.push(Diagnostic::warning(
                        "cam.contour_2d.tabs.moved",
                        format!("a tab point moved {dist:.3} mm to reach the chain"),
                    ));
                }
                (dist.is_finite()).then(|| at(s))
            })
            .collect(),
    }
}

/// The Z profile of a pass over its tabs: up to `top` over each tab, with
/// flats of half width `half` and ramps of length `ramp`. A closed path is
/// rotated so no tab straddles its start. Returns the path, the profile
/// and whether tabs overlapped.
fn tab_profile(
    path: &Path2,
    tabs: &[Point2],
    z: f64,
    top: f64,
    half: f64,
    ramp: f64,
) -> (Path2, Vec<(f64, f64)>, bool) {
    let mut path = path.clone();
    let len = path.length();
    let intervals = |p: &Path2| -> Vec<[f64; 4]> {
        let mut v: Vec<[f64; 4]> = tabs
            .iter()
            .map(|&t| {
                let s = p.closest(t).0;
                [s - half - ramp, s - half, s + half, s + half + ramp]
            })
            .collect();
        v.sort_by(|a, b| a[0].total_cmp(&b[0]));
        v
    };
    let mut iv = intervals(&path);
    if path.closed {
        for _ in 0..tabs.len() {
            let Some(w) = iv.iter().find(|i| i[0] < 0.0 || i[3] > len) else {
                break;
            };
            path = path.rotated(w[3].rem_euclid(len), 1e-9);
            iv = intervals(&path);
        }
    }
    let mut merged: Vec<[f64; 4]> = Vec::new();
    let mut overlap = false;
    for i in iv {
        match merged.last_mut() {
            Some(m) if i[0] < m[3] => {
                overlap = true;
                m[2] = m[2].max(i[2]);
                m[3] = m[3].max(i[3]);
            }
            _ => merged.push(i),
        }
    }
    let mut pts = vec![(f64::NEG_INFINITY, z)];
    for m in &merged {
        pts.extend([(m[0], z), (m[1], top), (m[2], top), (m[3], z)]);
    }
    pts.push((f64::INFINITY, z));
    let z_at = |s: f64| -> f64 {
        for w in pts.windows(2) {
            if s <= w[1].0 {
                if !w[0].0.is_finite() || !w[1].0.is_finite() || w[1].0 <= w[0].0 {
                    return w[1].1;
                }
                return w[0].1 + (w[1].1 - w[0].1) * (s - w[0].0) / (w[1].0 - w[0].0);
            }
        }
        z
    };
    let mut ss: Vec<f64> = pts
        .iter()
        .map(|p| p.0)
        .filter(|&s| s > 1e-9 && s < len - 1e-9)
        .collect();
    ss.insert(0, 0.0);
    ss.push(len);
    ss.dedup_by(|a, b| (*a - *b).abs() <= 1e-9);
    let profile = ss.into_iter().map(|s| (s, z_at(s))).collect();
    (path, profile, overlap)
}
