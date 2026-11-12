# Drop Misses Plan (fu-11)

**Goal:** the [crease checks](2026-11-11-crease-checks.md) left 4 flat-tool points of about 320 000 on the rough tiles cut 0.013-0.020 mm deep (tolerance 0.01 mm), each one a drop at a chord's end or middle that missed a contact inside its own footprint which a nearby drop found. Find which part of the drop misses, fix it generally (not for the 4 points), and check the drop against an **independent** reference, because `drop_bench` `VERIFY` compares with the full scan, which uses the same face solver and cannot see the solver's misses.

**Design:** [D07 §4.3, §8](../design/D07-cutter-queries.md), [CH-25](../design/challenges.md).

**Outcome in one line:** every miss is the contact-map Newton (`newton_contact`) failing on a B-spline face where the face is nearly level at a contact on the flat tool's rim (or a bull-nose's corner just outside its flat), beside a bump whose top is just outside: its `(R − r)·n̂_xy` term turns as fast as `κ/|∇z|` there and no start lies in its basin. Two climbs that stay well conditioned there fix it: from a stationary point the flat-bottom Newton finds just outside the flat (that bump's top), along the rim for a flat tool and up the tip height for a bull-nose; and for a flat tool whose best contact is an edge or vertex on its rim, along the rim on the faces beside it. The 4 points are gone (no flat or bull finishing point on the tiles is cut deeper than the tolerance), 0 of 180 000 random flat and bull drops over the tiles and two sculpted models are below the independent reference by more than 1e-6 mm (11 before, all on the tiles: 8 flat, 3 bull, up to 0.024 mm), at +0-6 % drop median for flat tools and −1 to +4 % for bull (ball unchanged).

## The 4 points

`finishing_bench` (feature `bench-stats`, `FB_WORST=1`), rough tiles 8 x 8, Ø6 flat, tolerance 0.01 mm. All four chords are level (both ends on the same edge contact, the merged output of several drops); a fresh drop at their quarter point touches a face on the tool's rim, above the chord, at a point that lies inside the footprints of the chord's own drops.

| Type | Chord (XY) | Point | Ends / middle: drop, reference (mm) | Missed contact |
|---|---|---|---|---|
| `spiral` | (32.858, 59.609) to (32.268, 59.426), 0.62 mm | 3/4, 0.0167 deep | end b: 1.66309 on edge 961, reference 1.70259 | face 236, rim, patch 945 |
| `spiral` | (69.902, 50.067) to (69.890, 50.104), 0.04 mm | 1/4, 0.020 | a, b, middle: 2.46527 on edge 1693, reference 2.48537-2.48538 | face 423, rim, patch 1695 |
| `spiral` | (12.214, 79.199) to (12.082, 79.110), 0.16 mm | 1/4, 0.020 | a, b, middle: 2.53535 on edge 511, reference 2.55156-2.56163 | face 127, rim, patch 509 |
| `scallop` | (33.901, 59.136) to (33.920, 59.136), 0.02 mm | 3/4, 0.013 | a, b, middle: 1.56715 on edge 961, reference 1.58004-1.58044 | face 240, rim, patch 963 |

The reference is `cutter::reference` (dense samples of every face, edge and vertex in reach, refined by compass search and Brent's method, with a compass search along the rim; no contact conditions and no Newton; every value it returns is a real tool position, so a drop below it has missed a contact).

**Which part misses.** In all ten drops the missed contact is on a B-spline face, on the tool's rim (`d = R`), beside a bump of the face whose top is just outside the footprint (0.001-0.06 mm beyond the rim in the traced drops), so the face is nearly level there along the rim. Traced per patch:
- *Not pruning, not the re-solve rule.* The patch that holds the contact was solved (its sub-box bound beats the answer), and so was the re-solve of a suspect patch (one per drop); both returned nothing near the contact. In two drops (`drop_all`, which solves every patch) a neighbouring patch, out of reach or pruned in the real drop, found it from its own starts by chance.
- *The contact map fails.* `newton_contact` solves `G(u, v) = S_xy + (R − r)·n̂_xy + r·n_xy = axis`. With `n̂_xy` the unit horizontal part of the normal, `G`'s Jacobian has a term `(R − r)·∂n̂_xy`, which grows as `κ/|∇z|`: 100-300 per unit of parameter on these patches against 2.4 for `S_xy`. From the patch's three starts (grid cells and projected targets, a cell or less from the contact) every run failed its line search; even from the dense reference's best sample, a few µm from the contact, it failed in some of the traced drops.
- *The flat-bottom Newton sees it but cannot take it.* `newton_flat` converged, from the same starts, to the bump's top: a stationary point of `z` just outside the footprint, rejected (rightly: it is outside). Not a clamped-step give-up: the heuristic of [drop-speed-2](2026-11-11-drop-speed-2.md) played no part.

The random checks below found 11 more misses before the fix, all on the tiles. The two bull misses traced have the same shape on the corner: the contact about 0.05 mm beyond the flat's edge (`d` ≈ 2.05 for a flat radius of 2), on a nearly level spot beside a bump top that `newton_flat` finds just outside the flat (0.07-0.14 mm beyond its edge). One flat miss was a second kind: a rim contact on a face 5 µm from a face corner, where the drop's best was the edge beside it and the face's own patch starts did not converge.

## The fix

All in `crates/fluut-cam/src/cutter/solve.rs` (`rim_ascent`, `tip_ascent`, `onto_rim`, `refine`, `face_climb_from`) and `drop.rs`; B-spline faces with flat and bull tools only (ball tools, analytic faces and meshes run the same code as before).

**Climbs.** Two local maximisations that stay well conditioned where the face is nearly level at the contact:
- *`rim_ascent`* (flat tool): a local maximum of `z` on the curve `d(u, v) = R` of the face. A point is brought onto the curve by Newton on `d = R` along `∇d` (`onto_rim`); each step is a Newton step for `z` along the curve's tangent in `(u, v)` with the Hessian of the Lagrangian `z − λ·d` (λ from the projection of `∇z` on `∇d`), at most a tenth of the radius along the surface, halved until `z` rises after projecting back. It moves along the rim only, so `κ/|∇z|` does not enter.
- *`tip_ascent`* (bull-nose): a local maximum of `φ = z − h(d)`, the tip height that touches the point, by Newton on `∇φ = 0` in `(u, v)` with a line search on `φ` (the gradient where the Hessian is not negative definite). `φ` is smooth across the flat's edge (`h′` is continuous there; `h″` jumps from 0 to `1/r`), so its Hessian stays well conditioned where the contact map's does not.

Both stop where the quadratic model predicts a rise below 1e-13 mm, never step off the face's parameter box (the boundary is the edges' part), and return a real point of the face in the footprint, scored and trim-checked as every other candidate; so, like every solver of the drop, they can miss but never place the tool too high.

**Where they start.**
1. *From a stationary point outside the flat* (`refine`): when `newton_flat` converges to a stationary point of `z` beyond the flat radius (the bump's top beside the flat's edge or the rim; for a bull-nose only one inside the footprint, where its corner is not steep), the climb starts there, unless the patch already has a contact as high as that top (the climb stays on the bump, below its top). This is the configuration of every miss above but one, and costs nothing where it does not arise.
2. *From the cells over an edge contact on the rim* (`drop`, flat tools): when the best contact after the search and the re-solves is an edge or vertex on the rim, the rim is climbed on every patch solved, from the centre of each of its sub-box cells whose XY box holds that contact. It covers a rim contact on a face beside its boundary that the face's patch starts miss (the face-corner miss).

**Proven or not.** The climbs are local searches from heuristic starts; nothing here is proven beyond "every result is a real point". What is new is that the starts are where the misses were and the method converges there. It is verified much more broadly than the last rounds, against an independent reference (below).

## Tried and dropped

- *A cell pass with a bound* (after the search, climb every sub-box cell that reaches the rim circle, or a bull's corner annulus, and whose top beats the best, highest first, until no cell can beat it): no misses, but 8-33 climbs per drop on the sculpted models (a cell's top includes the part of the cell outside the footprint, so on a slope the cells uphill of a rim contact all beat it): drops 2-3.5 times slower.
- *A climb wherever the contact map fails from a start*: 2.5-4 rim climbs per drop for the flat tool, drop medians 12-31 % slower (bull: 4-9 climbs, up to 50 % slower); the starts above find every miss it found.
- *Bull climbs from every stationary point beyond the flat* (also beyond the footprint, and when the patch already has a contact that high): the same misses found, but 1.9 climbs per drop on the tiles, drop medians +12-14 % on the tiles and the 12 mm model, the tiles' bull `scallop` 20 % slower.
- *A trust region for `tip_ascent`'s gradient steps*: twice the iterations.

## Measurements

Release, Windows 11, 12 threads (6 cores with SMT). Before = main 26620e5, after = this branch.

**Independent reference** (`drop_bench`, new `REFERENCE=1`: random points over the rough tiles 8 x 8, `sculpted` 12 x 12 faces of 3 mm and 6 x 6 of 12 mm, each grown by 3 mm; reference density 120 samples per face side; a miss is a drop more than 1e-6 mm below the reference). Seed 2 with 20 000 points per tool and model, seed 1 with 10 000: 90 000 flat and 90 000 bull drops.

| Seed | Model | Before: flat / bull below (worst mm) | After |
|---|---|---|---|
| 2 | rough tiles | 5 (1.7e-2) / 0 | 0 / 0 |
| 2 | sculpted 3 mm and 12 mm | 0 / 0 | 0 / 0 |
| 1 | rough tiles | 3 (1.8e-2) / 3 (2.4e-2) | 0 / 0 |
| 1 | sculpted 3 mm and 12 mm | 0 / 0 | 0 / 0 |

Ball drops run the same code as before (the climbs are for flat and bull tools only) and were not re-run here; they had no miss in 2 000 tiles drops at the start of this work.

**Full scan** (`drop_bench` `VERIFY`, seeds 4242 and 777, 72 000 drops): 0 below in all 24 runs; 0 above except 1 bull drop on the tiles (the full scan solves every patch with the same face solver; above means the drop found more, through a re-solve or the edge-seeded climb).

**S2 reference runs** (`s2`): 600 synthetic points, no misses (as before); 7 560 corpus points, 21 misses, the same counts and worst values as before (flat / ball / bull 8 / 8 / 5, worst 9.70e-4, 7.19e-4, 3.30e-4 mm; 5 / 0 / 0 with 1e-6 mm trims).

**Whole toolpaths on the rough tiles** (`finishing_bench`, `FB_SKIP=1`, tolerance 0.01 mm; flat before as measured at the start of this work, bull before re-run with the base binary; one run each, times move by a few %). "Deeper" counts the quarter, middle and three-quarter points of cut chords where the tool cuts deeper than the tolerance; "off" those off by more than the tolerance along the CL normal (into the part in brackets).

| Type | Tool | Drops | 1 thread s | 12 threads s | Deeper (worst mm) | Off (into) |
|---|---|---|---|---|---|---|
| parallel | flat | 166 317 → 155 969 | 5.65 → 5.68 | 1.23 → 1.07 | 0 → 0 | 9 (4) → 3 (0) |
| spiral | flat | 255 507 → 249 942 | 8.31 → 7.98 | 1.59 → 1.47 | 3 (0.0200) → 0 | 14 (6) → 7 (1) |
| scallop | flat | 1 107 246 → 1 085 278 | 34.40 → 34.16 | 6.61 → 6.48 | 1 (0.0130) → 0 | 30 (7) → 4 (1) |
| parallel | bull | 149 415 → 149 277 | 6.57 → 6.27 | 1.19 → 1.22 | 0 → 0 | 1 (0) → 1 (0) |
| spiral | bull | 198 497 → 198 230 | 8.07 → 8.35 | 1.46 → 1.46 | 0 → 0 | 0 → 0 |
| scallop | bull | 1 533 504 → 1 508 040 | 58.88 → 60.10 | 9.85 → 10.29 | 0 → 0 | 10 (1) → 4 (0) |

The flat tool takes 2-6 % fewer drops: a missed contact gives a gradient that disagrees with its neighbours', and the crease check split those chords. The points still off along the CL normal into the part (flat `spiral` 0.0103 mm, flat `scallop` the 0.040 mm chord along a wall of the crease-checks plan) are not cut deeper than the tolerance.

**Drop speed** (`drop_bench` rasters, `S2_QUICK=1`, one thread, best of 3 repetitions of 6 000 drops; base and new alternated twice, best of both). Median / p99 in µs; climbs per drop.

| Model | Tool | Before | After | Median | Climbs |
|---|---|---|---|---|---|
| rough S2 tiles, 5 000 faces | flat 6 | 23.4 / 69 | 24.7 / 71 | +6 % | 0.62 |
| | ball 6 | 30.4 / 83 | 30.0 / 80 | −1 % | 0 |
| | bull 6 r1 | 29.5 / 86 | 30.6 / 90 | +4 % | 0.81 |
| sculpted, 4 900 faces of 3 mm | flat 6 | 28.8 / 87 | 29.6 / 92 | +3 % | 0.34 |
| | ball 6 | 52.0 / 135 | 52.2 / 134 | 0 | 0 |
| | bull 6 r1 | 52.0 / 155 | 51.7 / 156 | −1 % | 0.10 |
| sculpted, 256 faces of 12 mm | flat 6 | 21.4 / 101 | 22.5 / 109 | +5 % | 0.77 |
| | ball 6 | 27.8 / 134 | 27.5 / 133 | −1 % | 0 |
| | bull 6 r1 | 30.4 / 148 | 30.5 / 146 | 0 | 0.09 |

A climb costs about 2 µs. Analytic faces and mesh bodies run no climb (B-spline faces only), so the analytic parts are unchanged.

## Golden G-code

None changed: the 13 golden files of `fluut-cli` (ball tools on the analytic dome and ridge, where no climb runs) pass byte-identical; nothing was re-blessed.

## Tests

`cutter::tests::tiles_rim_and_corner_contacts`: the seven tiles drops traced above (four chord ends, the face-corner rim contact, two bull drops) against the brute-force reference; it fails on the base (the first drop 0.0395 mm low). `drop_bench` `REFERENCE=1` is the broad check (about 25 minutes for 180 000 drops on 12 threads); not a test, it is too slow.

## Not done

- The drop is still a heuristic search: the climbs start where the misses were and converge there, which the reference check over 180 000 drops supports, but nothing bounds what a drop can miss. A cheap certificate for a patch's rim (a bound on `z` over the rim circle within a cell, tighter than the cell's top) was not found; the bounded cell pass costs twice the drop.
- Ball drops and the corpus beyond the S2 runs were not checked against the reference in this round (their code is unchanged); `contour` and `steep_and_shallow` toolpaths were not re-measured.
- The reference (`cutter::reference`) is itself sampled: a contact it misses is not counted. It found every miss above at density 120.
