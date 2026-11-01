// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rotary types (D15b §6, D09 §5): `rotary_rough`, `rotary_along_x`,
//! `rotary_around_a` and `rotary_spiral`, in `rotary` Setups.
//!
//! Heights are radii (D15b §1.3); the region is [`x_start`, `x_end`] × A
//! (§1.4). CL points come from radial drops ([`crate::radial`]) on an angle
//! grid whose step keeps the chord within the tolerance at the top radius
//! (spec 04 §4.7.4). CL radii below `bottom` are raised to it.

use crate::cutter::Cutter;
use crate::drive::faces;
use crate::geometry::BodyGeom;
use crate::layout::{layout_even, layout_fixed, levels};
use crate::radial::{RLinks, RPos, Radial, RotOut, Turned, face_extent, grid_count};
use crate::region::{Inside, Region2};
use crate::stock::{LayeredStock, union_all, wrap};
use crate::surface::FaceSets;
use crate::toolpath::{
    CamError, Generated, Role, RotaryInfo, StrategyInput, ToolpathType, prepare, tag,
};
use fluut_geom::{Point2, Surface, Vec2};
use fluut_model::Diagnostic;
use fluut_model::params::{Sel, ToolpathTypeId};
use fluut_model::project::SetupKind;
use fluut_model::resolve::{
    CutDirection, ParallelDirection, PassLayout, RampKind, ResolvedHeights, ResolvedParams,
    RetractPolicy, RingLink, XEnv, resolve,
};
use fluut_path::ToolpathResult;
use std::collections::BTreeSet;
use std::f64::consts::TAU;

#[derive(Debug)]
pub struct RotaryRough;
#[derive(Debug)]
pub struct RotaryAlongX;
#[derive(Debug)]
pub struct RotaryAroundA;
#[derive(Debug)]
pub struct RotarySpiral;

macro_rules! rotary_type {
    ($t:ident, $role:expr, $f:ident) => {
        impl ToolpathType for $t {
            fn id(&self) -> ToolpathTypeId {
                ToolpathTypeId::$t
            }

            fn role(&self) -> Role {
                $role
            }

            fn allowed_setups(&self) -> &'static [SetupKind] {
                &[SetupKind::Rotary]
            }

            fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
                let cx = Cx::new(input)?;
                let r = $f(&cx);
                cx.done(r)
            }
        }
    };
}

rotary_type!(RotaryRough, Role::Rough, rough);
rotary_type!(RotaryAlongX, Role::Finish, along_x);
rotary_type!(RotaryAroundA, Role::Finish, around_a);
rotary_type!(RotarySpiral, Role::Finish, spiral);

/// What every rotary type resolves first.
struct Cx<'a> {
    input: &'a StrategyInput<'a>,
    p: ResolvedParams,
    h: ResolvedHeights,
    info: RotaryInfo,
    sets: FaceSets,
    tol: f64,
    /// The X range.
    x0: f64,
    x1: f64,
    diags: std::cell::RefCell<Vec<Diagnostic>>,
}

type Out = Result<RotOut, Vec<Diagnostic>>;

impl<'a> Cx<'a> {
    fn new(input: &'a StrategyInput<'a>) -> Result<Cx<'a>, CamError> {
        let tp = input.toolpath;
        let fail = |d: Vec<Diagnostic>| CamError::Invalid(tag(d, tp));
        let r0 = resolve(&tp.params, input.tool, input.material);
        let g = input.geometry;
        let geo = &r0.geometry;
        let tol = r0.passes.machining_tolerance.max(1e-4);
        let excluded = faces(g, &geo.excluded_faces, &[], "geometry.excluded_faces")
            .map_err(|d| fail(vec![d]))?;
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
        let check = match &geo.check_faces {
            None => None,
            Some(c) => Some(
                faces(g, c, &geo.excluded_faces, "geometry.check_faces")
                    .map_err(|d| fail(vec![d]))?,
            ),
        };
        let ids = |v: &[(&fluut_topo::BrepBody, u32, String)]| -> BTreeSet<String> {
            v.iter().map(|x| x.2.clone()).collect()
        };
        let sets = FaceSets {
            drive: if geo.drive_faces.is_empty() {
                BTreeSet::new()
            } else {
                ids(&drive)
            },
            check: check.as_deref().map(ids),
            excluded: ids(&excluded),
        };
        // `selection`: radii and X extent of the drive faces (D15b §1.3).
        let mut sel: Option<(f64, f64)> = None;
        let mut xs: Option<(f64, f64)> = None;
        for (b, f, _) in &drive {
            if let Some((rlo, rhi, xlo, xhi)) = face_extent(b, *f, tol) {
                sel = Some(sel.map_or((rlo, rhi), |(a, c)| (a.min(rlo), c.max(rhi))));
                xs = Some(xs.map_or((xlo, xhi), |(a, c)| (a.min(xlo), c.max(xhi))));
            }
        }
        let prep = prepare(input, sel).map_err(|e| match e {
            CamError::Invalid(d) => fail(d),
            e => e,
        })?;
        let mut diags = prep.warnings;
        let Some(info) = input.rotary else {
            diags.push(Diagnostic::error(
                "cam.rotary.setup",
                "rotary types need the frame of a rotary Setup",
            ));
            return Err(fail(diags));
        };
        if let Some(None) = info.machine_axis {
            diags.push(
                Diagnostic::error(
                    "param.rotary.machine_axis",
                    "the Setup's machine has no rotary axis",
                )
                .at("type"),
            );
            return Err(fail(diags));
        }
        let env = XEnv {
            stock: info.stock_x,
            model: if geo.drive_faces.is_empty() {
                info.model_x
            } else {
                xs.unwrap_or(info.model_x)
            },
        };
        let (xa, xb) = prep
            .params
            .geometry
            .x_range
            .clone()
            .expect("rotary types have an X range");
        let (x0, x1) = (env.resolve(&xa), env.resolve(&xb));
        if x1 <= x0 {
            diags.push(
                Diagnostic::error(
                    "param.geometry.x_range.order",
                    format!("x_end {x1} must be beyond x_start {x0}"),
                )
                .at("geometry.x_end"),
            );
            return Err(fail(diags));
        }
        if x0 < info.stock_x.0 - 1e-9 || x1 > info.stock_x.1 + 1e-9 {
            diags.push(
                Diagnostic::warning(
                    "param.geometry.x_range.stock",
                    format!(
                        "the X range {x0} to {x1} goes beyond the stock ends {} and {}",
                        info.stock_x.0, info.stock_x.1
                    ),
                )
                .at("geometry.x_start"),
            );
        }
        let mut h = prep.heights;
        h.bottom = h.bottom.max(0.0);
        Ok(Cx {
            input,
            p: prep.params,
            h,
            info,
            sets,
            tol,
            x0,
            x1,
            diags: std::cell::RefCell::new(diags),
        })
    }

    fn warn(&self, code: &str, msg: &str) {
        let mut d = self.diags.borrow_mut();
        if !d.iter().any(|x| x.code == code) {
            d.push(Diagnostic::warning(code, msg));
        }
    }

    fn cutter(&self) -> Result<Cutter, Vec<Diagnostic>> {
        let t = self.input.tool;
        Cutter::new(t.diameter, t.corner_radius()).ok_or_else(|| {
            vec![Diagnostic::error(
                "cam.rotary.tool",
                "the tool shape has no radial drop",
            )]
        })
    }

    fn radial<'t>(&'t self, t: &'t Turned) -> Result<Radial<'t>, Vec<Diagnostic>> {
        Radial::new(t, &self.sets, self.cutter()?, self.p.passes.stock_to_leave)
            .map(|q| q.with_base(self.input.geometry))
            .ok_or_else(|| {
                vec![Diagnostic::error(
                    "cam.rotary.tool",
                    "stock to leave is too negative for the tool",
                )]
            })
    }

    fn turned(&self, angles: Vec<f64>) -> Result<Turned, Vec<Diagnostic>> {
        Turned::new(self.input.geometry, angles)
            .map_err(|m| vec![Diagnostic::error("cam.rotary.geometry", m)])
    }

    fn out(&self) -> RotOut {
        RotOut::new(
            self.tol,
            RLinks {
                clearance: self.h.clearance,
                retract: self.h.retract,
                feed: self.h.feed,
                full: self.p.linking.retract == RetractPolicy::Full,
            },
        )
    }

    /// CL radius at `x` on grid angle `k`, raised to `bottom`.
    fn r_at(&self, q: &Radial, k: usize, x: f64) -> f64 {
        q.drop(k, x).map_or(self.h.bottom, |r| r.max(self.h.bottom))
    }

    /// `pts` with points added where the radial drop between two samples
    /// leaves their chord by more than the tolerance (kinks where the
    /// contact moves from a face to an edge, D09 §5.3).
    fn refine_ring(&self, q: &Radial, pts: Vec<RPos>) -> Vec<RPos> {
        let mut out = vec![pts[0]];
        for w in pts.windows(2) {
            self.refine_span(q, w[0], w[1], 0, &mut out);
        }
        out
    }

    fn refine_span(&self, q: &Radial, a: RPos, b: RPos, depth: u32, out: &mut Vec<RPos>) {
        if depth < 6 && (a.r - b.r).abs() > 1e-9 {
            let m = RPos {
                x: 0.5 * (a.x + b.x),
                r: 0.0,
                a: 0.5 * (a.a + b.a),
            };
            let r = q
                .drop_at(m.a.to_radians(), m.x)
                .map_or(self.h.bottom, |r| r.max(self.h.bottom));
            if (r - 0.5 * (a.r + b.r)).abs() > self.tol {
                let m = RPos { r, ..m };
                self.refine_span(q, a, m, depth + 1, out);
                self.refine_span(q, m, b, depth + 1, out);
                return;
            }
        }
        out.push(b);
    }

    /// `param.rotary.limits` (and, for one-sense types, the unwind rule)
    /// for A from `lo` to `hi` degrees.
    fn check_limits(&self, lo: f64, hi: f64) -> Result<(), Vec<Diagnostic>> {
        let Some(Some(axis)) = self.info.machine_axis else {
            return Ok(());
        };
        if axis.continuous {
            return Ok(());
        }
        if let Some((a, b)) = axis.limits_deg
            && (lo < a - 1e-9 || hi > b + 1e-9)
        {
            return Err(vec![
                Diagnostic::error(
                    "param.rotary.limits",
                    format!("A moves from {lo}° to {hi}°, beyond the axis limits {a}° to {b}°"),
                )
                .at("type"),
            ]);
        }
        Ok(())
    }

    fn layout(&self, a: f64, b: f64, s: f64) -> Vec<f64> {
        match self.p.passes.pass_layout {
            PassLayout::Even => layout_even(a, b, s),
            PassLayout::FixedFromEdge => layout_fixed(a, b, s),
        }
    }

    /// Stepover from a scallop height on a flat (D15b §6, OQ-05).
    fn scallop_step(&self, h: f64) -> f64 {
        let r = self.input.tool.corner_radius();
        2.0 * (h * (2.0 * r - h)).max(0.0).sqrt()
    }

    fn done(self, out: Out) -> Result<Generated, CamError> {
        let tp = self.input.toolpath;
        let mut diags = self.diags.into_inner();
        let out = match out {
            Ok(o) => o,
            Err(e) => {
                diags.extend(e);
                return Err(CamError::Invalid(tag(diags, tp)));
            }
        };
        let result = ToolpathResult::new(out.finish()).map_err(CamError::Path)?;
        Ok(Generated {
            result,
            diagnostics: tag(diags, tp),
        })
    }
}

/// A turn's worth of samples at `x` on grid angles, in the sense `dir`
/// (+1 or −1), from grid angle `k0`: `(k, A in degrees from a0)`.
fn ring_angles(n: usize, k0: usize, dir: f64, a0: f64) -> Vec<(usize, f64)> {
    (0..=n)
        .map(|i| {
            let k = if dir > 0.0 {
                (k0 + i) % n
            } else {
                (k0 + n - i % n) % n
            };
            (k, a0 + dir * 360.0 * i as f64 / n as f64)
        })
        .collect()
}

fn around_a(cx: &Cx) -> Out {
    let t = cx.p.rotary_around_a().cloned().expect("rotary_around_a");
    let s = match (t.stepover_x, t.scallop_height) {
        (_, Some(h)) => cx.scallop_step(h),
        (Some(s), None) => s,
        (None, None) => return Err(Vec::new()),
    };
    let xs = cx.layout(cx.x0, cx.x1, s);
    let one_sense = t.direction != ParallelDirection::Zigzag;
    if one_sense
        && let Some(Some(axis)) = cx.info.machine_axis
        && !axis.continuous
    {
        return Err(vec![
            Diagnostic::error(
                "param.rotary_around_a.direction.unwind",
                "climb and conventional turn A the same way on every ring and need a \
                 continuous axis; use zigzag",
            )
            .at("passes.direction"),
        ]);
    }
    let n = grid_count(cx.h.top, cx.tol);
    let turned = cx.turned((0..n).map(|k| TAU * k as f64 / n as f64).collect())?;
    let q = cx.radial(&turned)?;
    let r_tool = cx.input.tool.radius();
    let mut out = cx.out();
    let (mut a_lo, mut a_hi) = (0.0f64, 0.0f64);
    let mut a_end = 0.0;
    let mut k_end = 0;
    for (i, &x) in xs.iter().enumerate() {
        let dir = match t.direction {
            ParallelDirection::Climb => 1.0,
            ParallelDirection::Conventional => -1.0,
            ParallelDirection::Zigzag => {
                if i % 2 == 0 {
                    1.0
                } else {
                    -1.0
                }
            }
        };
        let samples = ring_angles(n, k_end, dir, a_end);
        // Helical link: the X step spread over the last tool radius of arc.
        let blend = match (t.ring_link, xs.get(i + 1)) {
            (RingLink::Helical, Some(&nx)) => {
                let span = (r_tool / cx.h.top.max(r_tool)).min(1.0) * 360.0 / TAU;
                Some((nx, span))
            }
            _ => None,
        };
        let pts: Vec<RPos> = samples
            .iter()
            .map(|&(k, a)| {
                let left = 360.0 - (a - a_end).abs();
                let xk = match blend {
                    Some((nx, span)) if left < span => x + (nx - x) * (1.0 - left / span),
                    _ => x,
                };
                RPos::new(xk, cx.r_at(&q, k, xk), a)
            })
            .collect();
        let pts = cx.refine_ring(&q, pts);
        match out.cur() {
            Some(c) if i > 0 && t.ring_link == RingLink::Step => {
                // Stay down: up if the next ring starts higher, along X at
                // this A, then down (D15b §6.3).
                let k = samples[0].0;
                let steps = ((x - c.x).abs() / (0.5 * r_tool)).ceil().max(1.0) as usize;
                let lift = (0..=steps)
                    .map(|j| cx.r_at(&q, k, c.x + (x - c.x) * j as f64 / steps as f64))
                    .fold(c.r.max(pts[0].r), f64::max);
                out.link_down(RPos { r: lift, ..c });
                out.link_down(RPos { x, r: lift, a: c.a });
                out.link_down(pts[0]);
            }
            Some(_) if i > 0 => out.link_down(pts[0]),
            _ => out.approach(pts[0]),
        }
        out.cut(&pts);
        let last = pts[pts.len() - 1];
        a_end = last.a;
        k_end = samples[samples.len() - 1].0;
        a_lo = a_lo.min(last.a).min(pts[0].a);
        a_hi = a_hi.max(last.a).max(pts[0].a);
    }
    if !one_sense {
        cx.check_limits(a_lo, a_hi)?;
    }
    Ok(out)
}

fn along_x(cx: &Cx) -> Out {
    let t = cx.p.rotary_along_x().cloned().expect("rotary_along_x");
    let r_max = cx.h.top.max(1e-6);
    let preset = cx
        .input
        .tool
        .preset(cx.input.material)
        .and_then(|p| p.stepover);
    let step_deg = match (t.stepover_deg, t.scallop_height) {
        (_, Some(h)) => (cx.scallop_step(h) / r_max).to_degrees(),
        (Some(d), None) => d,
        (None, None) => match preset {
            Some(s) => (s / r_max).to_degrees(),
            None => return Err(Vec::new()),
        },
    };
    let (a0, a1) = (t.a_start_deg, t.a_end_deg);
    let angles: Vec<f64> = if a1 - a0 >= 360.0 - 1e-9 {
        let n = (360.0 / step_deg - 1e-9).ceil().max(1.0) as usize;
        (0..n).map(|k| a0 + 360.0 * k as f64 / n as f64).collect()
    } else {
        cx.layout(a0, a1, step_deg)
    };
    cx.check_limits(
        angles.iter().copied().fold(f64::INFINITY, f64::min),
        angles.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    )?;
    let turned = cx.turned(angles.iter().map(|a| a.to_radians()).collect())?;
    let q = cx.radial(&turned)?;
    let mut out = cx.out();
    let dx = (0.5 * cx.input.tool.radius()).max(10.0 * cx.tol);
    for (i, &a) in angles.iter().enumerate() {
        let forward = match t.direction {
            ParallelDirection::Climb => true,
            ParallelDirection::Conventional => false,
            ParallelDirection::Zigzag => i % 2 == 0,
        };
        let mut pts = sample_line(cx, &q, i, a, dx);
        if !forward {
            pts.reverse();
        }
        out.approach(pts[0]);
        out.cut(&pts);
    }
    Ok(out)
}

/// CL points along X at grid angle `k` (A = `a`), refined where the drop
/// leaves the chord by more than the tolerance.
fn sample_line(cx: &Cx, q: &Radial, k: usize, a: f64, dx: f64) -> Vec<RPos> {
    let n = ((cx.x1 - cx.x0) / dx).ceil().max(1.0) as usize;
    let xs: Vec<f64> = (0..=n)
        .map(|i| {
            if i == n {
                cx.x1
            } else {
                cx.x0 + (cx.x1 - cx.x0) * i as f64 / n as f64
            }
        })
        .collect();
    let mut pts = vec![RPos::new(xs[0], cx.r_at(q, k, xs[0]), a)];
    for w in xs.windows(2) {
        refine(cx, q, k, a, pts[pts.len() - 1], w[1], 0, &mut pts);
    }
    // Drop points on the line between their neighbours.
    let mut keep = vec![pts[0]];
    for i in 1..pts.len() {
        let (p, c) = (keep[keep.len() - 1], pts[i]);
        if let Some(nx) = pts.get(i + 1) {
            let f = (c.x - p.x) / (nx.x - p.x);
            if (p.r + f * (nx.r - p.r) - c.r).abs() <= 1e-9 {
                continue;
            }
        }
        keep.push(c);
    }
    keep
}

#[allow(clippy::too_many_arguments)]
fn refine(cx: &Cx, q: &Radial, k: usize, a: f64, p: RPos, x: f64, depth: u32, pts: &mut Vec<RPos>) {
    let e = RPos::new(x, cx.r_at(q, k, x), a);
    let xm = 0.5 * (p.x + x);
    let rm = cx.r_at(q, k, xm);
    if (rm - 0.5 * (p.r + e.r)).abs() > cx.tol {
        if depth < 8 {
            refine(cx, q, k, a, p, xm, depth + 1, pts);
            let last = pts[pts.len() - 1];
            refine(cx, q, k, a, last, x, depth + 1, pts);
            return;
        }
        // A cliff (a wall across X): step at the higher radius instead of
        // cutting the corner.
        if p.r > e.r {
            pts.push(RPos::new(x, p.r, a));
        } else {
            pts.push(RPos::new(p.x, e.r, a));
        }
    }
    pts.push(e);
}

fn spiral(cx: &Cx) -> Out {
    let t = cx.p.rotary_spiral().cloned().expect("rotary_spiral");
    let adv = match (t.advance, t.scallop_height) {
        (_, Some(h)) => cx.scallop_step(h),
        (Some(s), None) => s,
        (None, None) => return Err(Vec::new()),
    };
    let len = cx.x1 - cx.x0;
    let turns = (len / adv - 1e-9).ceil().max(1.0) as usize;
    let n = grid_count(cx.h.top, cx.tol);
    let dir = if t.direction == CutDirection::Climb {
        1.0
    } else {
        -1.0
    };
    let total = 360.0 * turns as f64;
    if dir > 0.0 {
        cx.check_limits(0.0, total)?;
    } else {
        cx.check_limits(-total, 0.0)?;
    }
    let turned = cx.turned((0..n).map(|k| TAU * k as f64 / n as f64).collect())?;
    let q = cx.radial(&turned)?;
    let (xs, xe) = if t.from_end {
        (cx.x1, cx.x0)
    } else {
        (cx.x0, cx.x1)
    };
    let m = n * turns;
    let pts: Vec<RPos> = (0..=m)
        .map(|i| {
            let f = i as f64 / m as f64;
            let x = if i == m { xe } else { xs + (xe - xs) * f };
            let k = if dir > 0.0 { i % n } else { (n - i % n) % n };
            RPos::new(x, cx.r_at(&q, k, x), dir * total * f)
        })
        .collect();
    let pts = cx.refine_ring(&q, pts);
    let mut out = cx.out();
    out.approach(pts[0]);
    out.cut(&pts);
    Ok(out)
}

fn rough(cx: &Cx) -> Out {
    let t = cx.p.rotary_rough().cloned().expect("rotary_rough");
    let (Some(step), Some(sx)) = (t.radial_step_down, t.stepover_x) else {
        return Err(Vec::new());
    };
    let stl = cx.p.passes.stock_to_leave;
    let top = cx.h.top;
    let bottom = cx.h.bottom;
    // Layers: even radii plus coaxial cylinders (D15b §6.1).
    let mut radii = levels(top, bottom, Some(step), None);
    for b in &cx.input.geometry.bodies {
        let BodyGeom::Brep(body) = &b.geom else {
            continue;
        };
        for f in &body.faces {
            if let Surface::Cylinder { frame, r } = &body.surfaces[f.surface as usize] {
                let ax = frame.z.get();
                let o = frame.origin;
                let coaxial = ax.x().abs() > 1.0 - 1e-9 && libm::hypot(o.y(), o.z()) < 1e-6;
                let rr = r + stl;
                if coaxial && f.same_sense && rr > bottom + 1e-9 && rr < top - 1e-9 {
                    radii.push(rr);
                }
            }
        }
    }
    radii.sort_by(|a, b| b.total_cmp(a));
    radii.dedup_by(|a, b| (*a - *b).abs() <= 1e-9);
    let xs = cx.layout(cx.x0, cx.x1, sx);
    let n = grid_count(cx.info.stock_radius.max(top), cx.tol);
    let turned = cx.turned((0..n).map(|k| TAU * k as f64 / n as f64).collect())?;
    let q = cx.radial(&turned)?;
    // Drops do not depend on the layer: one table per X position.
    let table: Vec<Vec<Option<f64>>> = xs
        .iter()
        .map(|&x| (0..n).map(|k| q.drop(k, x)).collect())
        .collect();
    let mut stock = match (cx.p.geometry.rest_previous, cx.input.in_process) {
        (true, Some(s)) => s.clone(),
        _ => LayeredStock::cylindrical(cx.info.stock_radius, cx.info.stock_x.0, cx.info.stock_x.1),
    };
    let r_tool = cx.input.tool.radius();
    let w = (r_tool - cx.tol).max(0.0);
    let ramp = cx.p.linking.ramp;
    let mut out = cx.out();
    let mut flip = false;
    let mut prev_r = top;
    let mut a_lo = 0.0f64;
    let mut a_hi = 0.0f64;
    for &r in &radii {
        let material = stock.above(r);
        let mut swept = Vec::new();
        for (xi, &x) in xs.iter().enumerate() {
            let has = material_at(&material, x, w, n);
            let cut: Vec<bool> = (0..n)
                .map(|k| has[k] && table[xi][k].is_none_or(|d| d <= r + 1e-9))
                .collect();
            for (k0, len) in runs(&cut) {
                let dir = match t.direction {
                    ParallelDirection::Climb => 1.0,
                    ParallelDirection::Conventional => -1.0,
                    ParallelDirection::Zigzag => {
                        flip = !flip;
                        if flip { 1.0 } else { -1.0 }
                    }
                };
                let step = 360.0 / n as f64;
                let a_first = k0 as f64 * step;
                let span = if len == n {
                    360.0
                } else {
                    (len - 1) as f64 * step
                };
                if span <= 0.0 {
                    continue;
                }
                let (a_s, a_e) = if dir > 0.0 {
                    (a_first, a_first + span)
                } else {
                    (a_first + span, a_first)
                };
                let start = RPos::new(x, r, a_s);
                // A helical ramp descends from the layer above while A turns
                // (D15b §6.1), when the sweep is long enough for it.
                let mut cut_from = start;
                let descent = prev_r - r;
                if ramp.kind == RampKind::Helix && descent > 1e-9 {
                    let tan = libm::tan(ramp.angle_deg.to_radians().clamp(1e-3, 1.5));
                    let turn = (descent / tan / r.max(1e-6)).to_degrees();
                    if turn < span {
                        out.approach(RPos { r: prev_r, ..start });
                        cut_from = RPos::new(x, r, a_s + dir * turn);
                        out.ramp(cut_from);
                    } else {
                        cx.warn(
                            "cam.linking.ramp_fallback",
                            "the helix does not fit in a sweep; the tool plunges there",
                        );
                        out.approach(start);
                    }
                } else {
                    out.approach(start);
                }
                out.cut(&[cut_from, RPos::new(x, r, a_e)]);
                a_lo = a_lo.min(a_s.min(a_e));
                a_hi = a_hi.max(a_s.max(a_e));
                for (lo, hi) in wrap(a_s.to_radians(), a_e.to_radians()) {
                    swept.push(Region2::rect(x - w, lo, x + w, hi));
                }
            }
        }
        stock.remove_swept(r, &union_all(swept));
        prev_r = r;
    }
    cx.check_limits(a_lo, a_hi)?;
    Ok(out)
}

/// For each grid angle, whether the stock strip [x − w, x + w] holds
/// material at that angle.
fn material_at(m: &Region2, x: f64, w: f64, n: usize) -> Vec<bool> {
    let mut has = vec![false; n];
    if m.is_empty() {
        return has;
    }
    for xs in [x - 0.9 * w, x, x + 0.9 * w] {
        for (t0, t1) in m.clip_line(Point2::new(xs, 0.0), Vec2::new(0.0, 1.0)) {
            for (k, h) in has.iter_mut().enumerate() {
                let th = TAU * k as f64 / n as f64;
                if th >= t0 - 1e-9 && th <= t1 + 1e-9 {
                    *h = true;
                }
            }
        }
    }
    // A point exactly on the seam counts on both sides.
    if m.contains(Point2::new(x, TAU)) != Inside::Out {
        has[0] = true;
    }
    has
}

/// Runs of `true` around the circle: `(first index, length)`. A run that
/// wraps through index 0 is one run; all `true` is one run of `n`.
fn runs(v: &[bool]) -> Vec<(usize, usize)> {
    let n = v.len();
    if v.iter().all(|b| *b) {
        return vec![(0, n)];
    }
    let Some(gap) = v.iter().position(|b| !*b) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let k = (gap + i) % n;
        if v[k] {
            let start = k;
            let mut len = 0;
            while i < n && v[(gap + i) % n] {
                len += 1;
                i += 1;
            }
            out.push((start, len));
        } else {
            i += 1;
        }
    }
    out
}
