// SPDX-License-Identifier: MIT OR Apache-2.0

//! `contour` (D09 §4.4, D15b §5.1): constant-Z (waterline) passes for
//! steep walls. Finishing type.
//!
//! Levels are even steps from `top` to `bottom` plus the Z of every
//! horizontal planar drive face; with a scallop height, gaps are halved
//! where the walls need it. At each level the CL contours come from the
//! drop function ([`crate::waterline`]); stretches whose contact is not on
//! a drive face or not within the slope limits are linked over, with exact
//! cut-off points.

use crate::drive::is_horizontal_plane;
use crate::follow::{Linker, Run, Sampler, Setup, merge, setup};
use crate::layout::levels;
use crate::moves::finish;
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, tag};
use crate::waterline::{End, WPoint, Waterline};
use fluut_geom::{Point2, Point3};
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{CutDirection, Order, RetractPolicy};
use fluut_path::ToolpathResult;

#[derive(Debug)]
pub struct Contour;

impl ToolpathType for Contour {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Contour
    }

    fn role(&self) -> Role {
        Role::Finish
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        let mut st = setup(input, "contour")?;
        let Some(c) = st.params.contour3d().cloned() else {
            return Err(CamError::Unavailable("not a contour toolpath".into()));
        };
        let settings = Levels {
            step_down: c.step_down,
            scallop_height: c.scallop_height,
            min_slope: c.min_slope_deg.to_radians(),
            max_slope: c.max_slope_deg.to_radians(),
            direction: c.direction,
            order: st.params.linking.order,
        };
        let region = st.region(input, waterline_grow(&st, input))?;
        let wl = waterline(&st, input, &region);
        let LevelRuns {
            runs,
            lost,
            outlines,
        } = settings.runs(&st, input, &wl);
        let mut diags = std::mem::take(&mut st.diags);
        if lost {
            diags.push(loop_open());
        }
        if runs.is_empty() {
            diags.push(Diagnostic::warning(
                "cam.contour.empty",
                "no part of the region is cut: check the slope limits, heights and boundary",
            ));
        }
        let clear = |a: Point3, b: Point3| outlines.clear(&st.q, a, b);
        link_runs(
            &st,
            input,
            &region,
            &runs,
            diags,
            "contour",
            Some(&clear),
            None,
        )
    }
}

/// The traced contours of every level, uncut parts included: what a
/// horizontal link at a level must not cross.
#[derive(Debug, Default)]
pub(crate) struct Outlines {
    levels: Vec<(f64, Vec<Vec<Point2>>)>,
}

impl Outlines {
    /// Whether the horizontal move from `a` to `b` (at `a`'s Z, a level)
    /// stays out of the material: `b` is not inside it and the move
    /// crosses no contour of the level away from `a`.
    pub fn clear(&self, q: &crate::surface::SurfaceQuery, a: Point3, b: Point3) -> bool {
        let z = a.z();
        let Some((_, lines)) = self.levels.iter().find(|(lz, _)| (lz - z).abs() <= 1e-9) else {
            return false;
        };
        if q.drop(b.x(), b.y())
            .is_some_and(|h| h.tip > z + crate::waterline::DELTA)
        {
            return false;
        }
        let (pa, pb) = (Point2::new(a.x(), a.y()), Point2::new(b.x(), b.y()));
        for l in lines {
            for w in l.windows(2) {
                if let Some(x) = crossing(pa, pb, w[0], w[1])
                    && (x - pa).length() > 1e-6
                {
                    return false;
                }
            }
        }
        true
    }
}

/// Where the segments `a`–`b` and `c`–`d` meet, if they do.
fn crossing(a: Point2, b: Point2, c: Point2, d: Point2) -> Option<Point2> {
    let r = b - a;
    let s = d - c;
    let den = r.perp_dot(s);
    if den.abs() < 1e-300 {
        return None;
    }
    let t = (c - a).perp_dot(s) / den;
    let u = (c - a).perp_dot(r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| a + r * t)
}

/// Runs of a waterline type, and what linking needs.
pub(crate) struct LevelRuns {
    pub runs: Vec<Run>,
    /// A contour ends inside the region (`cam.contour.loop_open`).
    pub lost: bool,
    pub outlines: Outlines,
}

/// `cam.contour.loop_open` (D15b §5.1).
pub(crate) fn loop_open() -> Diagnostic {
    Diagnostic::warning(
        "cam.contour.loop_open",
        "a contour ends inside the region and is linked over",
    )
}

/// The region margin of waterline types: contours around the drive faces
/// lie up to R + stock outside them.
pub(crate) fn waterline_grow(st: &Setup, input: &StrategyInput) -> f64 {
    input.tool.radius()
        + st.params.passes.stock_to_leave.max(0.0)
        + st.params.passes.machining_tolerance
}

pub(crate) fn waterline<'a>(
    st: &'a Setup<'a>,
    input: &StrategyInput,
    region: &'a crate::Region2,
) -> Waterline<'a> {
    let r = input.tool.radius();
    let tol = st.params.passes.machining_tolerance;
    Waterline::new(&st.q, region, tol, 0.5 * r, 0.5 * r)
}

/// Links runs and builds the result, with `cam.<ty>.lead.dropped`.
/// `retract` overrides the linking policy.
#[allow(clippy::too_many_arguments)]
pub(crate) fn link_runs(
    st: &Setup,
    input: &StrategyInput,
    region: &crate::Region2,
    runs: &[Run],
    mut diags: Vec<Diagnostic>,
    ty: &str,
    level_clear: Option<&dyn Fn(Point3, Point3) -> bool>,
    retract: Option<RetractPolicy>,
) -> Result<Generated, CamError> {
    let p = &st.params;
    let h = st.heights;
    let tol = p.passes.machining_tolerance;
    let pl = Sampler {
        q: &st.q,
        h: &h,
        min_slope: 0.0,
        max_slope: std::f64::consts::FRAC_PI_2,
        tol,
        max_step: 0.5 * input.tool.radius(),
    };
    let lk = Linker {
        pl: &pl,
        region,
        h: &h,
        retract: retract.unwrap_or(p.linking.retract),
        safety: p.linking.safety_distance,
        diameter: input.tool.diameter,
        lead_in: p.linking.lead_in,
        lead_out: p.linking.lead_out,
        level_clear,
        material_top: input.env.stock_top.max(h.top),
    };
    let (passes, dropped) = lk.link(runs);
    if dropped {
        diags.push(Diagnostic::warning(
            &format!("cam.{ty}.lead.dropped"),
            "a lead would gouge the model and was left out",
        ));
    }
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

/// Waterline settings: `contour`, or the steep part of
/// `steep_and_shallow`.
#[derive(Clone, Debug)]
pub(crate) struct Levels {
    pub step_down: f64,
    pub scallop_height: Option<f64>,
    pub min_slope: f64,
    pub max_slope: f64,
    pub direction: CutDirection,
    pub order: Order,
}

/// The contours of one level, cut where `need` (if any) is below the
/// local wall rule.
struct Traced {
    z: f64,
    contours: Vec<crate::waterline::Contour>,
    /// Added level (scallop height): cut only where sin β · √(8rh) < gap.
    gap: Option<f64>,
}

impl Levels {
    /// Cutting levels, top first: even steps no larger than `step_down`
    /// between `top`, the flats in (bottom, top] and `bottom`.
    fn zs(&self, st: &Setup) -> Vec<f64> {
        let h = &st.heights;
        let mut flats: Vec<f64> = Vec::new();
        for (b, f, _) in &st.drive {
            let face = &b.faces[*f as usize];
            let s = &b.surfaces[face.surface as usize];
            if !is_horizontal_plane(s) {
                continue;
            }
            let z = crate::geometry::flat_z(b, *f, 1e-3) + st.params.passes.stock_to_leave;
            if z > h.bottom + 1e-9 && z <= h.top + 1e-9 {
                flats.push(z.min(h.top));
            }
        }
        flats.sort_by(|a, b| b.total_cmp(a));
        flats.dedup_by(|a, b| (*a - *b).abs() <= 1e-6);
        let mut out = Vec::new();
        let mut hi = h.top;
        for b in flats.into_iter().chain(std::iter::once(h.bottom)) {
            if b >= hi - 1e-9 {
                if out.is_empty() && (b - hi).abs() <= 1e-9 {
                    out.push(b);
                }
                continue;
            }
            out.extend(levels(hi, b, Some(self.step_down), None));
            hi = b;
        }
        if out.is_empty() {
            out.push(h.bottom);
        }
        out
    }

    fn cut(&self, p: &WPoint) -> bool {
        let eps = 1e-12;
        p.drive && p.slope >= self.min_slope - eps && p.slope <= self.max_slope + eps
    }

    /// The cut runs of every level in machining order.
    pub fn runs(&self, st: &Setup, input: &StrategyInput, wl: &Waterline) -> LevelRuns {
        let tol = st.params.passes.machining_tolerance;
        let zs = self.zs(st);
        let mut traced: Vec<Traced> = zs
            .iter()
            .zip(wl.levels(&zs))
            .map(|(&z, contours)| Traced {
                z,
                contours,
                gap: None,
            })
            .collect();
        if let Some(sh) = self.scallop_height {
            let k = (8.0 * input.tool.corner_radius() * sh).sqrt();
            let gmin = tol.max(self.step_down / 64.0);
            let mut added = Vec::new();
            for w in traced.windows(2) {
                self.refine(wl, &w[0], &w[1], k, gmin, &mut added);
            }
            traced.extend(added);
            traced.sort_by(|a, b| b.z.total_cmp(&a.z));
        }
        // A contour that ends inside the region: lost by the trace, or cut
        // short at a slope or drive-face limit (linked over).
        let mut lost = traced
            .iter()
            .flat_map(|t| &t.contours)
            .any(|c| !c.closed && (c.ends.0 == End::Lost || c.ends.1 == End::Lost));
        // Runs per contour, per level.
        let mut groups: Vec<Group> = Vec::new();
        let mut k = (8.0 * input.tool.corner_radius() * self.scallop_height.unwrap_or(0.0)).sqrt();
        if !k.is_finite() {
            k = 0.0;
        }
        for (li, t) in traced.iter().enumerate() {
            let cut = |p: &WPoint| self.cut(p) && t.gap.is_none_or(|g| libm::sin(p.slope) * k < g);
            for c in &t.contours {
                if t.gap.is_none()
                    && c.pts.iter().any(|p| self.cut(p))
                    && c.pts.iter().any(|p| !self.cut(p))
                {
                    lost = true;
                }
                let mut rs = split(wl, c, t.z, &cut, tol);
                if rs.is_empty() {
                    continue;
                }
                if self.direction == CutDirection::Conventional {
                    rs.reverse();
                    for r in &mut rs {
                        r.reverse();
                    }
                }
                groups.push(Group {
                    level: li,
                    closed: c.closed && rs.len() == 1 && rs[0].first() == rs[0].last(),
                    runs: rs,
                });
            }
        }
        let outlines = Outlines {
            levels: traced
                .iter()
                .map(|t| {
                    let lines = t
                        .contours
                        .iter()
                        .map(|c| c.pts.iter().map(|w| w.p).collect())
                        .collect();
                    (t.z, lines)
                })
                .collect(),
        };
        LevelRuns {
            runs: order(groups, self.order, traced.len()),
            lost,
            outlines,
        }
    }

    /// Adds levels between `a` (upper) and `b` where a cut point at either
    /// has sin β · k < gap, halving until the gap is ≤ `gmin`.
    fn refine(
        &self,
        wl: &Waterline,
        a: &Traced,
        b: &Traced,
        k: f64,
        gmin: f64,
        out: &mut Vec<Traced>,
    ) {
        let g = a.z - b.z;
        if g <= gmin {
            return;
        }
        let needs = |t: &Traced| {
            t.contours.iter().flat_map(|c| &c.pts).any(|p| {
                self.cut(p)
                    && t.gap.is_none_or(|tg| libm::sin(p.slope) * k < tg)
                    && libm::sin(p.slope) * k < g
            })
        };
        if !needs(a) && !needs(b) {
            return;
        }
        let zm = 0.5 * (a.z + b.z);
        let m = Traced {
            z: zm,
            contours: wl.level(zm),
            gap: Some(g),
        };
        self.refine(wl, a, &m, k, gmin, out);
        self.refine(wl, &m, b, k, gmin, out);
        out.push(m);
    }
}

/// The cut runs of one contour of one level.
struct Group {
    level: usize,
    closed: bool,
    runs: Vec<Vec<Point3>>,
}

impl Group {
    fn start(&self) -> Point3 {
        self.runs[0][0]
    }

    fn bbox(&self) -> (Point2, Point2) {
        let mut lo = Point2::new(f64::INFINITY, f64::INFINITY);
        let mut hi = Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in self.runs.iter().flatten() {
            lo = Point2::new(lo.x().min(p.x()), lo.y().min(p.y()));
            hi = Point2::new(hi.x().max(p.x()), hi.y().max(p.y()));
        }
        (lo, hi)
    }

    /// A closed loop starting at its point nearest `p`.
    fn start_near(&mut self, p: Point3) {
        if !self.closed {
            return;
        }
        let r = &mut self.runs[0];
        let n = r.len() - 1;
        let k = (0..n)
            .min_by(|&i, &j| {
                let di = xy_dist(r[i], p);
                let dj = xy_dist(r[j], p);
                di.total_cmp(&dj)
            })
            .unwrap_or(0);
        if k > 0 {
            let mut v: Vec<Point3> = r[k..n].to_vec();
            v.extend_from_slice(&r[..=k]);
            *r = v;
        }
    }
}

fn xy_dist(a: Point3, b: Point3) -> f64 {
    libm::hypot(a.x() - b.x(), a.y() - b.y())
}

/// Splits a contour into its cut stretches at level `z` (exact cut-off
/// points), merged within `tol/4`; stretches shorter than `tol` dropped.
fn split(
    wl: &Waterline,
    c: &crate::waterline::Contour,
    z: f64,
    cut: &impl Fn(&WPoint) -> bool,
    tol: f64,
) -> Vec<Vec<Point3>> {
    let mut pts = c.pts.clone();
    if pts.len() < 2 {
        return Vec::new();
    }
    let flags: Vec<bool> = pts.iter().map(cut).collect();
    if c.closed {
        if flags.iter().all(|&f| f) {
            let run: Vec<Point3> = pts.iter().map(|w| lift(w.p, z)).collect();
            return keep(vec![run], tol);
        }
        // Start at an uncut point.
        let k = flags.iter().position(|&f| !f).unwrap_or(0);
        let n = pts.len() - 1;
        let mut v: Vec<WPoint> = pts[k..n].to_vec();
        v.extend_from_slice(&pts[..=k]);
        pts = v;
    }
    let flags: Vec<bool> = pts.iter().map(cut).collect();
    let mut runs = Vec::new();
    let mut cur: Vec<Point3> = Vec::new();
    for i in 0..pts.len() {
        if i > 0 && flags[i] != flags[i - 1] {
            let (inn, outp) = if flags[i] {
                (pts[i], pts[i - 1])
            } else {
                (pts[i - 1], pts[i])
            };
            let b = wl.cutoff(inn, outp, z, cut);
            cur.push(lift(b.p, z));
            if !flags[i] {
                runs.push(std::mem::take(&mut cur));
            }
        }
        if flags[i] {
            cur.push(lift(pts[i].p, z));
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    keep(runs, tol)
}

fn lift(p: Point2, z: f64) -> Point3 {
    Point3::new(p.x(), p.y(), z)
}

fn keep(runs: Vec<Vec<Point3>>, tol: f64) -> Vec<Vec<Point3>> {
    runs.into_iter()
        .filter(|r| r.len() >= 2 && r.windows(2).map(|w| (w[1] - w[0]).length()).sum::<f64>() > tol)
        .map(|r| {
            let closed = r.first() == r.last();
            let m = merge(r, 0.25 * tol);
            if closed && m.len() < 3 { Vec::new() } else { m }
        })
        .filter(|r| r.len() >= 2)
        .collect()
}

/// Orders the groups (D08 §8.1): by level, nearest next; or by area
/// (groups at adjacent levels whose boxes overlap), each area through all
/// its levels.
fn order(mut groups: Vec<Group>, order: Order, levels: usize) -> Vec<Run> {
    let n = groups.len();
    // Area of each group: union-find over overlapping boxes at adjacent
    // levels.
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    if order == Order::ByArea {
        let boxes: Vec<(Point2, Point2)> = groups.iter().map(Group::bbox).collect();
        for i in 0..n {
            for j in i + 1..n {
                if groups[j].level != groups[i].level + 1 && groups[i].level != groups[j].level + 1
                {
                    continue;
                }
                let (a, b) = (boxes[i], boxes[j]);
                let overlap = a.0.x() <= b.1.x()
                    && b.0.x() <= a.1.x()
                    && a.0.y() <= b.1.y()
                    && b.0.y() <= a.1.y();
                if overlap {
                    let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                    parent[ri.max(rj)] = ri.min(rj);
                }
            }
        }
    }
    let area: Vec<usize> = (0..n)
        .map(|i| {
            if order == Order::ByArea {
                find(&mut parent, i)
            } else {
                0
            }
        })
        .collect();
    let mut areas: Vec<usize> = Vec::new();
    for li in 0..levels {
        for (i, g) in groups.iter().enumerate() {
            if g.level == li && !areas.contains(&area[i]) {
                areas.push(area[i]);
            }
        }
    }
    let mut done = vec![false; n];
    let mut at: Option<Point3> = None;
    let mut out = Vec::new();
    for &a in &areas {
        for li in 0..levels {
            loop {
                let next = (0..n)
                    .filter(|&i| !done[i] && area[i] == a && groups[i].level == li)
                    .min_by(|&i, &j| match at {
                        None => i.cmp(&j),
                        Some(p) => xy_dist(groups[i].start(), p)
                            .total_cmp(&xy_dist(groups[j].start(), p))
                            .then(i.cmp(&j)),
                    });
                let Some(i) = next else { break };
                done[i] = true;
                if let Some(p) = at {
                    groups[i].start_near(p);
                }
                let g = &groups[i];
                for r in &g.runs {
                    out.push(Run {
                        pts: r.clone(),
                        pass: i + 1,
                        span: 0,
                        level: true,
                    });
                }
                at = g.runs.last().and_then(|r| r.last()).copied();
            }
        }
    }
    out
}
