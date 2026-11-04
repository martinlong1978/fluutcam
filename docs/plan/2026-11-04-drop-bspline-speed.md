# Drop Query on B-spline Models Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Bring the cutter drop query ([D07](../design/D07-cutter-queries.md) §8) on dense free-form (B-spline) models from 170-260 µs median per drop towards the 20 µs target. The [S2 report](spikes/s2-cutter-queries.md) §2 and §5 put the cost at about 5 B-spline patch solves per drop at about 40 µs each, and name warm starts from the previous drop and tighter patch bounds first. Analytic parts (about 5 µs on the corpus) must not get slower.

**Architecture:** Toolpaths query in order, so a drop usually touches the face the previous drop touched, near the same `(u, v)`. A `Warm` memory (the last few face contacts, `drop::Warm`) is passed to `drop_warm`. Before the branch and bound, `drop_warm` runs one Newton solve from each remembered `(u, v)` on its face. A converged contact inside the footprint and the trim is a real point of the face, so its `τ` is a valid lower bound: the search then starts with it as the best contact, and every primitive whose bound cannot beat it by more than `slack` is skipped without a solve. Anything that can be higher still has a bound above it and is solved as before, with the cold seeds. So a warm start only prunes: the answer is the highest contact the cold query finds, or the remembered contact itself (a real contact the cold query would have found by its own Newton start, to the solver tolerance) and never a lower one. With no memory, or a hint that does not converge, the query is the cold query.

Tighter bounds: each B-spline patch primitive keeps a grid of sub-boxes (sampled, grown by the bilinear interpolation bound as the other surface kinds are, and cut by the patch's pole box, which is proven). When a patch is reached in the best-first search its bound is recomputed as the highest bound of its sub-boxes, and it is skipped if that cannot beat the best contact. Further measured steps (cheaper seeding, evaluation cost) are added as the measurements show.

**Tech Stack:** as M2. No new dependencies.

**Design:** [`../design/D07-cutter-queries.md`](../design/D07-cutter-queries.md) §8; [`../design/challenges.md`](../design/challenges.md) CH-25 (S2 entry).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Results stay the same within the existing tolerance: the S2 reference runs (600 synthetic points, 7 560 corpus points) keep their misses (no new ones), the unit tests pass, and golden G-code does not change.
- A query's answer must not depend on the order of queries or the thread count beyond the solver tolerance: the memory is only a hint (the caller owns it, one per thread), a cold query is the fallback, and a test runs the same drops in shuffled order.
- `drop` keeps its signature and behaviour; `drop_warm` is new.
- Determinism: no `HashMap`; `libm` for transcendental functions.

## Review Focus

1. **Warm contacts are real contacts:** the warm Newton result goes through the same footprint and trim tests as a cold one.
2. **Sub-box bounds never exclude surface:** a test samples every B-spline patch and checks each point lies in a sub-box.
3. **Order independence:** shuffled drops against sequential ones, and both against cold.

---

### Task 1: Measure

**Files:** `crates/fluut-cam/examples/drop_bench.rs`.

- [ ] **Step 1:** A benchmark of drops in boustrophedon raster order on the synthetic 5 000-face B-spline model and the corpus bodies with most B-spline surfaces; median, p99, solves per drop; cold against warm. Record the cold numbers here.
- [ ] **Step 2:** Save the S2 reference output (`cargo run -p fluut-cam --example s2 --release` with `FLUUT_STEP_CORPUS`) before any change.

### Task 2: Tests first

**Files:** `crates/fluut-cam/src/cutter/tests.rs`, `crates/fluut-cam/src/cutter/geom.rs`.

- [ ] **Step 1:** Warm drops in sequential and in shuffled order give the heights of the cold drop within 1e-6 and never lower by more than 1e-7; a stale hint on another face is harmless; a warm run solves fewer faces than a cold one.
- [ ] **Step 2:** Sub-boxes contain their patches and are never above the patch box.

### Task 3: Warm starts

**Files:** `crates/fluut-cam/src/cutter/{drop.rs, solve.rs}`.

- [ ] **Step 1:** `Warm`, `drop_warm`, `solve::warm_face`; fail, implement, pass.

### Task 4: Tighter patch bounds

**Files:** `crates/fluut-cam/src/cutter/{geom.rs, drop.rs}`.

- [ ] **Step 1:** Sub-boxes per B-spline patch; refined bound at the leaf; fail, implement, pass.

### Task 5: Anything else the measurements show

- [ ] **Step 1:** Profile what is left, measure each change alone, keep what pays.

### Task 6: Measurements and documents

**Files:** this plan, D07 §8, `docs/design/challenges.md`, `docs/plan/README.md`.

- [ ] **Step 1:** Re-run the benchmark and the S2 references; record before and after; workspace checks; merge `main`; commit.

## Before (Task 1)

Windows, release, one thread, boustrophedon raster of 500 x 120 drops over the synthetic 5 000-face B-spline model (60 000 primitives), cold:

| Tool | Median (µs) | p99 (µs) | Faces solved per drop | Newton runs per drop |
|---|---|---|---|---|
| flat 6 | 238 | 852 | 5.07 | 30.4 |
| ball 6 | 167 | 480 | 5.31 | 15.9 |
| bull 6 r1 | 256 | 786 | 5.38 | 32.3 |

The corpus bodies with B-spline surfaces (at most 34 of 663 faces) are mostly analytic and run at about 5.5 µs median, 150 µs p99 (the p99 is edges and rims, not patches); they are in the benchmark to show they do not get slower.

S2 references before: 600 synthetic points, no misses; 7 560 corpus points, 21 misses by more than 1e-7 (worst 9.7e-4 mm, trim chord and model gaps, as in the S2 report), 5 of them with 1e-6 mm trims.
