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

- [x] **Step 1:** This plan; README table row. Commit.

### Task 2: Model: five types

**Files:** `crates/fluut-model/src/params/{meta.rs, tables.rs}`, `src/resolve.rs`, `src/validate.rs`; tests in `crates/fluut-model/tests/{finishing_3d.rs}`.

- Field tables, resolved parameters and the parameter-only validation codes of D15b §5.1–§5.5.
- [x] **Step 1:** Tests: each D15b §5 example parses, resolves and is written back byte for byte; every parameter-only code has a passing and a failing case; R fields change the result hash.
- [x] **Step 2:** Implement; pass; commit.

### Task 3: `follow` and `contour`

**Files:** Create `crates/fluut-cam/src/{follow.rs, waterline.rs, strategy/contour.rs}`; modify `strategy/parallel.rs` (use `follow`), `surface.rs` (contact drop), `toolpath.rs`; tests in `crates/fluut-cam/tests/{support/finish.rs, contour.rs}`, and `support/step.rs` (sphere faces, the dome part); `crates/fluut-cli/tests/{cli.rs, projects/dome/…, golden/dome-linuxcnc.ngc}`.

- [x] **Step 1:** Tests:
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
- [x] **Step 2:** Implement; the `parallel` tests and the ridge golden file pass unchanged; commit.

### Task 4: `horizontal`

**Files:** Create `crates/fluut-cam/src/strategy/horizontal.rs`; tests in `crates/fluut-cam/tests/horizontal.rs`.

- [x] **Step 1:** Tests:
  - flats only, at Z + axial stock;
  - walls kept at R + stock;
  - offset and raster;
  - flats at equal Z cleared together;
  - the D15b §5.2 codes;
  - no gouges;
  - translation invariance;
  - its toolpath in the dome project: STEP matches the writer, project canonical, golden G-code, translation invariance in the CLI.
- [x] **Step 2:** Implement; pass; commit.

### Task 5: `spiral`

**Files:** Create `crates/fluut-cam/src/strategy/spiral.rs`; tests in `crates/fluut-cam/tests/spiral.rs`.

- [x] **Step 1:** Tests:
  - even pitch reaching r_max;
  - centre, rotation and direction;
  - exact region clips;
  - `machine_corners`;
  - the scallop height;
  - the D15b §5.3 codes;
  - no gouges;
  - translation invariance;
  - its toolpath in the dome project: STEP matches the writer, project canonical, golden G-code, translation invariance in the CLI.
- [x] **Step 2:** Implement; pass; commit.

### Task 6: `scallop`

**Files:** Create `crates/fluut-cam/src/strategy/scallop.rs`; tests in `crates/fluut-cam/tests/scallop.rs`.

- [x] **Step 1:** Tests:
  - 3-D stepover ≤ s on a dome;
  - extra passes reported;
  - `direction`, `cut_direction` and `continuous`;
  - slope limits;
  - the D15b §5.4 codes;
  - no gouges;
  - translation invariance;
  - its toolpath in the dome project: STEP matches the writer, project canonical, golden G-code, translation invariance in the CLI.
- [x] **Step 2:** Implement; pass; commit.

### Task 7: `steep_and_shallow`

**Files:** Create `crates/fluut-cam/src/strategy/steep_and_shallow.rs`; tests in `crates/fluut-cam/tests/steep_and_shallow.rs`.

- [x] **Step 1:** Tests:
  - steep cuts only at slope ≥ threshold;
  - the shallow region with overlap;
  - both shallow patterns;
  - `region_order`;
  - the D15b §5.5 codes;
  - no gouges;
  - translation invariance;
  - its toolpath in the dome project: STEP matches the writer, project canonical, golden G-code, translation invariance in the CLI.
- [x] **Step 2:** Implement; pass; commit.

### Task 8: Timing and close

- [x] **Step 1:** Time per output point on the largest NIST body (outside the repository), release build, one thread.
- [x] **Step 2:** CH-03 findings in `challenges.md`; design changes in D09 §4; new codes in D15b §8. Mark this plan done with deviations; README rows. Commit and push.

**Done.** The five 3-D finishing types of D15b §5 (`contour`, `horizontal`, `spiral`, `scallop`
and `steep_and_shallow`) run on the M3 framework. Each one goes from STEP and TOML to
G-code: the `dome` project (`crates/fluut-cli/tests/projects/dome/`) has one toolpath of each
type, with golden `fluut/linuxcnc` output. Translation invariance holds for every type in
`fluut-cam` and in the CLI, with identical G-code and coordinates within 1e-6 mm. The branch
is merged with `main` (M4, M5, M6 part 1); all fifteen types are registered.

- **Waterlines.** Contours of a vertical cylinder, a sphere and the block walls match the
  closed forms within 1e-6 mm for ball, bull and flat tools, once the δ of CH-03 is allowed
  for. The level at the block top (CH-03) gives exactly the foot of the boss wall. The seam
  cut-offs between a drive and a check half-cylinder are within 1e-6 mm of the seam. Pocket
  inside corners close.
- **Scallop.** On the dome every pass point is within s + tol (3-D) of the previous pass.
- **Timing** (release build, outside the repository). Largest NIST body
  `nist_ctc_02_asme1_rc` (663 faces, 680 × 860 × 400 mm), Ø6 ball (Ø6 flat for `horizontal`),
  every face a drive face, tolerance 0.01 mm. Each type ran single-threaded, but up to five
  ran side by side on four cores, so the times are up to about 25 % high (`contour` and
  `horizontal` were timed again, after the last fix, with only two runs):

  | Type | Settings | Time | Output CL points | Per point |
  |---|---|---|---|---|
  | `spiral` | stepover 2 | 49.9 s | 278 608 | 179 µs |
  | `scallop` | stepover 2 | 255.2 s | 1 048 560 | 243 µs |
  | `contour` | step-down 20 (20 levels) | 946.8 s | 523 261 | 1 809 µs |
  | `steep_and_shallow` | step-down 20, stepover 2 | 958.3 s | 638 595 | 1 501 µs |
  | `horizontal` | Ø6 flat, stepover 3 | 1 212.3 s | 4 678 | (259 ms) |
  | `contour` (waterline plan) | step-down 20, one thread | 143.5 s | 79 288 | 1 810 µs |
  | `contour` (waterline plan) | step-down 2, one thread | 781.2 s | 656 974 | 1 189 µs |
  | `contour` (waterline plan) | step-down 2, 12 threads | 138.7 s | 656 974 | 211 µs |
  | `steep_and_shallow` (waterline plan) | as above, 12 threads | 67.5 s | 193 743 | 348 µs |
  | `horizontal` (waterline plan) | as above, 12 threads | 338.6 s | 4 678 | (72 ms) |

  The rows marked "waterline plan" are after [the waterline follow-up](2026-11-01-waterline-robust.md);
  the old `contour` row counted about 450 000 runaway points.

  `horizontal` writes few points (lines and rings on flats), so its time per point means
  little. Its time goes into one waterline per flat height.

  `spiral` and `scallop` cost about as much as `parallel` (180–197 µs, M3). Waterlines
  cost about ten times more. Each traced point needs a bracketed search of about 30 drops,
  and drops at walls cost 50–90 µs here, against a 16 µs mean over the body. Seeding (one
  τ profile per line, 1.4 M drops) takes 42 s. At a 2 mm step-down (200 levels) `contour`
  had not finished after 57 minutes and was stopped. The levels cost 15–90 s each, rising where the part
  has more contours.

  On this body many traces are lost at middle heights: 30 of 40 contours at one level end
  inside the region and report `cam.contour.loop_open`. One level ran a single contour to
  300 000 points before the closure fix in the last commit. The losses are probably drops
  that are inconsistent along a contour on its free-form faces (CH-12); this is not yet
  proven. Making the trace tolerate them is the first follow-up.

  Run it with `cargo run -p fluut-cam --example finishing --release`, `FLUUT_STEP_CORPUS` set,
  and `FLUUT_TYPES` / `FLUUT_STEP_DOWN` to choose.

Deviations from this plan:
- **No cone test.** The waterline closed forms are checked on a vertical cylinder, a sphere
  and box walls and edges; the test parts have no cone.
- **Region margin.** As planned, but the boundary loops of offsets start at their vertex
  nearest the region's lower-left corner (`scallop`). The offset library's start vertex is
  not translation invariant, and the first loop decided every later one.
- **Cliffs.** Not planned: at a vertical wall τ jumps and the drop's contact is on top of the
  wall. Slope and drive face then come from `SurfaceQuery::wall_at`, the face the side of the
  tool touches at the level. Without it, drive and check faces on a boss were mixed up.
- **Inside corners.** The tracer falls back to a circle search when the correction along the
  contact direction finds nothing. Contours in a pocket with sharp corners were lost
  without it.
- **Level links** go across at the upper level, checked against that level's traced
  contours, then straight down. A sloping 3-D link was planned, and it gouged a steep wall.
- **Loop closure** tolerates a lap that misses its seed by up to a quarter step, heading
  the same way, after a runaway contour on the corpus.
- **`cam.contour.loop_open`** is also raised when a contour is cut short at a slope or
  drive-face limit inside the region (the D15b wording), not only when a trace is lost.
- **`horizontal`** copies the `pocket_2d` ring generator, which may not change in this part;
  it can be shared after the merge.
- **`spiral`.** The sampler also checks the drop at the middle of each chord of a curved
  drive, and its sagitta. Loop drives sample their corners exactly. Closed runs are kept
  (the `parallel` filter dropped them).
- **`scallop` `continuous`** ends a fully cut loop one step early and stays down to the next
  loop's nearest point. That is the blend, made with the linker's stay-down policy.
- **`steep_and_shallow`.** As planned, but level (steep) and surface (shallow) runs are
  never joined by a stay-down link.
- **Gouge measure in tests.** Depth along the contact normal, (τ − z)·cos β. Level,
  vertical and near-vertical segments (cliff chords) are measured sideways. The vertical
  error overstates gouges on steep ground: a merged chord within tol/4 in 3-D can be 1.5·tol
  off vertically on a 6:1 slope.
- **Fixes outside the strategies:**
  - flat and bull drops on a convex sphere get a closed form (D07), because the numeric
    seeding missed near the seam;
  - Setup bounds and face Z ranges include curved face interiors, because a sphere cap rose
    above `model_top`;
  - the test STEP writer told the two halves of a circle apart (`main` fixed the same bug;
    its version is kept);
  - `analytic_uv` is `pub(crate)`.
  
  No golden file of the other projects changed.
- **New codes:** in D15b §8.
- **Branches.** The work is on `m6-3d-finishing`; the same commits are on this session's
  branch.

Follow-ups:
- Lost waterline traces on free-form corpus faces (see Timing). Re-seed a lost trace from
  its last good point, and test against the CH-12 reference drop.
- Speed. Waterlines cost a bracketed search per point (about 20–30 drops). Warm-starting
  from the previous point's bracket, a step-size predictor from curvature, and `rayon` over
  levels are the obvious steps. `horizontal` traces one waterline per flat height; on a
  part with many flats that dominates.
- The exact drive-face silhouette (D03 §5.3), for the `spiral` and `scallop` regions.
- Arc fitting of contour loops (D08 §5), and arcs from single analytic contacts (D09 §4.4).
- Plane sections (D03 §5.1) for `pocket_3d` and `adaptive_3d`, which can reuse the CH-03
  findings.
- Share the offset-ring generator between `pocket_2d` and `horizontal`.
