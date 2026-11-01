// SPDX-License-Identifier: MIT OR Apache-2.0

//! `engrave` (D09 §2.6, D15b §2.4): V-carving along the exact medial axis
//! of closed shapes. On a branch with clearance radius ρ the tip is at
//! depth (ρ − Rt) / tan α, so the cone touches both walls. Where that
//! would pass `max_depth` the area is left flat: the tool runs round it at
//! `max_depth`, and the area is reported (`cam.engrave.flat_area`) for a
//! clearing toolpath.

use super::contour_2d::selection_z;
use crate::chain::chain_pieces;
use crate::drive::{curve_pieces, nest};
use crate::layout::levels;
use crate::link::{Cut, LinkCx, Turn, link};
use crate::moves::finish;
use crate::path2::Path2;
use crate::region::medial::{Edge, MedialAxis, medial_axis};
use crate::region::{Region2, Seg2};
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, prepare, tag};
use fluut_geom::Point2;
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::{LeadKind, ResolvedHeights, resolve};
use fluut_path::ToolpathResult;

#[derive(Debug)]
pub struct Engrave;

/// What the calculation needs, after validation.
struct Setup {
    region: Region2,
    alpha: f64,
    rt: f64,
    max_depth: f64,
    step_down: Option<f64>,
    sharpen: bool,
    tol: f64,
    prep: crate::toolpath::Prepared,
}

impl Engrave {
    /// The flat areas the V tool leaves at `max_depth` (D15b §2.4): the
    /// shapes shrunk by Rt + `max_depth` · tan α. A command turns them into
    /// a WireBody for a clearing toolpath (D15b §9, decision 9).
    pub fn flat_areas(input: &StrategyInput) -> Result<Region2, CamError> {
        let s = setup(input)?;
        Ok(s.region.offset(-rho_max(&s)))
    }
}

fn rho_max(s: &Setup) -> f64 {
    s.rt + s.max_depth * libm::tan(s.alpha)
}

fn setup(input: &StrategyInput) -> Result<Setup, CamError> {
    let tp = input.toolpath;
    let fail = |d: Vec<Diagnostic>| CamError::Invalid(tag(d, tp));
    let r0 = resolve(&tp.params, input.tool, input.material);
    let tol = r0.passes.machining_tolerance.max(1e-4);
    let g = input.geometry;
    let chains = chain_pieces(
        curve_pieces(
            g,
            &r0.geometry.drive_curves,
            tol / 4.0,
            "geometry.drive_curves",
        )
        .map_err(|d| fail(vec![d]))?,
        g.tol_model().max(1e-5),
    );
    let prep = prepare(input, selection_z(&chains)).map_err(|e| match e {
        CamError::Invalid(d) => fail(d),
        e => e,
    })?;
    let mut errors = Vec::new();
    if chains.is_empty() {
        errors.push(Diagnostic::error(
            "cam.geometry.drive.empty",
            "the drive selection gives no curves to engrave",
        ));
    }
    for ch in &chains {
        if !ch.path.closed {
            errors.push(Diagnostic::error(
                "cam.pocket_2d.region.open",
                "a drive chain is not closed and cannot bound a shape",
            ));
        } else if ch.self_intersects() {
            errors.push(Diagnostic::error(
                "cam.geometry.chain_self_intersects",
                "a drive chain intersects itself",
            ));
        }
    }
    if !errors.is_empty() {
        let mut d = prep.warnings;
        d.extend(errors);
        return Err(fail(d));
    }
    let Some(e) = prep.params.engrave().cloned() else {
        return Err(CamError::Unavailable("not an engrave toolpath".into()));
    };
    let (Some(alpha), Some(max_depth)) = (input.tool.v_angle(), e.max_depth) else {
        return Err(CamError::Unavailable("engrave needs a V tool".into()));
    };
    let region = nest(chains.iter().map(|c| c.ccw_loop()).collect());
    Ok(Setup {
        region,
        alpha,
        rt: input.tool.tip_radius(),
        max_depth,
        step_down: e.step_down,
        sharpen: e.corner_sharpening,
        tol: prep.params.passes.machining_tolerance.max(1e-4),
        prep,
    })
}

impl ToolpathType for Engrave {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Engrave
    }

    fn role(&self) -> Role {
        Role::Finish
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        let tp = input.toolpath;
        let s = setup(input)?;
        let mut diags = s.prep.warnings.clone();
        let axis = medial_axis(&s.region).map_err(|e| {
            CamError::Invalid(tag(
                vec![Diagnostic::error("cam.engrave.medial_axis", e.to_string())],
                tp,
            ))
        })?;
        let tan = libm::tan(s.alpha);
        let rmax = rho_max(&s);
        let runs = carve_runs(&axis, &s, tan, rmax);
        let flat = s.region.offset(-rmax);
        if !flat.is_empty() {
            diags.push(Diagnostic::warning(
                "cam.engrave.flat_area",
                format!(
                    "areas wider than the V cuts at max depth {} are left flat; add a clearing \
                     toolpath for them",
                    s.max_depth
                ),
            ));
        }
        let depth = |rho: f64| ((rho - s.rt) / tan).clamp(0.0, s.max_depth);
        let mut items: Vec<Item> = paths_of(&runs)
            .into_iter()
            .map(|pts| Item::open(&pts, depth))
            .collect();
        items.extend(flat.loops.iter().map(|l| Item::Ring(Path2::from_loop(l))));
        let deepest = items
            .iter()
            .map(|it| match it {
                Item::Open { depth, .. } => depth.iter().copied().fold(0.0, f64::max),
                Item::Ring(_) => s.max_depth,
            })
            .fold(0.0, f64::max);
        let top = s.prep.heights.top;
        let order = order(items);
        let mut cuts = Vec::new();
        // Levels stop at the deepest point of the path: deeper levels
        // would repeat the last one.
        for z in levels(top, top - deepest, s.step_down, None) {
            for it in &order {
                cuts.push(it.cut(top, z));
            }
        }
        let mut linking = s.prep.params.linking.clone();
        linking.lead_in.kind = LeadKind::None;
        linking.lead_out.kind = LeadKind::None;
        let cx = LinkCx {
            heights: ResolvedHeights {
                bottom: top - s.max_depth,
                ..s.prep.heights
            },
            material_top: input.env.stock_top.max(top),
            linking: &linking,
            tool_diameter: input.tool.diameter,
            tol: s.tol,
            safe: &[],
        };
        let passes = link(&cuts, &cx, &mut diags);
        let p = &s.prep.params.passes;
        let passes = finish(passes, p.output_arcs, p.max_segment_length, s.tol);
        let result = ToolpathResult::new(passes).map_err(CamError::Path)?;
        Ok(Generated {
            result,
            diagnostics: tag(diags, tp),
        })
    }
}

/// How a run of the axis ends.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum End {
    /// A node where the run meets others.
    Node(usize),
    /// A convex corner of the boundary (clearance 0), possibly cut back.
    Corner,
    /// The edge of a flat area.
    Flat,
    /// Any other end: an arc centre, or where loops touch.
    Free,
}

/// A piece of the axis to cut: points with their clearance radius.
#[derive(Clone, Debug)]
struct Run {
    pts: Vec<(Point2, f64)>,
    ends: [End; 2],
}

/// A sample of the axis: point, radius, and where it came from.
#[derive(Clone, Copy, Debug)]
struct Sample {
    p: Point2,
    rho: f64,
    edge: usize,
    u: f64,
}

/// The parts of the axis the V tool follows: chains between branch nodes,
/// sampled, with insignificant corner ends pruned, flat areas cut out and
/// the ends shallower than the tip trimmed.
fn carve_runs(axis: &MedialAxis, s: &Setup, tan: f64, rmax: f64) -> Vec<Run> {
    let tol = s.tol;
    let z_scale = 1.0 / tan;
    let mut out = Vec::new();
    for (chain, ends) in chains(axis) {
        let mut pts: Vec<Sample> = Vec::new();
        for &(e, fwd) in &chain {
            let edge = &axis.edges[e];
            let mut us = axis.sample(edge, 0.25 * tol, z_scale);
            if !fwd {
                us.reverse();
            }
            for u in us {
                let (p, rho) = axis.eval(edge, u);
                if pts.last().is_some_and(|q| q.p == p) {
                    continue;
                }
                pts.push(Sample { p, rho, edge: e, u });
            }
        }
        if pts.len() < 2 {
            continue;
        }
        let corner = |end: End| end == End::Corner;
        // Prune corner ends whose cut alone removes less than the
        // tolerance.
        let gap_ok = |q: &Sample| -> bool {
            let edge = &axis.edges[q.edge];
            let (fl, fr) = axis.feet(edge, q.u);
            let (a, b) = (fl - q.p, fr - q.p);
            let (la, lb) = (a.length(), b.length());
            if la == 0.0 || lb == 0.0 {
                return true;
            }
            let c = (a.dot(b) / (la * lb)).clamp(-1.0, 1.0);
            // cos(φ/2) from cos φ.
            let half = (0.5 * (1.0 + c)).sqrt();
            if half <= 0.0 {
                return false;
            }
            let gap = q.rho * (1.0 / half - 1.0);
            gap * z_scale.max(1.0) <= tol
        };
        let mut lo = 0;
        let mut hi = pts.len();
        if corner(ends[0]) {
            while lo < hi && gap_ok(&pts[lo]) {
                lo += 1;
            }
            lo = lo.saturating_sub(1);
        }
        if corner(ends[1]) {
            while hi > lo && gap_ok(&pts[hi - 1]) {
                hi -= 1;
            }
            hi = (hi + 1).min(pts.len());
        }
        if hi < lo + 2 {
            continue;
        }
        let pts: Vec<(Point2, f64)> = pts[lo..hi].iter().map(|q| (q.p, q.rho)).collect();
        for mut run in split_flat(&pts, ends, rmax) {
            if !s.sharpen && run.ends.contains(&End::Flat) && run.ends.contains(&End::Corner) {
                continue;
            }
            trim_tip(&mut run, s.rt);
            if run.pts.len() >= 2 {
                out.push(run);
            }
        }
    }
    out
}

/// Edges in walking order, each with `true` when walked from `a` to `b`.
type Chain = Vec<(usize, bool)>;

/// Chains of oriented edges between nodes that are not of degree 2, and
/// closed chains through degree-2 nodes only. Each comes with its end
/// kinds.
fn chains(axis: &MedialAxis) -> Vec<(Chain, [End; 2])> {
    let n = axis.nodes.len();
    let mut at: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, e) in axis.edges.iter().enumerate() {
        at[e.a].push(i);
        at[e.b].push(i);
    }
    let end_of = |v: usize| -> End {
        if at[v].len() != 1 {
            End::Node(v)
        } else if axis.nodes[v].radius <= 10.0 * axis.tol {
            End::Corner
        } else {
            End::Free
        }
    };
    let other = |e: &Edge, v: usize| if e.a == v { e.b } else { e.a };
    let mut used = vec![false; axis.edges.len()];
    let mut out = Vec::new();
    let walk = |start: usize, first: usize, used: &mut Vec<bool>| {
        let mut chain = Vec::new();
        let (mut v, mut e) = (start, first);
        loop {
            used[e] = true;
            let edge = &axis.edges[e];
            chain.push((e, edge.a == v));
            v = other(edge, v);
            if at[v].len() != 2 || v == start {
                break;
            }
            match at[v].iter().copied().find(|&x| !used[x]) {
                Some(x) => e = x,
                None => break,
            }
        }
        (chain, v)
    };
    for (v, inc) in at.iter().enumerate() {
        if inc.len() == 2 {
            continue;
        }
        for &e in inc {
            if used[e] {
                continue;
            }
            let (chain, w) = walk(v, e, &mut used);
            out.push((chain, [end_of(v), end_of(w)]));
        }
    }
    // Cycles through degree-2 nodes only (an annulus).
    for e in 0..axis.edges.len() {
        if !used[e] {
            let v = axis.edges[e].a;
            let (chain, _) = walk(v, e, &mut used);
            out.push((chain, [End::Node(v), End::Node(v)]));
        }
    }
    out
}

/// The parts of `pts` with radius below `rmax`, split where the radius
/// crosses it (linearly between samples).
fn split_flat(pts: &[(Point2, f64)], ends: [End; 2], rmax: f64) -> Vec<Run> {
    let inside = |r: f64| r <= rmax;
    let cross = |a: (Point2, f64), b: (Point2, f64)| -> (Point2, f64) {
        let f = ((rmax - a.1) / (b.1 - a.1)).clamp(0.0, 1.0);
        (a.0 + (b.0 - a.0) * f, rmax)
    };
    let mut out = Vec::new();
    let mut cur: Vec<(Point2, f64)> = Vec::new();
    let mut start_end = ends[0];
    for (i, &q) in pts.iter().enumerate() {
        if inside(q.1) {
            if cur.is_empty() && i > 0 {
                cur.push(cross(pts[i - 1], q));
                start_end = End::Flat;
            }
            cur.push(q);
        } else if !cur.is_empty() {
            cur.push(cross(pts[i - 1], q));
            out.push(Run {
                pts: std::mem::take(&mut cur),
                ends: [start_end, End::Flat],
            });
        }
    }
    if !cur.is_empty() {
        out.push(Run {
            pts: cur,
            ends: [start_end, ends[1]],
        });
    }
    out
}

/// Drops the parts of corner ends where the radius is below the tip
/// radius: the depth there is 0.
fn trim_tip(run: &mut Run, rt: f64) {
    if rt <= 0.0 {
        return;
    }
    let cut = |pts: &mut Vec<(Point2, f64)>| {
        let k = pts.iter().position(|q| q.1 > rt);
        match k {
            None => pts.clear(),
            Some(0) => {}
            Some(k) => {
                let (a, b) = (pts[k - 1], pts[k]);
                let f = ((rt - a.1) / (b.1 - a.1)).clamp(0.0, 1.0);
                pts.drain(..k);
                pts.insert(0, (a.0 + (b.0 - a.0) * f, rt));
            }
        }
    };
    if run.ends[0] == End::Corner {
        cut(&mut run.pts);
    }
    if run.ends[1] == End::Corner {
        run.pts.reverse();
        cut(&mut run.pts);
        run.pts.reverse();
    }
}

/// Runs joined end to end at the nodes they share, into as few paths as a
/// greedy walk gives.
fn paths_of(runs: &[Run]) -> Vec<Vec<(Point2, f64)>> {
    let mut at: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for (i, r) in runs.iter().enumerate() {
        for e in r.ends {
            if let End::Node(v) = e {
                at.entry(v).or_default().push(i);
            }
        }
    }
    let mut used = vec![false; runs.len()];
    let mut out = Vec::new();
    let mut here: Option<Point2> = None;
    let open_end = |i: usize, k: usize, used: &[bool]| -> bool {
        // An end where no other unused run continues.
        match runs[i].ends[k] {
            End::Node(v) => at[&v].iter().filter(|&&j| !used[j]).count() == 1,
            _ => true,
        }
    };
    while used.iter().any(|u| !u) {
        // Start at a dead end nearest the last position, else at any end.
        let mut best: Option<(bool, f64, usize, usize)> = None;
        for i in (0..runs.len()).filter(|&i| !used[i]) {
            for k in 0..2 {
                let p = if k == 0 {
                    runs[i].pts[0].0
                } else {
                    runs[i].pts[runs[i].pts.len() - 1].0
                };
                let d = here.map_or(0.0, |h| h.distance(p));
                let dead = open_end(i, k, &used);
                let key = (!dead, d, i, k);
                if best.is_none_or(|b| {
                    (key.0, key.1)
                        .partial_cmp(&(b.0, b.1))
                        .is_some_and(|o| o.is_lt())
                }) {
                    best = Some(key);
                }
            }
        }
        let Some((_, _, mut i, mut k)) = best else {
            break;
        };
        let mut path: Vec<(Point2, f64)> = Vec::new();
        loop {
            used[i] = true;
            let mut pts = runs[i].pts.clone();
            if k == 1 {
                pts.reverse();
            }
            let far = runs[i].ends[1 - k];
            if path.is_empty() {
                path = pts;
            } else {
                path.extend(pts.into_iter().skip(1));
            }
            let End::Node(v) = far else {
                break;
            };
            let Some(&j) = at[&v].iter().find(|&&j| !used[j]) else {
                break;
            };
            k = if runs[j].ends[0] == End::Node(v) {
                0
            } else {
                1
            };
            i = j;
        }
        here = path.last().map(|q| q.0);
        out.push(path);
    }
    out
}

/// A path to cut at every level: open along the axis with a depth at each
/// point, or a closed ring round a flat area at `max_depth`.
#[derive(Clone, Debug)]
enum Item {
    Open { path: Path2, depth: Vec<f64> },
    Ring(Path2),
}

impl Item {
    fn open(pts: &[(Point2, f64)], depth: impl Fn(f64) -> f64) -> Item {
        let segs = pts
            .windows(2)
            .map(|w| Seg2 {
                a: w[0].0,
                b: w[1].0,
                bulge: 0.0,
            })
            .collect();
        Item::Open {
            path: Path2::open(segs),
            depth: pts.iter().map(|q| depth(q.1)).collect(),
        }
    }

    fn reversed(self) -> Item {
        match self {
            Item::Open { path, mut depth } => {
                depth.reverse();
                Item::Open {
                    path: path.reversed(),
                    depth,
                }
            }
            r => r,
        }
    }

    /// The cut at level `z`: depths clipped to it.
    fn cut(&self, top: f64, z: f64) -> Cut {
        match self {
            Item::Ring(path) => Cut::flat(path.clone(), z, Turn::Left, top),
            Item::Open { path, depth } => {
                let mut acc = 0.0;
                let mut profile = Vec::with_capacity(depth.len());
                profile.push((0.0, (top - depth[0]).max(z)));
                for (s, d) in path.segs.iter().zip(&depth[1..]) {
                    acc += s.length();
                    profile.push((acc, (top - d).max(z)));
                }
                let mut c = Cut::flat(path.clone(), z, Turn::Left, top);
                c.profile = profile;
                c
            }
        }
    }
}

/// Items in a nearest-next order: open paths may be reversed, rings start
/// at their point nearest the last end.
fn order(items: Vec<Item>) -> Vec<Item> {
    let mut left: Vec<Option<Item>> = items.into_iter().map(Some).collect();
    let mut out = Vec::new();
    let mut here: Option<Point2> = None;
    while left.iter().any(Option::is_some) {
        let mut best: Option<(f64, usize, bool)> = None;
        for (i, it) in left.iter().enumerate() {
            let Some(it) = it else {
                continue;
            };
            let cands: Vec<(f64, bool)> = match (it, here) {
                (_, None) => vec![(0.0, false)],
                (Item::Open { path, .. }, Some(h)) => vec![
                    (h.distance(path.start()), false),
                    (h.distance(path.end()), true),
                ],
                (Item::Ring(path), Some(h)) => {
                    vec![(path.at(path.closest(h).0).0.distance(h), false)]
                }
            };
            for (d, rev) in cands {
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, i, rev));
                }
            }
        }
        let Some((_, i, rev)) = best else {
            break;
        };
        let Some(mut it) = left[i].take() else {
            break;
        };
        if rev {
            it = it.reversed();
        }
        if let (Item::Ring(path), Some(h)) = (&it, here) {
            it = Item::Ring(path.rotated(path.closest(h).0, 1e-9));
        }
        here = Some(match &it {
            Item::Open { path, .. } => path.end(),
            Item::Ring(path) => path.end(),
        });
        out.push(it);
    }
    out
}
