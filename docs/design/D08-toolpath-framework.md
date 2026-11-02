# D08 — Toolpath Framework (`fluut-cam`, `fluut-path`)

Implements: [S 03 §3.4, §3.10], [S 04 §4.1, §4.3, §4.12], OQ-05, OQ-06, OQ-11. Depends on:
`fluut-geom`, `fluut-topo`, `fluut-path`, `cavalier_contours`, `rayon`.

The framework turns a resolved toolpath definition into a `ToolpathResult`. Strategies (D09)
supply only their drive pattern and a few hooks; region building, CL computation, fitting,
collision trimming, linking and feeds are shared.

## 1. Strategy interface

```rust
pub trait ToolpathType: Send + Sync {
    fn id(&self) -> ToolpathTypeId;
    fn role(&self) -> Role;                                  // Rough | Finish | Hole | Utility
    fn allowed_setups(&self) -> &'static [SetupKind];
    fn fields(&self) -> &'static [FieldMeta];                // D06 §3.3
    fn validate(&self, p: &ResolvedParams, cx: &ValidateCx) -> Vec<Diagnostic>;
    fn generate(&self, input: &StrategyInput, cx: &JobContext) -> Result<ToolpathResult, CamError>;
}

pub struct StrategyInput {
    pub frame: Arc<SetupGeometry>,       // bodies in work frame W + BVH (D03 §4.3)
    pub faces: FaceSets,                 // drive, check, excluded → QueryGeometry (D07 §2)
    pub tool: ToolAssembly,              // D07 §1
    pub params: ResolvedParams,          // all groups, defaults applied
    pub tol: Tol,                        // machining tolerance from params, model tol from bodies
    pub region: Region2,                 // §3, already built
    pub heights: ResolvedHeights,        // absolute Z (or radii in rotary Setups)
    pub stock_in: Option<Arc<LayeredStock>>, // in-process stock before this toolpath (§7)
    pub setup_kind: SetupKind,
}
```

Most strategies implement `generate` by composing the stages below:

```
region (§3) → drive pattern (strategy) → CL along drive curves (§4) → collision trim (§4.4)
           → fit segments (§5) → order and link (§8) → feeds (§9) → result + stats
```

## 2. Result types (`fluut-path`)

```rust
pub struct ToolpathResult {
    pub passes: Vec<Pass>,               // in machining order, links included as Link passes
    pub stats: PathStats,                // cut length, rapid length, min/max per axis, est. time (D11)
    pub stock_out: Option<Arc<LayeredStock>>,   // for rest machining downstream (§7)
    pub fingerprint: Blake3Hash,
}

pub struct Pass { pub kind: PassKind, pub segments: Vec<Segment> }   // Cut | Lead | Link | Ramp | Retract

pub enum Segment {
    Line   { a: AxisPos, b: AxisPos, m: MotionInfo },
    Arc    { a: AxisPos, b: AxisPos, centre: Point3, normal: UnitVec3, sweep: f64, m: MotionInfo },
    Helix  { a: AxisPos, b: AxisPos, centre: Point3, normal: UnitVec3, sweep: f64, m: MotionInfo },
    Rotary { a: AxisPos, b: AxisPos, radius: f64, m: MotionInfo },            // S 04 §4.7.5
    Spline { a: AxisPos, b: AxisPos, curve: Arc<BSplineCurve3>, m: MotionInfo }, // not emitted in v1
    Cycle  { at: AxisPos, cycle: CycleParams, m: MotionInfo },                 // drilling (S 03 §3.4)
    Marker { kind: MarkerKind },          // manual_nc: stop, optional stop, comment, dwell, pass-through
}

pub struct AxisPos { pub xyz: Point3, pub a: Option<f64>, pub b: Option<f64> }
pub struct MotionInfo { pub motion: Motion, pub feed: FeedRef, pub tol: f32, pub face: Option<PersistId> }
```

Invariants (checked in debug builds and tests): consecutive segments share `b == a` bitwise
[S 03 §3.4]; no NaN; arc `|a−centre| = |b−centre|` within 1e-9 relative.

## 3. Region building [S 04 §4.12.2]

All 2-D region work uses one arc-aware library (OQ-06: `cavalier_contours`), wrapped as:

```rust
pub struct Region2 { pub loops: Vec<Loop2> }    // outer CCW, holes CW; lines and arcs only
pub struct Loop2 { pub segs: Vec<Seg2>, pub source: Vec<SourceRef> }   // per segment: exact source curve

impl Region2 {
    pub fn offset(&self, d: f64) -> Region2;
    pub fn union(&self, o: &Region2) -> Region2;
    pub fn intersect(&self, o: &Region2) -> Region2;
    pub fn subtract(&self, o: &Region2) -> Region2;
    pub fn contains(&self, p: Point2) -> Inside;
    pub fn support(&self, dir: Vec2) -> (f64, f64);      // exact min/max of p·dir (pass layout)
    pub fn clip_line(&self, line: Line2) -> Vec<Interval>;   // exact intervals, refined (§3.1)
}
```

Region for a toolpath:

```
R  = machining box (Setup region, lines)
R ∩= drive-face outline (D03 §5.3), if drive faces are selected
R ∩= boundary curve, offset per tool containment (inside / centre / outside)
R  = R offset by border settings
→ tool-centre region: offset by −tool radius where the strategy keeps the tool inside
```

### 3.1 Exactness of B-spline boundaries

`cavalier_contours` works on lines and arcs. B-spline boundary curves are biarc-fitted
(D02 §7) within `tol.machining / 4`. Every `Seg2` keeps a `SourceRef` to its exact curve and
parameter range. When a pass is clipped at a biarc segment, the clip point is refined by Newton
on the exact source curve (after any offset, using the exact offset point `C(t) + d·n(t)`), so
the endpoint satisfies [S 04 §4.12.1]. See CH-13.

## 4. CL computation along drive curves

### 4.1 Drive curves

A strategy produces drive curves in the region: 2-D lines, arcs, spirals, or 3-D curves (rotary
lines, waterline contours). Each has exact start and end parameters (from region clipping).

### 4.2 Adaptive sampling [S 04 §4.3.2]

```
sample(curve, t0, t1):
    c0 = drop(curve(t0)), c1 = drop(curve(t1))         (D07)
    stack = [(t0,c0,t1,c1)]
    while stack:
        (ta,ca,tb,cb) = pop
        if ca.on ≠ cb.on and both are real contacts:     // contact changed
            (tm, cm) = transition(...)                    (D07 §5, exact)
            emit split at tm; push both halves; continue
        tm = (ta+tb)/2; cm = drop(curve(tm))
        if deviation(cm, chord(ca,cb)) > tol/2 or |tb−ta| > max_step: push halves
        else accept chord
```

- `max_step` bounds sample spacing so features smaller than the spacing are not jumped over:
  `max_step = min(tool radius / 2, region feature size)`.
- Points come out in curve order with exact first, last and transition points.

### 4.3 Direct emission [S 04 §4.3.1]

Before sampling, a strategy may ask `direct(curve, face)`: if the drive curve is a line or arc,
the whole curve lies over a single analytic face (checked by drops at both ends and one middle
point, plus a no-other-candidate bound check from the BVH), and the face/tool/curve combination
is in the closed-form table, the CL curve is produced directly as a line, arc or rotary segment.

### 4.4 Collision trimming [S 04 §4.14]

For each accepted sample, `collides` (D07 §6) is evaluated for each non-cutting part. Changes
of the predicate between neighbouring samples are bisected to `tol.solver`. Colliding intervals
are removed from the pass, recorded as `Diagnostic { code: "cam.collision.trimmed", location }`
with the minimum stick-out that would clear (max over the zone of required part bottom height).

## 5. Segment fitting [S 04 §4.3.3]

Input: CL point sequence with flags (exact endpoints, transitions). Output: segments within
`tol/2` of the samples, with exact points kept as segment ends.

```
i = 0
while i < n:
    j = longest k such that points i..k lie within tol/2 of chord(i,k)   (exponential + binary search)
    k = longest such that points i..k lie within tol/2 of a circle through i, mid, k
        and the arc plane is allowed (XY/XZ/YZ, or any if the post allows; D10 decides later)
    choose the longer of line/arc (arc must cover ≥ 4 points and sweep < 359°)
    helix: if arc test fails only by a linear axial term, fit helix
    never cross a point flagged exact-transition unless both sides belong to the same primitive
    emit segment; i = end
```

Tangent continuity [S 04 §4.3.3]: when two consecutive fitted pieces meet with a tangent change
below 0.5° and the underlying samples are smooth, the junction is adjusted (biarc join) within
the tolerance budget, so controllers do not decelerate.

## 6. Pass layout [S 04 §4.12.4]

```rust
pub fn layout(region: &Region2, dir: Vec2, stepover: f64, mode: PassLayout) -> Vec<f64>;
```

```
(a, b) = region.support(normal(dir))                 // exact extent across passes
W = b − a
even:            n = ceil(W / s);  return [a + i·W/n for i in 0..=n]
fixed_from_edge: return [a + i·s for i in 0..=floor(W/s)] + [b] if not already included
```

The same function lays out Z levels (with the exact flat-face heights inserted, then each gap
filled evenly so no gap exceeds step-down) and rotary X positions.

## 7. In-process stock (OQ-11, OQ-15)

```rust
pub struct LayeredStock {
    pub kind: StockFrame,                       // Planar (Z levels) | Cylindrical (radii, unrolled)
    pub levels: BTreeMap<OrdF64, Region2>,      // material remaining at each level
    pub top: f64,                               // stock top (or outer radius)
}

impl LayeredStock {
    pub fn initial(stock: &StockSpec, levels: &[f64]) -> Self;
    pub fn ensure_level(&mut self, z: f64);               // insert a level, cloning the next level above
    pub fn remove_swept(&mut self, z: f64, swept: &Region2);
    pub fn remaining(&self, z: f64) -> &Region2;
    pub fn reframed(&self, from: &Frame, to: &Frame) -> Self;   // carry into a child Setup (S 03 §3.9.6)
}
```

- Roughing and 2-D toolpaths update the stock: at each level, swept region = union over passes
  at or above that level of (pass centre-line region offset by tool radius at that height).
  A cut at level z also clears every level between z and the level above it.
- 3-D finishing toolpaths do not update the stock in v1 (CH-14).
- Rest roughing: at each level, the region to clear = `remaining(z) ∩ machinable(z)`.
- Reframing for an indexed child at a different angle: the parent stock is converted to a 3-D
  solid approximation (prisms between levels), rotated, and re-sliced at the child's levels.
  This is exact for prism stacks and conservative (keeps material) elsewhere (CH-15).

## 7.1 Machinable region at a level

`machinable(z)` = region where the tool can reach depth z without gouging = complement of
(section of the bodies at z offset by tool radius + stock_to_leave). Uses D03 §5.1 sections
(with CH-03 handling of flats).

## 8. Ordering and linking

### 8.1 Ordering

- Within a level or a set of parallel passes: strategy decides (zig-zag, one-way, climb).
- Across regions: nearest-neighbour from the current end point, then 2-opt improvement bounded
  to 50 ms per toolpath.
- Levels: top to bottom by default; `order = by_area` finishes each area through all levels
  first [S 04 §4.13.7].

### 8.2 Links

For each gap between consecutive cut passes:

```
if retract policy allows staying down and the straight link at max(end z, start z) is clear:
    // clear = drop along the link segment never exceeds link height (sampled + bound check),
    //         and holder check passes, and (roughing) the link does not cut uncut stock
    link = feed move along the straight line (or smooth S-link if transitions = smooth)
elif minimum retract: retract to (highest drop along the link) + safety distance, move, plunge
else: retract to retract height (or clearance if crossing Setup clearance), rapid, descend
```

Rapids are checked against model and stock with the full tool assembly; if blocked, raised to
clearance [S 04 §4.14].

M3 (2-D types, before the S2 drop queries): "clear" means the link stays inside the
tool-centre region of the same area and level; `minimum` stays down only for links up to one
tool diameter (D15 §2.6), and its retract height is `min(retract, max(stock top, top) +
safety_distance)`, the material bound standing in for the drops along the link. Every
toolpath starts above its first entry at its `clearance` height and ends there.

M3 `parallel` (with the S2 drop queries): a link stays down and follows the surface (drops
along it) when the policy allows it (`stay_down`, or `minimum` up to one tool diameter), the
straight link lies in the region and every point of it would be cut. Otherwise the tool lifts
to `safety_distance` above a conservative bound of the geometry under the link (the highest
primitive box within the tool radius), capped at `retract` for `minimum`. It then rapids down to the feed
height, or to `safety_distance` above the entry when the link is already below the feed
height, and plunges. Leads are in the vertical plane of the pass, tangent to it in 3-D, and dropped if
drops along them show a gouge.

**Rapid floor (2026-11-01, simulation findings).** A rapid on the way down to an entry stops at
`max(feed, material top + safety_distance)`; below that the tool feeds (`Plunge`). The material
top is the stock top, or `top` when that is higher. A toolpath whose `top` is below the stock
top (a floor pass, a hole under an unmachined counterbore) cannot tell, without the in-process
stock, whether an earlier toolpath cleared the material above `top`, so the safe choice is to
feed through it. The drill's R plane takes the same floor, since canned cycles rapid to it, and
so does `bore`'s approach. Toolpaths whose `top` is the stock top are unchanged. Using the
in-process stock to lower the floor where it is known to be clear is left for later. The 3-D
finishing linker (`follow.rs`) takes the same floor ([waterline plan](../plan/2026-11-01-waterline-robust.md)):
its lifts between runs go at least `safety_distance` above the material top, and its descents
stop there and plunge.

**Adaptive links (2026-11-01).** The straight entry move of an adaptive pass, its sliver moves
and the safe regions its stay-down links use are kept inside the tool-centre region; the shrunk
cleared region alone can reach across an island.

### 8.3 Leads and ramps

- Lead-in/out: tangent arc of the given radius in the plane of the pass (or vertical arc for
  3-D finishing), checked for gouge; shortened or dropped if it gouges (diagnostic).
- Ramps for entries into stock (roughing): helix (preferred; radius from param, clipped to the
  region), zig-zag along the first pass, profile ramp along the region boundary, or plunge.
  Ramp angle from linking group.

## 9. Feeds and motions

Each segment gets `Motion` (Rapid, Cut, Plunge, Ramp, LeadIn, LeadOut, Link, Retract) and a
`FeedRef` (symbolic: cut, plunge, ramp, lead_in, lead_out, transition). Numeric feeds are
resolved at post time from the tool group, so a feed change does not need recalculation.
Plunge feed applies to moves whose tool-axis component is downward and exceeds 45° from the
horizontal.

## 10. Parallelism and determinism

Passes (or levels) are independent until ordering: computed with `rayon` `par_iter`, collected
in index order. Ordering and linking are sequential. Branch-and-bound order ties are broken by
primitive index. Same inputs → identical result (D01 §6).

## 11. Tests

- Pass endpoint test [S 04 §4.12.7 (1)]: every pass endpoint lies on a region boundary, a
  contact transition, or a collision trim point, within `tol.solver`.
- Stepover test [S 04 §4.12.7 (2)].
- Translation invariance [S 04 §4.12.7 (3)] for every v1 strategy.
- Fitting: random smooth curves; max deviation ≤ tol/2 with the sampled points; fitted count
  near the minimum (within 20 % of an optimal dynamic-programming fit on small cases).
- Gouge check: dense sampling of the final segments against D07 drop (no point below τ − tol).
- Links: synthetic parts with walls; stay-down links never cross geometry or uncut stock.

## 12. Challenges

- CH-13 B-spline boundaries through a lines-and-arcs offset library (deferred).
- CH-14 3-D finishing toolpaths do not update in-process stock (deferred).
- CH-15 Reframing layered stock between indexed Setups (deferred).
