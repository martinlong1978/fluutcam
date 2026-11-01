# STEP P-curves and Fuzzing Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden the S1 STEP reader before M4 builds on it. Every coedge gets a p-curve, read from the file when it has one and otherwise projected from the 3-D edge (D04 §5.2, CH-08). The parser and the mapper get `cargo-fuzz` targets (D04 §10). Both follow-ups come from the [S1 report](spikes/s1-kernel-step.md) §5–§6.

**Architecture:**
- `fluut-geom` gets a 2-D curve module, `curve2`: `Curve2` (line, ellipse, B-spline) and `BSplineCurve2`, as sketched in D02 §3.1. `BSplineCurve2` wraps a `BSplineCurve3` with `z = 0`, so evaluation code is shared.
- `fluut-topo` gets a new field `BrepBody::pcurves: Vec<Option<Curve2>>`, indexed like `coedges`, and a new module `pcurve` with the projection. Both are additive: no existing type or function changes, and `BrepBody` literals that end in `..Default::default()` still compile. A p-curve uses the parameter of its edge's 3-D curve over `Edge::range`, so `S(pc(t))` should equal `C(t)`.
- `fluut-io-step` maps `PCURVE` (inside `SURFACE_CURVE` and `SEAM_CURVE`) and calls the projection for coedges without a usable file p-curve.
- `fuzz/` is a separate cargo-fuzz package, excluded from the workspace like `spikes/`.

**Tech Stack:** as S1; adds `cargo-fuzz` and `libfuzzer-sys` in `fuzz/` only (not a workspace dependency).

**Design:** [`../design/D02-geometry-core.md`](../design/D02-geometry-core.md) §3.1, [`../design/D03-topology-bvh.md`](../design/D03-topology-bvh.md) §1, §3.1, [`../design/D04-step-reader.md`](../design/D04-step-reader.md) §5.2, §6, §10, [`../design/challenges.md`](../design/challenges.md) CH-08.

## Global Constraints

All S1 Global Constraints apply. In addition:

- `fluut-topo` changes are additive: new fields and modules only. Another session reads its BVH.
- The D04 §5.2 tolerance clamp stays as it is. It is an open product decision (S1 report §6).
- The corpus stays outside the repository (`FLUUT_STEP_CORPUS`). Fuzz corpora and artefacts are not committed; crashing inputs become small regression tests instead.

## Review Focus

1. **Parametrisation**: every stored p-curve satisfies `|S(pc(t)) − C(t)| ≤ tol_model` at sample points, or is reported as a failure. No p-curve is stored silently wrong.
2. **Seams and poles**: the two coedges of a seam edge get p-curves one period apart; at a pole or apex, `u` comes from the neighbouring samples, not from the arbitrary value the closest-point query returns there.
3. **Untrusted input**: no panic, hang or unbounded allocation in `parse` and `map_bodies` on any byte string.

---

### Task 1: Plan

**Files:** Create `docs/plan/2026-11-01-step-pcurves-fuzz.md`; modify `docs/plan/README.md`.

- [x] **Step 1:** Write this plan and add it to the plans table.

### Task 2: 2-D curves and the p-curve store

**Files:** Create `crates/fluut-geom/src/curve2.rs`; modify `crates/fluut-geom/src/lib.rs`, `crates/fluut-topo/src/brep.rs`.

- `Curve2::{Line { origin, dir }, Ellipse { centre, a, b }, BSpline(Arc<BSplineCurve2>)}` with `eval`, `deriv`, `translated`. `Line` is `origin + t·dir` (`dir` need not be unit); `Ellipse` is `centre + cos t·a + sin t·b`, which also holds circles of either sense.
- `BrepBody::pcurves`, empty when no p-curves were computed, else one entry per coedge.
- [ ] **Step 1:** Tests: evaluation and derivative of each kind against finite differences; a 2-D B-spline equals the 3-D one it wraps.
- [ ] **Step 2:** Fail, implement, pass.

### Task 3: Projection

**Files:** Create `crates/fluut-topo/src/pcurve.rs`, `crates/fluut-topo/src/pcurve/tests.rs`.

- **Closed form** where the image is exact: any line, circle, ellipse or B-spline on a plane (affine map of the curve); axis-parallel lines on cylinders; generators through the apex of cones; coaxial circles on cylinders, cones, spheres and tori; meridian circles on spheres and tori.
- **Otherwise sampled:** closest point per sample (closed form on analytic surfaces, `Surface::closest` on others), unwrapped against the previous sample across periods, with `u` (or `v`) taken from the neighbours where the surface is degenerate in that direction. Tangents `(u′, v′)` come from `C′ = Su·u′ + Sv·v′`. The fit is a C¹ cubic B-spline (Hermite pieces); a piece is split until its midpoint maps within `tol_model / 2` of the projected point. Sample count is capped.
- **Seams:** per loop, each p-curve is shifted by whole periods so that it starts where the previous coedge ended. A joint at a degenerate point (pole, apex) does not fix the shift; such a coedge is aligned to the next one instead. Loops after the first are shifted next to the first loop. Seam coedges come out one period apart.
- **Check:** `|S(pc(t)) − C(t)|` at 16 points per edge and at the fit samples. The report gives coedges within `tol_model`, the worst deviation, failures by surface kind, and whether the 3-D edge itself lies off the surface.
- [ ] **Step 1:** Tests: cylinder with seam (two seam p-curves 2π apart, circles along `u`), sphere cap bounded by a latitude circle near the pole, sphere lune with meridians through both poles, tilted circle around a sphere's pole (sampled, `u` winds 2π), cone with an apex vertex, B-spline patch with a B-spline edge across it, plane with a B-spline edge.
- [ ] **Step 2:** Fail, implement, pass.

### Task 4: P-curves in the STEP mapper

**Files:** Modify `crates/fluut-io-step/src/map.rs`, `crates/fluut-io-step/src/map/tests.rs`, `crates/fluut-io-step/src/lib.rs`, `crates/fluut-io-step/examples/s1.rs`.

- `SURFACE_CURVE` and `SEAM_CURVE` keep their `PCURVE`s per surface. A coedge on a face whose surface has a file p-curve for its edge uses it if it passes the check (after an affine change of parameter where the file's parameter differs from ours); otherwise it is projected and counted as rejected. A seam edge's two file p-curves go to its two coedges by loop continuity.
- Parameter units: p-curve coordinates are scaled per surface kind (lengths by the length unit, angles by the angle unit).
- `MappedBody` gets a `pcurves: PcurveStats` summary.
- [ ] **Step 1:** Tests: a plane face and a cylinder face with file p-curves (`LINE` in `DEFINITIONAL_REPRESENTATION`), seam included; a wrong file p-curve is rejected and replaced; the cube and cylinder fixtures without p-curves get projected ones within `tol_model`.
- [ ] **Step 2:** Fail, implement, pass.
- [ ] **Step 3:** Corpus run: `FLUUT_STEP_CORPUS=… cargo run -p fluut-io-step --example s1 --release` prints p-curve counts, failures by surface kind and time.

### Task 5: Fuzzing

**Files:** Create `fuzz/Cargo.toml`, `fuzz/fuzz_targets/{parse.rs, map.rs}`, `fuzz/README.md`; modify `Cargo.toml` (exclude), parser and mapper where crashes show up, with regression tests in `parse/tests.rs` and `map/tests.rs`.

- `parse`: any bytes into `parse`. `map`: bytes into `parse`, then `map_bodies` (with p-curves) on success; seeded with small valid files (the test fixtures) so it reaches the mapper.
- [ ] **Step 1:** Run each target for a fixed time; record executions and coverage.
- [ ] **Step 2:** For each panic, hang or large allocation: add the input as a test, fail, fix, pass.

### Task 6: Report and design updates

**Files:** Create `docs/plan/spikes/step-pcurves-fuzz.md`; modify `docs/design/challenges.md` (CH-08), `docs/plan/README.md`.

- [ ] **Step 1:** Report: p-curve results on the corpus, fuzzing runs and fixes, what stays open.
- [ ] **Step 2:** Update CH-08; mark this plan done.
