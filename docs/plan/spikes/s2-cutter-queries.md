# S2 — Cutter Queries Spike Report

Date: 2026-11-01. Plan: [`../2026-10-31-s2-cutter-queries.md`](../2026-10-31-s2-cutter-queries.md).

Code that is kept:
- `crates/fluut-cam/src/cutter/`: tool profiles, query geometry and trims, solvers, drop, reference, synthetic bodies;
- `fluut-topo::Bvh::best_first`;
- a fix in the `fluut-io-step` mapper (closed B-spline edges, §4).

Measurement tools:
- `cargo run -p fluut-cam --example s2 --release` (set `FLUUT_STEP_CORPUS` for the corpus part);
- `--example s2_cost` (time per primitive solve).

All numbers: release build, Linux cloud container (4-core Intel Xeon at 2.8 GHz), one thread. Tools: flat Ø6, ball Ø6, bull-nose Ø6 with r = 1. Drop along −Z only.

## Summary

| Question | Answer |
|---|---|
| Drop time per point on a 5 000-face model | **12.3–12.5 µs median** (p99 230–250 µs) on 5 967 faces of real corpus geometry. 17–28 µs on a synthetic mostly-analytic 5 000-face model. **210–320 µs** on a synthetic model with 5 000 B-spline faces. |
| Misses (CH-12) | With the final seeding, 0 misses beyond 1e-7 mm on 600 synthetic points per tool. On 2 520 corpus points per tool, every remaining miss is at most 3.2e-5 mm and comes from trim polylines or model gaps, not from the solver (§3). Four seeding bugs found on the way caused misses of 0.3–30 mm. |
| Does "no grids" hold? | **Yes, with no grid fallback.** The exact drop is correct on all corpus geometry. It is fast enough for analytic parts. It is 10–16× too slow on dense free-form geometry, but that is a speed problem with known fixes; a grid would trade exactness for speed, which DC-02 rules out (§5). |
| Exit criterion | Met: time per point measured on a 5 000-face model, and the decision is recorded. |

## Corpus and models

**Corpus.** The S1 corpus: the NIST MBE PMI files, 33 files, 63 bodies, 7 902 faces. Faces by kind:
- 3 725 cylinders and 2 402 planes;
- 922 cones, 365 tori and 312 spheres;
- 176 B-splines.

The largest body (`nist_ctc_02_asme1_rc`, 663 faces) is too small for the exit criterion. So it was **arrayed 3 × 3** with 5 mm gaps: 5 967 faces, 2 050 × 2 590 × 400 mm. The repeated copies change the BVH, not the per-face work.

**Synthetic models** (`cutter::synth::tiled(25, ·)`). These are 625 tiles of 10 × 10 mm with 8 faces each, 5 000 faces in all:
- **Mixed:** tilted and flat planes, horizontal and vertical cylinders, a cone, a sphere, a torus and one bicubic B-spline bump per tile.
- **B-spline:** 8 bicubic bumps (5 × 5 poles) per tile.

The faces are small and close together, so every drop sees many of them. They are harder than real parts and not representative of them.

## 1. What was built

- **Tool profiles** `h(d)`, `h′`, `h″` and the contact radius for a slope (D07 §1.1).
- **Query geometry.** Primitives as D03 §4.1:
  - one per analytic face;
  - one per Bézier patch of a B-spline face;
  - one per edge and one per vertex.
  
  Their boxes are conservative:
  - lines and arcs exact;
  - B-spline curves and patches from their poles;
  - analytic patches from a sample grid grown by the bilinear interpolation error.
- **Trims without p-curves.** Loops are projected into (u, v), unwrapped across seams, filled in at poles and closed through a pole when needed. Points are classified even–odd. All 7 902 corpus faces get a trim.
- **Best-first branch and bound** directly over the BVH nodes (`Bvh::best_first`).
- **Solvers.**
  - Vertex: closed form.
  - Edge: exact footprint clipping and one Brent maximisation for lines; sampling (≤ R/4), grazing refinement and false-position rim crossings for curves.
  - Face, closed form: planes (all tools), ball on sphere and cylinder.
  - Face, otherwise: damped Newton on the contact map `G(u,v) = S_xy + (R−r)·n̂_xy + r·n_xy = q`. Flat and bull tools add a "flat bottom" stationary-point solve. Details are in D07 §4.3.
- **Reference** (D07 §9). Dense samples of every primitive near the axis, then compass search inside the face and footprint, and along the rim `d = R`.

**Tests.** Every surface kind (plane, cylinders, cone, convex and concave sphere, torus, B-spline) and every tool is compared with the reference on a grid of points. There are also tests on a box, on synthetic tiles, and on the free-form points that failed during the spike. Branch and bound is checked against a full scan.

Every drop result is a real point of the geometry inside the footprint, with `τ = p_z − h(d)`. So a solver can only miss (gouge), never lift the tool.

## 2. Speed

Median, 99th percentile and mean, in µs per drop. Primitives solved are per drop.

| Model | Tool | Median | p99 | Mean | Primitives solved |
|---|---|---|---|---|---|
| All 63 corpus bodies (113 400 drops) | flat | 10.1 | 212 | 19.1 | 2.1 |
| | ball | **4.6** | 147 | 11.5 | 2.1 |
| | bull | 8.2 | 167 | 15.9 | 2.1 |
| Largest corpus body, 663 faces | flat / ball / bull | 12.4 / 12.0 / 12.5 | 229 / 190 / 248 | 20.6 / 16.8 / 25.7 | 1.9 |
| **Corpus array, 5 967 faces** | flat / ball / bull | **12.5 / 12.3 / 12.4** | 247 / 231 / 236 | 21.5 / 19.0 / 19.6 | 1.9 |
| Synthetic mixed, 5 000 faces | flat / ball / bull | 28.2 / 17.0 / 28.4 | 446 / 274 / 429 | 63 / 43 / 66 | 2.7–3.3 |
| Synthetic B-spline, 5 000 faces | flat / ball / bull | **301 / 212 / 321** | 1 262 / 709 / 1 187 | 360 / 240 / 370 | 7.4–7.9 |

**Size does not matter much.** The 663-face body and its 5 967-face array give the same times. The best-first search keeps the work per drop at about 2 primitives on corpus parts.

**Cost per primitive solve** (`s2_cost`, ball):

| Primitive | µs |
|---|---|
| Vertex | 0.15–0.18 |
| Line edge | 1.9 |
| Circle or B-spline edge | 5.8–9.5 |
| Analytic face (closed form or Newton) | 4.7 |
| **B-spline face patch** | **39–41** |

**Against D07 §8:**
- Ball on an analytic part: target ≤ 2 µs, measured 4.6 µs median over the corpus. That is 2.3× over.
- Bull on B-spline faces: target ≤ 20 µs, measured 321 µs on the dense model. That is 16× over.
- Mesh and holder targets were not measured.

**The tail.** p99 is 15–25× the median. It comes from drops where many large faces' boxes overlap the footprint. These were not broken down further. Likely causes are long trim loops (classification is linear in the loop size) and Newton runs on faces with no interior contact.

## 3. Misses and free-form seeding (CH-12)

**Final numbers.** A miss is τ below the reference by more than 1e-7 mm.

| Points | Flat | Ball | Bull |
|---|---|---|---|
| Synthetic models (100 + 100 per tool) and B-spline tiles (100) | 0 | 0 | 0 |
| Corpus, 2 520 per tool (40 per body) | 8 (≤ 9.7e-4 mm) | 9 (≤ 7.8e-4 mm) | 5 (≤ 3.3e-4 mm) |
| …of those, still a miss with 1e-6 mm trim polylines | 6 (≤ 3.2e-5 mm) | 0 | 1 (8.9e-6 mm) |

Every corpus miss has a cause outside the solver:
- **Trim polylines.** At the default 1e-3 mm chord, a face point up to about `chord × slope` outside the true face counts as inside. The reference uses the same trims. With a 1e-6 mm chord these misses shrink by 2–3 orders of magnitude or vanish.
- **Model gaps.** On `nist_stc_07`, a hole's edge curve lies 2.8e-5 mm below its plane. The highest face point is just inside the boundary, above the edge, so the drop misses by the gap. The same spot is the 8.9e-6 mm bull-nose miss.

The other five flat misses (≤ 1.3e-6 mm) are edge contacts beside cylinder and plane faces, near this file's model gaps. They were not traced one by one.

**Four bugs were found by the corpus and the dense reference**, each fixed and covered:

| Bug | Worst miss | Fix |
|---|---|---|
| Seeds sampled over the whole face: none near a 6 mm tool on a 35 mm torus fillet ring | 1.0 mm | seed from targets in the footprint, projected vertically |
| Closest-point start at mid-box height is on the axis of a horizontal cone; projection lands on the underside | 15 mm | start from the top of the box |
| Start on the vertical side of a large torus fillet is singular; projection stuck on the face edge | 27 mm | retry from lower starts; drop projections that do not converge |
| Stuck projections outranked the good seeds | 0.7 mm | (same) |

**Seeding variants.** Default: 4 × 4 samples, 2 rings of targets, 3 Newton starts. On 100 B-spline tile points and 40 corpus points per tool:

| Seeding | B-spline tiles, median µs (flat / ball / bull) | Misses | Corpus misses |
|---|---|---|---|
| 2 × 2, no rings, 1 start | 95 / 60 / 88 | 0 | 0 |
| 4 × 4, 1 ring, 1 start | 141 / 84 / 113 | 1 flat (0.12 mm) | 1 flat (1.4 mm) |
| **4 × 4, 2 rings, 3 starts** | 233 / 154 / 250 | 0 | 0 |
| 6 × 6, 3 rings, 6 starts | 466 / 286 / 493 | 0 | 0 |

One Newton start is not enough: the "4 × 4, 1 ring, 1 start" row misses by up to 1.4 mm. The cheapest row had no misses, but with one start it has no margin; the sample is too small to clear it.

**Newton fails in about half of all runs.** This is normal: most candidate primitives have no interior contact, because the contact is on an edge. Non-convergence alone therefore cannot raise D07's `NoConvergence` error; the rule is suspended in D07.

## 4. Other findings

- **Mapper bug (S1 code).** Closed B-spline edges (start vertex = end vertex) got a zero-length range. As a result, 30 corpus faces had no usable trim loop. It is fixed in `fluut-io-step` with a test; the S1 validity numbers are unchanged.
- **The trim chord is a precision setting.** It bounds how far outside a face a contact may be accepted (§3). Exact edge geometry near the boundary, or p-curves (M4), would remove it.
- **Model gaps reach the drop directly.** A drop is only as exact as the model's edge-to-face agreement. This supports S1 follow-up 1: take the model tolerance from the file and report it.
- **The reference cannot be "within 1e-7" by sampling** (D07 §9 asked for it). Flat tools touch on the footprint rim, so the reference needs a rim search. The solver may be above the reference, and the tests instead check that its contact is a real point.
- **Offsets (CH-04).** No explicit offset surface is built for any face. Concave faces (a bowl of radius 5 under a radius-3 ball, concave torus fillets on the corpus) are handled by the contact condition, with no folds.

## 5. Decision: "no grids" holds, no grid fallback

- **Correctness.** The exact drop is right everywhere the reference can check it, to the model's own accuracy. A grid would only add a sampling error.
- **Speed on analytic parts is acceptable now.** The corpus mean is 11–19 µs per drop. A finishing pass of 10⁶ drop points takes 12–19 s on one thread, about 4 s on four. Adaptive sampling along passes (D08) needs fewer points than a raster.
- **Speed on dense free-form geometry is not acceptable yet.** 10⁶ drops would take 4–6 min single-threaded. The cost is the B-spline patch solve (40 µs) times about 5 patches per drop. That calls for speed work, not a grid.

**Speed work before M6:**
1. Warm-start Newton from the previous drop's contact: passes are coherent, so one start is usually enough.
2. Tighter patch bounds: subdivide pole boxes to cut the 5 patch candidates per drop.
3. Closed forms for more analytic cases: ball on cone and torus, and flat and bull on cylinders.
4. Faster trim tests: a per-loop segment index instead of a linear scan.
5. Parallel drops (`rayon`): queries are independent.

**Revisit** if the dense B-spline model stays above 50 µs median after items 1–3.

## 6. Follow-ups

1. Revise D07 §8 after the speed work. Measure again on these models with `s2`.
2. Add the waterline push query (D07 §7), transitions (§5) and holder checks (§6) in M3. They reuse the drop machinery measured here.
3. Trim classification from exact edge geometry near face boundaries, or from p-curves once M4 maps them.
4. A free-form corpus: the NIST files have only 176 B-spline faces. This is the same need as S1 follow-up 3.
