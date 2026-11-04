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

/// Offsets drop features narrower than this (mm): spikes, notches and
/// loops nanometres wide that `cavalier_contours` keeps from nearly
/// tangent input, and that chained offsets multiply (see
/// [`Loop2::tidied`] and `docs/plan/2026-11-03-region-micro-loops.md`).
/// Far below any machining tolerance, far above rounding.
pub const TIDY: f64 = 1e-5;

/// Segments up to this long (mm) are short for [`Loop2::tidied`]: a
/// vertex where such a line meets a line in the same direction is removed,
/// and such an arc flatter than [`TIDY_FLAT`] becomes a line. Longer
/// segments keep their vertices, so G-code points on straight edges stay.
pub const TIDY_SHORT: f64 = 1e-3;

/// The sagitta (mm) below which a short arc becomes a line.
pub const TIDY_FLAT: f64 = 1e-6;

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

thread_local! {
    /// How many [`catch_quiet`] calls are running on this thread.
    static QUIET: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// `catch_unwind` for a panic that is handled, not a crash: while `f` runs,
/// [`in_catch_quiet`] is true on this thread, and the application's crash
/// hook writes no report. The region operations run every
/// `cavalier_contours` call through it: a panic there is retried, or
/// becomes a [`RegionFailure`] diagnostic on the toolpath (spec 06 §6.3).
pub fn catch_quiet<R>(f: impl FnOnce() -> R) -> std::thread::Result<R> {
    QUIET.with(|q| q.set(q.get() + 1));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    QUIET.with(|q| q.set(q.get() - 1));
    r
}

/// Whether a [`catch_quiet`] call is running on this thread, so that a
/// panic now will be caught and handled.
pub fn in_catch_quiet() -> bool {
    QUIET.with(|q| q.get() > 0)
}

/// Runs `f`, a call into `cavalier_contours`, catching its panic.
pub(crate) fn guard<R>(op: RegionOp, f: impl FnOnce() -> R) -> Result<R, RegionFailure> {
    catch_quiet(|| {
        #[cfg(test)]
        fault::check();
        f()
    })
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

    /// A wrong offset result, for the containment guard.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Wrong {
        /// No loops.
        Empty,
        /// The input moved by 20 mm in X, whatever the distance.
        Moved,
    }

    thread_local! {
        /// The wrong result, and whether it is given once only.
        static WRONG: Cell<Option<(Wrong, bool)>> = const { Cell::new(None) };
    }

    /// Every offset on this thread gives `wrong` (none when `None`).
    pub(crate) fn set_wrong(wrong: Option<Wrong>) {
        WRONG.with(|w| w.set(wrong.map(|x| (x, false))));
    }

    /// The next offset on this thread gives `wrong`.
    pub(crate) fn set_wrong_once(wrong: Wrong) {
        WRONG.with(|w| w.set(Some((wrong, true))));
    }

    pub(super) fn wrong() -> Option<Wrong> {
        WRONG.with(|w| {
            let x = w.get();
            if let Some((_, true)) = x {
                w.set(None);
            }
            x.map(|(x, _)| x)
        })
    }

    pub(super) fn check() {
        if FAIL.with(Cell::get) {
            panic!("forced region failure");
        }
    }
}

/// How many offsets gave a first result that failed the containment check
/// of [`Region2::try_offset`] (a growth that does not hold its input, a
/// shrink that does not lie inside it) and were retried, in this process.
/// For tests: no offset should need it.
#[doc(hidden)]
pub fn containment_retries() -> usize {
    CONTAINMENT_RETRIES.load(std::sync::atomic::Ordering::Relaxed)
}

static CONTAINMENT_RETRIES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

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

    /// The loop without features narrower than [`TIDY`], or `None` when
    /// the whole loop is that narrow (twice its area under `TIDY` times its
    /// length). A vertex within `TIDY` of the next one is merged into it; a
    /// vertex between two lines where the loop turns back on itself (a
    /// spike) is removed when it lies within `TIDY` of the line through its
    /// neighbours; so is one between lines in the same direction (a shallow
    /// notch or a kink) when one of them is at most [`TIDY_SHORT`] long; and
    /// arcs at most `TIDY_SHORT` long whose sagitta is at most
    /// [`TIDY_FLAT`] become lines. Each change moves the boundary by at most
    /// `TIDY`.
    pub fn tidied(&self) -> Option<Loop2> {
        let n = self.verts.len();
        let mut v: Vec<Vertex> = (0..n)
            .map(|i| {
                let x = self.verts[i];
                let chord = x.p.distance(self.verts[(i + 1) % n].p);
                // The sagitta of an arc is chord · |bulge| / 2.
                if x.bulge != 0.0 && chord <= TIDY_SHORT && chord * x.bulge.abs() * 0.5 <= TIDY_FLAT
                {
                    Vertex { bulge: 0.0, ..x }
                } else {
                    x
                }
            })
            .collect();
        let mut changed = true;
        while changed && v.len() > 2 {
            changed = false;
            let mut i = 0;
            while i < v.len() && v.len() > 2 {
                let m = v.len();
                let (pr, cu, nx) = (v[(i + m - 1) % m], v[i], v[(i + 1) % m]);
                let remove = if cu.p.distance(nx.p) <= TIDY {
                    true
                } else if pr.bulge == 0.0 && cu.bulge == 0.0 {
                    let d = nx.p - pr.p;
                    let len = d.length();
                    let forward =
                        len > TIDY && (cu.p - pr.p).dot(d) >= 0.0 && (nx.p - cu.p).dot(d) >= 0.0;
                    let short = cu.p.distance(pr.p).min(cu.p.distance(nx.p)) <= TIDY_SHORT;
                    // Neighbours on top of each other: `cu` is the tip of a
                    // spike, whatever its length.
                    (!forward || short)
                        && (len <= TIDY || (cu.p - pr.p).perp_dot(d).abs() <= TIDY * len)
                } else {
                    false
                };
                if remove {
                    v.remove(i);
                    changed = true;
                } else {
                    i += 1;
                }
            }
        }
        let l = Loop2 { verts: v };
        if l.verts.len() < 2 || 2.0 * l.area().abs() <= TIDY * l.length() {
            return None;
        }
        Some(l)
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

    /// A polygon; the points may be in either order. Points within [`EPS`]
    /// of the previous one are left out, and a polygon with no area (fewer
    /// than three points left, or an area within `EPS`) is empty: zero-length
    /// edges fail assertions in `cavalier_contours`.
    pub fn polygon(pts: &[(f64, f64)]) -> Self {
        let mut verts: Vec<Vertex> = Vec::with_capacity(pts.len());
        for &(x, y) in pts {
            let p = Point2::new(x, y);
            if verts.last().is_none_or(|v| v.p.distance(p) > EPS) {
                verts.push(Vertex::new(x, y, 0.0));
            }
        }
        while verts.len() > 1 && verts[0].p.distance(verts[verts.len() - 1].p) <= EPS {
            verts.pop();
        }
        let l = Loop2 { verts };
        if l.verts.len() < 3 || l.area().abs() <= EPS {
            return Region2::empty();
        }
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
        // input (a collapsed slice), or give a wrong result: a growth that
        // does not hold the region or a shrink that does not lie inside it
        // (docs/plan/2026-11-04-region-empty-growth.md). The offset is then
        // retried a hair deeper or shallower.
        let attempt = |dd: f64| {
            let o = guard(RegionOp::Offset, || self.offset_raw(dd))?;
            if self.offset_holds(d, &o) {
                Ok(o)
            } else {
                let what = if d > 0.0 {
                    "the growth does not hold its input"
                } else {
                    "the shrink does not lie inside its input"
                };
                Err(RegionFailure {
                    op: RegionOp::Offset,
                    message: format!("{what} (offset by {dd})"),
                })
            }
        };
        let mut r = attempt(d);
        if r.as_ref().is_err_and(|e| e.message.contains("its input")) {
            CONTAINMENT_RETRIES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        for k in [1e-7, -1e-7, 1e-6] {
            if r.is_ok() {
                break;
            }
            r = attempt(d + k * d.signum());
        }
        r
    }

    /// Whether `o`, an offset of this region by `d`, is plausible: a growth
    /// holds the region and a shrink lies inside it, within the clean-up
    /// tolerance. Linear in the vertex count: the area does not shrink in a
    /// growth or grow in a shrink (by more than [`TIDY`] times the
    /// perimeter), and the first vertex of each loop of the inner region
    /// (this one for a growth, `o` for a shrink) lies inside the outer one
    /// or within `10 · TIDY` of its boundary. Loops narrower than `TIDY`,
    /// which the clean-up drops, are not checked.
    fn offset_holds(&self, d: f64, o: &Region2) -> bool {
        let (inner, outer) = if d > 0.0 { (self, o) } else { (o, self) };
        let slack = TIDY * self.perimeter().max(o.perimeter());
        if inner.area() > outer.area() + slack {
            return false;
        }
        inner.loops.iter().all(|l| {
            let Some(v) = l.verts.first() else {
                return true;
            };
            2.0 * l.area().abs() <= TIDY * l.length()
                || outer.contains_near(v.p) != Inside::Out
                || outer.boundary_distance(v.p) <= 10.0 * TIDY
        })
    }

    /// [`Region2::contains`], leaving out segments that cannot matter
    /// (above, below or left of `p` by more than their size).
    fn contains_near(&self, p: Point2) -> Inside {
        contains_in(
            self.segs().filter(move |s| {
                // An arc lies within its chord's box grown by the chord
                // length times max(1, |bulge|).
                let m = s.a.distance(s.b) * s.bulge.abs().max(1.0) + EPS;
                p.y() >= s.a.y().min(s.b.y()) - m
                    && p.y() <= s.a.y().max(s.b.y()) + m
                    && p.x() <= s.a.x().max(s.b.x()) + m
            }),
            p,
        )
    }

    fn offset_raw(&self, d: f64) -> Region2 {
        let shape = Shape::from_plines(self.loops.iter().map(Loop2::to_pline));
        // cavalier_contours offsets to the left of the loop direction, which
        // is inward for counter-clockwise loops.
        #[cfg(test)]
        match fault::wrong() {
            Some(fault::Wrong::Empty) => return Region2::empty(),
            Some(fault::Wrong::Moved) => return self.translated(Vec2::new(20.0, 0.0)),
            None => {}
        }
        let out = shape.parallel_offset(-d, &ShapeOffsetOptions::new());
        let outer = Region2 {
            loops: out
                .ccw_plines
                .iter()
                .filter_map(|ip| Loop2::from_pline(&ip.polyline).tidied())
                .collect(),
        };
        // A hole outside every outer loop is dropped. `cavalier_contours`
        // keeps the offset of a hole that crosses no other offset without
        // checking it against the other loops of the shape: shrinking a
        // ring 0.01 mm wide around a hole by 0.02 left the grown hole alone,
        // a region of negative area (docs/plan/2026-11-04-region-empty-growth.md).
        let holes: Vec<Loop2> = out
            .cw_plines
            .iter()
            .filter_map(|ip| Loop2::from_pline(&ip.polyline).tidied())
            .filter(|l| outer.contains_near(l.verts[0].p) == Inside::In)
            .collect();
        let mut loops = outer.loops;
        loops.extend(holes);
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
        // A result whose area is impossible for the operation (a union
        // smaller than either input, an intersection larger than either, a
        // difference larger than `self`) is retried like a panic
        // (docs/plan/2026-11-04-region-empty-growth.md).
        let (sa, oa) = (self.area(), o.area());
        let slack = TIDY * (self.perimeter() + o.perimeter());
        let run = |a: &Region2, b: &Region2| {
            let r = guard(RegionOp::Boolean, || boolean::boolean(a, b, op))?;
            let ra = r.area();
            let plausible = match op {
                boolean::Op::Union => ra >= sa.max(oa) - slack,
                boolean::Op::Intersect => ra <= sa.min(oa) + slack && ra >= -slack,
                boolean::Op::Subtract => ra <= sa + slack && ra >= -slack,
            };
            if plausible {
                Ok(r)
            } else {
                Err(RegionFailure {
                    op: RegionOp::Boolean,
                    message: format!("the result has an impossible area ({ra} from {sa} and {oa})"),
                })
            }
        };
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
