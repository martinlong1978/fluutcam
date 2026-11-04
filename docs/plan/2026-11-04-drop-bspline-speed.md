# Drop Query on B-spline Models Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Bring the cutter drop query ([D07](../design/D07-cutter-queries.md) §8) on dense free-form (B-spline) models from 170-260 µs median per drop towards the 20 µs target. The [S2 report](spikes/s2-cutter-queries.md) §2 and §5 put the cost at about 5 B-spline patch solves per drop at about 40 µs each, and name warm starts from the previous drop and tighter patch bounds first. Analytic parts (about 5 µs on the corpus) must not get slower.

**Architecture:** Two ideas were planned; measurement kept one.

*Tighter bounds (kept).* Each B-spline patch primitive and each B-spline edge keeps a grid of sub-boxes (8 x 8 per patch, 32 segments per edge): the box of the cell's corner points grown by the bilinear (linear) interpolation bound, with the second derivative sampled and doubled as `sampled_box` already does, cut by the pole box (proven), stored as `f32` rounded outwards (`SubBox`: XY extent and top Z, 20 bytes). `Bvh::best_first_refined` takes a second bound for items: when an item reaches the front of the queue it goes back under `sub_bound` (the highest bound of its sub-boxes) if that is lower, so primitives are solved in the order of the tight bound and the ones that cannot beat the best contact are never solved.

*Warm starts (tried, not kept).* A `Warm` memory of the last face contacts, refined by one Newton run each before the search, giving a real contact whose height is a floor for the branch and bound. It never changed the number of solves, even with an oracle floor (the exact answer's face and parameters as the hint): with the refined order the patch holding the contact is always solved first, and the others are bounded by their sub-boxes, not by order. The patch holding the contact cannot be skipped, because a bound cannot be tighter than the cell size at a maximum (the bound is first order in the cell, the contact second order), so the cold solve is what certifies it. The hint cost 1-3 Newton runs per drop for no gain, so it was removed. Seeding the 17 target projections from the previous drop was estimated at about 2 us per solve (a projection is 2-3 iterations of 0.2 us) and not built.

*Rim contacts (found on the way).* Newton accepts a residual below 1e-8, which leaves a flat tool's rim contact up to 1e-8 outside the footprint, and the footprint test (1e-9) then turned a true contact away depending on where Newton stopped. Redundant solves of neighbouring patches hid it; with pruning it showed (7 of 12 000 smooth-face drops low by 0.4 to 24 um). `newton_contact` now takes one more step with the Jacobian it has (no extra evaluation).

**Tech Stack:** as M2. No new dependencies.

**Design:** [`../design/D07-cutter-queries.md`](../design/D07-cutter-queries.md) §8; [`../design/challenges.md`](../design/challenges.md) CH-25 (S2 entry).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Results stay the same within the existing tolerance: the S2 reference runs (600 synthetic points, 7 560 corpus points) keep their misses (no new ones), the unit tests pass, and golden G-code does not change.
- A query's answer must not depend on the order of queries or the thread count: the query keeps no state, and a test runs the same drops in shuffled order.
- `drop` keeps its signature.
- Determinism: no `HashMap`; `libm` for transcendental functions.

## Review Focus

1. **Sub-boxes never exclude surface:** tests sample every patch and edge of two models and check each point lies in a sub-box. The second-derivative bound is sampled and doubled (as `sampled_box`), not proven; the cut by the pole box is proven.
2. **Pruning exposes weak patch solves:** a patch whose own solve misses a contact next to a face boundary used to be rescued by Newton runs from neighbouring patches. See Open below.
3. **Memory:** sub-boxes cost 20 bytes x 64 = 1.3 kB per B-spline patch and 640 bytes per B-spline edge, about ten times the primitive's own box. A model of 100 000 patches takes about 130 MB more.

---

### Task 1: Measure
- [x] Benchmark `drop_bench` (boustrophedon raster, best of repetitions; the machine is shared). Added `synth::sculpted`, a smooth free-form model (the S2 tiles are random +-1 mm over 2.4 mm faces, which no real model is), and a `VERIFY=1` mode against the full scan.
- [x] S2 reference output saved before any change.

### Task 2: Tests first
- [x] Smooth B-spline drops equal the full scan, in shuffled order too; rim contacts of a flat tool (four points that were low); patch and edge sub-boxes contain their geometry; `best_first_refined` visits in the refined order.

### Task 3: Warm starts
- [x] Built, measured, removed (see Architecture).

### Task 4: Tighter bounds
- [x] Sub-boxes and `best_first_refined`. Sub-box grid measured (sculpted, ball, patch solves per drop): 3: 6.3, 4: 5.2, 6: 4.1, 8: 3.6, 12: 3.0, 24: 2.3; time is best at 8 (finer grids cost more in bound tests than they save). Edge segments, edge solves per drop: 4: 4.1, 8: 2.8, 16: 1.8, 32: 1.2.

### Task 5: Documents
- [x] D07 section 8, challenges.md, plan README.

## After

Same raster, release, one thread, best of repetitions, old and new binaries alternated on the shared machine (quick run, 6 000 drops). Median / p99 in us; solves are numeric face solves per drop.

| Model | Tool | Before | After | Solves |
|---|---|---|---|---|
| rough S2 tiles, 5 000 faces | flat 6 | 248 / 859 | 65 / 250 | 5.22 to 1.25 |
| | ball 6 | 170 / 467 | 57 / 160 | 5.34 to 1.44 |
| | bull 6 r1 | 266 / 875 | 74 / 272 | 5.40 to 1.35 |
| sculpted, 4 900 faces of 3 mm | flat 6 | 370 / 968 | 136 / 312 | 6.92 to 2.55 |
| | ball 6 | 255 / 558 | 91 / 191 | 11.1 to 3.60 |
| | bull 6 r1 | 488 / 1073 | 185 / 408 | 9.15 to 3.34 |
| sculpted, 256 faces of 12 mm | flat 6 | 607 / 1577 | 128 / 326 | 12.5 to 2.29 |
| | ball 6 | 345 / 593 | 97 / 236 | 15.7 to 2.98 |
| | bull 6 r1 | 678 / 1587 | 153 / 394 | 14.3 to 2.84 |
| two NIST bodies with most B-spline surfaces | all | 5.5 / 150 | 5.3 / 100-135 | unchanged (mostly analytic) |

A 2.5 to 4.5 times faster drop on free-form models; the 20 us target is not reached (about 20 us per patch solve remains, and the contact's own patch must be solved). Edge solves went from 2.6-5.5 to 0.5-1.2 per drop. Build time is unchanged.

S2 reference runs, after: 600 synthetic points no misses; 7 560 corpus points 21 misses, the same counts and worst values as before (8 / 8 / 5, worst 9.7e-4, 7.2e-4, 3.3e-4 mm). Workspace tests pass (1 180 in the first full run); no golden file changed.

## Open

- A contact within about 0.05 mm of a face boundary on a large smooth face can be missed by a flat tool when the contact's own patch solve fails (all three Newton starts end elsewhere or fail) and no neighbour patch is solved any more: 1 in 300 flat drops on 12 mm faces, at (14.998, 47.477) of `sculpted(4, 12, 9)`; 0 of 300 before. Starts that end where an earlier one did, replaced by fresh ones, did not fix it and cost 20-80 % more Newton runs; not kept. Needs better starts near boundaries.
- Warm starts: see Architecture. A floor cannot help; a restricted certification of the contact's patch would need a second-order argument.
