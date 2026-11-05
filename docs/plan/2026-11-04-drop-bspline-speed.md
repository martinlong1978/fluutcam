# Drop Query on B-spline Models Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Bring the cutter drop query ([D07](../design/D07-cutter-queries.md) §8) on dense free-form (B-spline) models from 170-260 µs median per drop towards the 20 µs target. The [S2 report](spikes/s2-cutter-queries.md) §2 and §5 put the cost at about 5 B-spline patch solves per drop at about 40 µs each, and name warm starts from the previous drop and tighter patch bounds first. Analytic parts (about 5 µs on the corpus) must not get slower.

**Architecture:** Two ideas were planned; measurement kept one.

*Tighter bounds (kept).* Each B-spline patch primitive and each B-spline edge keeps a grid of sub-boxes (8 x 8 per patch, 32 segments per edge): the box of the cell's corner points grown by the bilinear (linear) interpolation bound, with the second derivative sampled and doubled as `sampled_box` already does, cut by the pole box (proven), stored as `f32` rounded outwards (`SubBox`: XY extent and top Z, 20 bytes). `Bvh::best_first_refined` takes a second bound for items: when an item reaches the front of the queue it goes back under `sub_bound` (the highest bound of its sub-boxes) if that is lower, so primitives are solved in the order of the tight bound and the ones that cannot beat the best contact are never solved.

*Warm starts (tried, not kept).* A `Warm` memory of the last face contacts, refined by one Newton run each before the search, giving a real contact whose height is a floor for the branch and bound. It never changed the number of solves, even with an oracle floor (the exact answer's face and parameters as the hint): with the refined order the patch holding the contact is always solved first, and the others are bounded by their sub-boxes, not by order. The patch holding the contact cannot be skipped, because a bound cannot be tighter than the cell size at a maximum (the bound is first order in the cell, the contact second order), so the cold solve is what certifies it. The hint cost 1-3 Newton runs per drop for no gain, so it was removed. Seeding the 17 target projections from the previous drop was estimated at about 2 us per solve (a projection is 2-3 iterations of 0.2 us) and not built.

*Suspect patches solved again (added after review).* Pruning removed the accidental cover that neighbouring patch solves gave a patch whose own Newton starts fail beside a face boundary (a contact within about 0.05 mm of the boundary, a flat tool: all starts fail or stop short). Two such drops were low by 1.3e-2 and 1.7e-2 mm (a gouge). Rule: after the search, a solved patch that touches the boundary of its face, gave nothing as high as the best contact, and has a boundary-strip sub-box (`face_edge_bound`) above the best contact is solved again with richer seeds (grid +2, rings +1, twice the starts). Tried first and dropped: solving the neighbouring patches (whose bound is often -inf, the rescue was by chance), a suspect rule on every patch (2-10 extra solves per drop), and one on the best seed's height (the seeds were 0.3 mm low in the miss).

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
2. **Pruning exposes weak patch solves:** a patch whose own solve misses a contact next to a face boundary used to be rescued by Newton runs from neighbouring patches; now it is solved again (see Architecture).
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

Same raster, release, one thread, best of 3 repetitions of 6 000 drops, old and new binaries alternated on the shared machine. Median / p99 in us. "Solves" are numeric patch solves per drop, "rescues" the patches solved again.

| Model | Tool | Before | After | Solves before to after | Rescues |
|---|---|---|---|---|---|
| rough S2 tiles, 5 000 faces | flat 6 | 248 / 859 | 137 / 576 | 5.22 to 1.25 | 1.1 |
| | ball 6 | 170 / 467 | 120 / 371 | 5.34 to 1.44 | 1.3 |
| | bull 6 r1 | 266 / 875 | 157 / 595 | 5.40 to 1.35 | 1.2 |
| sculpted, 4 900 faces of 3 mm | flat 6 | 370 / 968 | 238 / 679 | 6.92 to 2.55 | 1.2 |
| | ball 6 | 255 / 558 | 184 / 406 | 11.1 to 3.60 | 2.2 |
| | bull 6 r1 | 488 / 1073 | 337 / 929 | 9.15 to 3.34 | 2.0 |
| sculpted, 256 faces of 12 mm | flat 6 | 607 / 1577 | 130 / 631 | 12.5 to 2.29 | 0.5 |
| | ball 6 | 345 / 593 | 103 / 427 | 15.7 to 2.98 | 0.9 |
| | bull 6 r1 | 678 / 1587 | 180 / 807 | 14.3 to 2.84 | 0.8 |
| two NIST bodies with most B-spline surfaces | all | 5.5 / 150 | 5.3 / 100-135 | unchanged (mostly analytic) | 0 |

The drop is 1.4 to 3.8 times faster on free-form models (the 20 us target is not reached). Without the re-solves it was 2.5 to 4.5 times faster; the re-solves cost 20-40 % and are not rare (0.5-2.2 per drop), because on small faces every patch touches a face boundary. A cheaper trigger that still catches both misses was not found. Edge solves went from 2.6-5.5 to 0.5-1.2 per drop.

Zero misses against the full scan (`drop_bench` with `VERIFY=<model>`, 3 000 random points per tool over each model grown by 3 mm, solving every primitive for the reference): sculpted 4 x 4 faces of 12 mm, 6 x 6 faces of 12 mm, 12 x 12 faces of 3 mm and the S2 tiles 4 x 4 (rough): 0 below on all 12 tool and model runs (9 000 drops per model); 3 drops were above the full scan (the re-solve finds contacts the full scan misses). Before the re-solves there were 2 below (the two regression points of the test `contacts_beside_a_face_boundary_are_not_missed`).

S2 reference runs: 600 synthetic points no misses; 7 560 corpus points 21 misses, the same counts and worst values as before the plan (8 / 8 / 5, worst 9.7e-4, 7.2e-4, 3.3e-4 mm; measured before the re-solves, which only add solves for B-spline patches and the corpus has few). Workspace tests pass; no golden file changed.

## Open

- Memory: see Review Focus 3.
- Warm starts: see Architecture. A floor cannot help; a restricted certification of the contact's patch would need a second-order argument.
- The re-solve triggers often; better Newton starts beside face boundaries (all three starts of the failing patch fail there) would remove the need.
