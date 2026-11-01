# S5 Follow-ups Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Follow-ups 1–3 of the S5 report (§7): remove GJK from flat and bull ramps and plunges seen from the side, try cylindrical dexels for rotary Setups against the tri-dexel, and improve the Z family's parallelism on rasters.

**Architecture:** All work stays in `crates/fluut-sim`.
- `sweep.rs`: the side method covers any chord with a fixed axis, not only level chords.
- `ball_ray` gets a fast path for rotary chords.
- A new module `cyl.rs` holds the cylindrical layout. It has radial rays on an (x, θ) grid and X rays on a (y, z) grid, cut by the same `Sweep::ray`.
- `sim.rs` schedules tiles differently.
- Measurements go in `examples/s5.rs`.

**Tech Stack:** as S5; no new dependency.

**Design:** [`spikes/s5-simulation.md`](spikes/s5-simulation.md) §1, §3, §6, §7; [`../design/D11-simulation.md`](../design/D11-simulation.md) §2, §3, §7; [`../design/challenges.md`](../design/challenges.md) CH-22.

## Global Constraints

All S5 Global Constraints apply. In addition:

- The swept volume stays the one S5 defined: `tool ⊕ chord` for a fixed axis, the hull of the end poses on rotary chords. New methods are checked against brute force and GJK, as `sweep/tests.rs` does.
- "Before" numbers are re-measured on the machine that measures "after", because the S5 machine is not available.

## Conventions

- **Cylindrical layout.** The stock is the rotary cylinder `x0..x1`, radius `R`, on the X axis of the stock frame.
  - **Radial rays:** ray `(i, j)` starts on the axis at `x = x0 + (i + ½)·res` and runs along `(0, sin θ_j, cos θ_j)`, with `θ_j = j·2π/nθ`. This is the tool axis at `A = θ_j` (D11 §2: the axis at `A` is `R_x(−A)·Z`). `nθ = ⌈2πR / res⌉`, so neighbouring rays are at most `res` apart at the outer radius. Spans are radii.
  - **X rays:** the tri-dexel's X family, but only rays whose centre lies inside the stock circle are stored. The rest are empty for the whole run.

## Review Focus

1. **Side method on ramps.** The line at height `h` above the first tip meets the sweep where the tool's section at height `h − β·Δh` meets it, for `β` along the chord.
   - Straight flank: a capsule of radius `R`, closed form.
   - Corner bands of a bull nose: the feasible `β` come from a line against a 2-D capsule, then a 1-D convex minimum. The result must agree with GJK to 1e-3 mm and with brute force.
2. **Ball fast path.** An entry point found on the bottom capsule is only taken when its normal `n` has `n·w0 ≤ 0` and `n·w1 ≤ 0`; then it lies on the hull's boundary. An exit point on the top capsule needs `n·w ≥ 0`. Anything else falls back to the full hull.
3. **Determinism.** Every layout and schedule gives the same bits on 1 and 4 threads. Every tile applies its chords in program order.

---

### Task 1: Plan

**Files:** Create `docs/plan/2026-11-01-s5-followups.md`; modify `docs/plan/README.md`.

- [ ] **Step 1:** Write this plan and add it to the plans table.

### Task 2: Ramps and plunges seen from the side

**Files:** Modify `crates/fluut-sim/src/sweep.rs`, `crates/fluut-sim/src/sweep/tests.rs`, `crates/fluut-sim/examples/s5.rs`.

- `Method::Side` covers a fixed axis and a line across it, for any chord.
  - Level chords keep the S5 code.
  - Otherwise `β ∈ [0, 1]` splits into the bottom corner band, the straight flank and the top corner band, by the height `h − β·Δh` of the line above the tip.
  - On the flank the section is a disc of radius `R` moving along a line, so the interval is a 2-D capsule.
  - In a corner band (bull only) the section has radius `R − r + √(r² − v²)`, where `v` is the height above or below the corner centre. The `β` for which it reaches the line form an interval: the point `(b, v)`, with `b` the line's offset, lies in a 2-D capsule of radius `r` around `[−(R − r), R − r] × {0}`. The entry and exit are convex in `β`; a golden-section search finds them.
- GJK stays only for flat and bull tools on lines that are neither along nor across the axis, and on rotary chords.
- [ ] **Step 1:** Tests: flat and bull ramps and plunges against GJK, 300 random lines each, within 1e-3 mm. The brute-force test also covers steep ramps. `flat_plunge_is_cylinder` checks the side method exactly.
- [ ] **Step 2:** Fail, implement, pass. `examples/s5.rs rays` times the new side case instead of GJK.

### Task 3: Cylindrical dexels for rotary Setups

**Files:** Create `crates/fluut-sim/src/cyl.rs`, `crates/fluut-sim/src/cyl/tests.rs`; modify `crates/fluut-sim/src/{lib.rs, sweep.rs, sim.rs, spike/probe.rs}`, `crates/fluut-sim/examples/s5.rs`.

- **Ball fast path on rotary chords.** Take the line's entry from the bottom capsule (centres `c0`, `c2`) and its exit from the top capsule (`c1`, `c3`), under the normal rule of Review Focus 2. Fall back to the full hull otherwise. This also serves the tri-dexel, so both layouts are measured with it.
- **`CylDexel`:**
  - `new(stock, res)` for `Stock::Cylinder`;
  - `cut(&mut self, sweeps) -> Stats`, by tiles of θ rows (radial) and z rows (X), with each tile taking its chords in program order;
  - `volumes()`, `memory()`, `hash()`.
- **Which radial rays a chord can touch:** the θ range comes from the angles of the chord box's (y, z) corners, wrapping at 2π; the whole circle if the box contains the axis. The radius range runs from the box's nearest to its farthest point from the axis.
- **Spike measurements:** `spike::probe::probe_line` (exact spans on any line) and `spike::probe::radial_error` (bilinear surface in (x, θ) through the outermost span ends, against the probe).
- [ ] **Step 1:** Tests:
  - Ball fast path against the full hull on random rotary and fixed chords.
  - Volume of a fresh `CylDexel` against πR²L.
  - A full rotary turn cuts a ring: radial tops at `8 ± sagitta`, X rays outside the ring untouched.
  - Probe equals the grid on sampled rays.
  - The same hash on 1 and 4 threads.
- [ ] **Step 2:** Fail, implement, pass.
- [ ] **Step 3:** `examples/s5.rs cyl`: memory, time per move at 0.4/0.2/0.1 mm (1 and 4 threads), tests per chord per family, and error against the exact probe in the S5 rotary window. Re-run `rotary` for the tri-dexel with the fast path.

### Task 4: Z-family parallelism on rasters

**Files:** Modify `crates/fluut-sim/src/sim.rs` (and `cyl.rs` if it shares the scheduler).

- First measure the speed-up of each family from 1 to 4 threads on the relief finish (`Stats::secs`).
- Then schedule only tiles that have chords, as separate work items. If that is not enough, interleave them so neighbouring busy tiles go to different threads, or batch chords by region.
- The order of chords within a tile does not change.
- [ ] **Step 1:** Test: `same_hash_on_one_and_many_threads` still passes. Add a raster whose batch covers a narrow band of a wide grid.
- [ ] **Step 2:** Implement and measure the per-family speed-up before and after.

### Task 5: Report

**Files:** Modify `docs/plan/spikes/s5-simulation.md`, `docs/design/challenges.md`, `docs/design/D11-simulation.md`, `docs/plan/README.md`.

- [ ] **Step 1:** Add a dated "Follow-ups" section to the S5 report, with before and after numbers.
- [ ] **Step 2:** Update CH-22 and D11 §2, §3 and §7 where the findings change them. Mark this plan done.
