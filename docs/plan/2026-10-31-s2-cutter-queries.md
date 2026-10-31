# S2 — Cutter Queries Spike Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Find out whether the exact drop query of D07 is fast and reliable enough to carry 3-axis finishing without a grid (DC-02). Exit criterion from the roadmap: drop query time per point on a 5 000-face model, and a decision whether "no grids" holds or needs a fallback. CH-04, CH-12 and CH-25 updated.

**Architecture:** The spike adds the module `fluut-cam::cutter`, meant to be kept: tool profiles (`profile`), the query geometry built from a `fluut-topo` body (`geom`: primitives, conservative boxes, trim polygons), per-primitive solvers (`solve`), the branch-and-bound drop (`drop`), a brute-force reference (`reference`) and synthetic test bodies (`synth`). `fluut-topo::Bvh` gains a best-first branch-and-bound traversal. Only the drop along −Z (3-axis) is built; the general axis, transitions (D07 §5), holders (§6) and push queries (§7) are left for M3. Measurements are printed by `cargo run -p fluut-cam --example s2 --release`; with `FLUUT_STEP_CORPUS` set, the example also runs on the corpus bodies through `fluut-io-step` (a dev-dependency; layer 2 is below `fluut-cam`'s layer 4).

**Tech Stack:** as S1; no new external crates.

**Design:** [`../design/D07-cutter-queries.md`](../design/D07-cutter-queries.md) §1–§4, §8, §9; [`../design/D03-topology-bvh.md`](../design/D03-topology-bvh.md) §4; [`../design/D02-geometry-core.md`](../design/D02-geometry-core.md); [`../design/challenges.md`](../design/challenges.md) CH-04, CH-12, CH-25.

**Corpus:** the S1 corpus (NIST MBE PMI files) at `FLUUT_STEP_CORPUS`, outside the repository. No corpus body has 5 000 faces, so the 5 000-face model is synthetic (Task 5) and says so.

## Global Constraints

All M0 to S1 Global Constraints apply. In addition:

- A drop result is always a real point of the geometry inside the tool footprint, so a solver can only under-estimate τ (gouge), never over-estimate it. Misses are what the tests and measurements look for.
- No grid, height field or sampled surface in the query itself (DC-02). Sampling is allowed for building conservative boxes, trim polygons and seeds, and in the reference.
- Corpus numbers are measurements, not pass/fail tests.

## Review Focus

1. **Conservative bounds**: every primitive box contains the trimmed primitive, so branch-and-bound never prunes the true contact.
2. **No misses**: on every test surface and tool, τ ≥ reference − 1e-7 at all sampled points.
3. **Trims**: contacts outside the trimmed face are rejected; contacts inside are kept, also on periodic surfaces and faces that reach a pole.

---

### Task 1: Tool profiles

**Files:** Create `crates/fluut-cam/src/cutter/{mod.rs, profile.rs}`.

- `Cutter { radius, corner }` for flat (`corner = 0`), ball (`corner = radius`) and bull-nose: `h(d)`, `h′(d)`, `h″(d)` as D07 §1.1, and the contact radius for a surface slope (`d` where `h′(d) = tan θ`).
- [ ] **Step 1:** Tests: values and continuity at `Rf` and `R`; contact radius inverts `h′`.
- [ ] **Step 2:** Fail, implement, pass.

### Task 2: Query geometry and best-first search

**Files:** Create `crates/fluut-cam/src/cutter/{geom.rs, trim.rs}`; modify `crates/fluut-topo/src/bvh.rs`.

- Primitives as D03 §4.1: one per analytic face, one per Bézier patch of a B-spline face that meets the face's UV box, one per edge, one per vertex.
- Conservative boxes: lines and arcs exact; B-spline curves and patches from their poles; analytic patches from a UV sample grid grown by a second-derivative bound.
- Trims: each loop projected into UV (closest point, then Newton along the loop), unwrapped across seams, closed through poles; even–odd classification. Faces whose loops cannot be projected are reported, not dropped silently.
- `Bvh::best_first(bound, visit)`: visits leaf items in decreasing bound order and stops when no bound beats the current best.
- [ ] **Step 1:** Tests: boxes contain dense samples of each primitive kind; trim classification on a plane with a hole, a cylinder band across the seam, a sphere cap through the pole; `best_first` agrees with a full scan.
- [ ] **Step 2:** Fail, implement, pass.

### Task 3: Solvers and the brute-force reference

**Files:** Create `crates/fluut-cam/src/cutter/{solve.rs, reference.rs}`.

- Vertex: closed form. Edge: closed form for lines with flat and ball tools; otherwise footprint clipping and 1-D maximisation (D07 §4.2).
- Face: closed form for planes (all tools) and ball on sphere and cylinder; otherwise damped 2-D Newton on the contact condition of D07 §4.3, seeded from a small grid on the primitive's UV box, plus the flat-bottom case (horizontal surface points) for flat and bull tools.
- Reference (D07 §9): dense samples of every face, edge and vertex, then local refinement of the best samples.
- [ ] **Step 1:** Tests: for each surface kind (plane, cylinder, cone, sphere, torus, B-spline) and each tool, τ against the reference on a grid of points (Review Focus 2); results are real points (Global Constraints).
- [ ] **Step 2:** Fail, implement, pass.

### Task 4: Drop query

**Files:** Create `crates/fluut-cam/src/cutter/drop.rs`.

- `drop(geom, cutter, x, y, tol) -> Option<Contact>` with the best-first search of Task 2; `Contact { tau, point, on }`, `on` = face, edge or vertex. Statistics per query (primitives solved, Newton failures with a bound above the result).
- [ ] **Step 1:** Tests: multi-face bodies against the reference; a cube (flat top, edges, vertices); the gouge property on random points.
- [ ] **Step 2:** Fail, implement, pass.

### Task 5: Measurements

**Files:** Create `crates/fluut-cam/src/cutter/synth.rs`, `crates/fluut-cam/examples/s2.rs`.

- Synthetic 5 000-face models: tiles of analytic and B-spline patches, in two mixes (mostly analytic, as the corpus; and B-spline heavy).
- Per model and tool: build time, drop time per point (median and 99th percentile) on a regular pass pattern, primitives solved per query, misses against the reference on a sample of points.
- Seeding (CH-12): miss rate of the free-form face solver for different seed grids.
- Corpus (if `FLUUT_STEP_CORPUS` is set): the same numbers for the largest bodies.
- [ ] **Step 1:** Run; record numbers.

### Task 6: Report and decisions

**Files:** Create `docs/plan/spikes/s2-cutter-queries.md`; modify `docs/design/challenges.md` (CH-04, CH-12, CH-25), D07, `docs/plan/README.md`.

- [ ] **Step 1:** Report: speed against D07 §8, misses, the "no grids" decision; challenge notes; D07 changes.
- [ ] **Step 2:** Workspace checks; commit.
