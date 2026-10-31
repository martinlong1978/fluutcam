# M1 — Geometry Complete and Path Types Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish `fluut-geom` to D02 (surfaces, offsets, closest point, plane sections, biarcs) and add `fluut-path` (toolpath result types, D08 §2), so that M2 (post engine) and spikes S1–S3 have their inputs.

**Architecture:** `fluut-path` is a new layer-1 crate depending only on `fluut-geom`. Surfaces follow STEP (ISO 10303-42) parametrisations exactly so STEP p-curves need no conversion (D02 §4.1).

**Tech Stack:** as M0; adds `criterion` (dev, benchmarks).

**Spec:** [`../design/D02-geometry-core.md`](../design/D02-geometry-core.md), [`../design/D08-toolpath-framework.md`](../design/D08-toolpath-framework.md) §2, §9.

## Global Constraints

All M0 Global Constraints apply. In addition:

- STEP surface parametrisations (ISO 10303-42): plane `S = o + u·x + v·y`; cylinder `S = o + r(cos u·x + sin u·y) + v·z`; cone `S = o + (r + v·tan α)(cos u·x + sin u·y) + v·z`; sphere `S = o + r·cos v(cos u·x + sin u·y) + r·sin v·z`; torus `S = o + (R + ρ cos v)(cos u·x + sin u·y) + ρ sin v·z`.
- Surface normal is `Su × Sv` normalised (face sense is applied by D03, not here).
- `Segment` invariants (D08 §2): consecutive segments share `b == a` bitwise; no NaN; arc `|a−centre| = |b−centre|` within 1e-9 relative.
- Feeds are symbolic (`FeedRef`) and resolved at post time (D08 §9).

## Review Focus

1. **Singular surface points** (sphere poles, cone apex): `normal` returns `None`; `closest` still returns a valid (u, v). Tests in Task 2.
2. **Point on the axis of a surface of revolution** (closest point to a point on the cylinder/sphere axis): any u is correct; result must be finite and in range. Tests in Task 2.
3. **Offset through the axis** (concave cylinder offset by more than its radius): `offset` returns `None`, not a negative radius. Tests in Task 2.
4. **Zero-length segments and full-circle arcs** in `fluut-path`: zero-length line is rejected by `PassBuilder`; a 360° arc (a == b) is valid and keeps its sweep. Tests in Task 1.
5. **`linearize` of tiny or huge arcs** (radius 1e-6 mm, radius 1e5 mm): chord error within tolerance and at least one segment, never zero segments. Tests in Task 1.

---

### Task 1: `fluut-path` crate

**Files:** Create `crates/fluut-path/{Cargo.toml, src/lib.rs, src/segment.rs, src/pass.rs, src/linearize.rs}`

**Interfaces:**
- Consumes: `fluut_geom::{Point3, Vec3, UnitVec3, BSplineCurve3, Aabb3}`.
- Produces:
  - `AxisPos { xyz: Point3, a: Option<f64>, b: Option<f64> }` (rotary angles in degrees, as the machine sees them).
  - `enum Motion { Rapid, Cut, Plunge, Ramp, LeadIn, LeadOut, Link, Retract }`; `enum FeedRef { Rapid, Cut, Plunge, Ramp, LeadIn, LeadOut, Transition }`.
  - `MotionInfo { motion: Motion, feed: FeedRef, tol: f32, face: Option<PersistId> }`; `PersistId(pub u128)`.
  - `enum CycleKind { Drill, Spot, Peck, ChipBreak, Tap, Ream, Bore }`; `CycleParams { kind, top_z: f64, depth_z: f64, retract_z: f64, peck: Option<f64>, dwell_s: Option<f64> }`.
  - `enum MarkerKind { Stop, OptionalStop, Comment(String), Dwell(f64), PassThrough(String) }`.
  - `enum Segment { Line{a,b,m}, Arc{a,b,centre,normal,sweep,m}, Helix{a,b,centre,normal,sweep,m}, Rotary{a,b,radius,m}, Spline{a,b,curve,m}, Cycle{at,cycle,m}, Marker{kind} }` with `start -> Option<&AxisPos>`, `end -> Option<&AxisPos>`, `motion -> Option<&MotionInfo>`, `length -> f64` (tool-tip path length; helix = √((r·sweep)² + Δh²)).
  - `enum PassKind { Cut, Lead, Link, Ramp, Retract }`; `Pass { kind, segments: Vec<Segment> }`; `fn Pass::check(&self) -> Result<, PathError>` (invariants above).
  - `PathStats { cut_length, rapid_length, bounds: Aabb3 }`; `ToolpathResult { passes: Vec<Pass>, stats: PathStats }` with `fn ToolpathResult::new(passes) -> Result<Self, PathError>` (checks, computes stats). `fingerprint` and `stock_out` are added by `fluut-core`/`fluut-cam` later.
  - `fn linearize(seg: &Segment, tol: f64) -> Vec<Segment>` — arcs/helices to lines with chord height ≤ tol; endpoints shared bitwise with the original.
  - `enum PathError { Discontinuous{pass, index}, NonFinite{pass, index}, ArcRadiusMismatch{pass, index}, ZeroLength{pass, index} }`.

- [x] **Step 1:** Tests: `continuity_checked` (two lines with `b ≠ a` → `Discontinuous`); `nan_rejected`; `arc_radius_mismatch`; `zero_length_line_rejected`; `full_circle_arc_valid` (a == b, sweep 2π, length 2πr); `helix_length`; `linearize_arc_chord` (radius 10, sweep π/2, tol 0.01: every chord sagitta ≤ 0.01; first `a` and last `b` bitwise equal to the arc's); `linearize_tiny_and_huge` (radius 1e-6 and 1e5, at least one segment); `stats_sum` (cut and rapid lengths add up).
- [x] **Step 2:** Fail, implement, pass. Add `fluut-path = 1` already in `xtask/layers.toml`.
- [x] **Step 3:** Workspace checks (fmt, clippy -D warnings, test, check-layers).

### Task 2: Analytic surfaces

**Files:** Create `crates/fluut-geom/src/surface/{mod.rs, analytic.rs}`

**Interfaces:**
- Produces:
  - `enum Surface { Plane{frame}, Cylinder{frame, r}, Cone{frame, r, semi_angle}, Sphere{frame, r}, Torus{frame, major, minor}, BSpline(Arc<BSplineSurface>) }` (Extrusion, Revolution, Offset in Task 4) with constructors returning `Option` (positive finite radii; `0 < semi_angle < π/2`).
  - `enum SurfaceKind`; `struct SurfDerivs { p: Point3, su, sv, suu, suv, svv: Vec3 }`; `struct Curvatures { k1, k2: f64, d1, d2: Vec3 }`; `struct UvBox { u: Interval, v: Interval }`; `struct SurfClosest { u, v, point: Point3, dist: f64 }`.
  - `impl Surface`: `kind`, `eval(u,v)`, `derivs(u,v)`, `normal(u,v) -> Option<UnitVec3>` (None when `|Su×Sv| < 1e-14·(|Su||Sv|)` or either is ~0), `curvatures(u,v) -> Option<Curvatures>` (from fundamental forms; None at singular points), `closest(p, uv: UvBox, tol) -> SurfClosest`, `offset(d) -> Option<Surface>` (D02 §4.3 table; None when the "valid while" condition fails), `periodicity -> (Option<f64>, Option<f64>)`, `transformed(&Affine) -> Option<Surface>` (similarities only; radii scale).
  - Closest point for analytic kinds: closed form in the local frame (angle via `libm::atan2` wrapped into the u range as `Curve3::closest` does), then clamped to `uv`; if clamped, refine with 2-D damped Newton inside the box (shared helper `newton_uv`, D02 §6).

- [x] **Step 1:** Tests: `plane_eval_frame`; for each kind `derivs_match_finite_difference`; `normal_points_outward` (sphere: normal·(S−o) > 0; cylinder radial); `sphere_pole_normal_none` (v = π/2); `cone_apex_normal_none`; `curvatures_closed_form` (cylinder k = {0, −1/r} sign per normal, sphere −1/r twice, torus outer equator −1/(R+ρ)… compare magnitudes); `closest_round_trip` proptest (`closest(eval(u,v)) ≈ (u,v)` away from singularities, dist < 1e-9); `closest_on_axis_finite` (cylinder, sphere centre); `closest_clamped_to_box`; `offset_then_distance` (random point on offset surface is |d| from the base, D02 §9); `concave_offset_none` (cylinder r = 2 offset −3 → None); `mirror_transform`.
- [x] **Step 2:** Fail, implement, pass. Note: under a mirror a surface's parametrisation must reverse one parameter (plane: v; others: u); documented on `Surface::transformed`.

### Task 3: B-spline surfaces

**Files:** Create `crates/fluut-geom/src/surface/bspline.rs`

**Interfaces:**
- Produces: `BSplineSurface::new(deg_u, deg_v, knots_u, knots_v, poles (row-major nu × nv, u major), nu, nv, weights) -> Result<_, BSplineError>`; `domain -> UvBox`; `derivs(u, v) -> SurfDerivs` (tensor de Boor A3.6, rational quotient rule A4.4); `Surface::BSpline` arms of eval/derivs/normal/curvatures/closest/transformed. Closest: seeds from a 9×9 grid per Bézier patch, best seed refined by `newton_uv`.
- [x] **Step 1:** Tests: `bilinear_patch`; `rational_cylinder_patch_matches_cylinder` (degree (2,1) quarter cylinder NURBS vs `Surface::Cylinder` within 1e-12); `derivs_match_finite_difference`; `closest_round_trip`.
- [x] **Step 2:** Fail, implement, pass. `Surface::offset` returns `None` for B-splines until the `Offset` wrapper lands in Task 4.

### Task 4: Extrusion, revolution, offset surfaces; ellipse and offset curves

**Files:** Modify `surface/mod.rs`, `curve.rs`; create `surface/swept.rs`
- Ellipse curve (`a ≥ b > 0`, closest by bracketed Newton on the angle), `Curve3::Offset`; `Surface::{Extrusion, Revolution, Offset}` per D02 §4.1/§4.3.
- Tests: ellipse derivs/closest/length (perimeter vs Ramanujan II within 1e-9 relative); extrusion of circle equals cylinder; revolution of line equals cone; offset wrapper vs analytic offset within 1e-10 (D02 §9).
- **Done.** Perimeter is checked against the exact Gauss–Kummer series (1e-9 absolute; Ramanujan II is only ~1e-5 here). Added `Surface::transformed_param` and `Surface::transform_flips_normal` (CH-24) with a property test over every kind.

### Task 5: Plane sections

**Files:** Create `crates/fluut-geom/src/section.rs`
- `fn plane_section(s: &Surface, uv: UvBox, plane: &Plane, tol: &Tol) -> SectionResult` — closed forms for analytic surfaces (D02 §5 table), marching for the rest (D02 §5.1). Tangential contacts are reported, not resolved (CH-03).
- **Done.** Implemented as `surface/section.rs` + `surface/march.rs`: `plane_section(s, uv, &SectionPlane, tol) -> SurfaceSection { curves, coincident, touch_points }`, each curve flagged `closed` / `tangent`. Exact for planes, cylinders (circle, lines, ellipse), spheres, and cones/tori cut perpendicular to or through the axis. Oblique cone and torus sections and all free-form surfaces are marched to `machining/2` and fitted as G1 cubic Hermite B-splines; marching wraps across full-period seams. Deviations from D02 §5: oblique cone conics are marched, not exact rational B-splines (revisit in S1); a loop lying entirely inside one seed cell (1/8 of a patch) is missed (tracked under CH-03).

### Task 6: Biarc and point-sequence fitting

**Files:** Create `crates/fluut-geom/src/fit.rs`
- `fn biarc_fit(c: &Curve3, range, tol) -> Vec<Seg2or3>`; `fn fit_points(points, tol) -> Vec<FitSeg>` (greedy longest run, Kåsa circle, max-deviation check), D02 §7.
- **Done.** `biarc_fit(c, range, plane_normal, tol) -> Vec<FitSeg>` (equal-chord biarcs, split at >90° turns and at the worst sample) and `fit_points(points, tol) -> Vec<FitSeg>` (greedy longest run by doubling + bisection). Arcs in `fit_points` pass through the run's first, middle and last points instead of a Kåsa fit, so ends stay exact; every point is still checked against `tol`. The "within 20 % of optimal" check of D08 §11 is left to M3, where fitting is used.

### Task 7: Benchmarks and supply-chain check — done

- `criterion` benchmarks for D02 §8 targets; `cargo deny check` in CI `lint` job.

Done: `crates/fluut-geom/benches/geom.rs` (`cargo bench -p fluut-geom`); `EmbarkStudios/cargo-deny-action@v2` in the `lint` job. Path dependencies carry a version so `wildcards = "deny"` passes.

Measured on the development machine (Windows 11, release profile, criterion median):

| Operation | D02 §8 target | Measured | Status |
|---|---|---|---|
| eval, analytic (torus) | 20 ns | 36 ns | 1.8× over |
| derivs to 2nd, bicubic B-spline | 250 ns | 421 ns | 1.7× over |
| closest point, analytic (sphere) | 50 ns | 88 ns | 1.8× over |
| closest point, bicubic B-spline | 5 µs | 99 µs | 20× over |
| plane ∩ cylinder | 200 ns | 70 ns | met |

Optimisations made: fixed-size basis arrays for low degree, unchecked frame `y`, branch-and-bound over Bézier patches by pole bounds (was a dense grid seed: 1.1 ms). The gaps are recorded as CH-25. The B-spline closest point matters most (drop-cutter inner loop); it moves to spike S2, which owns the drop-cutter cost model.

Tasks 4–7 get full step lists when Tasks 1–3 are done (their shapes depend on the surface API settled in Tasks 2–3).
