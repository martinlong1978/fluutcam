# B-spline Geometry Speed Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Cut the cutter drop query ([D07](../design/D07-cutter-queries.md) §8) on dense free-form (B-spline) models, 103-337 µs median per drop after the [drop-speed plan](2026-11-04-drop-bspline-speed.md) against a 20 µs target, by making each B-spline patch solve cheaper (about 40 µs, 1.3-3.6 per drop plus re-solves). Then speed up B-spline closest point (99 µs in M1 against D02 §8's 5 µs, [CH-25](../design/challenges.md)) with what helps. Analytic parts (about 5 µs) must not get slower.

**Architecture:** Measured first (timers around the parts of a patch solve and of a drop, evaluation counters; the instrumentation is not kept). In a patch solve almost all the time was surface evaluation: about 30 points, 55 first-order and 15-150 second-order evaluations per solve at about 350 ns each (`derivs`), the flat-bottom Newton taking 100-140 of them for flat and bull tools. In a drop, after the evaluation was faster, the re-solves of suspect patches took 25-45 %, the edges 15-25 % and the sub-box bounds 5-15 %.

*Bézier form of B-spline surfaces and curves (kept).* `BSplineSurface` and `BSplineCurve3` (degree up to 7) keep every patch or span in Bézier form, made once from the poles by blossoming (de Boor's algorithm with one argument per level). Evaluation finds the span by a binary search over the distinct knots, uses the Bernstein basis (cubic in closed form; derivatives from the basis one and two degrees down) and contracts along v, then u. No knot differences, no divisions, no zeroed work arrays. `derivs` 350 to 77 ns, `derivs1` to 48 ns; results agree with de Boor to 1e-12 (surfaces) and 1e-11 (curves). De Boor stays above degree 7. Memory: 16 points (384 bytes) per bicubic patch, less than the sub-boxes.

*Bounds from the Bézier points (kept, proven).* A patch's box is now the pole box cut by the box of its Bézier points, which also contain it (convex hull, weights positive) and are much tighter. The 8 x 8 sub-boxes of a patch are the boxes of the Bézier points of the surface over each cell (restriction matrices from blossoms), where they were sampled (corner points grown by the doubled sampled second derivative): proven, lower tops in 99 % of cells (by 0.016-0.054 mm on average), fewer patches solved, and faster to build. Both are grown by 1e-13 relative for rounding.

*Proven early exits (kept, same results).* `newton_flat` (the flat-bottom stationary point) stops when a point comes round again: a step is a function of the point alone, so the iteration cycles through steps that all failed the convergence test and cannot converge; 80 % of its 40-iteration failures were such cycles, found after 9 iterations on average. `newton_contact` stops when a line-search trial is the current point (every shorter step clamps or rounds to it too). `sub_bound` skips a cell whose top does not beat the best bound so far (a cell's bound is at most its top) and a cell out of reach on one axis before `hypot`. A converged vertical projection hands back its point instead of evaluating it again. B-spline closest point tests trial points with `eval` and takes `derivs` only when one is accepted. Each is bit-identical (drop_bench raster drops compared bit for bit).

*Re-solves from the best cells (kept).* The suspect-patch rule of the drop-speed plan stays; the patch is now solved again with Newton started from the centres of its (at most 8) cells whose bounds beat the best contact, highest first, instead of a richer seeding (6 x 6 grid, 25 projected targets, 6 starts). Measured on the drop_bench rasters against the old rule: 3 starts missed one drop by 0.019 mm, 5 or more none; 8 kept for margin. Only cells of the boundary strip, and cells without a limit, were tried first: the strip alone missed the same drop.

*Tried and dropped.* A lane layout of the bicubic points (x, y, z apart, four lanes along u): no faster than the plain layout (the build uses SSE2 only). Capping the flat-bottom Newton at fewer iterations: results that raised a face's best contact came from every iteration count up to 40, so a cap is not safe. Warm starts were not retried (no new reason).

**Tech Stack:** as M2. No new dependencies.

**Design:** [`../design/D07-cutter-queries.md`](../design/D07-cutter-queries.md) §8; [`../design/D02-geometry-core.md`](../design/D02-geometry-core.md) §8; [`../design/challenges.md`](../design/challenges.md) CH-25.

## Global Constraints

All earlier Global Constraints apply, and those of the drop-speed plan:

- Results stay the same within the existing tolerance: the S2 reference runs (600 synthetic points, 7 560 corpus points) keep their misses (no new ones), `drop_bench` `VERIFY=1` against the full scan shows no new misses, golden G-code does not change, the unit tests pass.
- A query keeps no state; answers do not depend on the order of queries or the thread count. The Bézier forms are caches made with the surface or curve.
- `drop` keeps its signature. Determinism: no `HashMap`; `libm` for transcendental functions.
- Any bound used for pruning is proven or shown by a sampling test never to exclude surface.

## Review Focus

1. **Bézier boxes contain their surface:** proven by the convex hull property; tests sample every patch (whole and partial) of six surfaces, polynomial and rational, clamped and unclamped, and check each point lies in its patch box and cell box. The 1e-13 growth covers evaluated points that round just outside the hull.
2. **The re-solve rule is a seeding change:** checked by sampling (see After), not proven, as the rule it replaces.
3. **Evaluation bits changed:** drops differ from the base by at most 1.5e-13 mm where they differ at all (before the re-solve change); the solver exits are bit-identical.

---

### Task 1: Measure
- [x] Baseline `drop_bench` (S2_QUICK, best of 3 x 6 000 drops) and `geom` benches; time the parts of a patch solve and of a drop; count evaluations per solve.
- [x] `drop_bench` gains `DUMP=<file>` (every raster drop's tip height, to compare two builds drop by drop) and `VERIFY_SEED`; `s2` gains `S2_ONLY` and `S2_SHARD` (to run its parts in pieces).

### Task 2: Faster evaluation
- [x] Bézier form of surfaces; test against de Boor (clamped, unclamped, double knot, degrees 1 to 5, rational, points outside the domain, at knots).
- [x] Closed-form cubic basis; polynomial derivatives without the homogeneous pass.
- [x] Bézier form of curves; test against de Boor to third derivatives.

### Task 3: Proven early exits
- [x] Cycle exit in `newton_flat`, same-point exit in `newton_contact`, cell skipping in `sub_bound`, projected point reused, `eval` for closest-point trials.

### Task 4: Fewer solves
- [x] Patch boxes from the Bézier points; proven Bézier sub-boxes; tests.
- [x] Re-solves from the best cells.

### Task 5: Documents
- [x] D07 §8, D02 §8, challenges.md CH-25, plan README. `Known-Limits` states no free-form drop speed (only mesh against exact faces, unchanged).

## After

Same rasters as the drop-speed plan (`drop_bench`, `S2_QUICK=1`, release, one thread, best of 3 repetitions of 6 000 drops; old and new binaries alternated twice on the shared machine, best of both). Median / p99 in µs. "Solves" are patch solves per drop (re-solves apart).

| Model | Tool | Before | After | Speed-up | Solves before to after | Re-solves before to after |
|---|---|---|---|---|---|---|
| rough S2 tiles, 5 000 faces | flat 6 | 137 / 562 | 24.6 / 71 | 5.6 | 1.25 to 0.94 | 1.09 to 0.70 |
| | ball 6 | 117 / 365 | 30.0 / 79 | 3.9 | 1.44 to 1.16 | 1.26 to 1.02 |
| | bull 6 r1 | 156 / 593 | 30.9 / 91 | 5.1 | 1.36 to 1.02 | 1.20 to 0.85 |
| sculpted, 4 900 faces of 3 mm | flat 6 | 231 / 655 | 37.5 / 115 | 6.2 | 2.55 to 2.21 | 1.20 to 0.94 |
| | ball 6 | 177 / 392 | 52.1 / 132 | 3.4 | 3.60 to 3.22 | 2.23 to 1.88 |
| | bull 6 r1 | 333 / 920 | 65.9 / 212 | 5.1 | 3.34 to 2.96 | 1.97 to 1.66 |
| sculpted, 256 faces of 12 mm | flat 6 | 128 / 610 | 29.6 / 121 | 4.3 | 2.29 to 2.05 | 0.51 to 0.40 |
| | ball 6 | 101 / 419 | 28.0 / 134 | 3.6 | 2.98 to 2.77 | 0.86 to 0.76 |
| | bull 6 r1 | 178 / 779 | 40.3 / 185 | 4.4 | 2.84 to 2.60 | 0.77 to 0.64 |
| two NIST bodies with most B-spline surfaces | all | 4.9-5.3 / 227-295 | 4.7-5.3 / 130-150 | 1.0 (p99 2) | mostly analytic | |

The drop is 3.4 to 6.2 times faster on free-form models: 25-66 µs median, 1.2 to 3.3 times the 20 µs target. A B-spline patch solve (`s2_cost`, ball, rough tiles) takes 10.3 µs instead of 29.8 µs on the same machine (40 µs in S2); a curved edge 6.4 instead of 8.8 µs. On analytic parts nothing changed: the S2 corpus drops (all 63 bodies) give the same medians (2.1-6.0 µs) and slightly lower means.

`geom` benches (criterion, same machine): `derivs` bicubic 350 to 77 ns, `derivs1` 48 ns (D02 §8 target 250 ns for first order), B-spline closest point 62 to 1.8 µs (99 µs in M1; target 5 µs, met on the bench query); analytic benches unchanged.

Zero misses against the full scan (`drop_bench` `VERIFY=<model>`, 3 000 random points per tool over each model grown by 3 mm): sculpted 4 x 4 and 6 x 6 faces of 12 mm, 12 x 12 faces of 3 mm and the rough tiles 4 x 4, seeds 4242 and 777: 0 below in all 24 tool, model and seed runs (72 000 drops); 1-3 above per run on the rough tiles, flat tool (the re-solve finds contacts the full scan misses). On the drop_bench rasters (54 000 drops) no drop is below the base by more than 1e-7 mm; one (rough tiles, flat) is 0.007 mm above it.

S2 reference runs: 600 synthetic points no misses; 7 560 corpus points 21 misses, the same counts and worst values as before (8 / 8 / 5, worst 9.7e-4, 7.2e-4, 3.3e-4 mm). Workspace tests pass; no golden file changed.

## Open

- The 20 µs target is still 1.2 to 3.3 times away. What is left per drop on the 3 mm sculpted model: patch solves 35-55 %, re-solves 18-33 %, edges 8-11 %, sub-box bounds 6-14 % (instrumented run). Inside a patch solve the flat-bottom Newton (flat and bull tools) is the largest part: its non-cycling failures run all 40 iterations.
- Edge sub-boxes are still sampled (32 segments, linear bound with the sampled second derivative doubled); Bézier subdivision of the spans would make them proven.
- The re-solve still triggers often (0.4-1.9 per drop).
