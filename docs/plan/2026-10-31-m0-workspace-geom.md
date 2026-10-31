# M0 — Workspace and Geometry Foundations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Cargo workspace that builds and lints cleanly, with the first part of `fluut-geom`: basic types, tolerances, root finders, line/circle curves and B-spline curves.

**Architecture:** Workspace per [D14 §1] under ``. Only `fluut-geom` and `xtask` exist at the end of M0; other crates are added by later milestones. `fluut-geom` wraps `glam` so no other crate uses `glam` directly ([D02 §1]).

**Tech Stack:** Rust stable 1.99.1 (MSVC on Windows), `glam` (f64 types), `libm`, `smallvec`, `thiserror`; dev: `proptest`, `approx`.

**Spec:** [`../design/D01-workspace-architecture.md`](../design/D01-workspace-architecture.md), [`../design/D02-geometry-core.md`](../design/D02-geometry-core.md), [`../design/D14-build-test-release.md`](../design/D14-build-test-release.md).

## Global Constraints

- Licence: every source file starts with `// SPDX-License-Identifier: MIT OR Apache-2.0`; workspace `license = "MIT OR Apache-2.0"` ([D14 §1]).
- No copyleft dependencies; `cargo deny` licence allow-list: MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, Unicode-DFS-2016.
- `unsafe_code = "forbid"` at workspace level ([D14 §2]).
- `f64::sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `exp`, `ln`, `powf`, `hypot`, `cbrt`, `sin_cos` are banned via `clippy.toml` `disallowed-methods`; use `libm` ([D01 §6]). `sqrt` is allowed (IEEE correctly rounded).
- Lengths `f64` mm; angles radians in code ([D01 §2.1]).
- No global epsilons: every comparison takes a `Tol` or explicit tolerance ([D02 §2]).
- Library code never panics on bad input; it returns `Result` or `Option` ([design README]).
- Release profile: `lto = "thin"`, `codegen-units = 1`, `panic = "unwind"`; no `target-cpu=native` ([D14 §2]).
- Toolchain pinned in `rust-toolchain.toml`: `channel = "1.99.1"`, components `rustfmt`, `clippy`, `rust-analyzer`, `rust-src`.

## Review Focus

1. **Degenerate inputs to constructors** (zero vector to `UnitVec3::new`, zero radius circle, frame with parallel axes): expect `None`/`Err`, never NaN propagation. Tests in Tasks 2 and 5.
2. **Root finder at bracket ends and double roots** (f(lo) = 0, tangent root): expect the root returned, not "no sign change". Tests in Task 4.
3. **B-spline evaluation at the last knot** (t = knots[last]): expect the last pole, not an index panic. Test in Task 6.
4. **Circle parameter wrap** (closest point near t = 0 / 2π, range crossing 2π): expect the result inside the requested range. Test in Task 5.
5. **Mirror transforms** (`Affine` with det < 0 applied to a circle): expect a valid circle with the sense reversed (frame z flipped), not a left-handed frame. Test in Task 5.

---

### Task 1: Workspace scaffold, lints, layer check, CI

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `clippy.toml`, `rustfmt.toml`, `deny.toml`, `.gitignore`, `README.md`, `LICENSE-MIT`, `LICENSE-APACHE`
- Create: `crates/fluut-geom/Cargo.toml`, `crates/fluut-geom/src/lib.rs`
- Create: `xtask/Cargo.toml`, `xtask/src/main.rs`, `xtask/layers.toml`
- Create: `.cargo/config.toml` (alias `xtask = "run -p xtask --"`)
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: `cargo xtask check-layers` — reads `xtask/layers.toml` (`[layers] fluut-geom = 0, fluut-path = 1, …` from D01 §1 diagram, lower number = lower layer) and `cargo metadata`; fails if any workspace crate depends on a crate with an equal-or-higher layer number, or on a workspace crate missing from `layers.toml`.

- [ ] **Step 1:** Write workspace `Cargo.toml` with `[workspace] resolver = "3"`, `members = ["crates/*", "xtask"]`, `[workspace.package]` (edition 2024, licence, `rust-version = "1.99"`), `[workspace.dependencies]` (`glam = { version = "0.30", features = ["serde"] }` without serde for now, `libm`, `smallvec`, `thiserror`, `proptest`, `approx`), `[workspace.lints]` (`rust.unsafe_code = "forbid"`, `clippy.all = "warn"`, `clippy.disallowed_methods = "deny"`), and the release profile.
- [ ] **Step 2:** `xtask check-layers` test: a unit test with an in-memory dependency list `[("fluut-geom", ["fluut-path"])]` and layers `{fluut-geom: 0, fluut-path: 1}` returns one violation; the reverse returns none.
- [ ] **Step 3:** Implement `fn violations(layers: &BTreeMap<String, u32>, deps: &[(String, Vec<String>)]) -> Vec<String>` and `main` that runs `cargo metadata --format-version 1 --no-deps` (parse with `serde_json`).
- [ ] **Step 4:** Run `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`, `cargo xtask check-layers`. All pass.
- [ ] **Step 5:** CI workflow: jobs `lint` (ubuntu: fmt, clippy, check-layers) and `test` (matrix windows-latest, macos-latest, ubuntu-latest: `cargo test --workspace --locked`). `cargo deny` joins the `lint` job in M1; `deny.toml` is written now.
- [ ] **Step 6:** Commit `fluutcam: workspace scaffold, lints, layer check, CI`.

### Task 2: Basic types (`fluut-geom::types`)

**Files:**
- Create: `crates/fluut-geom/src/types.rs`, `src/frame.rs`, `src/transform.rs`, `src/bounds.rs`
- Test: inline `#[cfg(test)] mod tests` per file

**Interfaces:**
- Produces:
  - `Point3(pub DVec3)`, `Vec3(pub DVec3)`, `Point2(pub DVec2)`, `Vec2(pub DVec2)` with `new(x,y[,z])`, `Point3 - Point3 -> Vec3`, `Point3 + Vec3 -> Point3`, `Vec3` add/sub/neg/`* f64`, `dot`, `cross`, `length`, `distance`.
  - `UnitVec3` — `fn new(v: Vec3) -> Option<UnitVec3>` (None if `|v| < 1e-300` or non-finite), `fn get(self) -> Vec3`, consts `X`, `Y`, `Z`.
  - `Frame { origin: Point3, z: UnitVec3, x: UnitVec3 }` — `fn new(origin, z: Vec3, x_ref: Vec3) -> Option<Frame>` (x = x_ref minus its z component, normalised; None if parallel within 1e-12), `fn y(&self) -> UnitVec3`, `fn to_world(&self, local: Point3) -> Point3`, `fn to_local(&self, world: Point3) -> Point3`, `fn world_xy -> Frame`.
  - `Rigid` — `identity`, `from_rotation_translation(DQuat, Vec3)`, `apply_point`, `apply_vec`, `then(&self, next: &Rigid) -> Rigid`, `inverse`.
  - `Affine` — `identity`, `from_rigid(&Rigid)`, `scale(sx, sy, sz)`, `apply_point`, `apply_vec`, `then`, `fn reverses_orientation(&self) -> bool` (det < 0).
  - `Aabb3 { min, max }` — `empty`, `from_points(&[Point3])`, `union`, `expand(d)`, `contains`, `is_empty`; `Interval { lo, hi }` — `new` (None if lo > hi or NaN), `length`, `contains`, `clamp`.

- [ ] **Step 1:** Tests: `unit_vec_rejects_zero` (`UnitVec3::new(Vec3::new(0.,0.,0.)).is_none`); `frame_orthonormal` (frame from z=(0,0,2), x_ref=(1,1,0) has |x|=|y|=|z|=1 and pairwise dot < 1e-15); `frame_rejects_parallel`; `frame_round_trip` (to_local(to_world(p)) == p within 1e-12); `rigid_then_inverse_is_identity`; `affine_mirror_reverses` (`Affine::scale(-1.,1.,1.).reverses_orientation`); `aabb_union_contains`; `interval_rejects_inverted`.
- [ ] **Step 2:** Run `cargo test -p fluut-geom` — fail (types missing).
- [ ] **Step 3:** Implement. Rotation via `glam::DQuat`; `Affine` holds `DMat3` + `DVec3`.
- [ ] **Step 4:** Run tests — pass. Add proptest `rigid_preserves_distance` (random quat from `libm`-free `DQuat::from_axis_angle` with random axis — note: `glam` uses `f64::sin` internally; acceptable for test construction only, record as CH-01 follow-up if bit-identity needs `glam` quats avoided in production paths).
- [ ] **Step 5:** Commit `fluut-geom: basic types, frames, transforms, bounds`.

### Task 3: Tolerances (`fluut-geom::tol`)

**Files:** Create `crates/fluut-geom/src/tol.rs`

**Interfaces:**
- Produces: `Tol { model: f64, machining: f64, solver: f64, angle: f64 }`; `fn Tol::for_machining(model: f64, machining: f64) -> Tol` with `solver = min(1e-6, machining / 1000)`, `angle = 1e-9`.

- [ ] **Step 1:** Tests: `solver_from_machining` (`Tol::for_machining(1e-3, 0.01).solver == 1e-6`; `for_machining(1e-3, 0.0001).solver == 1e-7`; `for_machining(1e-3, 0.1).solver == 1e-6`).
- [ ] **Step 2:** Fail, implement, pass.
- [ ] **Step 3:** Commit `fluut-geom: Tol`.

### Task 4: Root finders (`fluut-geom::roots`)

**Files:** Create `crates/fluut-geom/src/roots.rs`

**Interfaces:**
- Produces:
  - `fn quadratic(a: f64, b: f64, c: f64) -> SmallVec<[f64; 2]>` — real roots ascending; stable form `q = −½(b + sign(b)√disc)`; `a == 0` falls back to linear; double root returned once.
  - `fn cubic(a, b, c, d) -> SmallVec<[f64; 3]>` — Cardano/trigonometric seeds (via `libm::cbrt`, `libm::acos`, `libm::cos`), each polished by 2 Newton steps on the original polynomial; roots within `1e-12·(1+|r|)` merged.
  - `enum RootError { NoSignChange, NotConverged { best: f64 } }`
  - `fn newton_bracketed(f: impl Fn(f64) -> (f64, f64), lo: f64, hi: f64, tol_x: f64) -> Result<f64, RootError>` — `f` returns (value, derivative); bisection fallback when a step leaves the bracket or does not halve |f|; max 50 iterations ([D02 §6]); `f(lo) == 0` or `f(hi) == 0` returns that end.

- [ ] **Step 1:** Tests: `quadratic_cancellation` (`quadratic(1., -1e8, 1.)` gives roots ≈ 1e-8 and 1e8, the small one with relative error < 1e-15); `quadratic_double_root` (`(1,-2,1)` → `[1.0]`); `quadratic_none` (`(1,0,1)` → empty); `cubic_three_roots` (`(x-1)(x-2)(x-3)` → `[1,2,3]` within 1e-12); `cubic_double_root` (`(x-1)²(x+2)` → `[-2, 1]`); `newton_endpoint_root` (`f(x)=x`, bracket `[0,1]` → `0`); `newton_no_sign_change` → `Err(NoSignChange)`; `newton_cos` (`f=cos`, `[1,2]` → π/2 within 1e-14); proptest: random roots r1 in [-10,10], gaps in [0.5,10], leading coefficient ±[0.1,10], recovered by `cubic` within 1e-8·(1+|r|) (root accuracy is about ε·|terms|/|f′(r)|, so closer roots are covered by the double-root tests instead).
- [ ] **Step 2:** Fail, implement, pass.
- [ ] **Step 3:** Commit `fluut-geom: quadratic, cubic and bracketed Newton root finders`.

### Task 5: Analytic curves (`fluut-geom::curve`) — Line and Circle

**Files:** Create `crates/fluut-geom/src/curve.rs` (split into `curve/` per kind when Ellipse and Offset arrive in M1)

**Interfaces:**
- Consumes: Task 2 types, Task 3 `Tol`, Task 4 roots.
- Produces:
  - `enum Curve3 { Line { origin: Point3, dir: UnitVec3 }, Circle { frame: Frame, radius: f64 }, BSpline(Arc<BSplineCurve3>) }` — `Ellipse` and `Offset` added in M1.
  - `fn Curve3::circle(frame: Frame, radius: f64) -> Option<Curve3>` (None if radius ≤ 0 or non-finite).
  - `impl Curve3`: `eval(t) -> Point3`, `derivs(t, n) -> SmallVec<[Vec3; 4]>` (n ≤ 3; index 0 is the point as a vector from origin), `natural_range -> Option<Interval>` (circle `[0, 2π]`, line None), `closest(p, range: Interval, tol: &Tol) -> (f64, f64)`, `length(range, tol) -> f64`, `tessellate(range, chord) -> Vec<(f64, Point3)>`, `transformed(&Affine) -> Option<Curve3>` (None if a non-uniform scale turns a circle into an ellipse — ellipse comes in M1), `as_circle -> Option<(&Frame, f64)>`.
  - Circle parametrisation: `C(t) = o + r(cos t·x + sin t·y)` (STEP).
  - Tessellation step: `Δθ = 2·acos(1 − chord/r)`, `chord ≥ r` gives 4 segments minimum per full circle.

- [ ] **Step 1:** Tests: `circle_eval_quadrants`; `circle_derivs_match_finite_difference` (h=1e-6, rel 1e-6); `circle_closest_wraps` (point at angle −0.1 with range `[0, 2π]` returns t ≈ 2π − 0.1); `circle_closest_clamps_to_range` (range `[0, π/2]`, point at angle π → endpoint nearer); `circle_closest_centre` (p = centre: returns range.lo, dist = r); `line_closest`; `circle_length` (`range [0, π]` → π·r); `tessellate_chord_error` (max sagitta ≤ chord); `mirror_circle_flips_z` (`Affine::scale(-1,1,1)` on world-XY circle gives frame z = −Z and same point set); `non_uniform_scale_circle_is_none`.
- [ ] **Step 2:** Fail, implement, pass. Angles via `libm::atan2`; never `f64::atan2`.
- [ ] **Step 3:** Commit `fluut-geom: Curve3 line and circle`.

### Task 6: B-spline curves (`fluut-geom::bspline`)

**Files:** Create `crates/fluut-geom/src/bspline/mod.rs` (curve type), `bspline/basis.rs`

**Interfaces:**
- Consumes: Tasks 2–5.
- Produces:
  - `struct BSplineCurve3 { degree: u8, knots: Vec<f64>, poles: Vec<Point3>, weights: Option<Vec<f64>>, periodic: bool }` (fields private; getters).
  - `enum BSplineError { BadKnotCount, KnotsDecreasing, BadWeights, DegreeZeroOrTooHigh }`; `fn BSplineCurve3::new(degree, knots, poles, weights) -> Result<Self, BSplineError>` — requires `knots.len == poles.len + degree + 1`, non-decreasing, weights > 0 and same length as poles, `1 ≤ degree ≤ 25`.
  - `basis::find_span(n, p, u, knots) -> usize` (NURBS Book A2.1, with `u == knots[n+1]` returning `n`), `basis::ders_basis_funs(span, u, p, nd, knots) -> SmallVec<[[f64; 26]; 4]>` (A2.3).
  - `impl BSplineCurve3`: `domain -> Interval`, `eval(t)`, `derivs(t, n)` (rational via homogeneous coordinates and the quotient rule, A4.2).
  - `Curve3::BSpline` arms in `eval`, `derivs`, `natural_range`, `transformed` (apply to poles; weights unchanged). `closest` (dense seeds + bracketed Newton), `length` (adaptive 5-point Gauss–Legendre per span) and `tessellate` (adaptive subdivision) for B-splines are basic versions; M1 replaces the closest-point seeding with D02 §3.2 control-polygon projection and adds benchmarks.

- [ ] **Step 1:** Tests: `rejects_bad_knot_count`; `rejects_decreasing_knots`; `linear_bspline_is_polyline` (degree 1, poles (0,0,0),(1,0,0),(1,1,0), knots [0,0,1,2,2]: eval(1.5) = (1,0.5,0)); `eval_at_end_knot_is_last_pole`; `bezier_cubic_matches_bernstein` (knots [0,0,0,0,1,1,1,1], compare to Bernstein formula at 11 points within 1e-15); `derivs_match_finite_difference`; `rational_quarter_circle_exact` (degree 2, poles (1,0,0),(1,1,0),(0,1,0), weights [1, √2/2, 1]: every eval at 101 points has |p| = 1 within 1e-12 — [D02 §9]); `full_rational_circle_matches_circle` (9-pole NURBS circle vs `Curve3::Circle` point set within 1e-12, by closest point on the analytic circle).
- [ ] **Step 2:** Fail, implement, pass.
- [ ] **Step 3:** Run full workspace checks (Task 1 Step 4 commands). All pass.
- [ ] **Step 4:** Commit `fluut-geom: B-spline curves (de Boor, rational)`.

---

## Out of scope for M0 (scheduled in M1)

Ellipse and offset curves; B-spline closest-point seeding per D02 §3.2 and bounds; all surfaces; offsets; plane sections; biarcs; benchmarks; `cargo deny` in CI.
