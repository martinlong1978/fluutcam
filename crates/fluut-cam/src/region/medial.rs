// SPDX-License-Identifier: MIT OR Apache-2.0

//! The exact medial axis of a [`Region2`] bounded by lines and arcs
//! (CH-17, D09 §2.6).
//!
//! The axis is traced edge by edge (plan
//! `docs/plan/2026-11-01-medial-axis-engrave.md`). The sites are the lines
//! and arcs of the boundary and its reflex vertices. Each edge is the
//! bisector of two sites, kept exactly as the pair and a parameter range
//! on the right-hand site: [`MedialAxis::eval`] gives the point and its
//! clearance radius in closed form. The curves are lines, parabolas,
//! hyperbolas and ellipses ([`Conic`]).
//!
//! Walking along an edge with site R on the right, the foot on R moves
//! forward along the boundary (loops are counter-clockwise, holes
//! clockwise) and the foot on the left site L moves backward. An edge ends
//! where a third site becomes as near (a branch vertex), where a foot
//! leaves its site (the edge goes on with the next site), or at a convex
//! corner (a leaf with clearance 0).

mod site;

pub use site::{Geom, dir, equidistant, equidistant_where};

use super::{Region2, Seg2};
use crate::path2::seg_tangent;
use fluut_geom::{Point2, Vec2};
use std::collections::{BTreeMap, VecDeque};
use std::f64::consts::TAU;
use std::fmt;

/// Joints that turn by less than this (radians) are tangent: no branch
/// starts there and no point site is made.
const TANGENT: f64 = 1e-9;

/// Intervals an edge is split into for the search of third sites.
const SPANS: usize = 16;

/// A boundary element the axis keeps its distance from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Site {
    pub geom: Geom,
    /// Index of the loop in the region.
    pub loop_ix: usize,
    /// The next site along the boundary when the joint is reflex or
    /// tangent (a foot moving forward goes on there); `None` at a convex
    /// corner.
    pub next: Option<usize>,
    pub prev: Option<usize>,
    /// `(min, max)` corners of the site's bounding box.
    pub bbox: (Point2, Point2),
}

/// A vertex of the axis: a branch point, a leaf, or a point where an edge
/// changes sites.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub p: Point2,
    /// Clearance radius: the distance to the nearest boundary point.
    pub radius: f64,
    /// The sites at that distance, sorted.
    pub sites: Vec<usize>,
}

/// An edge: the points at equal distance from sites `left` and `right`
/// whose foot on `right` has a parameter in `[u0, u1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    pub left: usize,
    pub right: usize,
    pub u0: f64,
    pub u1: f64,
    /// Nodes at `u0` and `u1`.
    pub a: usize,
    pub b: usize,
}

/// The curve an edge lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conic {
    Line,
    Parabola,
    Hyperbola,
    /// Includes circles (concentric arcs).
    Ellipse,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MedialError {
    /// The trace did not finish within its step limit (a bug or input far
    /// outside the tolerances).
    Runaway,
}

impl fmt::Display for MedialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MedialError::Runaway => f.write_str("the medial axis trace did not finish"),
        }
    }
}

/// The medial axis of a region.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MedialAxis {
    pub sites: Vec<Site>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Distance tolerance used by the trace.
    pub tol: f64,
}

impl MedialAxis {
    /// The point of edge `e` whose foot on its right site is at `u`, and
    /// its clearance radius. The ends are the nodes, bit for bit.
    pub fn eval(&self, e: &Edge, u: f64) -> (Point2, f64) {
        if u == e.u0 {
            let n = &self.nodes[e.a];
            return (n.p, n.radius);
        }
        if u == e.u1 {
            let n = &self.nodes[e.b];
            return (n.p, n.radius);
        }
        let r = &self.sites[e.right].geom;
        let l = &self.sites[e.left].geom;
        if let (Geom::Line { .. }, Geom::Line { .. }) = (l, r) {
            // Point and radius are affine in u: interpolate the nodes,
            // which is exact and stays accurate between nearly parallel
            // lines.
            let f = (u - e.u0) / (e.u1 - e.u0);
            let (a, b) = (&self.nodes[e.a], &self.nodes[e.b]);
            return (a.p + (b.p - a.p) * f, a.radius + (b.radius - a.radius) * f);
        }
        match r.bisector_at(l, u, f64::INFINITY) {
            Some((p, rho)) => (p, rho.max(0.0)),
            None => {
                // Not reached on a traced edge; interpolate the ends.
                let f = (u - e.u0) / (e.u1 - e.u0);
                let (a, b) = (&self.nodes[e.a], &self.nodes[e.b]);
                (a.p + (b.p - a.p) * f, a.radius + (b.radius - a.radius) * f)
            }
        }
    }

    /// The feet of the point at `u` on the left and right sites.
    pub fn feet(&self, e: &Edge, u: f64) -> (Point2, Point2) {
        let (p, _) = self.eval(e, u);
        let l = &self.sites[e.left].geom;
        let r = &self.sites[e.right].geom;
        let ul = l.param(p).clamp(0.0, l.len());
        let ur = u.clamp(0.0, r.len());
        (l.foot(ul), r.foot(ur))
    }

    /// The kind of curve edge `e` lies on.
    pub fn conic(&self, e: &Edge) -> Conic {
        let l = self.sites[e.left].geom;
        let r = self.sites[e.right].geom;
        match (l, r) {
            (Geom::Line { .. }, Geom::Line { .. }) => Conic::Line,
            (Geom::Line { .. }, Geom::Circle { .. }) | (Geom::Circle { .. }, Geom::Line { .. }) => {
                Conic::Parabola
            }
            (Geom::Circle { c: c1, r: r1, .. }, Geom::Circle { c: c2, r: r2, .. }) => {
                if c1 == c2 || l.side() != r.side() {
                    Conic::Ellipse
                } else if r1 == r2 {
                    Conic::Line
                } else {
                    Conic::Hyperbola
                }
            }
        }
    }

    /// Parameters along `e`, from `u0` to `u1`, such that the polyline
    /// through the points in (x, y, `z_scale`·ρ) is within `tol` of the
    /// curve.
    pub fn sample(&self, e: &Edge, tol: f64, z_scale: f64) -> Vec<f64> {
        let at = |u: f64| -> [f64; 3] {
            let (p, rho) = self.eval(e, u);
            [p.x(), p.y(), rho * z_scale]
        };
        let mut out = vec![e.u0];
        let mut stack = vec![(e.u0, at(e.u0), e.u1, at(e.u1), 0u32)];
        while let Some((a, pa, b, pb, depth)) = stack.pop() {
            let flat = depth >= 24
                || [0.25, 0.5, 0.75].iter().all(|&f| {
                    let q = at(a + (b - a) * f);
                    let m = [
                        pa[0] + (pb[0] - pa[0]) * f,
                        pa[1] + (pb[1] - pa[1]) * f,
                        pa[2] + (pb[2] - pa[2]) * f,
                    ];
                    let d2 = (q[0] - m[0]).powi(2) + (q[1] - m[1]).powi(2) + (q[2] - m[2]).powi(2);
                    d2 <= tol * tol
                });
            if flat {
                out.push(b);
            } else {
                let m = 0.5 * (a + b);
                let pm = at(m);
                // Right half first on the stack, so the left half is
                // emitted first.
                stack.push((m, pm, b, pb, depth + 1));
                stack.push((a, pa, m, pm, depth + 1));
            }
        }
        out
    }

    /// Number of edges at each node.
    pub fn degrees(&self) -> Vec<usize> {
        let mut d = vec![0; self.nodes.len()];
        for e in &self.edges {
            d[e.a] += 1;
            d[e.b] += 1;
        }
        d
    }
}

/// The medial axis of `region`. Loops must not cross; they may touch.
pub fn medial_axis(region: &Region2) -> Result<MedialAxis, MedialError> {
    let (lo, hi) = if region.is_empty() {
        (Point2::new(0.0, 0.0), Point2::new(0.0, 0.0))
    } else {
        region.bbox()
    };
    let scale = (hi.x() - lo.x()).max(hi.y() - lo.y()).max(1.0);
    let tol = 1e-8 * scale;
    let sites = build_sites(region, tol);
    let mut t = Tracer::new(sites, region.loops.len(), lo, hi, tol);
    t.run()?;
    Ok(MedialAxis {
        sites: t.sites,
        nodes: t.nodes,
        edges: t.edges,
        tol,
    })
}

fn bbox_of(s: &Seg2) -> (Point2, Point2) {
    let (x0, x1) = s.support_x();
    let (y0, y1) = s.support_y();
    (Point2::new(x0, y0), Point2::new(x1, y1))
}

fn left(v: Vec2) -> Vec2 {
    Vec2::new(-v.y(), v.x())
}

/// Sites of every loop, in boundary order: for each segment, the reflex
/// point at its start (if any), then the segment.
fn build_sites(region: &Region2, tol: f64) -> Vec<Site> {
    let mut out: Vec<Site> = Vec::new();
    for (li, l) in region.loops.iter().enumerate() {
        let segs: Vec<Seg2> = l.segs().filter(|s| s.a.distance(s.b) > tol).collect();
        let n = segs.len();
        if n == 0 {
            continue;
        }
        // (site index of segment i, joint kind at its start)
        let mut seg_site = Vec::with_capacity(n);
        let mut joint_site: Vec<Option<usize>> = Vec::with_capacity(n);
        let mut tangent = Vec::with_capacity(n);
        for i in 0..n {
            let prev = &segs[(i + n - 1) % n];
            let s = &segs[i];
            let t_in = seg_tangent(prev, 1.0);
            let t_out = seg_tangent(s, 0.0);
            let turn = libm::atan2(t_in.perp_dot(t_out), t_in.dot(t_out));
            tangent.push(turn.abs() <= TANGENT);
            if turn < -TANGENT {
                let nin = left(t_in);
                out.push(Site {
                    geom: Geom::Circle {
                        c: s.a,
                        r: 0.0,
                        start: libm::atan2(nin.y(), nin.x()),
                        sweep: turn,
                    },
                    loop_ix: li,
                    next: None,
                    prev: None,
                    bbox: (s.a, s.a),
                });
                joint_site.push(Some(out.len() - 1));
            } else {
                joint_site.push(None);
            }
            let geom = match s.arc() {
                None => {
                    let d = s.b - s.a;
                    let len = d.length();
                    Geom::Line {
                        a: s.a,
                        t: d * (1.0 / len),
                        len,
                    }
                }
                Some(g) => Geom::Circle {
                    c: g.centre,
                    r: g.radius,
                    start: g.start,
                    sweep: g.sweep,
                },
            };
            out.push(Site {
                geom,
                loop_ix: li,
                next: None,
                prev: None,
                bbox: bbox_of(s),
            });
            seg_site.push(out.len() - 1);
        }
        for i in 0..n {
            let a = seg_site[(i + n - 1) % n];
            let b = seg_site[i];
            if let Some(p) = joint_site[i] {
                out[a].next = Some(p);
                out[p].prev = Some(a);
                out[p].next = Some(b);
                out[b].prev = Some(p);
            } else if tangent[i] {
                out[a].next = Some(b);
                out[b].prev = Some(a);
            }
        }
    }
    out
}

/// A uniform grid of site bounding boxes.
#[derive(Debug)]
struct Grid {
    lo: Point2,
    cell: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<usize>>,
}

impl Grid {
    fn new(sites: &[Site], lo: Point2, hi: Point2) -> Grid {
        let n = sites.len().max(1);
        let k = ((n as f64).sqrt().ceil() as usize).clamp(1, 256);
        let span = (hi.x() - lo.x()).max(hi.y() - lo.y()).max(1e-9);
        let cell = span / k as f64 * (1.0 + 1e-9);
        let mut g = Grid {
            lo,
            cell,
            nx: k,
            ny: k,
            cells: vec![Vec::new(); k * k],
        };
        for (i, s) in sites.iter().enumerate() {
            let (x0, y0, x1, y1) = g.range(s.bbox.0, s.bbox.1);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    g.cells[y * g.nx + x].push(i);
                }
            }
        }
        g
    }

    fn range(&self, a: Point2, b: Point2) -> (usize, usize, usize, usize) {
        let ix = |v: f64, o: f64, n: usize| -> usize {
            let f = ((v - o) / self.cell).floor();
            if f < 0.0 { 0 } else { (f as usize).min(n - 1) }
        };
        (
            ix(a.x(), self.lo.x(), self.nx),
            ix(a.y(), self.lo.y(), self.ny),
            ix(b.x(), self.lo.x(), self.nx),
            ix(b.y(), self.lo.y(), self.ny),
        )
    }

    /// Sites whose boxes may meet the box `a`–`b`, sorted.
    fn query(&self, a: Point2, b: Point2, stamp: &mut [u32], epoch: u32) -> Vec<usize> {
        let (x0, y0, x1, y1) = self.range(a, b);
        let mut out = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &i in &self.cells[y * self.nx + x] {
                    if stamp[i] != epoch {
                        stamp[i] = epoch;
                        out.push(i);
                    }
                }
            }
        }
        out.sort_unstable();
        out
    }
}

/// An angular gap between consecutive feet at a node: an outgoing edge.
#[derive(Debug, Clone, Copy)]
struct Gap {
    right: usize,
    left: usize,
    ur: f64,
    ul: f64,
    /// Start angle and width of the gap, counter-clockwise.
    a0: f64,
    width: f64,
    done: bool,
}

#[derive(Debug, Clone, Copy)]
enum Event {
    ExitR,
    At(Point2, f64),
    Node(usize),
}

struct Tracer {
    sites: Vec<Site>,
    loops: usize,
    grid: Grid,
    stamp: Vec<u32>,
    epoch: u32,
    /// Third sites already tried on the current trace: `tried[i] ==
    /// traces`.
    tried: Vec<usize>,
    traces: usize,
    tol: f64,
    nodes: Vec<Node>,
    gaps: Vec<Vec<Gap>>,
    edges: Vec<Edge>,
    queue: VecDeque<(usize, usize)>,
    at: BTreeMap<(i64, i64), Vec<usize>>,
    /// Nodes in the middle of an edge (seeds): node and its two sites.
    seeds: Vec<(usize, usize, usize)>,
    lo: Point2,
    hi: Point2,
}

impl Tracer {
    fn new(sites: Vec<Site>, loops: usize, lo: Point2, hi: Point2, tol: f64) -> Tracer {
        let grid = Grid::new(&sites, lo, hi);
        let n = sites.len();
        Tracer {
            sites,
            loops,
            grid,
            stamp: vec![0; n],
            epoch: 0,
            tried: vec![0; n],
            traces: 0,
            tol,
            nodes: Vec::new(),
            gaps: Vec::new(),
            edges: Vec::new(),
            queue: VecDeque::new(),
            at: BTreeMap::new(),
            seeds: Vec::new(),
            lo,
            hi,
        }
    }

    fn query(&mut self, a: Point2, b: Point2) -> Vec<usize> {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.epoch = 1;
        }
        self.grid.query(a, b, &mut self.stamp, self.epoch)
    }

    fn run(&mut self) -> Result<(), MedialError> {
        let limit = 64 * self.sites.len() + 1024;
        let mut steps = 0usize;
        let mut covered = vec![false; self.loops];
        while let Some(li) = (0..self.loops).find(|&li| {
            !covered[li]
                && self
                    .sites
                    .iter()
                    .any(|s| s.loop_ix == li && s.geom.len() > 0.0)
        }) {
            let seed_site = (0..self.sites.len())
                .filter(|&i| self.sites[i].loop_ix == li)
                .max_by(|&i, &j| {
                    arc_len(&self.sites[i].geom)
                        .total_cmp(&arc_len(&self.sites[j].geom))
                        .then(j.cmp(&i))
                });
            let Some(ss) = seed_site else {
                covered[li] = true;
                continue;
            };
            let found = [0.5, 0.3, 0.7, 0.1, 0.9, 0.4, 0.6]
                .iter()
                .find_map(|&f| self.seed(ss, f));
            match found {
                None => covered[li] = true,
                Some((p, rho)) => {
                    let v = match self.lookup(p) {
                        Some(v) => v,
                        None => self.make_node(p, rho),
                    };
                    let n = &self.nodes[v];
                    if self.gaps[v].len() == 2 && n.sites.len() == 2 {
                        self.seeds.push((v, n.sites[0], n.sites[1]));
                    }
                    for k in 0..self.gaps[v].len() {
                        if !self.gaps[v][k].done {
                            self.queue.push_back((v, k));
                        }
                    }
                    while let Some((v, k)) = self.queue.pop_front() {
                        steps += 1;
                        if steps > limit {
                            return Err(MedialError::Runaway);
                        }
                        self.trace(v, k);
                    }
                    for n in self.nodes.iter().filter(|n| n.radius > self.tol) {
                        for &s in &n.sites {
                            covered[self.sites[s].loop_ix] = true;
                        }
                    }
                    for e in &self.edges {
                        covered[self.sites[e.left].loop_ix] = true;
                        covered[self.sites[e.right].loop_ix] = true;
                    }
                    covered[li] = true;
                }
            }
        }
        Ok(())
    }

    /// The largest empty disc touching site `s` at the fraction `frac` of
    /// its length: a point of the axis. `None` when the disc found is not
    /// empty (the foot is where loops touch).
    fn seed(&self, s: usize, frac: f64) -> Option<(Point2, f64)> {
        let g = self.sites[s].geom;
        let u = frac * g.len();
        let tol = self.tol;
        let mut best = match g {
            Geom::Circle { r, .. } if g.is_concave() => r,
            _ => f64::INFINITY,
        };
        for (i, o) in self.sites.iter().enumerate() {
            if i == s {
                continue;
            }
            if let Some((p, rho)) = g.bisector_at(&o.geom, u, tol)
                && rho > tol
                && rho < best
                && o.geom
                    .cone_dist(p, tol)
                    .is_some_and(|d| (d - rho).abs() <= tol)
            {
                best = rho;
            }
        }
        if !best.is_finite() || best <= tol {
            return None;
        }
        let p = g.foot(u) + g.normal(u) * best;
        let clear = self
            .sites
            .iter()
            .filter_map(|o| o.geom.cone_dist(p, tol))
            .fold(f64::INFINITY, f64::min);
        ((clear - best).abs() <= tol).then_some((p, best))
    }

    fn key(&self, p: Point2) -> (i64, i64) {
        let cell = 4.0 * self.tol;
        ((p.x() / cell).floor() as i64, (p.y() / cell).floor() as i64)
    }

    /// An existing node within the merge distance of `p`.
    fn lookup(&self, p: Point2) -> Option<usize> {
        let (kx, ky) = self.key(p);
        let mut best: Option<(f64, usize)> = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                if let Some(v) = self.at.get(&(kx + dx, ky + dy)) {
                    for &i in v {
                        let d = self.nodes[i].p.distance(p);
                        if d <= 4.0 * self.tol && best.is_none_or(|b| d < b.0) {
                            best = Some((d, i));
                        }
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// A new node at `p`: its clearance, touching sites and gaps.
    fn make_node(&mut self, p: Point2, rho_hint: f64) -> usize {
        let tol = self.tol;
        let reach = rho_hint.max(0.0) + 4.0 * tol;
        let near = self.query(
            Point2::new(p.x() - reach, p.y() - reach),
            Point2::new(p.x() + reach, p.y() + reach),
        );
        let ds: Vec<(usize, f64)> = near
            .iter()
            .filter_map(|&i| self.sites[i].geom.cone_dist(p, tol).map(|d| (i, d)))
            .collect();
        let rho = ds
            .iter()
            .map(|d| d.1)
            .fold(f64::INFINITY, f64::min)
            .max(0.0);
        let rho = if rho.is_finite() { rho } else { rho_hint };
        let touching: Vec<usize> = ds
            .iter()
            .filter(|d| (d.1 - rho).abs() <= tol)
            .map(|d| d.0)
            .collect();
        let id = self.nodes.len();
        let mut gaps = Vec::new();
        if rho > tol {
            // (start angle, end angle, site, u at start, u at end)
            let mut feet: Vec<(f64, f64, usize, f64, f64)> = touching
                .iter()
                .map(|&i| {
                    let g = self.sites[i].geom;
                    match g {
                        Geom::Circle {
                            c, start, sweep, ..
                        } if g.is_concave() && p.distance(c) <= tol => {
                            let a = start.rem_euclid(TAU);
                            (a, a + sweep, i, 0.0, g.len())
                        }
                        _ => {
                            let u = g.param(p).clamp(0.0, g.len());
                            let f = g.foot(u);
                            let a = libm::atan2(f.y() - p.y(), f.x() - p.x()).rem_euclid(TAU);
                            (a, a, i, u, u)
                        }
                    }
                })
                .collect();
            feet.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));
            // Gaps of the union of the foot intervals, going round twice
            // so that an interval across angle 0 covers the feet it holds.
            let m = feet.len();
            if m > 0 {
                let (mut end, mut right, mut ur) = (feet[0].1, feet[0].2, feet[0].4);
                for k in 1..2 * m {
                    let f = feet[k % m];
                    let off = if k >= m { TAU } else { 0.0 };
                    let (a0, a1) = (f.0 + off, f.1 + off);
                    let width = a0 - end;
                    if k >= m && width * rho > 4.0 * tol && (f.2 != right || m == 1) {
                        gaps.push(Gap {
                            right,
                            left: f.2,
                            ur,
                            ul: f.3,
                            a0: end.rem_euclid(TAU),
                            width,
                            done: false,
                        });
                    }
                    if a1 >= end {
                        (end, right, ur) = (a1, f.2, f.4);
                    }
                }
            }
        }
        let mut sorted = touching;
        sorted.sort_unstable();
        self.nodes.push(Node {
            p,
            radius: rho,
            sites: sorted,
        });
        self.gaps.push(gaps);
        if rho > tol {
            let k = self.key(p);
            self.at.entry(k).or_default().push(id);
        }
        id
    }

    /// Marks the gap of node `w` that the edge arriving from `back` (a
    /// point on that edge) came through.
    fn arrive(&mut self, w: usize, back: Point2) {
        let p = self.nodes[w].p;
        let a = libm::atan2(back.y() - p.y(), back.x() - p.x()).rem_euclid(TAU);
        let gaps = &mut self.gaps[w];
        let mut best: Option<(f64, usize)> = None;
        for (k, g) in gaps.iter().enumerate() {
            if g.done {
                continue;
            }
            let rel = (a - g.a0).rem_euclid(TAU);
            // Angular distance outside the gap (0 inside).
            let off = if rel <= g.width {
                0.0
            } else {
                (rel - g.width).min(TAU - rel)
            };
            if best.is_none_or(|b| off < b.0) {
                best = Some((off, k));
            }
        }
        if let Some((_, k)) = best {
            gaps[k].done = true;
        }
    }

    fn find_or_make(&mut self, p: Point2, rho: f64, back: Point2) -> usize {
        // Leaves are never shared: loops may touch at a corner.
        if rho > self.tol
            && let Some(w) = self.lookup(p)
        {
            self.arrive(w, back);
            return w;
        }
        let w = self.make_node(p, rho);
        self.arrive(w, back);
        for k in 0..self.gaps[w].len() {
            if !self.gaps[w][k].done {
                self.queue.push_back((w, k));
            }
        }
        w
    }

    /// Traces the edge leaving node `v` through gap `k`.
    fn trace(&mut self, v: usize, k: usize) {
        if self.gaps[v][k].done {
            return;
        }
        self.gaps[v][k].done = true;
        let tol = self.tol;
        let g = self.gaps[v][k];
        let (mut l, mut r, mut ul, mut ur) = (g.left, g.right, g.ul, g.ur);
        let p0 = self.nodes[v].p;
        // A foot already at the end of its site goes on to the next site.
        for _ in 0..8 {
            let rg = self.sites[r].geom;
            let lg = self.sites[l].geom;
            if ur >= rg.len() - rg.ptol(p0, tol) {
                match self.sites[r].next {
                    Some(n) => {
                        r = n;
                        ur = 0.0;
                        continue;
                    }
                    None => return,
                }
            }
            if ul <= lg.ptol(p0, tol) {
                match self.sites[l].prev {
                    Some(n) => {
                        l = n;
                        ul = self.sites[n].geom.len();
                        continue;
                    }
                    None => return,
                }
            }
            break;
        }
        if l == r {
            return;
        }
        let rg = self.sites[r].geom;
        let lg = self.sites[l].geom;
        let len_r = rg.len();
        let mut ev_u = len_r;
        let mut ev = Event::ExitR;
        let ahead = |u: f64, q: Point2| u > ur && q.distance(p0) > 4.0 * tol;
        // The foot on L reaches the start of L.
        if let Some((q, rho)) = lg.bisector_at(&rg, 0.0, tol) {
            let u = rg.param(q);
            if ahead(u, q) && u < ev_u {
                ev_u = u;
                ev = Event::At(q, rho.max(0.0));
            }
        }
        // The edge passes through the centre of a concave L.
        if let Geom::Circle { c, r: rad, .. } = lg
            && lg.is_concave()
            && rg.cone_dist(c, tol).is_some_and(|d| (d - rad).abs() <= tol)
        {
            let u = rg.param(c);
            if ahead(u, c) && u < ev_u {
                ev_u = u;
                ev = Event::At(c, rad);
            }
        }
        // A seed in the middle of this edge.
        for &(sv, sa, sb) in &self.seeds {
            if sv != v && ((sa == l && sb == r) || (sa == r && sb == l)) {
                let q = self.nodes[sv].p;
                let u = rg.param(q);
                if ahead(u, q) && u <= ev_u {
                    ev_u = u;
                    ev = Event::Node(sv);
                }
            }
        }
        // Third sites, searched interval by interval along the edge so
        // that the boxes stay small; the search stops at the first
        // interval that holds an event.
        // The sites linked to L and R meet them where a foot leaves its
        // site: those are the exit events above, not branch vertices.
        let linked = [
            self.sites[l].prev,
            self.sites[l].next,
            self.sites[r].prev,
            self.sites[r].next,
        ];
        let us: Vec<f64> = (0..=SPANS)
            .map(|i| ur + (ev_u - ur) * (i as f64 / SPANS as f64))
            .collect();
        let discs: Vec<Option<(Point2, f64)>> =
            us.iter().map(|&u| rg.bisector_at(&lg, u, tol)).collect();
        self.traces += 1;
        let mark = self.traces;
        for i in 0..SPANS {
            if ev_u <= us[i] {
                break;
            }
            // Sites that may come as near within this interval.
            let (blo, bhi, near): (Point2, Point2, Vec<(Point2, f64)>) =
                match (discs[i], discs[i + 1]) {
                    (Some(a), Some(b)) => {
                        let grow = 2.0 * (a.0.distance(b.0) + (a.1 - b.1).abs()) + 8.0 * tol;
                        let (ra, rb) = (a.1.max(0.0) + grow, b.1.max(0.0) + grow);
                        (
                            Point2::new(
                                (a.0.x() - ra).min(b.0.x() - rb),
                                (a.0.y() - ra).min(b.0.y() - rb),
                            ),
                            Point2::new(
                                (a.0.x() + ra).max(b.0.x() + rb),
                                (a.0.y() + ra).max(b.0.y() + rb),
                            ),
                            vec![(a.0, ra), (b.0, rb)],
                        )
                    }
                    _ => (self.lo, self.hi, Vec::new()),
                };
            let cands = self.query(blo, bhi);
            for c in cands {
                if c == l || c == r || linked.contains(&Some(c)) || self.tried[c] == mark {
                    continue;
                }
                let cg = self.sites[c].geom;
                // A site clear of both discs (grown to cover the discs
                // between them) cannot become as near here.
                if !near.is_empty() && !near.iter().any(|&(q, reach)| cg.distance_lb(q) <= reach) {
                    continue;
                }
                self.tried[c] = mark;
                // Raw solutions behind the start or past the current
                // end are not worth polishing.
                let slack = 1e-6 * (1.0 + len_r);
                let (lo_u, hi_u) = (ur - slack, ev_u + slack);
                let mut pts = equidistant_where([&lg, &rg, &cg], p0, tol, |q, rho| {
                    let u = rg.param(q);
                    rho > -1e-6 * (1.0 + rho.abs()) - tol && u > lo_u && u < hi_u
                });
                if let Geom::Circle { c: cc, r: rad, .. } = cg
                    && cg.is_concave()
                {
                    pts.push((cc, rad));
                }
                for (q, rho) in pts {
                    if rho < -tol {
                        continue;
                    }
                    let ok = |g: &Geom| g.cone_dist(q, tol).is_some_and(|d| (d - rho).abs() <= tol);
                    if !(ok(&lg) && ok(&rg) && ok(&cg)) {
                        continue;
                    }
                    let u = rg.param(q);
                    if ahead(u, q) && u < ev_u {
                        ev_u = u;
                        ev = Event::At(q, rho.max(0.0));
                    }
                }
            }
        }
        let (pe, rhoe, node) = match ev {
            Event::Node(n) => (self.nodes[n].p, self.nodes[n].radius, Some(n)),
            Event::At(q, rho) => (q, rho, None),
            Event::ExitR => match rg.bisector_at(&lg, len_r, tol) {
                Some((q, rho)) => (q, rho.max(0.0), None),
                None => {
                    // The bisector ends before R does; take its last
                    // valid point.
                    let (mut a, mut b) = (ur, len_r);
                    for _ in 0..60 {
                        let m = 0.5 * (a + b);
                        if rg.bisector_at(&lg, m, tol).is_some() {
                            a = m;
                        } else {
                            b = m;
                        }
                    }
                    ev_u = a;
                    match rg.bisector_at(&lg, a, tol) {
                        Some((q, rho)) => (q, rho.max(0.0), None),
                        None => return,
                    }
                }
            },
        };
        // A leaf is a corner of the boundary: take it exactly.
        let (pe, rhoe) = if rhoe <= tol && node.is_none() {
            (rg.foot(ev_u.clamp(0.0, len_r)), 0.0)
        } else {
            (pe, rhoe)
        };
        if pe.distance(p0) <= 4.0 * tol {
            return;
        }
        let back = rg
            .bisector_at(&lg, 0.5 * (ur + ev_u), tol)
            .map_or(p0, |b| b.0);
        let w = match node {
            Some(n) => {
                self.arrive(n, back);
                n
            }
            None => self.find_or_make(pe, rhoe, back),
        };
        self.edges.push(Edge {
            left: l,
            right: r,
            u0: ur,
            u1: ev_u,
            a: v,
            b: w,
        });
    }
}

/// Length of a site along the boundary (0 for points).
fn arc_len(g: &Geom) -> f64 {
    match *g {
        Geom::Line { len, .. } => len,
        Geom::Circle { r, sweep, .. } => r * sweep.abs(),
    }
}

#[cfg(test)]
mod tests;
