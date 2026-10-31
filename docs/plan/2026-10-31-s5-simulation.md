# S5 — Simulation Spike Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Find out whether a tri-dexel stock (D11 §2–3) is fast and small enough for FluutCAM's relief and rotary work (CH-22). Exit criterion from the roadmap: memory and update time on the reference rotary and relief projects.

**Architecture:** The spike starts the layer-4 crate `fluut-sim`, meant to be kept: stock shapes, the tri-dexel grid, tools, swept volumes and material removal along `fluut-path` segments. The reference projects do not exist yet, so the spike builds synthetic stand-ins in `fluut-sim/src/spike/`, which may be thrown away. Measurements are printed by `cargo run -p fluut-sim --example s5 --release`.

**Tech Stack:** as S3; `fluut-sim` adds `smallvec` (already a workspace dependency) and `rayon` (MIT OR Apache-2.0, already in the lock file through `criterion`).

**Design:** [`../design/D11-simulation.md`](../design/D11-simulation.md) §2, §3, §7; [`../design/D08-toolpath-framework.md`](../design/D08-toolpath-framework.md) §2; [`../design/challenges.md`](../design/challenges.md) CH-22; spec 03 §3.9.3 (rotary frame).

## Global Constraints

All M0 to S3 Global Constraints apply. In addition:

- Results are deterministic: the same program gives bit-identical stock on any thread count. Tests compare a hash of the stock.
- Material removal is exact up to the swept volume of each chord: rays are cut by the exact convex swept body, not by sampled tool positions. Curved moves are split into chords as D11 §3 says.
- The tool is a solid of revolution with a straight shank that reaches past the stock. Holder and shank collisions are out of scope.
- The report states numbers measured on this machine in release builds, with the stock, tool, program and resolution used.

## Conventions

- **Stock frame.** The tri-dexel lives in the stock frame S. For three-axis Setups S is the work frame W. For rotary Setups the rotary axis is the X axis of S through Y = Z = 0, and `p_W = R_x(A) · p_S` (spec 03 §3.9.3, right-hand rule), so the tool tip at machine position `xyz, A` is at `R_x(−A) · xyz` in S and its axis is `R_x(−A) · Z`.
- **Rays.** Family *k* runs along axis *k*. Ray (i, j) of a family sits at the centres of a grid of spacing `res` over the stock box. Intervals are stored as `f32` offsets from the stock box minimum along the ray.

## Review Focus

1. **Swept volume**: the ray interval is the exact intersection with tool ⊕ chord (fixed axis) or the convex hull of the two end poses (rotary chords), checked against brute-force sampling of tool positions.
2. **Interval arithmetic**: subtraction keeps intervals sorted, disjoint and non-empty; nothing is ever added.
3. **Determinism**: chords are applied to every ray in program order whatever the thread count.

---

### Task 1: `fluut-sim` crate, stock and dexel grids

**Files:** Create `crates/fluut-sim/{Cargo.toml, src/lib.rs, src/stock.rs, src/dexel.rs}`; modify the root `Cargo.toml`.

- `Stock::{Box { min, max }, Cylinder { x0, x1, radius }}` with `bbox` and `extent_along(p, dir)` (largest `(q − p)·dir` over the stock).
- `DexelGrid` (one family) and `TriDexel { res, origin, grids: [DexelGrid; 3] }`; each ray a `SmallVec<[Span; 2]>` of `f32` spans.
- `TriDexel::new(&Stock, res)`: box rays are filled end to end; cylinder rays get the exact chord of the cylinder.
- `Span` subtraction, `volume()`, `memory()` (bytes, inline and heap), `hash()` (FNV-1a of the bits, in ray order).
- [ ] **Step 1:** Tests: box ray counts and volume; cylinder volume within resolution of πR²L; subtracting a middle, an end, a covering and a disjoint interval.
- [ ] **Step 2:** Fail, implement, pass.

### Task 2: Tools and swept volumes

**Files:** `src/tool.rs`, `src/sweep.rs`, `src/gjk.rs`.

- `Tool { radius, corner }` with `flat`, `ball`, `bull`: the solid `disk(R − r) ⊕ axis segment ⊕ ball(r)`, which is a flat end mill for r = 0, a ball for r = R and a bull nose between.
- `Pose { tip, axis, length }` and `Sweep` = convex hull of two poses (tool ⊕ segment when the axes are equal).
- `Sweep::ray(o, dir) -> Option<(t0, t1)>`:
  - ball: exact, as the hull of up to four sphere centres ⊕ ball (edge capsules and thickened faces);
  - flat and bull with the ray along the tool axis: exact 1-D minimum over the chord;
  - otherwise: GJK ray cast on the support function (D11 §3), to 1e-4 mm.
- [ ] **Step 1:** Tests: ball along a line is a capsule; flat plunge is a cylinder; bull vertical ray depth equals the profile; GJK agrees with the closed forms on random rays; all ray results agree with brute-force sampled poses within sampling error.
- [ ] **Step 2:** Fail, implement, pass.

### Task 3: Moves and material removal

**Files:** `src/motion.rs`, `src/sim.rs`.

- Segments to chords: lines as they are; arcs and helices split so the sagitta is at most `res/4`; rotary moves (and lines with a changing A) split so the sagitta at the stock's largest radius is at most `res/4`, each chord the hull of its end poses; splines through `fluut_path::linearize`; cycles as a plunge; markers ignored.
- `Simulator::run(&mut TriDexel, &Tool, segments)`: chords in batches; rays are cut in parallel by tiles, each tile applying the batch in program order.
- [ ] **Step 1:** Tests: ball slot along X leaves the analytic profile; arc groove; full rotary turn cuts a ring of the cylinder to the tip radius; same hash on 1 and 4 threads; cut volume of a pocket equals the analytic volume within resolution.
- [ ] **Step 2:** Fail, implement, pass.

### Task 4: Reference stand-ins and measurements

**Files:** `src/spike/{mod.rs, relief.rs, rotary.rs, probe.rs}`, `examples/s5.rs`.

- **Relief:** 200 × 150 × 25 mm box; a smooth relief surface between z = 8 and z = 24; Ø3 ball raster finish along X at 0.3 mm stepover, 0.5 mm moves, straight from the full stock.
- **Rotary:** Ø60 × 150 mm cylinder; a wrapped relief r(x, θ) between 24 and 29 mm; Ø6 ball spiral wrap at 0.5 mm per turn, Rotary segments of 2°.
- Per project and resolution (0.4, 0.2, 0.1 mm): rays, memory, time per move and per million chords, on 1 thread and all threads.
- **Error vs resolution:** on a 20 × 20 mm window, the largest and RMS error of a bilinear surface through the dexel ends against an exact single-ray probe at random points (Z family, slopes under 45°), for resolutions 0.4 to 0.025 mm.
- [ ] **Step 1:** Stand-in generators with tests (continuous paths, inside the stock); the probe agrees with the full grid at grid points.
- [ ] **Step 2:** Run and record.

### Task 5: Report and design updates

**Files:** Create `docs/plan/spikes/s5-simulation.md`; modify `docs/design/challenges.md` (CH-22), `docs/design/D11-simulation.md`, `docs/plan/README.md`.

- [ ] **Step 1:** Write the report; S5 note on CH-22; D11 changes where the findings require them; roadmap row.
- [ ] **Step 2:** Workspace checks; commit; push the branch.
