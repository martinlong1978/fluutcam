# Waterline Flat Levels and Patch Solve Plan (M6 follow-up)

**Status: done (2026-11-03).** See [Done](#done).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The open issues of the [waterline robustness plan](2026-11-01-waterline-robust.md):
- no `cam.contour.loop_open` on the largest NIST body (`nist_ctc_02_asme1_rc`) at 20, 5 and 2 mm step-down: 4 traces were lost at the flat level Z −289.9997 at 20 mm, and 1 contour of 846 at 5 mm;
- less time per drop in the B-spline patch solve (about 160 µs, the largest cost per drop), with results bit-identical or provably within 10⁻⁷ mm, and deterministic.

**Architecture:**
- Diagnose on the corpus body with the waterline example and small scratch probes; rebuild each cause as a small synthetic test.
- Fix each cause where it is: the level (`strategy/contour.rs`, `strategy/horizontal.rs`), the tracer (`waterline.rs`) or the drop (`cutter`).
- Speed: profile the patch solve, then cut its cost without changing which contact wins.

**Tech Stack:** as M6. No new crates.

**Design:** [D09](../design/D09-strategies.md) §4.4; [D07](../design/D07-cutter-queries.md) §3, §4.3, §8; [challenges](../design/challenges.md) CH-03, CH-12; [M6 part 2 plan](2026-11-01-m6-3d-finishing.md).

## Global Constraints

All M6 Global Constraints apply. In addition:

- Tests first. The exactness tests stay: closed forms within 1e-6 mm, the CH-03 wall foot, no gouges.
- Corpus geometry stays outside the repository; failures found on it are rebuilt as small synthetic cases.
- Golden G-code changes only with a reason recorded here, and only when the new output is as good or better. The CLI simulation test stays clean with no new allowed findings. Translation invariance holds.
- Determinism (D01 §6): `libm`, `BTreeMap` where order reaches output, results the same on every platform and thread count.

## Review Focus

1. The diagnosis: is each cause shown on the corpus, and does its synthetic test fail without the fix?
2. Drops stay exact: a faster patch solve returns the same contact (or one within 10⁻⁷ mm).
3. Speed numbers measured the same way as the earlier timing tables.

---

### Task 1: Plan and diagnosis

- [x] **Step 1:** This plan; README row.
- [x] **Step 2:** Reproduce the lost traces at 20 and 5 mm; find the causes (below).

### Task 2: Fix the lost traces

**Files:** `crates/fluut-cam/src/{geometry.rs, waterline.rs, surface.rs, strategy/contour.rs, strategy/horizontal.rs}`, `crates/fluut-cam/tests/contour.rs`.

- [x] **Step 1:** Tests: a flat beside a near-flat ramp with sub-micron model gaps (the 20 mm case); a trace whose backward half closes the loop (the 5 mm case); lost ends that meet, a lost re-trace of a closed loop and an overhanging wall (the 2 mm cases). Each fails before its fix.
- [x] **Step 2:** Implement; pass; corpus check at 20, 5 and 2 mm; commit.

### Task 3: Patch solve speed

**Files:** `crates/fluut-cam/src/cutter/solve.rs`, `crates/fluut-geom/src/surface/{bspline.rs, mod.rs}`.

- [x] **Step 1:** Profile the patch solve: where its time goes, per drop and per traced point.
- [x] **Step 2:** Tests that pin the drops (reference and corpus-like synthetic faces) before changing it.
- [x] **Step 3:** Cut the cost; measure `contour`, `spiral`, `scallop`; commit.

### Task 4: Close

- [x] **Step 1:** CH-03, CH-12, D07, D09; timing rows (old rows kept).
- [x] **Step 2:** Mark this plan done; README row. Merge `main`; all checks; commit.

## Diagnosis (Task 1)

Run with `cargo run -p fluut-cam --example waterline --release`, `FLUUT_STEP_CORPUS` set, and `FLUUT_LEVELS` or `FLUUT_STEP_DOWN`.

**20 mm, the flat level Z −289.9997.** The flat face is the plane Z −290. Its level was the top of its box (`face_z_range`), which takes its edges: B-spline edges that stand up to 0.33 µm above the plane, inside the model tolerance (0.01 mm in this file). So the level lay 0.3 µm above the flat, inside the model's gaps:

| Level | Contours | Lost |
|---|---|---|
| −290.0003 | 8 | 0 |
| −290 | 8 | 0 |
| −289.9999 | 9 | 2 |
| −289.9998 | 13 | 6 |
| −289.9997 | 11 | 4 |
| −289.9995 | 11 | 4 |
| −289.9993 | 9 | 2 |
| −289.999 | 8 | 0 |

At the lost ends two near-flat B-spline faces (571, 572) meet the flat. Their surfaces stand about 0.3 µm above their own boundary edges. A ball over the flat next to such a face misses it: the face's tangent point is trimmed away, and the gap is below the trim chord (10⁻³ mm), so the boundary search of the earlier plan does not count it. τ steps up by 0.3 µm where the face's tangent point enters its trim. A level inside that step makes the contour follow the step. There the contact of the probe just inside is on a face that rises along the step, not across it, so the contact direction the tracer steps by is parallel to the contour; the trace stalls at its smallest step and is lost.

The `contour` level of a horizontal planar face is now its plane's Z (exact for a plane normal to Z; the plane's height over the face's boundary for one tilted within the flatness angle). The flat is then outside at z + δ (CH-03), and the faces and edges standing above it are inside, so the contour runs around them.

**5 mm, Z −185.** One contour of 846 is reported lost. Its forward trace stops at an inside corner where a cliff (the top of a drafted wall's edge leaving the footprint) meets a smooth contour. Its backward trace goes the other way round the whole loop and closes at the seed. `trace` then still joined the two halves as an open contour with lost ends. A backward trace that closes now gives the closed contour on its own.

**2 mm.** Two more contours, at levels between flats that the 5 mm run did not hit, and one cut-off:
- Z −194.384: both halves of a trace stopped at the same inside corner, 0.14 µm apart. A trace lost at both ends whose ends are within tol/4, with the gap between them outside the material, is now closed across the gap.
- Z −232.232: a trace stopped at two inside corners where a cliff meets a smooth contour; a seed later on the level traced the whole loop through them. A lost trace that lies on a closed contour of the same level (within tol/2) is now dropped as a re-trace.
- Z −142.592: not a lost trace. At a cliff the slope comes from the wall at the level (`wall_at`), and a wall leaning out by a little gave more than 90°, past the default 0–90° limits, so part of the contour was linked over with `loop_open`. An overhanging wall now counts as vertical.

Looking back from −n in the circle search (to turn more than a right angle at such corners) was tried; it did not close Z −232.232 and cost drops, so it was left out.

## Speed (Task 3)

Measured with a scratch harness outside the repository: 20 000 ball drops (Ø6) on short lines near the 34 B-spline faces of the NIST body, then every B-spline patch in the footprint of the first 4 000 axes solved alone (20 542 solves). Counts per patch solve: about 70 surface evaluations in the vertical projections of the seed targets, 16 for the 4 × 4 sample, and 37 contact-map evaluations in Newton, three quarters of them trial points that the line search rejects. Time per solve: projections 20 µs, Newton 16 µs, sample 2 µs (after the change).

Changes, all with the same bits as before:
- `BSplineSurface::eval` (point only) and `derivs1` (point and first derivatives) share the code of `derivs` up to the order they need. `Surface::eval` and `derivs1` use them; a test checks the bits against `derivs`, rational surfaces and degree 9 included.
- Vertical projections use first derivatives.
- Newton's line search tests a trial point with `G` alone (first derivatives, the same normal code) and computes the Jacobian only for an accepted step. A test checks `G` bit for bit against the contact map for ball, bull and flat tools.

Not done: caching the 4 × 4 samples per patch (2 µs), and better starts or a tighter patch bound. Those change which seeds or primitives are tried, and so the bits of some drops.

| Measure (largest NIST body) | Before (69b7f50) | After (3f233bb) |
|---|---|---|
| B-spline patch solve | 51–57 µs | 39.5–40 µs |
| Drop near B-spline faces (0.48 patch solves per drop) | 53.8–54.7 µs | 45.8–46.3 µs |
| Drop hash, patch-solve hash | equal | equal |
| `contour` traced point, 5 levels (Z −40 … −280), drops per point 5.9–7.5 | 682 µs | 622 µs |
| `contour` 5 mm, 12 threads | 60.5 s, 226 µs/point | 52.5 s, 196 µs/point |
| `spiral` stepover 2, 12 threads | 41.3–41.8 s, 149–151 µs/point | 40.4 s, 145–146 µs/point |
| `scallop` stepover 2, 12 threads | 334.6 s, 457 µs/point | 317.6 s, 434 µs/point |

The machine was shared, so single runs vary by up to about 20 %; the drop and solve figures are pairs of runs. `spiral` and `scallop` drops are mostly on analytic faces, so they gain little. Point counts did not change in any run.

## Done

Lost and cut-off waterline traces on the largest NIST body had four causes: flat levels inside the model's gaps (the level was the top of the flat's edges, not its plane), a backward trace that closed but was joined as open, traces that stopped at inside corners where a cliff meets a smooth contour, and overhanging walls reading as steeper than the slope limit. All are fixed with synthetic tests. The B-spline patch solve is about 30 % faster with every drop bit-identical.

**Corpus** (`nist_ctc_02_asme1_rc`, Ø6 ball, tolerance 0.01, every face a drive face, `examples/finishing.rs`):

| Run | Before (a512761) | After |
|---|---|---|
| `contour` 20 mm, `loop_open` | yes (4 traces at Z −289.9997) | no |
| `contour` 5 mm, `loop_open` (waterline example: lost contours) | no (1 of 846) | no (0 of 846) |
| `contour` 2 mm, `loop_open` | yes | no |
| `horizontal` Ø6 flat, stepover 3 | 4 678 points | 4 678 points (319.9 s, 12 threads) |
| `steep_and_shallow` step-down 20, stepover 2, 12 threads | 193 743 points, 67.5 s | 190 551 points, 90.0 s, no diagnostics (the flat level moved to the plane; the machine was busy) |

Golden G-code: no golden file changed. The reference projects have no flats with edges off their plane, no lost traces and no overhanging walls. The CLI simulation test is unchanged, with no new allowed findings; translation invariance holds.

Deviations from this plan:
- **More causes than planned.** The 2 mm run had three more (above), found and fixed in the same way.
- **`horizontal` keeps the top of the flat's edges.** With the plane's Z, the flat's own edges 0.3 µm above it hold a flat tool up within R of them, and on the NIST body the walls covered the flat at Z −290: 825 points and 56 m of cutting were lost. Its wall waterline belongs above the flat's edges.

Open:
- A flat or bull tool at a flat `contour` level is also held up within R of edges that stand above the plane, so the contour there can run R from such edges. Seen with a ball only so far (ribbons 0.04 mm wide).
- The tracer still stops at some inside corners where a cliff meets a smooth contour; the closing rules above catch the cases seen.
- The patch solve's vertical projections (half its time) and samples could be cached or seeded from neighbouring queries, at the cost of bit-identity.
