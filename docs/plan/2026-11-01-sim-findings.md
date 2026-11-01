# Simulation Findings Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the problems the M7 simulation found in the reference projects ([M7 plan](2026-11-01-m7-simulation.md), last deviation): rapids through material in `block` t2 and `holes` t2, and the gouges in `plate2d`. Fix them in the generator, not in the projects, and make the simulator verify every reference project on each test run.

**Architecture:** Changes in `fluut-cam`: the shared linker (`link.rs`), the hole strategies (`drill.rs`, `bore.rs`) and the adaptive path search (`adaptive.rs`, `strategy/adaptive_2d.rs`). The CLI tests run `fluut sim` on every reference project. `follow.rs`, `waterline.rs` and the 3-D finishing strategies are being changed elsewhere and are not touched here.

**Tech Stack:** as M7. No new dependencies.

**Design:** [`../design/D08-toolpath-framework.md`](../design/D08-toolpath-framework.md) §7, §8.2; [`../design/D11-simulation.md`](../design/D11-simulation.md) §5; [`../design/D15-toolpath-parameters.md`](../design/D15-toolpath-parameters.md) §2.4; spec 04 §4.14.

## Global Constraints

All earlier Global Constraints apply. In addition:

- Test first: each fix starts with a test that fails because a rapid passes through material or a cut leaves the tool-centre region.
- When a fix needs a design decision, take the safe behaviour (retract higher, feed instead of rapid) and record it in D08.
- Re-bless golden G-code only where the change is provably safer; list every re-blessed file below with its reason. Translation invariance must still hold.
- Never hide a real finding. A finding the test allows is listed with its reason.

## Evidence

Reproduced with `fluut sim` at the default 0.2 mm on 1786e2a. Coordinates are in the Setup's work frame.

1. **`block` t2: 7 rapids in stock**, the first at (67.041, 15.100, −5.900). t2 is a floor raster whose `top` is the floor selection + 0.1 (Z −7.9), so its feed height is −5.9. t1 left 0.2 mm on the walls and 0.1 on the floor. The linker (`link.rs`) lifts to `link_height` = stock top + safety (Z 1), rapids across, then **rapids down to the feed height**, wherever the entry is. t2 has no stock to leave, so entries on the edge of its tool-centre region are 3 mm from the walls; t1 stopped at 3.2 mm, so the tool's side rapids down through the 0.2 mm of wall from Z 0 to −5.9.
   - Root cause: D08 §8.2 lets the descent from the link height to the feed height be a rapid, and D15 §2.4 puts the feed height at `top + 2`. That is safe only when nothing lies above `top`. The linker already knows the material bound (`LinkCx::material_top` = max(stock top, `top`)) and uses it for the link height, but not for the descent. It has no in-process stock for this toolpath (t2 does not read `rest_from`), so it cannot know what t1 left.
   - D08 §8.2 and spec 04 §4.14 require links and rapids to be checked against the model and the stock, and raised when blocked. The linker treats the stock as cleared down to `top`.
2. **`holes` t2: 1 rapid in stock** at (39.867, 24.900, −0.900). The Ø5 hole at (40, 25) lies under a Ø10 counterbore (Z −3 to 0 in W) that no toolpath machines. The drill's heights resolve per hole against that hole's selection, so its feed height (the R plane) is −3 + 2 = −1. `drill.rs` rapids down to it, and G83 rapids back to it between pecks.
   - Root cause: the same as 1, in the hole strategies' own approach. `drill.rs` rapids to `h.feed`, and `bore.rs` does too. Neither bounds that by the stock top, although `drill.rs` uses the stock top to decide whether a move between holes is blocked.
3. **`plate2d`: gouges in t4, t2 and t3.**
   - t4 (adaptive pocket), 8 mm at (48.7, 35.3, −9.0): **a real gouge, not caused by the project's depths.** The ray is 0.02 mm inside the island (centre (45, 40), radius 6), and the 8 mm is the island's height. The tool centre comes within 8.85 mm of the island's axis; the tool-centre region keeps 9.0. The G-code has a 3.5 mm straight move from (49.964, 32.466) to (52.483, 34.949) at Z −9. Both ends are 9.02 mm from the axis, and the chord between them dips to 8.85.
     - Root cause: the move is a stay-down link (`Motion::Link`) into the next pass. `link.rs` allows it because it lies inside that pass's safe region, the cleared region shrunk by the tool radius (`adaptive::shrink`). That region is not limited to the tool-centre region: around the island it reaches inside it (the final contour's safe region by 254 mm² on this pocket). For the same reason, `adaptive::clear` begins each pass with a straight move from the nearest point of the shrunk cleared region to the pass start, and `plan_area` makes straight sliver moves. Neither is checked against the tool-centre region, which is not convex around an island.
   - t2 (score, 0.8 mm) and t3 (chamfer, 1.4 mm): by design. A score line 0.8 mm below the model top and a 1.5 mm chamfer on the outer edge are features the flat-topped model does not have, so the simulation reports them as gouges. The depths are the project's (`bottom = model_top − 0.8`, `chamfer_width = 1.5`).

Not in the original report, found while reproducing:

4. **`dome` t5 (steep and shallow): 1 rapid in stock** at (83.308, 5.300, −16.500). `follow.rs` rapids to `safety_distance` above each entry and assumes that a finishing toolpath's surface has been roughed. `dome` has no roughing toolpath. The root cause is the same as 1, in the 3-D linker. `follow.rs` is being changed in another branch, so it is left open here.
5. **`shaft` cannot be simulated**: `Session::simulate` reports `sim.result.missing` for the `manual_nc` stop t5, which has no tool, and builds box stock for a rotary Setup.

## Decisions

- **Rapid floor.** A rapid descends only to `max(feed, material_top + safety_distance)`. Below that the move is a plunge at plunge feed, through material the toolpath cannot see. This is the safe choice: without the in-process stock the generator cannot tell that an earlier toolpath cleared the material between the stock top and `top`. The drill's R plane and the bore's approach use the same floor (stock top + safety distance). A toolpath whose `top` is the stock top is unchanged, since its feed height is above the floor. Recorded in D08 §8.2.
- **Adaptive entries.** An entry move, a sliver move or a stay-down link must lie inside the tool-centre region. The safe regions given to the linker are intersected with it (grown by 10⁻⁴ mm for rounding). The entry is the nearest point of the cleared region from which the straight move stays inside. If there is none, the pass starts at its start point and the linker approaches it from above. A sliver move that would leave the region is not made, and its material counts as left.

## Tasks

### Task 1: Plan

**Files:** Create `docs/plan/2026-11-01-sim-findings.md`.

- [x] **Step 1:** Reproduce, find the root causes, write this plan.

### Task 2: Every reference project in the simulation

**Files:** `crates/fluut-cli/tests/cli.rs`; `crates/fluut-core/src/session.rs`, `crates/fluut-core/tests/simulate.rs`.

- A CLI test runs `fluut sim` at the default 0.2 mm rays on every reference project, one thread per project. Every error must be on an allowed list that gives the reason.
- `Session::simulate` skips `manual_nc` toolpaths (they have no motion) and reports `sim.setup.rotary` for rotary Setups, which it cannot simulate yet.
- [x] **Step 1:** Write the test; it fails on `block`, `holes` and `plate2d` t4. `simulate_block_finds_no_errors_and_caches` now expects no findings.

### Task 3: Rapid floor in the linker and the hole strategies

**Files:** `crates/fluut-cam/src/{link.rs, strategy/drill.rs, strategy/bore.rs}`; D08 §8.2.

- [x] **Step 1:** Tests: a cut whose `top` is below the stock top gets no rapid below stock top + safety; a drill under an unmachined counterbore gets its R plane above the stock.
- [x] **Step 2:** Fail, implement, pass. Re-bless and list the changed files.

### Task 4: Adaptive entries inside the tool-centre region

**Files:** `crates/fluut-cam/src/{adaptive.rs, strategy/adaptive_2d.rs}`.

- [x] **Step 1:** Test: on a pocket with a round island, every move of every pass lies in the tool-centre region (within the tolerance). `clears_the_plate_pocket` checked only the end points before.
- [x] **Step 2:** Fail, implement, pass. Re-bless and list the changed files.

### Task 5: Finish

**Files:** this plan, `docs/plan/README.md`.

- [ ] **Step 1:** Merge `main`, run all checks, mark done, add the README row.

## Re-blessed golden files

Each change replaces a rapid through material, or a move into the part, with a feed or a retract. No coordinate of a cutting move changes.

| File | Change | Why it is safer |
|---|---|---|
| `block-linuxcnc.ngc`, `block-grbl.gcode` | t2: the 7 `G0 Z-5.9` after each rapid at Z 1 (the first from Z 10) are gone; the `G1 Z-7.9 F600` plunge now starts at Z 1 | The tool feeds through the 0.2 mm of wall stock t1 left instead of rapiding through it (finding 1) |
| `holes-linuxcnc.ngc`, `holes-grbl.gcode` | t2, hole at (40, 25): R plane −1 → 1 (`G0 Z1`, `R1`; GRBL's expanded pecks retract to Z 1) | The drill no longer rapids 1 mm into the unmachined Ø10 counterbore (finding 2) |
| `plate2d-linuxcnc.ngc` | t4: one stay-down link `G1 X52.483 Y34.949` at Z −9 becomes a retract to Z 1, a rapid and a plunge at the same point | The link cut 0.15 mm into the island, 8 mm deep (finding 3) |
