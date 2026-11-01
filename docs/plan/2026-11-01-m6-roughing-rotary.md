# M6 — Roughing, Rotary and Utility Types Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The third part of M6. It adds:
- in-process stock (D08 §7, spec 04 §4.12.6, OQ-11);
- `rest_from = "previous"` for `pocket_2d` and `adaptive_2d`;
- the 3-D roughing types `adaptive_3d` and `pocket_3d` (D15b §4);
- the rotary types `rotary_rough`, `rotary_along_x`, `rotary_around_a` and `rotary_spiral` (D15b §6);
- `manual_nc` (D15b §7).

Each type gets its parameters, validation, strategy, a golden G-code reference project and an invariance test.

**Architecture:**
- `fluut-model`:
  - a field table, a resolved struct and the D15b validation rules for each type;
  - an X position field kind (`{ ref, offset }` with the D15b §1.4 references) and a text field kind (for `manual_nc.text`);
  - the height rules for rotary Setups (D15b §1.3): radii, `below_axis`;
  - which types update and read in-process stock (D15b §1.10);
  - `PostCaps` learns about `manual_nc` blocks, and `ValidateCx` about the machine's rotary axis.
- `fluut-cam`:
  - `stock.rs`: `LayeredStock`, planar or cylindrical, updated from a `ToolpathResult`;
  - `section.rs`: plane sections of the Setup's bodies as exact 2-D regions;
  - `radial.rs`: radial drops for rotary Setups, from the S2 drop query on the bodies turned about X;
  - one strategy module per type in `strategy/`.

  Framework edits stay small and additive:
  - `StrategyInput.in_process`: the in-process stock after the earlier toolpaths in the Setup;
  - `StrategyInput.rotary`: the rotary data of a rotary Setup (stock cylinder, X range of the stock).
- `fluut-core`:
  - the frame of a `rotary` Setup (spec 03 §3.9.3);
  - in-process stock carried from toolpath to toolpath in a Setup;
  - result hashes that include the earlier toolpaths a `rest_from` toolpath reads;
  - the Setup kind passed to the post.
- `fluut-post`: a toolpath with no motion (only markers) emits its markers and nothing else, so `manual_nc` changes no tool.
- `fluut-cli`: reference projects with golden G-code and invariance tests.

**Tech Stack:** as M3. No new crates.

**Design:** [D15b](../design/D15b-toolpath-parameters-2.md) §1, §4, §6, §7, §9; [D15](../design/D15-toolpath-parameters.md) §1.9, §2.3; [D08](../design/D08-toolpath-framework.md) §7; [D09](../design/D09-strategies.md) §4.1, §4.2, §5, §6.1; spec 03 §3.9, §3.10.5; spec 04 §4.4, §4.7, §4.12.6; [S3 report](spikes/s3-region-2d.md); [S5 report](spikes/s5-simulation.md); M6 part 1 [plan](2026-11-01-m6-2d-holes.md); M4 [plan](2026-11-01-m4-model-persistence.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask check-layers` and `cargo deny check`.
- Determinism (D01 §6): `libm` for transcendental functions, `BTreeMap` wherever iteration order reaches output.
- M6 part 2 (`m6-3d-finishing`) edits the type registry, `ToolpathTypeId`, the field tables and validation at the same time. Edits to these shared places are additive, so the merge is mechanical.
- Do not edit the 3-D finishing types, `fluut-app`, `fluut-io-*` or `third_party/`.
- No DeskProto-derived content.
- One commit per finished part.

## Scope decisions

- **In-process stock** is `LayeredStock` (D08 §7). The region stored at key `z` is the material in the slab from the next key below up to `z`. A cut with its tip at `z` clears the slabs above `z`. A level inside a slab splits it, and the new lower slab copies the old one (D08 `ensure_level`).
  - **Planar stock:** the Setup's stock box, sliced at the levels where toolpaths cut.
  - **Cylindrical stock:** layers at constant radius. Each region is drawn in (x, θ), with θ in radians over [0, 2π). This is the unrolled cylinder of spec 04 §4.12.6 with θ·r scaled by 1/r. The scale changes from layer to layer, and the rotary sweeps cut regions with straight sides only, so the regions stay exact.
- **Updating stock from a result.** The stock is updated from the `ToolpathResult` rather than from each strategy, so every type that updates stock (D15b §1.10) uses the same code:
  - Feed moves are grouped into runs at one Z. The swept region of a run is the union of capsules of radius R around its moves. Arcs are split into chords within the machining tolerance, and the radius is reduced by that tolerance, so the stock never loses material that was not cut.
  - A ramp or helix counts at its highest Z.
  - A drill cycle removes a disc of the tool radius at its depth.
  - The tool clears its full radius only above its corner or cone height. A cut is therefore applied that much above the tip, which keeps material rather than losing it.
  - For cylindrical stock, a rotary move at radius r clears the strip [x − R, x + R] × [θ₀, θ₁] above r.
- **Which toolpaths count.** The in-process stock of a toolpath comes from the earlier enabled toolpaths in its Setup whose type updates stock. Stock carried into a child Setup (`from_previous`, re-sliced for an indexed child) is not in this plan: a `from_previous` child still starts from the parent's stock box, as in M4.
- **Hashes.** The result hash of a toolpath that reads in-process stock includes the result hashes of the earlier stock-updating toolpaths in its Setup and whether each is enabled. Toolpaths that do not read stock keep their hash.
- **`rest_from` in 2-D.** At each level the region is cut to the remaining stock grown by R (D15b §2.2). For `adaptive_2d`, the area outside the remaining stock counts as cleared, as in an open pocket. Levels with the same remaining stock share one path search.
- **3-D sections.** `machinable(z)` comes from plane sections of the Setup's solids. Each face's surface is cut by the plane, the pieces are clipped to the face's trim, joined into closed loops, and projected onto XY as lines and arcs:
  - **Flats (CH-03).** The section is taken `δ` = 1e-4 mm above the level, so a level at a flat's height sees the walls above it.
  - **Material above the level.** The forbidden region at a level is the union of the forbidden regions of every level at or above it. A part that is wider higher up (an overhang) is then never cut from below, at least at the level heights.
  - **Excluded faces** still close the section loops. Only non-horizontal excluded faces would change the result, and v1 does not handle that case.
  - **Stock to leave.**
    - When every face is a drive face, the section is grown by R + `stock_to_leave`.
    - With check faces, the section is grown by R + min(`stock_to_leave`, 0). Then the drive pieces are swept by R + `stock_to_leave`, or the check pieces by R (decision 6), whichever applies.
- **3-D levels.** The levels are the even levels of `top`…`bottom`, plus, with `flat_detection`, the Z + `stock_to_leave_axial` of every horizontal planar face between them. `order = "by_area"` is treated as `by_level`, because the areas change from level to level.
- **`adaptive_3d` fine step-down.** The fine levels between two main levels are cut after the lower main level, top down. Each clears the remaining stock within reach at that level. Parts narrower than `fine_step_down` (a morphological opening) are left: these are the steps on steep walls.
- **Rotary frame.** The work frame W has X along the rotary axis, and Y and Z zero on it. The tool is at machine (x, 0, r) with A = θ, and touches the part point (x, r·sin θ, r·cos θ) of W (D09 §5). The rotary axis line comes from a cylinder or cone face (its axis), a straight edge, or a model axis through the model origin. `a_zero_toward` defaults to +Z. A `cylinder` stock is centred on the bodies' X extent. Rotary heights are radii (D15b §1.3):
  - `model_top` and `model_bottom` come from edge samples of the faces in the X range. Planes parallel to the axis add their distance to the axis. Planes normal to the axis (the ends) are left out.
  - `selection` uses the drive faces in the same way.
- **Radial drop.** Rotary types need the radial drop of spec 04 §4.7.2. The S2 drop query is vertical, so the bodies are turned by −θ about X and a vertical drop is made at (x, 0). Turned query geometries are built once per angle and kept for the toolpath. Angles lie on a grid with Δθ from the tolerance at the largest radius (spec 04 §4.7.4), and every type samples on that grid.
- **`rotary_rough`** does not section the bodies by cylinders. At radius r the tool may stand at (x, θ) when the radial drop there, with `stock_to_leave`, is at or below r. This uses the true tool shape, so the unrolled offset of CH-19 is not needed, and `cam.rotary_rough.unrolled_offset` is never raised. The θ ends of each sweep are on the angle grid.
- **Rotary output.** Cuts at fixed A are `Line` moves with `a` set, and A changes are `Rotary` segments with the CL radius. Runs of samples at the same radius (within 1e-7 mm) become one `Rotary` segment, which is the coaxial fast path of spec 04 §4.7.3. Links lift radially to the retract radius.
- **`manual_nc`** has no tool. Validation, calculation, the hash and the post skip the tool. The post keeps the active tool, spindle and coolant: a toolpath with only markers emits only its markers. `param.manual_nc.post` needs the post's `manual_nc` block, so `PostCaps` gets a `manual_nc` flag.

## Review Focus

1. **Stock never loses uncut material:** updates are conservative (chords, corner heights, ramps at their top).
2. **Exact level ends:** flat levels at face Z + `stock_to_leave_axial`; rotary layer radii and the spiral end exact at `x_end`.
3. **Rest roughing** clears only remaining stock, and a second identical roughing cuts nothing.
4. **Rotary output:** A words through `fluut/mixin/rotary-a`, rotary feed per `feed.rotary_method`, and one rotary line per coaxial cylinder sweep.
5. **Invariance:** translation for 3-axis projects; moving the part along X for rotary projects.

---

### Task 1: Plan

- [ ] **Step 1:** This plan; README table row. Commit.

### Task 2: In-process stock

**Files:** `fluut-cam/src/stock.rs`; `fluut-cam/src/toolpath.rs`; `fluut-core/src/session.rs`, `graph.rs`; `fluut-model/src/params/meta.rs`.

- [ ] **Step 1:** Tests: initial planar and cylindrical stock; `ensure_level` copies the slab above; a cut at z clears only the slabs above z; swept regions of lines, arcs, helices and cycles; ball and drill tools apply their cut above the tip; the result of `pocket_2d` removes its pocket; hashes change with earlier toolpaths only for readers.
- [ ] **Step 2:** Implement; pass. Commit.

### Task 3: `rest_from` in `pocket_2d` and `adaptive_2d`

- [ ] **Step 1:** Tests: `param.geometry.rest_from.unsupported` is gone; a rest pocket after a larger pocket cuts only the ring left; a repeated roughing cuts nothing; adaptive rest enters from cleared material.
- [ ] **Step 2:** Implement. Commit.

### Task 4: Sections and `pocket_3d`, `adaptive_3d`

- [ ] **Step 1:** Tests: sections of a block, a pocket, a boss, a cylinder; flat levels; each D15b §4 code passes and fails; the §4 examples round-trip; no CL point inside the part grown by R + stock to leave; rest from a previous roughing.
- [ ] **Step 2:** Implement; reference project with both types; golden; translation invariance. Commit.

### Task 5: Rotary frame and rotary types

- [ ] **Step 1:** Tests: rotary frame from a face axis; radii; radial drop on a cylinder and a box; each D15b §1.3, §1.4 and §6 code passes and fails; §6 examples round-trip; around-A sweeps on a coaxial cylinder are one rotary segment; the spiral ends at `x_end`; `rotary_rough` keeps clear of the part.
- [ ] **Step 2:** Implement; reference project posted with a 4-axis post; golden; invariance under moving the part along X. Commit.

### Task 6: `manual_nc`

- [ ] **Step 1:** Tests: example round trip; each code; markers in G-code with no tool change.
- [ ] **Step 2:** Implement; add to a reference project. Commit.

### Task 7: Close

- [ ] **Step 1:** Mark this plan done with deviations; README rows. Push.
