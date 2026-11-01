// SPDX-License-Identifier: MIT OR Apache-2.0

//! `bore` (D09 §2.8, D15b §2.6): helical milling of cylindrical holes and
//! bosses, with an even pitch that ends exactly at the bottom.

use crate::holes::{find, shortest_order};
use crate::link::{LinkCx, Turn, lead_in_path, lead_out_path, link_height};
use crate::moves::{Mv, Out, finish};
use crate::path2::Path2;
use crate::region::{Region2, Seg2};
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, prepare, tag};
use fluut_geom::{Point2, Point3, UnitVec3};
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{BoreFeature, CutDirection, HeightEnv, LeadKind, resolve};
use fluut_path::{PassKind, ToolpathResult};
use std::f64::consts::TAU;

#[derive(Debug)]
pub struct Bore;

impl ToolpathType for Bore {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Bore
    }

    fn role(&self) -> Role {
        Role::Finish
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        let tp = input.toolpath;
        let fail = |d: Vec<Diagnostic>| CamError::Invalid(tag(d, tp));
        let r0 = resolve(&tp.params, input.tool, input.material);
        let tol = r0.passes.machining_tolerance.max(1e-4);
        let found =
            find(input.geometry, &r0.geometry, "bore", tol, true).map_err(|d| fail(vec![d]))?;
        let mut cyls = found.cyls;
        let feature = r0.bore().map_or(BoreFeature::Hole, |b| b.feature);
        for c in cyls.iter_mut().filter(|c| c.circle) {
            c.boss = feature == BoreFeature::Boss;
        }
        let sel = cyls.iter().fold(None, |acc: Option<(f64, f64)>, c| {
            Some(match acc {
                None => (c.lo, c.hi),
                Some((lo, hi)) => (lo.min(c.lo), hi.max(c.hi)),
            })
        });
        let prep = prepare(input, sel).map_err(|e| match e {
            CamError::Invalid(mut d) => {
                d.extend(found.errors.iter().cloned());
                fail(d)
            }
            e => e,
        })?;
        let mut diags = prep.warnings;
        diags.extend(found.warnings);
        let p = &prep.params;
        let Some(b) = p.bore().cloned() else {
            return Err(CamError::Unavailable("not a bore toolpath".into()));
        };
        let mut errors = found.errors;
        if cyls.is_empty() {
            errors.push(Diagnostic::error(
                "cam.geometry.drive.empty",
                "the selection gives no holes or bosses to bore",
            ));
        }
        let r = input.tool.radius();
        let stl = p.passes.stock_to_leave;
        let fs = if b.finishing_pass {
            b.finishing_stepover
        } else {
            0.0
        };
        // Helix and final radii per cylinder.
        let mut radii = Vec::new();
        for c in &cyls {
            let (helix, last) = if c.boss {
                (c.radius + r + stl + fs, c.radius + r + stl)
            } else {
                (c.radius - r - stl - fs, c.radius - r - stl)
            };
            if !c.boss && helix <= 0.0 {
                errors.push(Diagnostic::error(
                    "cam.bore.hole_small",
                    format!(
                        "a Ø{} hole is too small for the tool and stock to leave",
                        2.0 * c.radius
                    ),
                ));
            } else if !c.boss && helix > r {
                diags.push(Diagnostic::warning(
                    "cam.bore.core_left",
                    format!(
                        "the helix in a Ø{} hole leaves a core in the centre: clear it first",
                        2.0 * c.radius
                    ),
                ));
            }
            radii.push((helix, last));
        }
        if !errors.is_empty() {
            diags.extend(errors);
            return Err(fail(diags));
        }
        let order = shortest_order(
            &cyls.iter().map(|c| c.centre).collect::<Vec<_>>(),
            Point2::new(0.0, 0.0),
        );
        let mut out = Out::new(tol);
        let h0 = prep.heights;
        for (k, &i) in order.iter().enumerate() {
            let c = cyls[i];
            let env = HeightEnv {
                selection: Some((c.lo, c.hi)),
                ..input.env
            };
            let h = p.heights.resolve(&env).map_err(|d| fail(vec![d]))?;
            let link_cx = LinkCx {
                heights: h,
                material_top: input.env.stock_top.max(h.top),
                linking: &p.linking,
                tool_diameter: input.tool.diameter,
                tol,
                safe: &[],
            };
            let bottom = h.bottom + p.passes.stock_to_leave_axial;
            let (rh, rf) = radii[i];
            // Climb keeps the material on the right (spindle M3): a hole
            // runs counter-clockwise, a boss clockwise.
            let ccw = (b.direction == CutDirection::Climb) != c.boss;
            let sweep = if ccw { TAU } else { -TAU };
            let cen = c.centre;
            let s = cen + fluut_geom::Vec2::new(rh, 0.0);
            let circle = |rad: f64| -> Path2 {
                let p = Path2::from_loop(&Region2::circle(cen.x(), cen.y(), rad).loops[0]);
                if ccw { p } else { p.reversed() }
            };
            let side = if ccw != c.boss {
                Turn::Left
            } else {
                Turn::Right
            };
            // Lead-in: from the hole centre (a half circle onto the helix),
            // or from outside the boss (D15b §2.6).
            let lead_in: Option<Path2> = match (p.linking.lead_in.kind, c.boss) {
                (LeadKind::None, _) => None,
                (_, false) => Some(Path2::open(vec![Seg2 {
                    a: cen,
                    b: s,
                    bulge: if ccw { 1.0 } else { -1.0 },
                }])),
                (_, true) => lead_in_path(&circle(rh), side, &p.linking.lead_in),
            };
            let entry = lead_in.as_ref().map_or(s, Path2::start);
            // Approach.
            match out.cur() {
                None => out.start(Point3::new(entry.x(), entry.y(), h.clearance)),
                Some(q) => {
                    let z = link_height(q.z().max(h.top), &link_cx)
                        .max(link_height(h0.top.max(h.top), &link_cx));
                    out.line(Point3::new(q.x(), q.y(), z), Mv::Retract);
                    out.line(Point3::new(entry.x(), entry.y(), z), Mv::Rapid);
                }
            }
            out.line(Point3::new(entry.x(), entry.y(), h.feed), Mv::Rapid);
            out.pass(PassKind::Link, None);
            out.line(Point3::new(entry.x(), entry.y(), h.top), Mv::Plunge);
            out.pass(PassKind::Ramp, None);
            if let Some(li) = &lead_in {
                out.path(li, Mv::LeadIn);
            }
            // Helix turns, even pitch, the last exactly at the bottom.
            let depth = h.top - bottom;
            let n = ((depth / b.pitch) - 1e-9).ceil().max(1.0) as usize;
            for t in 1..=n {
                let z = if t == n {
                    bottom
                } else {
                    h.top - depth * t as f64 / n as f64
                };
                out.helix(cen, sweep, z, Mv::Cut);
            }
            let full = |o: &mut Out| {
                let Some(a) = o.cur() else { return };
                o.arc(
                    a,
                    Point3::new(cen.x(), cen.y(), a.z()),
                    UnitVec3::Z,
                    sweep,
                    Mv::Cut,
                );
            };
            if b.bottom_circle {
                full(&mut out);
            }
            let mut last = rh;
            if b.finishing_pass && (rf - rh).abs() > 0.0 {
                out.line(Point3::new(cen.x() + rf, cen.y(), bottom), Mv::Cut);
                full(&mut out);
                last = rf;
            }
            // Lead-out: back to the centre (hole) or out (boss).
            let e = cen + fluut_geom::Vec2::new(last, 0.0);
            let lead_out: Option<Path2> = match (p.linking.lead_out.kind, c.boss) {
                (LeadKind::None, _) => None,
                (_, false) => Some(Path2::open(vec![Seg2 {
                    a: e,
                    b: cen,
                    bulge: if ccw { 1.0 } else { -1.0 },
                }])),
                (_, true) => lead_out_path(&circle(last), side, &p.linking.lead_out),
            };
            if let Some(lo) = &lead_out {
                out.path(lo, Mv::LeadOut);
            }
            out.pass(PassKind::Cut, None);
            if k + 1 == order.len()
                && let Some(q) = out.cur()
            {
                out.line(Point3::new(q.x(), q.y(), h.clearance), Mv::Retract);
                out.pass(PassKind::Retract, None);
            }
        }
        let passes = finish(out.passes, p.passes.output_arcs, None, tol);
        let result = ToolpathResult::new(passes).map_err(CamError::Path)?;
        Ok(Generated {
            result,
            diagnostics: tag(diags, tp),
        })
    }
}
