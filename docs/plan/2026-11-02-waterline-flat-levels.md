# Waterline Flat Levels and Patch Solve Plan (M6 follow-up)

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

**Files:** `crates/fluut-cam/src/{geometry.rs, waterline.rs, strategy/contour.rs, strategy/horizontal.rs}`, `crates/fluut-cam/tests/contour.rs`.

- [ ] **Step 1:** Tests: a flat beside a near-flat ramp with sub-micron model gaps (the 20 mm case); a trace whose backward half closes the loop (the 5 mm case). Both fail before the fix.
- [ ] **Step 2:** Implement; pass; corpus check at 20, 5 and 2 mm; commit.

### Task 3: Patch solve speed

**Files:** `crates/fluut-cam/src/cutter/{solve.rs, geom.rs, drop.rs}`.

- [ ] **Step 1:** Profile the patch solve: where its time goes, per drop and per traced point.
- [ ] **Step 2:** Tests that pin the drops (reference and corpus-like synthetic faces) before changing it.
- [ ] **Step 3:** Cut the cost; measure `contour`, `spiral`, `scallop`; commit.

### Task 4: Close

- [ ] **Step 1:** CH-03, CH-12, D07, D09; timing rows (old rows kept).
- [ ] **Step 2:** Mark this plan done; README row. Merge `main`; all checks; commit.

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

The level of a horizontal planar face is now its plane's Z (exact for a plane normal to Z; the plane's height over the face's boundary for one tilted within the flatness angle). The flat is then outside at z + δ (CH-03), and the faces and edges standing above it are inside, so the contour runs around them.

**5 mm, Z −185.** One contour of 846 is reported lost. Its forward trace stops at an inside corner where a cliff (the top of a drafted wall's edge leaving the footprint) meets a smooth contour. Its backward trace goes the other way round the whole loop and closes at the seed. `trace` then still joined the two halves as an open contour with lost ends. A backward trace that closes now gives the closed contour on its own.
