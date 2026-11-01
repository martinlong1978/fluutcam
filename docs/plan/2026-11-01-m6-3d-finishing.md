# M6 part 2 — 3-D Finishing Types Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The five 3-D finishing types of D15b §5 on the M3 framework: `contour` (waterline), `horizontal`, `spiral`, `scallop` and `steep_and_shallow`. Each one goes from a STEP file and a TOML project to G-code from the CLI, with a golden file and the translation-invariance test.

**Architecture:**
- `fluut-model`: five new `ToolpathTypeId`s, with the field tables, resolved parameters and validation codes of D15b §5. The additions sit after the M3 types, so the M6 part 1 branch (2-D and hole types) merges by adding lines.
- `fluut-cam::follow` (new): the surface-following building blocks of `parallel`, moved out of `strategy/parallel.rs` and made generic over the drive curve:
  - adaptive CL sampling with exact cut-off points;
  - cut runs;
  - linking with stay-down links, lifts and vertical leads.
  
  `parallel` uses them unchanged; the ridge golden file proves it.
- `fluut-cam::waterline` (new): constant-Z CL contours of the drop function τ(x, y) (D07 §7, D09 §4.4), with seeds, tracing, region clipping, slope classification and the CH-03 handling.
- `fluut-cam::strategy::{contour, horizontal, spiral, scallop, steep_and_shallow}`.
- `fluut-cam::surface`: one addition, a drop that also returns the contact direction and the waterline slope.
- `fluut-cli`: a `dome` reference project, an 80 × 60 × 10 block with a cylindrical boss (r = 12) and a hemispherical top at Z 28. Each type adds one toolpath to it, with golden G-code (`dome-linuxcnc.ngc`), translation invariance and a `fluut calc` check.

**Tech Stack:** as M3. No new crates.

**Design:** [D15b](../design/D15b-toolpath-parameters-2.md) §1, §5, §8, §9; [D15](../design/D15-toolpath-parameters.md) §1, §2, §5; [D09](../design/D09-strategies.md) §4; [D08](../design/D08-toolpath-framework.md); [D07](../design/D07-cutter-queries.md) §1, §7; [challenges](../design/challenges.md) CH-03, CH-14, CH-18; [S2 report](spikes/s2-cutter-queries.md); [M3 parallel plan](2026-11-01-m3-parallel.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Determinism (D01 §6): `libm` for transcendental functions, `BTreeMap` wherever iteration order reaches output. Levels and passes are computed in index order.
- Keep edits to shared files small and additive: the type registry, `ToolpathTypeId`, the field tables, `resolve.rs`, `validate.rs` and the test STEP writer. M6 part 1 changes the same files.
- Do not change the 2-D strategies, `fluut-core`, `fluut-io-*` or `third_party/`. Change `fluut-model` only where a type needs it.
- No DeskProto-derived content. No corpus files in the repository.

## Scope decisions

- **Waterlines from drop queries.** A body plane section (D03 §5.1) with chaining does not exist yet, and a section offset by R is exact only for vertical walls. So `contour` takes its CL contours straight from the drop function: the contour at level z is the boundary of {τ > z}. That is exact for every tool and wall shape.
  - **Seeds.** Seed lines run along X across the region, at most R/2 apart. Each line's τ profile is sampled once, adaptively, as in `parallel`. Every crossing of a level on a line is a seed, refined by bisection. One profile serves all levels, so seeding costs the same for 10 levels as for 1000.
  - **Tracing.** Step along the contour; then correct along the contact direction, which is the unit XY vector from the tool axis to the contact. τ rises along that vector, so it is the contour normal wherever τ is smooth. Use regula falsi with bisection fallback on τ − z, to 1e-7 mm. Each point is taken on the τ ≤ z side, so the tool at z does not gouge.
  - **Step length.** At most R/2, so a feature is not stepped over. The step is halved while the contact direction turns by more than the chord rule allows (sagitta ≤ tol/4). A kink, such as an inside corner, ends with a step of `tol/16`.
  - **Missed loops.** A loop that falls between two seed lines is missed. This is a contour less than R/2 across, near a peak. Such a loop leaves material only within about R²/(32 ρ) of the peak, where ρ is the peak's radius of curvature. Peaks are shallow, so other passes cover them.
  - **Discontinuities.** No gradient is needed. A flat tool against a vertical wall makes τ jump, and the bracket still finds the boundary.
- **CH-03 (tangent levels).** A level at the Z of a horizontal face makes {τ = z} an area under a flat or bull tool, so classifying τ against z there is noise. Every level is therefore classified against z + δ, δ = 1e-6 mm: the section "just above" of the CH-03 workaround. The CL point is output at z. The gouge this allows is δ. A plateau top at the level gives no contour; a floor at the level gives the wall foot. The record goes in `challenges.md`.
- **Waterline slope.** As `parallel` (D15 §5.1), with one difference. An edge or vertex contact on the rim of a flat tool counts as 90°, because τ jumps there (a wall). The slope is taken at the τ > z side of the bracket, which touches the wall. Cut-off points along a contour are bisected along the chord and projected back onto the contour, to 1e-7 mm.
- **Region.** For `contour` and `steep_and_shallow`, the drive faces' XY box grown by R + max(stock, 0) + tol, so the outer wall contours lie inside it. It is then limited by `boundary`, as in `parallel`. `spiral` and `scallop` use the `parallel` region: the box, not the exact silhouette (D03 §5.3 is still owed).
- **Levels** (`contour`): `levels(top, bottom, step_down)`, plus every horizontal planar drive face's Z in (bottom, top]. Gaps are then filled evenly so none exceeds `step_down`.
  - With `scallop_height`, a gap is halved where a cut point at either of its levels has slope β with gap > sin β · √(8 r h). The new level cuts only at points whose own β needed it.
  - Halving stops at gap ≤ max(tol, step_down / 64), because near-flat ground can never satisfy the rule.
- **Order** (`contour`):
  - `by_level`: top to bottom, nearest loop next; a closed loop starts at its point nearest the tool.
  - `by_area`: loops at adjacent levels whose XY boxes overlap form one area. Each area is cut through all its levels, in order of first appearance.
- **Direction:** climb keeps the material ({τ > z}) on the right (spindle M3).
- **Links.** Through `follow`, as `parallel`, with one change. A stay-down link between levels or loops is a straight 3-D line, allowed when no sampled drop along it is above it by more than tol/4; it does not follow the surface.
- **`horizontal`.**
  - Flats are the horizontal planar drive faces with Z in [bottom, top]. Faces at equal Z, within `tol_model`, are cleared together.
  - Machinable region at the flat: {τ ≤ z_flat + stock + δ}, from the waterline with the cutter grown by `stock_to_leave`. That is the section just above the flat, offset by R + stock (D08 §7.1), and it is exact for sloped walls too.
  - Area: the face region offset by −R, intersected with the machinable region.
  - Cut at z_flat + `stock_to_leave_axial`. Offset rings (outside in) or a raster, as `pocket_2d`, and linked with the 2-D linker.
  - The ring generator is a small copy, because `pocket_2d` may not change in this part.
- **`spiral`.** The exact Archimedean spiral r = s'·φ/2π, evaluated directly; the output is lines through CL points, so the biarc chain of D09 §4.6 is not needed.
  - Region clips are bisected on the angle to 1e-7 mm.
  - Sampling as `parallel`, plus the chord's sagitta in XY ≤ tol/8.
  - `machine_corners` samples the region boundary as a last pass.
  - The scallop stepover is computed as in `parallel`.
- **`scallop`.**
  - Pass 0 is the region boundary, dropped.
  - Pass k+1 is the region offset by the running sum of steps, where step k = s · cos θ_max and θ_max is the largest cut slope on passes k and k+1. The step is recomputed until it is stable, at most 4 times.
  - A step reduced because pass k+1 is steeper is an inserted pass: `cam.scallop.extra_passes` (CH-18).
  - The step is never below s/10, because a vertical wall can never satisfy s · cos θ.
  - `continuous` ends each loop one step early and links straight, along the surface, to the start of the next.
- **`steep_and_shallow`.**
  - Steep part: `contour` with slope limits [threshold, 90].
  - Shallow region: the cut runs of a probe `parallel` at the shallow stepover (pass angle 0, slope limits [0, threshold]), each widened into a strip of the stepover, unioned and offset by `overlap`. Across the passes this is the shallow area to within the stepover; along them it is exact.
  - The shallow part is `parallel` or `scallop` in that region, with no slope limit. `region_order` picks which part goes first.
- **Tool shapes.** Flat, ball and bull, as `parallel`. Others give `cam.<type>.tool_shape` (E). `horizontal` warns for ball tools (`param.horizontal.tool_shape`).
- **Mesh faces.** `param.horizontal.drive_faces.mesh` uses the `parallel` mesh test: every face is a planar triangle.

## Review Focus

1. **No gouges:** dense drops along every cut and link segment are never above it by more than the tolerance, for every type and tool shape.
2. **Waterline exactness:** CL contours on a vertical cylinder, a sphere and a cone match the closed forms within 1e-6 mm, and a flat level gives the wall foot (CH-03).
3. **Scallop:** the 3-D distance between scallop passes is ≤ s; the scallop height holds on a ball tool.
4. **Climb and conventional** directions per type (spindle M3).
5. **Translation invariance** on the dome project, for every type.
6. **`parallel` unchanged:** the ridge golden file and its tests pass without changes.

---

### Task 1: Plan

- [ ] **Step 1:** This plan; README table row. Commit.

### Task 2: Model: five types

**Files:** `crates/fluut-model/src/params/{meta.rs, tables.rs}`, `src/resolve.rs`, `src/validate.rs`; tests in `crates/fluut-model/tests/{finishing_3d.rs}`.

- Field tables, resolved parameters and the parameter-only validation codes of D15b §5.1–§5.5.
- [ ] **Step 1:** Tests: each D15b §5 example parses, resolves and is written back byte for byte; every parameter-only code has a passing and a failing case; R fields change the result hash.
- [ ] **Step 2:** Implement; pass; commit.

### Task 3: `follow` and `contour`

**Files:** Create `crates/fluut-cam/src/{follow.rs, waterline.rs, strategy/contour.rs}`; modify `strategy/parallel.rs` (use `follow`), `surface.rs` (contact drop), `toolpath.rs`; tests in `crates/fluut-cam/tests/{support/finish.rs, contour.rs}`, and `support/step.rs` (sphere faces, the dome part); `crates/fluut-cli/tests/{cli.rs, projects/dome/…, golden/dome-linuxcnc.ngc}`.

- [ ] **Step 1:** Tests:
  - waterlines of a vertical cylinder, a sphere and a cone against closed forms;
  - a flat level gives the wall foot (CH-03);
  - levels include flats, and the scallop-adaptive levels;
  - slope limits with exact cut-off points;
  - climb and conventional;
  - `by_level` and `by_area`;
  - the D15b §5.1 codes (`cam.contour.loop_open` too);
  - no gouges with any tool;
  - translation invariance;
  - its toolpath in the dome project: STEP matches the writer, project canonical, golden G-code, translation invariance in the CLI.
- [ ] **Step 2:** Implement; the `parallel` tests and the ridge golden file pass unchanged; commit.

### Task 4: `horizontal`

**Files:** Create `crates/fluut-cam/src/strategy/horizontal.rs`; tests in `crates/fluut-cam/tests/horizontal.rs`.

- [ ] **Step 1:** Tests:
  - flats only, at Z + axial stock;
  - walls kept at R + stock;
  - offset and raster;
  - flats at equal Z cleared together;
  - the D15b §5.2 codes;
  - no gouges;
  - translation invariance;
  - its toolpath in the dome project: STEP matches the writer, project canonical, golden G-code, translation invariance in the CLI.
- [ ] **Step 2:** Implement; pass; commit.

### Task 5: `spiral`

**Files:** Create `crates/fluut-cam/src/strategy/spiral.rs`; tests in `crates/fluut-cam/tests/spiral.rs`.

- [ ] **Step 1:** Tests:
  - even pitch reaching r_max;
  - centre, rotation and direction;
  - exact region clips;
  - `machine_corners`;
  - the scallop height;
  - the D15b §5.3 codes;
  - no gouges;
  - translation invariance;
  - its toolpath in the dome project: STEP matches the writer, project canonical, golden G-code, translation invariance in the CLI.
- [ ] **Step 2:** Implement; pass; commit.

### Task 6: `scallop`

**Files:** Create `crates/fluut-cam/src/strategy/scallop.rs`; tests in `crates/fluut-cam/tests/scallop.rs`.

- [ ] **Step 1:** Tests:
  - 3-D stepover ≤ s on a dome;
  - extra passes reported;
  - `direction`, `cut_direction` and `continuous`;
  - slope limits;
  - the D15b §5.4 codes;
  - no gouges;
  - translation invariance;
  - its toolpath in the dome project: STEP matches the writer, project canonical, golden G-code, translation invariance in the CLI.
- [ ] **Step 2:** Implement; pass; commit.

### Task 7: `steep_and_shallow`

**Files:** Create `crates/fluut-cam/src/strategy/steep_and_shallow.rs`; tests in `crates/fluut-cam/tests/steep_and_shallow.rs`.

- [ ] **Step 1:** Tests:
  - steep cuts only at slope ≥ threshold;
  - the shallow region with overlap;
  - both shallow patterns;
  - `region_order`;
  - the D15b §5.5 codes;
  - no gouges;
  - translation invariance;
  - its toolpath in the dome project: STEP matches the writer, project canonical, golden G-code, translation invariance in the CLI.
- [ ] **Step 2:** Implement; pass; commit.

### Task 8: Timing and close

- [ ] **Step 1:** Time per output point on the largest NIST body (outside the repository), release build, one thread.
- [ ] **Step 2:** CH-03 findings in `challenges.md`; design changes in D09 §4; new codes in D15b §8. Mark this plan done with deviations; README rows. Commit and push.
