// SPDX-License-Identifier: MIT OR Apache-2.0

//! 2-D regions bounded by lines and arcs (design D08 §3).
//!
//! A [`Region2`] is a set of closed loops: outer loops counter-clockwise,
//! holes clockwise. Each loop is a list of vertices in the bulge form of
//! `cavalier_contours`: the segment from vertex `i` to vertex `i + 1` is a
//! line when the bulge is 0, else an arc with `bulge = tan(sweep / 4)`,
//! positive counter-clockwise.

use cavalier_contours::polyline::{PlineCreation, PlineSource, PlineSourceMut, Polyline};
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};
use fluut_geom::{Point2, Vec2};
use std::f64::consts::{PI, TAU};

/// Points closer than this to the boundary are [`Inside::On`] it, and
/// vertices closer than this are merged.
pub const EPS: f64 = 1e-9;

/// The `cavalier_contours` operation that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionOp {
    Offset,
    Boolean,
}

/// A `cavalier_contours` call that panicked, and still did after the
/// retries of [`Region2::try_offset`] and the booleans. The infallible
/// methods raise it as a panic payload, and `toolpath::calculate` turns it
/// into an error on the toolpath, so a calculation fails instead of the
/// application (spec 06 §6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionFailure {
    pub op: RegionOp,
    /// The panic message.
    pub message: String,
}

impl RegionFailure {
    /// The diagnostic code.
    pub fn code(&self) -> &'static str {
        match self.op {
            RegionOp::Offset => "cam.region.offset_failed",
            RegionOp::Boolean => "cam.region.boolean_failed",
        }
    }

    /// Raises the failure as a panic payload. `resume_unwind` does not call
    /// the panic hook, so no crash report is written for it.
    pub fn raise(self) -> ! {
        std::panic::resume_unwind(Box::new(self))
    }
}

impl std::fmt::Display for RegionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let op = match self.op {
            RegionOp::Offset => "offset",
            RegionOp::Boolean => "boolean operation",
        };
        write!(f, "a 2-D region {op} failed ({})", self.message)
    }
}

impl std::error::Error for RegionFailure {}

/// Runs `f`, a call into `cavalier_contours`, catching its panic.
pub(crate) fn guard<R>(op: RegionOp, f: impl FnOnce() -> R) -> Result<R, RegionFailure> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        #[cfg(test)]
        fault::check();
        f()
    }))
    .map_err(|e| {
        let message = if let Some(s) = e.downcast_ref::<&str>() {
            (*s).to_owned()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else if let Some(r) = e.downcast_ref::<RegionFailure>() {
            r.message.clone()
        } else {
            "unknown panic".to_owned()
        };
        RegionFailure { op, message }
    })
}

/// A forced failure of every [`guard`]ed call on this thread, for tests.
#[cfg(test)]
pub(crate) mod fault {
    use std::cell::Cell;

    thread_local! {
        static FAIL: Cell<bool> = const { Cell::new(false) };
    }

    pub(crate) fn set(on: bool) {
        FAIL.with(|f| f.set(on));
    }

    pub(super) fn check() {
        if FAIL.with(Cell::get) {
            panic!("forced region failure");
        }
    }
}

/// One vertex of a loop and the bulge of the segment that starts there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub p: Point2,
    pub bulge: f64,
}

impl Vertex {
    pub const fn new(x: f64, y: f64, bulge: f64) -> Self {
        Vertex {
            p: Point2::new(x, y),
            bulge,
        }
    }
}

/// A line (bulge 0) or arc segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seg2 {
    pub a: Point2,
    pub b: Point2,
    pub bulge: f64,
}

/// Centre, radius, start angle and signed sweep of an arc segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcGeom {
    pub centre: Point2,
    pub radius: f64,
    pub start: f64,
    pub sweep: f64,
}

impl Seg2 {
    pub fn is_line(&self) -> bool {
        self.bulge == 0.0
    }

    /// Arc geometry, or `None` for a line.
    pub fn arc(&self) -> Option<ArcGeom> {
        if self.is_line() {
            return None;
        }
        let b = self.bulge;
        let d = self.b - self.a;
        let half = d * 0.5;
        let m = self.a + half;
        // Left normal of the chord, scaled so that the centre lies at
        // m + n · (1 − b²) / (2b).
        let n = Vec2::new(-half.y(), half.x());
        let centre = m + n * ((1.0 - b * b) / (2.0 * b));
        let radius = d.length() * (1.0 + b * b) / (4.0 * b.abs());
        let start = libm::atan2(self.a.y() - centre.y(), self.a.x() - centre.x());
        Some(ArcGeom {
            centre,
            radius,
            start,
            sweep: 4.0 * libm::atan(b),
        })
    }

    /// Length along the segment.
    pub fn length(&self) -> f64 {
        match self.arc() {
            None => self.a.distance(self.b),
            Some(g) => g.radius * g.sweep.abs(),
        }
    }

    /// Point at parameter `t` in [0, 1] (proportional to length).
    pub fn point_at(&self, t: f64) -> Point2 {
        match self.arc() {
            None => self.a + (self.b - self.a) * t,
            Some(g) => g.point(g.start + g.sweep * t),
        }
    }

    /// The signed area between the segment and the origin, so that the sum
    /// over a closed loop is the loop's signed area.
    fn signed_area(&self) -> f64 {
        let chord = 0.5 * (self.a.x() * self.b.y() - self.b.x() * self.a.y());
        match self.arc() {
            None => chord,
            Some(g) => {
                let s = g.sweep;
                chord + 0.5 * g.radius * g.radius * (s - libm::sin(s))
            }
        }
    }

    pub(crate) fn support_x(&self) -> (f64, f64) {
        self.support(Vec2::new(1.0, 0.0))
    }

    pub(crate) fn support_y(&self) -> (f64, f64) {
        self.support(Vec2::new(0.0, 1.0))
    }

    /// Minimum and maximum of `p · dir` over the segment.
    fn support(&self, dir: Vec2) -> (f64, f64) {
        let pa = self.a.0.dot(dir.0);
        let pb = self.b.0.dot(dir.0);
        let (mut lo, mut hi) = (pa.min(pb), pa.max(pb));
        if let Some(g) = self.arc() {
            let len = dir.length();
            let phi = libm::atan2(dir.y(), dir.x());
            let c = g.centre.0.dot(dir.0);
            if g.contains_angle(phi) {
                hi = hi.max(c + g.radius * len);
            }
            if g.contains_angle(phi + PI) {
                lo = lo.min(c - g.radius * len);
            }
        }
        (lo, hi)
    }
}

impl ArcGeom {
    pub fn point(&self, angle: f64) -> Point2 {
        let (s, c) = libm::sincos(angle);
        Point2::new(
            self.centre.x() + self.radius * c,
            self.centre.y() + self.radius * s,
        )
    }

    /// Whether the direction `angle` from the centre lies on the arc.
    pub fn contains_angle(&self, angle: f64) -> bool {
        let d = (angle - self.start).rem_euclid(TAU);
        if self.sweep >= 0.0 {
            d <= self.sweep
        } else {
            d == 0.0 || d >= TAU + self.sweep
        }
    }
}

/// A closed loop of segments.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Loop2 {
    pub verts: Vec<Vertex>,
}

impl Loop2 {
    pub fn segs(&self) -> impl Iterator<Item = Seg2> + Clone + '_ {
        let n = self.verts.len();
        (0..n).map(move |i| Seg2 {
            a: self.verts[i].p,
            b: self.verts[(i + 1) % n].p,
            bulge: self.verts[i].bulge,
        })
    }

    /// Signed area: positive for counter-clockwise loops.
    pub fn area(&self) -> f64 {
        self.segs().map(|s| s.signed_area()).sum()
    }

    pub fn is_ccw(&self) -> bool {
        self.area() > 0.0
    }

    /// The same loop traversed the other way.
    pub fn reversed(&self) -> Loop2 {
        let n = self.verts.len();
        let verts = (0..n)
            .map(|i| {
                // Vertex i of the result starts the segment that was
                // segment (n − 1 − i) of the original, reversed.
                let j = (2 * n - 1 - i) % n;
                Vertex {
                    p: self.verts[(j + 1) % n].p,
                    bulge: -self.verts[j].bulge,
                }
            })
            .collect();
        Loop2 { verts }
    }

    pub fn length(&self) -> f64 {
        self.segs().map(|s| s.length()).sum()
    }

    pub(crate) fn to_pline(&self) -> Polyline<f64> {
        let mut pl = Polyline::with_capacity(self.verts.len(), true);
        for v in &self.verts {
            pl.add(v.p.x(), v.p.y(), v.bulge);
        }
        pl
    }

    pub(crate) fn from_pline<P: PlineSource<Num = f64>>(pl: &P) -> Loop2 {
        let verts = pl
            .iter_vertexes()
            .map(|v| Vertex::new(v.x, v.y, v.bulge))
            .collect();
        Loop2 { verts }
    }
}

/// Where a point lies relative to a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inside {
    In,
    On,
    Out,
}

/// A 2-D region: outer loops counter-clockwise, holes clockwise.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Region2 {
    pub loops: Vec<Loop2>,
}

impl Region2 {
    pub fn empty() -> Self {
        Region2::default()
    }

    pub fn is_empty(&self) -> bool {
        self.loops.is_empty()
    }

    /// The axis-aligned rectangle with corners `(x0, y0)` and `(x1, y1)`.
    pub fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Region2::polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
    }

    /// A polygon; the points may be in either order.
    pub fn polygon(pts: &[(f64, f64)]) -> Self {
        let l = Loop2 {
            verts: pts.iter().map(|&(x, y)| Vertex::new(x, y, 0.0)).collect(),
        };
        Region2::from_outer(l)
    }

    /// A disc, as two half-circle arcs.
    pub fn circle(cx: f64, cy: f64, r: f64) -> Self {
        Region2::from_outer(Loop2 {
            verts: vec![Vertex::new(cx + r, cy, 1.0), Vertex::new(cx - r, cy, 1.0)],
        })
    }

    fn from_outer(l: Loop2) -> Self {
        let l = if l.is_ccw() { l } else { l.reversed() };
        Region2 { loops: vec![l] }
    }

    /// This region with the outer loops of `hole` added as holes. The hole
    /// must lie inside this region and not touch any loop.
    pub fn with_hole(mut self, hole: &Region2) -> Self {
        for l in &hole.loops {
            let l = if l.is_ccw() { l.reversed() } else { l.clone() };
            self.loops.push(l);
        }
        self
    }

    pub fn area(&self) -> f64 {
        self.loops.iter().map(Loop2::area).sum()
    }

    /// Total length of all loops.
    pub fn perimeter(&self) -> f64 {
        self.loops.iter().map(Loop2::length).sum()
    }

    pub fn segs(&self) -> impl Iterator<Item = Seg2> + Clone + '_ {
        self.loops.iter().flat_map(Loop2::segs)
    }

    /// Distance from `p` to the nearest boundary point.
    pub fn boundary_distance(&self, p: Point2) -> f64 {
        self.segs()
            .map(|s| seg_distance(&s, p))
            .fold(f64::INFINITY, f64::min)
    }

    pub fn contains(&self, p: Point2) -> Inside {
        contains_in(self.segs(), p)
    }

    /// Exact minimum and maximum of `p · dir` over the region.
    pub fn support(&self, dir: Vec2) -> (f64, f64) {
        self.segs()
            .map(|s| s.support(dir))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (c, d)| {
                (a.min(c), b.max(d))
            })
    }

    /// The region grown by `d` (shrunk when `d` is negative). Corners that
    /// grow become arcs of radius `d`; loops that shrink to nothing vanish,
    /// and loops that meet are joined.
    ///
    /// A failure of `cavalier_contours` is raised as a [`RegionFailure`]
    /// panic payload, which `toolpath::calculate` reports on the toolpath.
    pub fn offset(&self, d: f64) -> Region2 {
        self.try_offset(d).unwrap_or_else(|e| e.raise())
    }

    /// As [`Region2::offset`], returning a failure of `cavalier_contours`.
    pub fn try_offset(&self, d: f64) -> Result<Region2, RegionFailure> {
        if self.is_empty() {
            return Ok(Region2::empty());
        }
        if d == 0.0 {
            return Ok(self.clone());
        }
        // `cavalier_contours` can fail an internal assertion on degenerate
        // input (a collapsed slice). The offset is then retried a hair
        // deeper or shallower.
        let mut r = guard(RegionOp::Offset, || self.offset_raw(d));
        for k in [1e-7, -1e-7, 1e-6] {
            if r.is_ok() {
                break;
            }
            r = guard(RegionOp::Offset, || self.offset_raw(d + k * d.signum()));
        }
        r
    }

    fn offset_raw(&self, d: f64) -> Region2 {
        let shape = Shape::from_plines(self.loops.iter().map(Loop2::to_pline));
        // cavalier_contours offsets to the left of the loop direction, which
        // is inward for counter-clockwise loops.
        let out = shape.parallel_offset(-d, &ShapeOffsetOptions::new());
        let loops = out
            .ccw_plines
            .iter()
            .chain(&out.cw_plines)
            .map(|ip| Loop2::from_pline(&ip.polyline))
            .filter(|l| l.verts.len() >= 2 && l.area().abs() > EPS)
            .collect();
        Region2 { loops }
    }

    pub fn union(&self, o: &Region2) -> Region2 {
        self.try_union(o).unwrap_or_else(|e| e.raise())
    }

    pub fn intersect(&self, o: &Region2) -> Region2 {
        self.try_intersect(o).unwrap_or_else(|e| e.raise())
    }

    /// This region minus `o`.
    pub fn subtract(&self, o: &Region2) -> Region2 {
        self.try_subtract(o).unwrap_or_else(|e| e.raise())
    }

    /// As [`Region2::union`], returning a failure of `cavalier_contours`.
    pub fn try_union(&self, o: &Region2) -> Result<Region2, RegionFailure> {
        self.guarded(o, boolean::Op::Union)
    }

    /// As [`Region2::intersect`], returning a failure of `cavalier_contours`.
    pub fn try_intersect(&self, o: &Region2) -> Result<Region2, RegionFailure> {
        self.guarded(o, boolean::Op::Intersect)
    }

    /// As [`Region2::subtract`], returning a failure of `cavalier_contours`.
    pub fn try_subtract(&self, o: &Region2) -> Result<Region2, RegionFailure> {
        self.guarded(o, boolean::Op::Subtract)
    }

    /// A boolean that survives `cavalier_contours` assertions on nearly
    /// degenerate input (collinear segments within rounding): it is
    /// retried with `o` moved by a nanometre, which is far below any
    /// tolerance.
    fn guarded(&self, o: &Region2, op: boolean::Op) -> Result<Region2, RegionFailure> {
        let run =
            |a: &Region2, b: &Region2| guard(RegionOp::Boolean, || boolean::boolean(a, b, op));
        let mut last = match run(self, o) {
            Ok(r) => return Ok(r),
            Err(e) => e,
        };
        for (dx, dy) in [(1e-9, 0.7e-9), (-0.7e-9, 1e-9), (0.3e-9, -1e-9)] {
            match run(self, &o.translated(Vec2::new(dx, dy))) {
                Ok(r) => return Ok(r),
                Err(e) => last = e,
            }
        }
        // Then with almost-collinear vertices removed from both.
        let (a, b) = (self.cleaned(), o.cleaned());
        for (dx, dy) in [(0.0, 0.0), (1e-9, 0.7e-9)] {
            match run(&a, &b.translated(Vec2::new(dx, dy))) {
                Ok(r) => return Ok(r),
                Err(e) => last = e,
            }
        }
        if let Ok(path) = std::env::var("FLUUT_DUMP") {
            let _ = std::fs::write(
                path,
                format!(
                    "{:?}
{:?}
",
                    self, o
                ),
            );
        }
        Err(last)
    }

    /// The region with arcs flatter than 1e-7 mm turned into lines and
    /// line vertices within 1e-7 mm of the chord of their neighbours
    /// removed (a shape change below any tolerance).
    pub fn cleaned(&self) -> Region2 {
        const FLAT: f64 = 1e-7;
        let loops = self
            .loops
            .iter()
            .map(|l| {
                let n = l.verts.len();
                let mut v: Vec<Vertex> = l
                    .verts
                    .iter()
                    .enumerate()
                    .map(|(i, x)| {
                        let next = l.verts[(i + 1) % n].p;
                        // Sagitta of an arc ≈ chord · bulge / 2.
                        if (x.p.distance(next) * x.bulge).abs() * 0.5 <= FLAT {
                            Vertex { bulge: 0.0, ..*x }
                        } else {
                            *x
                        }
                    })
                    .collect();
                let mut changed = true;
                while changed && v.len() > 3 {
                    changed = false;
                    let mut i = 0;
                    while i < v.len() && v.len() > 3 {
                        let m = v.len();
                        let (pr, cu, nx) = (v[(i + m - 1) % m], v[i], v[(i + 1) % m]);
                        let d = nx.p - pr.p;
                        let len = d.length();
                        let off = if len > 0.0 {
                            ((cu.p - pr.p).x() * d.y() - (cu.p - pr.p).y() * d.x()).abs() / len
                        } else {
                            cu.p.distance(pr.p)
                        };
                        let between = (cu.p - pr.p).dot(d) >= 0.0 && (nx.p - cu.p).dot(d) >= 0.0;
                        if pr.bulge == 0.0 && cu.bulge == 0.0 && off <= FLAT && between {
                            v.remove(i);
                            changed = true;
                        } else {
                            i += 1;
                        }
                    }
                }
                Loop2 { verts: v }
            })
            .collect();
        Region2 { loops }
    }

    /// The region moved by `d`.
    pub fn translated(&self, d: Vec2) -> Region2 {
        Region2 {
            loops: self
                .loops
                .iter()
                .map(|l| Loop2 {
                    verts: l
                        .verts
                        .iter()
                        .map(|v| Vertex {
                            p: v.p + d,
                            bulge: v.bulge,
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Parameter intervals `(t0, t1)` where the line `p + t · dir` lies in
    /// the closed region, sorted and merged. Ends come from exact line–line
    /// and line–circle intersections; single touching points are dropped.
    pub fn clip_line(&self, p: Point2, dir: Vec2) -> Vec<(f64, f64)> {
        let mut ts: Vec<f64> = self.segs().flat_map(|s| line_hits(&s, p, dir)).collect();
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|x, y| (*x - *y).abs() * dir.length() <= EPS);
        let mut out: Vec<(f64, f64)> = Vec::new();
        for w in ts.windows(2) {
            let (t0, t1) = (w[0], w[1]);
            if self.contains(p + dir * (0.5 * (t0 + t1))) == Inside::Out {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.1 == t0 => last.1 = t1,
                _ => out.push((t0, t1)),
            }
        }
        out
    }

    /// `(min, max)` corners of the bounding box.
    pub fn bbox(&self) -> (Point2, Point2) {
        let (x0, x1) = self.support(Vec2::new(1.0, 0.0));
        let (y0, y1) = self.support(Vec2::new(0.0, 1.0));
        (Point2::new(x0, y0), Point2::new(x1, y1))
    }
}

/// [`Region2::contains`] over the given boundary segments. Segments that
/// neither come within [`EPS`] of `p` nor meet the ray from `p` in the +x
/// direction may be left out without changing the answer.
pub(crate) fn contains_in<I>(segs: I, p: Point2) -> Inside
where
    I: Iterator<Item = Seg2> + Clone,
{
    for s in segs.clone() {
        let near = match s.arc() {
            None => true,
            Some(g) => (p.distance(g.centre) - g.radius).abs() <= EPS,
        };
        if near && seg_distance(&s, p) <= EPS {
            return Inside::On;
        }
    }
    // The point is more than EPS from the boundary, so a nudge far
    // below EPS cannot change the answer; it resolves points that lie
    // on a chord or on the ray through a vertex.
    let w = winding(segs.clone(), p)
        .unwrap_or_else(|| winding(segs, p + Vec2::new(EPS * 0.031, EPS * 0.017)).unwrap_or(0));
    if w > 0 { Inside::In } else { Inside::Out }
}

/// Winding number of the polygon of chords, plus ±1 for each arc whose
/// circular segment (between chord and arc) holds the point; `None`
/// when the point lies on a chord's line within an arc's circle.
fn winding(segs: impl Iterator<Item = Seg2>, p: Point2) -> Option<i32> {
    let mut w = 0i32;
    for s in segs {
        let (a, b) = (s.a, s.b);
        let cross = (b - a).perp_dot(p - a);
        if a.y() <= p.y() {
            if b.y() > p.y() && cross > 0.0 {
                w += 1;
            }
        } else if b.y() <= p.y() && cross < 0.0 {
            w -= 1;
        }
        if let Some(g) = s.arc()
            && p.distance(g.centre) < g.radius
        {
            if cross == 0.0 {
                return None;
            }
            if g.sweep > 0.0 && cross < 0.0 {
                w += 1;
            } else if g.sweep < 0.0 && cross > 0.0 {
                w -= 1;
            }
        }
    }
    Some(w)
}

/// Parameters `t` where the line `p + t · dir` meets a segment; both ends
/// of a collinear overlap.
fn line_hits(s: &Seg2, p: Point2, dir: Vec2) -> Vec<f64> {
    let dd = dir.dot(dir);
    match s.arc() {
        None => {
            let e = s.b - s.a;
            let den = dir.perp_dot(e);
            let w = s.a - p;
            if den.abs() <= EPS * dir.length() * e.length() {
                // Parallel: hits only when collinear, at both ends.
                if w.perp_dot(dir).abs() <= EPS * dir.length() {
                    return vec![w.dot(dir) / dd, (s.b - p).dot(dir) / dd];
                }
                return vec![];
            }
            let t = w.perp_dot(e) / den;
            let u = w.perp_dot(dir) / den;
            if (-EPS..=1.0 + EPS).contains(&u) {
                vec![t]
            } else {
                vec![]
            }
        }
        Some(g) => {
            // |p + t·dir − c|² = r²
            let w = p - g.centre;
            let b = w.dot(dir);
            let c = w.dot(w) - g.radius * g.radius;
            let disc = b * b - dd * c;
            if disc < 0.0 {
                return vec![];
            }
            let r = disc.sqrt();
            [(-b - r) / dd, (-b + r) / dd]
                .into_iter()
                .filter(|&t| {
                    let q = p + dir * t;
                    g.contains_angle(libm::atan2(q.y() - g.centre.y(), q.x() - g.centre.x()))
                })
                .collect()
        }
    }
}

/// Distance from `p` to a segment.
pub fn seg_distance(s: &Seg2, p: Point2) -> f64 {
    match s.arc() {
        None => {
            let d = s.b - s.a;
            let l2 = d.dot(d);
            let t = if l2 > 0.0 {
                ((p - s.a).dot(d) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            p.distance(s.a + d * t)
        }
        Some(g) => {
            let ang = libm::atan2(p.y() - g.centre.y(), p.x() - g.centre.x());
            if g.contains_angle(ang) {
                (p.distance(g.centre) - g.radius).abs()
            } else {
                p.distance(s.a).min(p.distance(s.b))
            }
        }
    }
}

pub(crate) mod boolean;
pub mod medial;

#[cfg(test)]
mod tests;
