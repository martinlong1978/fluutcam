// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ordering is the strategy's; this module links cut passes (D08 §8): the
//! approach from clearance, retracts per policy, stay-down links, leads,
//! ramps with fallback, and the final retract.

use crate::moves::{Mv, Out};
use crate::path2::{Path2, left, unit};
use crate::region::{Inside, Region2, Seg2};
use fluut_geom::{Point2, Point3, Vec2};
use fluut_model::Diagnostic;
use fluut_model::resolve::{
    Lead, LeadKind, LinkingGroup, RampKind, ResolvedHeights, RetractPolicy,
};
use fluut_path::{CompSide, Pass, PassKind};
use std::f64::consts::{PI, TAU};

/// Which side of the travel direction the tool is on, away from the
/// material. Leads come from this side.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Turn {
    Left,
    Right,
}

/// One cutting pass, before linking.
#[derive(Clone, Debug)]
pub struct Cut {
    pub path: Path2,
    /// Cutting level.
    pub z: f64,
    /// Z along the path as `(arc length, z)` breakpoints, linear between
    /// them (tabs); empty when the whole pass is at `z`.
    pub profile: Vec<(f64, f64)>,
    pub tool_side: Turn,
    pub comp: Option<CompSide>,
    /// Material is already cleared down to this Z at the entry (the level
    /// above, or the top): ramps start here.
    pub ramp_from: f64,
    /// Index into [`LinkCx::safe`]: the tool-centre region at this level,
    /// where links may stay down and helix or zig-zag ramps may move.
    pub area: Option<usize>,
}

impl Cut {
    pub fn flat(path: Path2, z: f64, tool_side: Turn, ramp_from: f64) -> Cut {
        Cut {
            path,
            z,
            profile: Vec::new(),
            tool_side,
            comp: None,
            ramp_from,
            area: None,
        }
    }

    fn start_z(&self) -> f64 {
        self.profile.first().map_or(self.z, |p| p.1)
    }

    fn end_z(&self) -> f64 {
        self.profile.last().map_or(self.z, |p| p.1)
    }
}

#[derive(Debug)]
pub struct LinkCx<'a> {
    pub heights: ResolvedHeights,
    /// Highest material Z any link may meet (stock top or toolpath top).
    pub material_top: f64,
    pub linking: &'a LinkingGroup,
    pub tool_diameter: f64,
    pub tol: f64,
    pub safe: &'a [Region2],
}

const EPS: f64 = 1e-9;

/// Links `cuts` in order into passes, from clearance back to clearance.
pub fn link(cuts: &[Cut], cx: &LinkCx, diags: &mut Vec<Diagnostic>) -> Vec<Pass> {
    let mut out = Out::new(cx.tol);
    let mut fell_back = false;
    let mut prev: Option<&Cut> = None;
    for cut in cuts {
        if cut.path.is_empty() {
            continue;
        }
        let lead_in = lead_in_path(&cut.path, cut.tool_side, &cx.linking.lead_in);
        let plain_entry = lead_in.as_ref().map_or(cut.path.start(), Path2::start);
        let z0 = cut.start_z();
        let stay = match (prev, out.cur()) {
            (Some(p), Some(e)) => can_stay(p, e, cut, plain_entry, z0, cx),
            _ => false,
        };
        let mut skip_lead = false;
        if stay {
            out.line(Point3::new(plain_entry.x(), plain_entry.y(), z0), Mv::Link);
            out.pass(PassKind::Link, None);
        } else {
            let plan = plan_ramp(cut, lead_in.as_ref(), plain_entry, z0, cx);
            if plan.fell_back {
                fell_back = true;
            }
            skip_lead = plan.skip_lead;
            let e = plan.entry;
            let mut z_now = match out.cur() {
                None => {
                    out.start(Point3::new(e.x(), e.y(), cx.heights.clearance));
                    cx.heights.clearance
                }
                Some(p) => {
                    let h = link_height(p.z().max(z0), cx);
                    out.line(Point3::new(p.x(), p.y(), h), Mv::Retract);
                    out.line(Point3::new(e.x(), e.y(), h), Mv::Rapid);
                    out.pass(PassKind::Link, None);
                    h
                }
            };
            let floor = rapid_floor(cx);
            if z_now > floor {
                out.line(Point3::new(e.x(), e.y(), floor), Mv::Rapid);
                z_now = floor;
            }
            let ramp_top = cut.ramp_from.min(z_now).max(z0);
            let plunge_to = if plan.kind == RampKind::Plunge {
                z0
            } else {
                ramp_top
            };
            out.line(Point3::new(e.x(), e.y(), plunge_to), Mv::Plunge);
            if plunge_to > z0 {
                run_ramp(&mut out, &plan, z0);
            }
            out.pass(PassKind::Ramp, None);
        }
        if let Some(li) = &lead_in
            && !skip_lead
        {
            out.path(li, Mv::LeadIn);
        }
        cut_moves(&mut out, cut);
        if let Some(lo) = lead_out_path(&cut.path, cut.tool_side, &cx.linking.lead_out) {
            out.path(&lo, Mv::LeadOut);
        }
        out.pass(PassKind::Cut, cut.comp);
        prev = Some(cut);
    }
    if let Some(p) = out.cur() {
        out.line(Point3::new(p.x(), p.y(), cx.heights.clearance), Mv::Retract);
        out.pass(PassKind::Retract, None);
    }
    if fell_back {
        diags.push(Diagnostic::warning(
            "cam.linking.ramp_fallback",
            "the chosen ramp did not fit at some entries; a fallback ramp was used",
        ));
    }
    out.passes
}

/// Retract height for a link from a pass at `z` (D08 §8.2). `minimum`
/// stays `safety_distance` above the highest material the link can meet;
/// without drop queries (S2) that is the material top.
pub fn link_height(z: f64, cx: &LinkCx) -> f64 {
    let h = match cx.linking.retract {
        RetractPolicy::Full => cx.heights.retract,
        RetractPolicy::Minimum | RetractPolicy::StayDown => cx
            .heights
            .retract
            .min(cx.material_top + cx.linking.safety_distance),
    };
    h.max(z)
}

/// Lowest Z a rapid may reach on its way down to an entry (D08 §8.2): the
/// feed height, but not below `safety_distance` above the material top.
/// Without the in-process stock the linker cannot tell whether an earlier
/// toolpath cleared the material above `top`, so below this it feeds.
pub fn rapid_floor(cx: &LinkCx) -> f64 {
    cx.heights
        .feed
        .max(cx.material_top + cx.linking.safety_distance)
}

/// Whether the link from `end` to the next entry may stay at depth: same
/// level, the straight move inside the next cut's tool-centre region, and
/// the policy allows it (`stay_down` always, `minimum` up to one tool
/// diameter, `full` never).
fn can_stay(prev: &Cut, end: Point3, next: &Cut, entry: Point2, z0: f64, cx: &LinkCx) -> bool {
    let max_len = match cx.linking.retract {
        RetractPolicy::Full => return false,
        RetractPolicy::Minimum => cx.tool_diameter,
        RetractPolicy::StayDown => f64::INFINITY,
    };
    let (Some(_), Some(a)) = (prev.area, next.area) else {
        return false;
    };
    if (end.z() - z0).abs() > EPS || (prev.end_z() - z0).abs() > EPS {
        return false;
    }
    let p = Point2::new(end.x(), end.y());
    let d = entry - p;
    if d.length() > max_len {
        return false;
    }
    let Some(region) = cx.safe.get(a) else {
        return false;
    };
    segment_inside(region, p, entry)
}

pub(crate) fn segment_inside(region: &Region2, p: Point2, q: Point2) -> bool {
    if region.contains(p) == Inside::Out || region.contains(q) == Inside::Out {
        return false;
    }
    let d = q - p;
    if d.length() <= EPS {
        return true;
    }
    region
        .clip_line(p, d)
        .iter()
        .any(|&(t0, t1)| t0 <= 1e-9 && t1 >= 1.0 - 1e-9)
}

/// Lead-in ending at the path start (D15 §2.6): a quarter arc tangent to
/// the path from the tool side, after an optional straight; or a straight
/// tangent line.
pub fn lead_in_path(path: &Path2, side: Turn, lead: &Lead) -> Option<Path2> {
    let p = path.start();
    let t = path.start_tangent();
    let n = tool_normal(t, side);
    let segs = match lead.kind {
        LeadKind::None => return None,
        LeadKind::Linear if lead.length > 0.0 => vec![line(p + t * -lead.length, p)],
        LeadKind::Linear => return None,
        LeadKind::Arc => {
            let r = lead.radius;
            let s = p + n * r + t * -r;
            let mut v = Vec::new();
            if lead.length > 0.0 {
                v.push(line(s + n * lead.length, s));
            }
            v.push(Seg2 {
                a: s,
                b: p,
                bulge: quarter_bulge(t, n),
            });
            v
        }
    };
    Some(Path2::open(segs))
}

/// Lead-out starting at the path end, the mirror of [`lead_in_path`].
pub fn lead_out_path(path: &Path2, side: Turn, lead: &Lead) -> Option<Path2> {
    let e = path.end();
    let t = path.end_tangent();
    let n = tool_normal(t, side);
    let segs = match lead.kind {
        LeadKind::None => return None,
        LeadKind::Linear if lead.length > 0.0 => vec![line(e, e + t * lead.length)],
        LeadKind::Linear => return None,
        LeadKind::Arc => {
            let r = lead.radius;
            let f = e + n * r + t * r;
            let mut v = vec![Seg2 {
                a: e,
                b: f,
                bulge: quarter_bulge(t, n),
            }];
            if lead.length > 0.0 {
                v.push(line(f, f + n * lead.length));
            }
            v
        }
    };
    Some(Path2::open(segs))
}

fn tool_normal(t: Vec2, side: Turn) -> Vec2 {
    match side {
        Turn::Left => left(t),
        Turn::Right => left(t) * -1.0,
    }
}

/// Bulge of a quarter arc turning from the tool side onto direction `t`.
fn quarter_bulge(t: Vec2, n: Vec2) -> f64 {
    let b = libm::tan(PI / 8.0);
    if t.perp_dot(n) > 0.0 { b } else { -b }
}

fn line(a: Point2, b: Point2) -> Seg2 {
    Seg2 { a, b, bulge: 0.0 }
}

/// How an entry descends.
struct RampPlan {
    kind: RampKind,
    entry: Point2,
    skip_lead: bool,
    fell_back: bool,
    /// Helix centre and signed turn.
    helix: Option<(Point2, f64, f64)>,
    /// Zig-zag leg.
    zigzag: Option<(Point2, Point2, f64)>,
    /// Profile: the path to follow down once (closed paths), or a leg to
    /// go out and back on `n` times (open paths).
    profile: Option<(Path2, Option<usize>)>,
}

fn plan_ramp(cut: &Cut, lead_in: Option<&Path2>, entry: Point2, z0: f64, cx: &LinkCx) -> RampPlan {
    let r = &cx.linking.ramp;
    let plain = RampPlan {
        kind: RampKind::Plunge,
        entry,
        skip_lead: false,
        fell_back: false,
        helix: None,
        zigzag: None,
        profile: None,
    };
    let dz = cut.ramp_from - z0;
    if r.kind == RampKind::Plunge || dz <= EPS {
        return plain;
    }
    let tan = libm::tan(r.angle_deg.to_radians());
    let dir = lead_in.map_or_else(|| cut.path.start_tangent(), Path2::start_tangent);
    let region = cut.area.and_then(|a| cx.safe.get(a));
    let try_helix = || -> Option<RampPlan> {
        let region = region?;
        let rad = r.radius;
        for (sign, n) in [(1.0, left(dir)), (-1.0, left(dir) * -1.0)] {
            let c = entry + n * rad;
            if region.contains(c) == Inside::In && region.boundary_distance(c) >= rad - EPS {
                let turns = (dz / (TAU * rad * tan) - 1e-9).ceil().max(1.0);
                return Some(RampPlan {
                    kind: RampKind::Helix,
                    helix: Some((c, sign * TAU, turns)),
                    ..plain_clone(&plain)
                });
            }
        }
        None
    };
    let try_zigzag = || -> Option<RampPlan> {
        let first = lead_in.map_or(cut.path.segs[0], |l| l.segs[0]);
        if !first.is_line() {
            return None;
        }
        let len = first.length().min(2.0 * cx.tool_diameter);
        if len <= 10.0 * cx.tol {
            return None;
        }
        let far = entry + unit(first.b - first.a) * len;
        let mut legs = (dz / (len * tan) - 1e-9).ceil().max(1.0);
        if legs % 2.0 != 0.0 {
            legs += 1.0;
        }
        Some(RampPlan {
            kind: RampKind::Zigzag,
            zigzag: Some((entry, far, legs)),
            ..plain_clone(&plain)
        })
    };
    let try_profile = || -> Option<RampPlan> {
        if !cut.profile.is_empty() {
            return None;
        }
        let path = &cut.path;
        let total = path.length();
        if total <= 10.0 * cx.tol {
            return None;
        }
        let need = dz / tan;
        if path.closed {
            let laps = (need / total).floor();
            let rem = need - laps * total;
            let mut segs = Vec::new();
            if rem > 10.0 * cx.tol {
                segs.extend(path.rotated(total - rem, 1e-9).sub(0.0, rem, 1e-9).segs);
                let k = segs.len() - 1;
                segs[k].b = path.start();
            }
            for _ in 0..laps as usize {
                segs.extend(path.segs.iter().copied());
            }
            let ramp = Path2::open(segs);
            Some(RampPlan {
                kind: RampKind::Profile,
                entry: ramp.start(),
                skip_lead: true,
                profile: Some((ramp, None)),
                ..plain_clone(&plain)
            })
        } else {
            let leg = path.sub(0.0, total.min(0.5 * need), 1e-9);
            let mut legs = (need / leg.length() - 1e-9).ceil().max(2.0) as usize;
            legs += legs % 2;
            Some(RampPlan {
                kind: RampKind::Profile,
                entry: path.start(),
                skip_lead: true,
                profile: Some((leg, Some(legs))),
                ..plain_clone(&plain)
            })
        }
    };
    let chosen = match r.kind {
        RampKind::Helix => try_helix().or_else(try_zigzag),
        RampKind::Zigzag => try_zigzag(),
        RampKind::Profile => try_profile(),
        RampKind::Plunge => None,
    };
    match chosen {
        Some(p) if p.kind == r.kind => p,
        Some(p) => RampPlan {
            fell_back: true,
            ..p
        },
        None => RampPlan {
            fell_back: true,
            ..plain
        },
    }
}

fn plain_clone(p: &RampPlan) -> RampPlan {
    RampPlan {
        kind: p.kind,
        entry: p.entry,
        skip_lead: p.skip_lead,
        fell_back: p.fell_back,
        helix: None,
        zigzag: None,
        profile: None,
    }
}

/// Descends from the current Z to `z0` along the planned ramp.
fn run_ramp(out: &mut Out, plan: &RampPlan, z0: f64) {
    let Some(start) = out.cur() else { return };
    let z_hi = start.z();
    match plan.kind {
        RampKind::Helix => {
            if let Some((c, turn, n)) = plan.helix {
                let n = n as usize;
                for k in 1..=n {
                    let z = if k == n {
                        z0
                    } else {
                        z_hi + (z0 - z_hi) * k as f64 / n as f64
                    };
                    out.helix(c, turn, z, Mv::Ramp);
                }
            }
        }
        RampKind::Zigzag => {
            if let Some((a, b, legs)) = plan.zigzag {
                let n = legs as usize;
                for k in 1..=n {
                    let z = if k == n {
                        z0
                    } else {
                        z_hi + (z0 - z_hi) * k as f64 / n as f64
                    };
                    let p = if k % 2 == 1 { b } else { a };
                    out.line(Point3::new(p.x(), p.y(), z), Mv::Ramp);
                }
            }
        }
        RampKind::Profile => match &plan.profile {
            Some((path, None)) => out.path_ramp(path, z0, Mv::Ramp),
            Some((leg, Some(n))) => {
                let back = leg.reversed();
                for k in 1..=*n {
                    let z = if k == *n {
                        z0
                    } else {
                        z_hi + (z0 - z_hi) * k as f64 / *n as f64
                    };
                    out.path_ramp(if k % 2 == 1 { leg } else { &back }, z, Mv::Ramp);
                }
            }
            None => {}
        },
        RampKind::Plunge => {}
    }
}

/// The cutting moves of `cut`, with its Z profile.
fn cut_moves(out: &mut Out, cut: &Cut) {
    if cut.profile.is_empty() {
        out.path(&cut.path, Mv::Cut);
        return;
    }
    let cuts: Vec<f64> = cut.profile.iter().map(|p| p.0).collect();
    let split = cut.path.split_at(&cuts, 1e-9);
    let z_at = |s: f64| -> f64 {
        let pr = &cut.profile;
        if s <= pr[0].0 {
            return pr[0].1;
        }
        for w in pr.windows(2) {
            if s <= w[1].0 {
                let f = if w[1].0 > w[0].0 {
                    (s - w[0].0) / (w[1].0 - w[0].0)
                } else {
                    1.0
                };
                return w[0].1 + (w[1].1 - w[0].1) * f;
            }
        }
        pr[pr.len() - 1].1
    };
    let mut acc = 0.0;
    for s in &split.segs {
        acc += s.length();
        out.seg(s, z_at(acc), Mv::Cut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluut_model::resolve::{Order, Ramp, Transitions};
    use fluut_path::{Motion, Segment, ToolpathResult};

    const H: ResolvedHeights = ResolvedHeights {
        clearance: 30.0,
        retract: 25.0,
        feed: 22.0,
        top: 20.0,
        bottom: 0.0,
    };

    fn linking(retract: RetractPolicy, lead: LeadKind, ramp: RampKind) -> LinkingGroup {
        let l = Lead {
            kind: lead,
            radius: 3.0,
            length: 2.0,
        };
        LinkingGroup {
            retract,
            safety_distance: 1.0,
            transitions: Transitions::Direct,
            lead_in: l,
            lead_out: l,
            ramp: Ramp {
                kind: ramp,
                angle_deg: 3.0,
                radius: 3.0,
            },
            order: Order::ByLevel,
        }
    }

    fn square(x0: f64, s: f64) -> Path2 {
        Path2::from_loop(&Region2::rect(x0, 0.0, x0 + s, s).loops[0])
    }

    fn run(cuts: &[Cut], l: &LinkingGroup, safe: &[Region2]) -> (ToolpathResult, Vec<Diagnostic>) {
        let cx = LinkCx {
            heights: H,
            material_top: 20.0,
            linking: l,
            tool_diameter: 6.0,
            tol: 0.01,
            safe,
        };
        let mut d = Vec::new();
        let passes = link(cuts, &cx, &mut d);
        (ToolpathResult::new(passes).unwrap(), d)
    }

    fn motions(r: &ToolpathResult) -> Vec<Motion> {
        r.segments()
            .filter_map(|s| s.motion().map(|m| m.motion))
            .collect()
    }

    #[test]
    fn approach_plunge_cut_and_final_retract() {
        let l = linking(RetractPolicy::Full, LeadKind::None, RampKind::Plunge);
        let (r, d) = run(
            &[Cut::flat(square(0.0, 10.0), 15.0, Turn::Right, 20.0)],
            &l,
            &[],
        );
        assert!(d.is_empty());
        let segs: Vec<&Segment> = r.segments().collect();
        assert_eq!(segs[0].start().unwrap().xyz, Point3::new(0.0, 0.0, 30.0));
        assert_eq!(segs[0].end().unwrap().xyz, Point3::new(0.0, 0.0, 22.0));
        assert_eq!(segs[1].end().unwrap().xyz, Point3::new(0.0, 0.0, 15.0));
        assert_eq!(
            motions(&r),
            [
                Motion::Rapid,
                Motion::Plunge,
                Motion::Cut,
                Motion::Cut,
                Motion::Cut,
                Motion::Cut,
                Motion::Retract
            ]
        );
        assert_eq!(r.segments().last().unwrap().end().unwrap().xyz.z(), 30.0);
    }

    #[test]
    fn rapids_stay_above_the_material_when_top_is_lower() {
        // A floor pass whose `top` (10) is below the stock top (20): an
        // earlier toolpath may have left material above `top`, so rapids end
        // `safety_distance` above the stock and the tool feeds from there.
        let h = ResolvedHeights {
            clearance: 30.0,
            retract: 25.0,
            feed: 12.0,
            top: 10.0,
            bottom: 0.0,
        };
        let cuts = [
            Cut {
                area: Some(0),
                ..Cut::flat(square(0.0, 10.0), 8.0, Turn::Right, 10.0)
            },
            Cut {
                area: Some(0),
                ..Cut::flat(square(12.0, 10.0), 8.0, Turn::Right, 10.0)
            },
        ];
        let safe = [Region2::rect(-1.0, -1.0, 11.0, 11.0)];
        for policy in [
            RetractPolicy::Full,
            RetractPolicy::Minimum,
            RetractPolicy::StayDown,
        ] {
            let l = linking(policy, LeadKind::None, RampKind::Plunge);
            let cx = LinkCx {
                heights: h,
                material_top: 20.0,
                linking: &l,
                tool_diameter: 6.0,
                tol: 0.01,
                safe: &safe,
            };
            let r = ToolpathResult::new(link(&cuts, &cx, &mut Vec::new())).unwrap();
            let mut plunges = 0;
            for s in r.segments() {
                let (Some(m), Some(a), Some(b)) = (s.motion(), s.start(), s.end()) else {
                    continue;
                };
                if m.motion == Motion::Rapid {
                    assert!(a.xyz.z().min(b.xyz.z()) >= 21.0 - 1e-9, "{policy:?}: {s:?}");
                }
                if m.motion == Motion::Plunge {
                    assert_eq!(a.xyz.z(), 21.0, "{policy:?}");
                    plunges += 1;
                }
            }
            assert_eq!(plunges, 2, "{policy:?}");
        }
    }

    #[test]
    fn retract_policies() {
        let cuts = [
            Cut {
                area: Some(0),
                ..Cut::flat(square(0.0, 10.0), 15.0, Turn::Right, 20.0)
            },
            Cut {
                area: Some(0),
                ..Cut::flat(square(12.0, 10.0), 15.0, Turn::Right, 20.0)
            },
        ];
        let safe = [Region2::rect(-1.0, -1.0, 30.0, 11.0)];
        let top_of_link = |r: &ToolpathResult| {
            r.passes()
                .iter()
                .find(|p| p.kind == PassKind::Link)
                .map(|p| p.segments[0].end().unwrap().xyz.z())
        };
        let (full, _) = run(
            &cuts,
            &linking(RetractPolicy::Full, LeadKind::None, RampKind::Plunge),
            &safe,
        );
        assert_eq!(top_of_link(&full), Some(25.0));
        let (min, _) = run(
            &cuts,
            &linking(RetractPolicy::Minimum, LeadKind::None, RampKind::Plunge),
            &safe,
        );
        // 12 mm apart is more than one tool diameter: retract to top + 1.
        assert_eq!(top_of_link(&min), Some(21.0));
        let (down, _) = run(
            &cuts,
            &linking(RetractPolicy::StayDown, LeadKind::None, RampKind::Plunge),
            &safe,
        );
        assert_eq!(top_of_link(&down), Some(15.0));
        assert!(motions(&down).contains(&Motion::Link));
        // Outside the safe region the link retracts even with stay_down.
        let small = [Region2::rect(-1.0, -1.0, 11.0, 11.0)];
        let (up, _) = run(
            &cuts,
            &linking(RetractPolicy::StayDown, LeadKind::None, RampKind::Plunge),
            &small,
        );
        assert_eq!(top_of_link(&up), Some(21.0));
    }

    #[test]
    fn arc_leads_come_from_the_tool_side() {
        let p = square(0.0, 10.0);
        let lead = Lead {
            kind: LeadKind::Arc,
            radius: 3.0,
            length: 2.0,
        };
        // Counter-clockwise square, tool outside: on the right.
        let li = lead_in_path(&p, Turn::Right, &lead).unwrap();
        assert_eq!(li.end(), p.start());
        assert_eq!(li.start(), Point2::new(-3.0, -5.0));
        assert!((li.end_tangent() - p.start_tangent()).length() < 1e-12);
        let lo = lead_out_path(&p, Turn::Right, &lead).unwrap();
        assert_eq!(lo.start(), p.end());
        assert!((lo.start_tangent() - p.end_tangent()).length() < 1e-12);
        let lin = lead_in_path(
            &p,
            Turn::Left,
            &Lead {
                kind: LeadKind::Linear,
                ..lead
            },
        )
        .unwrap();
        assert_eq!(lin.start(), Point2::new(-2.0, 0.0));
        let l = linking(RetractPolicy::Minimum, LeadKind::Arc, RampKind::Plunge);
        let (r, _) = run(&[Cut::flat(p, 15.0, Turn::Right, 20.0)], &l, &[]);
        assert!(motions(&r).contains(&Motion::LeadIn) && motions(&r).contains(&Motion::LeadOut));
    }

    #[test]
    fn helix_ramp_and_fallbacks() {
        let l = linking(RetractPolicy::Minimum, LeadKind::None, RampKind::Helix);
        let cut = Cut {
            area: Some(0),
            ..Cut::flat(square(0.0, 10.0), 15.0, Turn::Left, 20.0)
        };
        let (r, d) = run(
            std::slice::from_ref(&cut),
            &l,
            &[Region2::rect(-5.0, -5.0, 15.0, 15.0)],
        );
        assert!(d.is_empty());
        let helix: Vec<&Segment> = r
            .segments()
            .filter(|s| matches!(s, Segment::Helix { .. }))
            .collect();
        // 5 mm at 3° on a 3 mm radius: 2π·3·tan 3° = 0.988 mm per turn.
        assert_eq!(helix.len(), 6);
        assert_eq!(helix[5].end().unwrap().xyz, Point3::new(0.0, 0.0, 15.0));
        // A region too small for the helix: zig-zag along the first line.
        let (r, d) = run(
            std::slice::from_ref(&cut),
            &l,
            &[Region2::rect(-1.0, -1.0, 4.0, 4.0)],
        );
        assert_eq!(d[0].code, "cam.linking.ramp_fallback");
        assert!(r.segments().all(|s| !matches!(s, Segment::Helix { .. })));
        assert!(motions(&r).contains(&Motion::Ramp));
        // A first arc segment rules out zig-zag too: plunge.
        let circle = Path2::from_loop(&Region2::circle(0.0, 0.0, 2.0).loops[0]);
        let c = Cut {
            area: Some(0),
            ..Cut::flat(circle, 15.0, Turn::Left, 20.0)
        };
        let (r, d) = run(&[c], &l, &[Region2::circle(0.0, 0.0, 2.5)]);
        assert_eq!(d.len(), 1);
        assert!(!motions(&r).contains(&Motion::Ramp));
    }

    #[test]
    fn profile_ramp_ends_at_the_path_start() {
        let l = linking(RetractPolicy::Minimum, LeadKind::Arc, RampKind::Profile);
        let p = square(0.0, 10.0);
        let (r, d) = run(&[Cut::flat(p.clone(), 15.0, Turn::Right, 20.0)], &l, &[]);
        assert!(d.is_empty());
        let ramp: Vec<&Segment> = r
            .segments()
            .filter(|s| s.motion().is_some_and(|m| m.motion == Motion::Ramp))
            .collect();
        let need = 5.0 / libm::tan(3f64.to_radians());
        let len: f64 = ramp.iter().map(|s| s.length()).sum();
        assert!((len - libm::hypot(need, 5.0)).abs() < 1e-6, "{len}");
        assert_eq!(
            ramp.last().unwrap().end().unwrap().xyz,
            Point3::new(0.0, 0.0, 15.0)
        );
        // The lead-in is skipped after a profile ramp; the lead-out stays.
        assert!(!motions(&r).contains(&Motion::LeadIn));
        assert!(motions(&r).contains(&Motion::LeadOut));
        // Open paths go out and back.
        let open = Path2::open(vec![line(Point2::new(0.0, 0.0), Point2::new(40.0, 0.0))]);
        let (r, _) = run(&[Cut::flat(open, 15.0, Turn::Right, 20.0)], &l, &[]);
        let first_ramp = r
            .segments()
            .find(|s| s.motion().is_some_and(|m| m.motion == Motion::Ramp))
            .unwrap();
        assert_eq!(first_ramp.start().unwrap().xyz.x(), 0.0);
    }

    #[test]
    fn tab_profile_rises_and_falls() {
        let l = linking(RetractPolicy::Minimum, LeadKind::None, RampKind::Plunge);
        let p = Path2::open(vec![line(Point2::new(0.0, 0.0), Point2::new(30.0, 0.0))]);
        let cut = Cut {
            profile: vec![
                (0.0, 10.0),
                (10.0, 10.0),
                (12.0, 12.0),
                (18.0, 12.0),
                (20.0, 10.0),
                (30.0, 10.0),
            ],
            ..Cut::flat(p, 10.0, Turn::Right, 20.0)
        };
        let (r, _) = run(&[cut], &l, &[]);
        let cut_pts: Vec<Point3> = r
            .segments()
            .filter(|s| s.motion().is_some_and(|m| m.motion == Motion::Cut))
            .map(|s| s.end().unwrap().xyz)
            .collect();
        assert_eq!(
            cut_pts,
            [
                Point3::new(10.0, 0.0, 10.0),
                Point3::new(12.0, 0.0, 12.0),
                Point3::new(18.0, 0.0, 12.0),
                Point3::new(20.0, 0.0, 10.0),
                Point3::new(30.0, 0.0, 10.0)
            ]
        );
    }
}
