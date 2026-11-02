// SPDX-License-Identifier: MIT OR Apache-2.0

//! `drill` (D09 §3.1, D15b §3.1): one canned cycle per hole, holes from
//! coaxial cylinder stacks or circles, in shortest or selection order.

use crate::cutter::profile::Cutter;
use crate::holes::{Stack, find, shortest_order, stacks};
use crate::moves::{Mv, Out};
use crate::region::Inside;
use crate::strategy::boundary::boundary_region;
use crate::surface::{FaceSets, SurfaceQuery};
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, prepare, tag};
use fluut_geom::{Point2, Point3};
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{
    BoundarySide, DepthTo, DrillCycle, HeightEnv, HoleOrder, RetractMode, resolve,
};
use fluut_path::{CycleKind, CycleParams, PassKind, ToolpathResult};

#[derive(Debug)]
pub struct Drill;

impl ToolpathType for Drill {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Drill
    }

    fn role(&self) -> Role {
        Role::Hole
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        let tp = input.toolpath;
        let fail = |d: Vec<Diagnostic>| CamError::Invalid(tag(d, tp));
        let r0 = resolve(&tp.params, input.tool, input.material);
        let tol = r0.passes.machining_tolerance.max(1e-4);
        let d = input.tool.diameter;
        let found =
            find(input.geometry, &r0.geometry, "drill", tol, false).map_err(|e| fail(vec![e]))?;
        let mut holes = stacks(&found.cyls, d, tol);
        // `boundary` keeps the holes whose axis is inside (D15b §3.1).
        if let (Some(b), Some(bp)) = (
            boundary_region(input, &r0, tol).map_err(fail)?,
            &r0.geometry.boundary,
        ) {
            holes.retain(|h| {
                let inside = b.contains(h.centre) != Inside::Out;
                inside == (bp.side == BoundarySide::Inside)
            });
        }
        let sel = holes.iter().fold(None, |acc: Option<(f64, f64)>, h| {
            Some(match acc {
                None => (h.bottom, h.top),
                Some((lo, hi)) => (lo.min(h.bottom), hi.max(h.top)),
            })
        });
        let prep = prepare(input, sel).map_err(|e| match e {
            CamError::Invalid(mut v) => {
                v.extend(found.errors.iter().cloned());
                v.extend(found.warnings.iter().cloned());
                fail(v)
            }
            e => e,
        })?;
        let mut diags = prep.warnings;
        diags.extend(found.warnings);
        let p = &prep.params;
        let Some(dr) = p.drill().cloned() else {
            return Err(CamError::Unavailable("not a drill toolpath".into()));
        };
        let mut errors = found.errors;
        if holes.is_empty() {
            errors.push(Diagnostic::error(
                "cam.geometry.drive.empty",
                "the selection gives no holes to drill",
            ));
        }
        if !errors.is_empty() {
            diags.extend(errors);
            return Err(fail(diags));
        }
        let order: Vec<usize> = match dr.hole_order {
            HoleOrder::Selection => (0..holes.len()).collect(),
            HoleOrder::Shortest => shortest_order(
                &holes.iter().map(|h| h.centre).collect::<Vec<_>>(),
                Point2::new(0.0, 0.0),
            ),
        };
        let kind = match dr.cycle {
            DrillCycle::Drill => CycleKind::Drill,
            DrillCycle::Spot | DrillCycle::Dwell => CycleKind::Spot,
            DrillCycle::Peck => CycleKind::Peck,
            DrillCycle::ChipBreak => CycleKind::ChipBreak,
            DrillCycle::Tap => CycleKind::Tap,
            DrillCycle::Ream => CycleKind::Ream,
            DrillCycle::Bore => CycleKind::Bore,
        };
        let peck =
            matches!(dr.cycle, DrillCycle::Peck | DrillCycle::ChipBreak).then_some(dr.peck_depth);
        let point = input.tool.point_angle().to_radians();
        let extra = match dr.depth_to {
            DepthTo::Tip => 0.0,
            DepthTo::FullDiameter => 0.5 * d / libm::tan(0.5 * point),
        };
        let blocks = LinkCheck::new(input, d);
        let mut out = Out::new(tol);
        let mut mismatch = false;
        let mut deep = false;
        let mut prev: Option<(Point2, f64)> = None;
        for &i in &order {
            let hole: Stack = holes[i];
            let env = HeightEnv {
                selection: Some((hole.bottom, hole.top)),
                ..input.env
            };
            let h = p.heights.resolve(&env).map_err(|e| fail(vec![e]))?;
            let bottom = match hole.cone_tip {
                Some(tip) if dr.to_cone_tip && !hole.circle => tip,
                _ => h.bottom,
            };
            let depth_z = bottom - extra;
            if dr.cycle != DrillCycle::Spot && (hole.diameter - d).abs() > 0.05 * d {
                mismatch = true;
            }
            if input
                .tool
                .flute_length
                .is_some_and(|fl| h.top - depth_z > fl)
            {
                deep = true;
            }
            // The R plane: the feed height, but not below the stock top +
            // the safety distance, since the cycle rapids to it and the
            // material above this hole's top may not have been cut (D08
            // §8.2).
            let r_z = h
                .feed
                .max(input.env.stock_top.max(h.top) + p.linking.safety_distance);
            let c = hole.centre;
            match prev {
                None => {
                    out.start(Point3::new(c.x(), c.y(), h.clearance));
                    out.line(Point3::new(c.x(), c.y(), h.retract), Mv::Rapid);
                }
                Some((q, r_plane)) => {
                    // Between holes: at the retract height (G98), or at the
                    // R plane (G99) unless something is in the way.
                    let z = match dr.retract_mode {
                        RetractMode::Retract => h.retract,
                        RetractMode::Feed if blocks.blocked(q, c, r_plane.max(r_z)) => h.retract,
                        RetractMode::Feed => r_plane.max(r_z),
                    };
                    out.line(Point3::new(q.x(), q.y(), z), Mv::Rapid);
                    out.line(Point3::new(c.x(), c.y(), z), Mv::Rapid);
                }
            }
            out.line(Point3::new(c.x(), c.y(), r_z), Mv::Rapid);
            out.pass(PassKind::Link, None);
            out.cycle(CycleParams {
                kind,
                top_z: h.top,
                depth_z,
                retract_z: r_z,
                peck,
                dwell_s: (dr.dwell_s > 0.0).then_some(dr.dwell_s),
            });
            out.pass(PassKind::Cut, None);
            prev = Some((c, r_z));
        }
        if let Some(q) = out.cur() {
            out.line(
                Point3::new(q.x(), q.y(), prep.heights.clearance),
                Mv::Retract,
            );
            out.pass(PassKind::Retract, None);
        }
        if mismatch {
            diags.push(Diagnostic::warning(
                "cam.drill.diameter_mismatch",
                "a hole's diameter differs from the tool diameter by more than 5 %",
            ));
        }
        if deep {
            diags.push(Diagnostic::warning(
                "cam.drill.depth.flute",
                "a hole is deeper than the flute length",
            ));
        }
        let result = ToolpathResult::new(out.passes).map_err(CamError::Path)?;
        Ok(Generated {
            result,
            diagnostics: tag(diags, tp),
        })
    }
}

/// Whether a move between holes at a height meets the model or the stock
/// (D15b §3.1: raised to the retract height when blocked).
struct LinkCheck<'a> {
    query: Option<SurfaceQuery<'a>>,
    stock_top: f64,
}

impl<'a> LinkCheck<'a> {
    fn new(input: &'a StrategyInput, d: f64) -> LinkCheck<'a> {
        let query = Cutter::flat(d)
            .and_then(|c| SurfaceQuery::new(input.geometry, &FaceSets::default(), c, 0.0));
        LinkCheck {
            query,
            stock_top: input.env.stock_top,
        }
    }

    fn blocked(&self, a: Point2, b: Point2, z: f64) -> bool {
        let model = self
            .query
            .as_ref()
            .map_or(f64::NEG_INFINITY, |q| q.link_bound(a, b));
        z < model.max(self.stock_top)
    }
}
