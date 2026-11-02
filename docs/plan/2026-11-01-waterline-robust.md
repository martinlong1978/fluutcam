# Waterline Robustness and Speed Plan (M6 follow-up)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The two waterline follow-ups of the [M6 part 2 plan](2026-11-01-m6-3d-finishing.md):
- no lost traces (`cam.contour.loop_open`) on contours that really close, on the largest NIST body at 20 mm and 5 mm step-down;
- at least 3 times less time per traced point for `contour`, and `contour` at a 2 mm step-down finishing on that body.

**Architecture:**
- Find the cause of the lost traces first, with a diagnostic example on the corpus body and small synthetic regression cases in the repository.
- Fix the cause where it is: in the drop query (`fluut-cam::cutter`) if the drops are wrong, in the tracer (`fluut-cam::waterline`) if the tracing is.
- Speed: cut the drops per traced point (a better predictor and corrector in the tracer) and the cost per drop (drops that only decide a side of the level can stop early).
- Levels in parallel only if D01 §6 allows it and the output stays bit-identical.

**Tech Stack:** as M6. No new crates.

**Design:** [D09](../design/D09-strategies.md) §4.4; [D07](../design/D07-cutter-queries.md) §1, §3, §7; [D01](../design/D01-workspace-architecture.md) §6; [challenges](../design/challenges.md) CH-03, CH-12; [M6 part 2 plan](2026-11-01-m6-3d-finishing.md).

## Global Constraints

All M6 Global Constraints apply. In addition:

- Tests first. The exactness tests stay: cylinder, sphere and box contours within 1e-6 mm of the closed forms, the CH-03 wall foot, and no gouges.
- Corpus geometry stays outside the repository. Failures found on the corpus are rebuilt as small synthetic cases.
- Golden G-code changes only with a reason recorded here, and only when the new output is as good or better (no gouge, within tolerance). Translation invariance holds.
- Determinism (D01 §6): `libm`, `BTreeMap` where order reaches output, results the same on every platform and thread count.

## Review Focus

1. The diagnosis: is the cause proven on the corpus, and does the synthetic case fail without the fix?
2. Drops stay safe: a drop result is a real point of the geometry in the footprint (D07), never too high.
3. Speed numbers measured the same way as the M6 timing table.

---

### Task 1: Plan and diagnostics

- [x] **Step 1:** This plan; README row.
- [x] **Step 2:** `examples/waterline.rs`: per level on the largest corpus body, the contours, the lost ones, points, drops and time per point.
- [x] **Step 3:** Reproduce the losses; find the cause (below).

### Task 2: Fix the cause

**Files:** `crates/fluut-cam/src/cutter/{drop.rs, geom.rs, solve.rs, trim.rs, tests.rs}`.

- [x] **Step 1:** Tests: a drafted cylinder band whose top edge is 3 µm off the surface. Ball drops across the top of the wall have no jumps and are within 1e-3 mm of the brute-force reference; the boundary search finds contacts between two of its points.
- [x] **Step 2:** Implement; pass; corpus check; commit.

### Task 3: Speed

**Files:** `crates/fluut-cam/src/{waterline.rs, surface.rs}`, `cutter/drop.rs`.

- [x] **Step 1:** Profile: drops per point by phase (predict, bracket, bisect, closure), cost per drop.
- [x] **Step 2:** Tests that pin the contour geometry (closed forms, CH-03, no gouge) before changing the tracer.
- [x] **Step 3:** Cut the drops per point and the cost per drop; measure; commit.
- [x] **Step 4:** Levels in parallel, if D01 §6 allows it and the output is bit-identical.

### Task 4: Close

- [x] **Step 1:** Corpus runs at 20, 5 and 2 mm step-down; timing rows.
- [x] **Step 2:** CH-12 and CH-03 in `challenges.md`, D09 §4.4, D07 if the drop changed. Mark this plan done with deviations; README row. Merge `main`; all checks; commit.

## Diagnosis (Task 1)

Run with `cargo run -p fluut-cam --example waterline --release`, `FLUUT_STEP_CORPUS` set; `FLUUT_STEP_DOWN` (default 20) or `FLUUT_LEVELS=z1,z2` choose the levels, `FLUUT_VERBOSE` lists the lost contours.

**Before** (body `nist_ctc_02_asme1_rc`, Ø6 ball, tolerance 0.01, every face a drive face, 20 mm step-down, levels from Z 20 to −360): 391 contours, 188 lost. The losses are at Z −20 to −160. Z −20 had 70 contours, 31 lost, and 14 of them ran to 20 000 points or more; that level alone took 209 s.

**Where the traces end.** Every lost end is the tracer stuck at its smallest step with nothing found, neither along the contact direction nor by the circle search. Both directions of a trace stop at the same few points, and the same points recur at every level from Z −20 to −160, so they belong to vertical features. Each stuck point is on a cliff: the probe just inside is at Z −2.9 while the level is −40.

**The drops are wrong there.** A map of the level classification around a stuck point shows a crack: a line one sample wide (at 0.3 µm spacing) where τ ≤ z, inside material where τ ≈ −2.9. On the crack the drop gives τ = −131.4, and the brute-force reference (D07 §9) gives −2.862. The full scan without pruning gives −131.4 too, so the solver misses the contact, not the search.

**Why.** The face is a cylinder of radius 25 whose axis is tilted 2° (a drafted hole wall), cut off at Z 0 by a B-spline edge. A Ø6 ball beside it touches the near-vertical wall at the rim of its footprint.
- The face solver's closed form gives the tangent point at Z +0.0009, just above the face, so the trim rejects it. The highest point of the face in the footprint is then on its boundary, and the edge solver is meant to cover that.
- But the edge curve is 3 µm off the surface (a model gap), and on the crack it is 3.000014 mm from the axis: just out of reach.
- On a near-vertical wall 3 µm of gap is a band of axis positions about 3 µm wide where nothing is found near the top of the wall, and the drop falls to a contact 128 mm lower.

So the cause is a drop miss (CH-12), on an analytic face, not on free-form faces as the M6 plan guessed. The tracer's step-and-correct logic, seams and the circle search are not at fault: they follow τ, and τ is wrong in a band thinner than any step. A synthetic drafted band with a 3 µm edge gap shows the same: drops fall about 38 mm over a band up to 3 µm wide.

**Fix (Task 2).** A face whose best stationary point is outside its trim searches its own boundary: the trim loops mapped onto the surface (`QueryFace::rim`), each segment bounded from its ends and refined by Brent's method, with the footprint crossings found by bisection. Every point is a real point of the face, so the drop is never too high. The boundary result is used only when it beats every other contact by more than the trim chord (1e-3 mm): that is where the edges missed it. Elsewhere the edge contact wins as before, so models without gaps give the same drops, and no golden file changed.

**After the fix** at Z −20, −40, −140 and −160 (before: 31, 27, 16 and 6 lost): 40, 8, 9 and 10 contours, all closed. The runaway contours at Z −20 are gone; they were traces hopping across the cracks.

## Speed (Task 3)

Profiled on the corpus body, per level (Z −40 to −300, closed contours only): 22–31 drops per traced point, 120–200 µs per drop, about 2.6–5.4 ms per traced point (the machine was shared, so times varied by up to 2×; drop counts did not). Three quarters of the points are on cliffs: a ball beside a vertical wall rests on its top until the wall leaves the footprint, so τ jumps there and only bisection worked. Most of a drop's time is B-spline patch solves (about 160 µs each) and edge solves.

- **Windowed drops.** `DropOptions::ceiling` stops the search at the first contact at or above it; with `floor`, a classifying drop is exact within 4 max steps of the level and cheap beyond it. The side is the full drop's (tested on the tile model).
- **Newton corrector.** τ rises at tan β along the contact direction, so from an exact probe the root is about −(τ − z)/tan β away; a ±5 % bracket around it, then regula falsi. 4.6–5 drops per point where it applies.
- **Cliff steps.** At a cliff the inside contact P holds the tip up until it leaves the footprint, so the next trial is just past |axis − P| = R. Outside, the bracket closes at once; inside, its contact gives the next trial. On a steep wall (not a cliff) the contact slides and the trials barely shrink, so cliff steps stop when a move is not under half the one before.
- **Arc predictor** along the last step's curvature, and steps capped by the chord rule ahead (fewer rejected steps).
- **Parallel.** Seed lines and levels run on `rayon`, collected in order (D01 §6). Seeding: 54–66 s → 8–10 s on 12 threads.

Result: 6–7.5 drops per traced point (was 22–31), and 0.3–0.95 ms per traced point on one thread at the same levels.

## Done

Lost traces were drop misses at model gaps on drafted walls (CH-12), fixed in the drop. The tracer takes 6–7.5 drops per point instead of 22–31, levels and seed lines run in parallel, and `contour` at a 2 mm step-down finishes on the largest NIST body. The 3-D finishing linker keeps the rapid floor.

**Corpus** (`nist_ctc_02_asme1_rc`, Ø6 ball, tolerance 0.01, every face a drive face; `examples/finishing.rs` and `examples/waterline.rs`):

| Run | Before (bc624ba) | After |
|---|---|---|
| `contour` 20 mm, lost contours (waterline example) | 188 of 391 | 0 of 196 |
| `contour` 20 mm, `loop_open` in the toolpath | yes | yes: 4 traces at the flat level Z −289.9997 |
| `contour` 5 mm, lost contours (waterline example) | not run | 1 of 846 |
| `contour` 20 mm, one thread | 772.5 s, 523 261 points, 1 476 µs/point | 143.5 s, 79 288 points, 1 810 µs/point |
| `contour` 20 mm, 12 threads | — | 35.9 s, 452 µs/point |
| `contour` 5 mm, 12 threads | — | 58.0 s, 271 098 points, 214 µs/point |
| `contour` 2 mm | stopped after 57 min | 781.2 s one thread (1 189 µs/point); 138.7 s on 12 threads |
| `steep_and_shallow` 20 mm | 958.3 s | 67.5 s (12 threads) |
| `horizontal` Ø6 flat | 1 212.3 s | 338.6 s (12 threads) |

The time per output point at 20 mm looks worse on one thread, but the old figure was flattered: 450 000 of its 523 261 points were runaway traces (contours re-traced across the cracks), which cost little per point. The honest comparison is total time (5.4× less on one thread, for the same contours closed rather than lost) and the time per traced point at the same levels (about 3–8× less). The 3× per-point target is met per traced point, not per output point at 20 mm.

Deviations from this plan:
- **Root cause in the drop, not the tracer.** The fix is in `cutter` (face boundaries, D07); the tracer only got faster.
- **Slivers.** A lost contour around material under tol/4 high for a step inwards is dropped (CH-03): at Z 0 the drafted walls' top edges stand up to 0.9 µm above the flat they meet.
- **Rapid floor in `follow.rs`** (asked for during the work, after `sim-findings`): lifts between runs go at least `safety_distance` above the material top, descents stop there and plunge. The CLI simulation allowance for `dome` t5 is removed.
- **Golden G-code.** `dome-linuxcnc.ngc` changed twice. (1) The tracer: waterline and flat points moved along the same contours, by up to 0.001 mm in the printed coordinates; the same point count in every section, every point on the contour within 10⁻⁷ mm (closed-form tests within 10⁻⁶), the chord rule and the no-gouge tests unchanged. (2) The rapid floor: lifts at Z 0.095 and a rapid down to Z −17.989 became lifts and descents to Z 1 (stock top + safety), then feed. Both are as good or safer. No other golden file changed; translation invariance holds.
- **Not done:** sharing the B-spline seeding between neighbouring drops (the largest remaining cost per drop), and arcs from analytic contacts.

Open:
- The flat level at Z −289.9997 at 20 mm keeps 4 lost traces on a real contour (closed at Z −289.999); a tangent-level case not yet diagnosed.
- One lost contour of 846 at 5 mm.
- `horizontal` still spends most of its time in waterlines at flat heights, which are tangent levels.
