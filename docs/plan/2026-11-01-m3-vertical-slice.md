# M3 — Vertical Slice Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A STEP file plus a TOML project go to G-code from the command line, and the translation-invariance test passes (roadmap exit criterion). This plan covers all of M3 except the `parallel` strategy, which needs the S2 drop queries and is added after S2 merges.

**Architecture:**
- `fluut-model` (layer 3) gets the minimal project document and the D15 parameter metadata:
  - Parameter groups are stored as ordered, metadata-checked values (`ParamMap`). Load, save and the result hash all come from one field table per type, so they cannot drift apart.
  - Numbers remember whether the file wrote them as integers or floats, so a file round-trips byte for byte.
  - Typed views (`Resolved*` structs) are built from the values with D15 defaults at calculation time.
- `fluut-topo` gains a `WireBody` (curves only, spec 03 §3.2.1).
- `fluut-cam` (layer 4) gets the D08 framework:
  - the `ToolpathType` trait and the type registry;
  - drive chains from selections; levels and layout;
  - linking with retract policies, leads, ramps with fallback, and feeds;
  - output fitting options (`output_arcs`, `max_segment_length`);
  - the strategies `contour_2d` and `pocket_2d`.

  `parallel` has a registry slot that reports "not available" until S2.
- `fluut-core` (layer 6) gets:
  - the model store: STEP import with ids;
  - Setup frames, stock and the height environment;
  - the invalidation graph with node hashes;
  - an in-memory result cache, calculation jobs, and post jobs through `fluut-post`.
- `fluut-cli` (layer 8) adds the `fluut` binary with `calc` and `post`.

**Tech Stack:** as M2 and S3. `fluut-model` adds `serde` (only for the tool library) and `sha2`. No new external crates.

**Design:** [D15](../design/D15-toolpath-parameters.md) (all), [D06](../design/D06-project-model.md) §3, §7, [D08](../design/D08-toolpath-framework.md), [D09](../design/D09-strategies.md) §2.2, §2.4, [D01](../design/D01-workspace-architecture.md) §4–§5, [D10](../design/D10-post-engine.md) §4.6; spec 02 §2.1, 03 §3.10, 04 §4.12–4.13.

## Global Constraints

All earlier Global Constraints apply. In addition:

- Determinism (D01 §6): `libm` for transcendental functions, `BTreeMap` wherever iteration order reaches output, no clock in output (the CLI takes `--date`/`--time`, defaulting to the current UTC time only when not given).
- Do not modify `crates/fluut-cam/src/cutter/`, `crates/fluut-io-step/src/` internals, `crates/fluut-sim` or `third_party/` (parallel sessions).
- No DeskProto-derived content. No corpus files in the repository.

## Scope decisions

- **Setups:** `three_axis` only. Other kinds load and give `setup.kind.unsupported` (E).
  - Setup fields: `id`, `name`, `kind`, `machine`, `material`, `work_offset`, `geometry = { bodies }`, `orientation = { tool_axis, x_axis }` (signed model axes only), `stock = { kind = "block", margin }`, `zero` (`stock_top_corner`, `stock_top_centre`, `stock_bottom_corner`, `point`) and `clearance = { kind = "plane", offset }`.
  - These fields are recorded in D06 §1.1.
  - `based_on` and inheritance are M4.
- **Entity ids:** body `b-<step id>`, face `f-<step id>.<face index>`, edge `e-<step id>.<edge index>`. The ids are stable for an unchanged file. Re-import matching (spec 03 §3.5) is M4.
- **WireBody input:** the STEP reader maps solids and sheets only, and `fluut-io-2d` is M7. M3 builds WireBodies in code (tests) through the model store API. Golden projects use solids: walls, edges and floor faces.
- **In-process stock:** out of scope (needs D08 §7 `LayeredStock`). `rest_from = "previous"` is the validation error `param.geometry.rest_from.unsupported` (E).
- **Minimum retract:** without drop queries (S2), the material bound along a link is the higher of the stock top and the toolpath top. The retract height is `min(retract, bound + safety_distance)`. S2 replaces the bound with drops along the link.
- **Gouge checks of leads:** deferred. 2-D types do not check against `check_faces` (D15 decision 5).
- **Machine definitions:** not loaded in M3. Validation rules that need the machine run only when the caller passes machine limits.
- **Result cache:** in memory. The disk sidecar is M4.
- **Hash:** `sha2` (SHA-256), already in the workspace; D01 §4 names `blake3`. Recorded in D01.

## Review Focus

1. **Byte-identical round trip** of every D15 example (comments stripped), integer/float kept as written.
2. **R/P split:** every P field leaves the result hash unchanged; every R field changes it.
3. **Exact ends:** level heights hit `bottom + stock_to_leave_axial` exactly; tab ends exact; pass endpoints on offset loops.
4. **Climb/conventional** orientation for inside, outside, left, right and pocket rings (spindle M3).
5. **Translation invariance:** moving model and zero by (0.123457, −0.987651, 0.31) gives identical G-code and coordinates within ε_s.

---

### Task 1: Plan

- [x] **Step 1:** This plan; README table row. Commit.

### Task 2: Parameter metadata and values (`fluut-model`)

**Files:** Create `crates/fluut-model/src/params.rs`, `src/params/{meta.rs, value.rs, tables.rs, tests.rs}`.

- `FieldMeta { key, group, kind, default: DefaultRule, class: Class, expr }`, `Class { R, P, N }`, `Group { ToolParams, Geometry, Heights, Passes, Linking }`, `FieldKind` (Length, Feed, Speed, Angle, Count, Bool, Enum, Height, Selection, Point, Points, Table(sub-fields)).
- `ParamMap`: values in metadata order. `Num { v: f64, int: bool }` keeps the TOML integer/float form.
- Per-type field tables for `contour_2d`, `pocket_2d`, `parallel`. Each table holds the common fields the type uses, in the canonical order (this order matches the D15 examples).
- Load from `toml::Value`:
  - A wrong TOML type or an enum value that is not in the list is an error.
  - An unknown key in `passes`, or a group key the type does not declare, is an error.
  - An unknown key in the other groups is a warning.
- [x] **Step 1:** Tests: every kind parses and rejects a wrong type; unknown keys; undeclared common key (`boundary` on `contour_2d`); int/float kept.
- [x] **Step 2:** Implement; pass; commit.

### Task 3: Project document, tool library, load and save (`fluut-model`)

**Files:** Create `crates/fluut-model/src/{project.rs, project/{read.rs, write.rs, tests.rs}, tools.rs}`.

- `Project { format, name, units, geometry, setups }`, `GeometrySource { id, path, kind, hash, unit, bodies }`, `Setup` (fields as Scope decisions), `Toolpath { id, name, kind, tool, enabled, template, groups }`.
- The writer uses canonical key order and aligns `=` within a table. Toolpaths are indented 2 spaces and their group tables 4, as in spec 02 §2.1. Defaults and absent keys are not written.
- `ToolLibrary` (spec 02 §2.4) with presets; `ToolRef` = `library#tool`.
- [x] **Step 1:** Tests: each D15 example (§3.4, §4.3, §5.3, read from the D15 file, comments stripped) round-trips byte-identically inside a project; a full project round-trips; format checks (unknown major is an error, newer minor a warning).
- [x] **Step 2:** Implement; pass; commit.

### Task 4: Resolution, validation and the result hash (`fluut-model`)

**Files:** Create `crates/fluut-model/src/{resolve.rs, validate.rs, hash.rs}` and tests.

- `resolve(tp, tool, material) -> ResolvedParams` (typed: tool feeds, geometry, height specs, common and type passes, linking). D15 defaults: const, `FromTool` preset, computed (`0.5·D`, `feed.cut`, …).
- `HeightEnv` → `ResolvedHeights` (Z in W).
- `validate(tp, resolved, cx) -> Vec<Diagnostic>`: one function per D15 code for `contour_2d`, `pocket_2d`, common fields, and the parameter-only `parallel` rules. `Diagnostic { code, severity, message, toolpath, field }`.
- `result_hash(...)` over R fields, the tool geometry, R presets and Setup inputs. P fields are left out.
- [x] **Step 1:** Tests: every code with one passing and one failing case; every P field leaves the hash unchanged and every R field changes it; inch entry round trip to 1e-12 (`Num` from inches).
- [x] **Step 2:** Implement; pass; commit.

### Task 5: Framework (`fluut-cam`)

**Files:** Create `crates/fluut-topo/src/wire.rs`; `crates/fluut-cam/src/{toolpath.rs, chain.rs, levels.rs, link.rs, emit.rs}` and tests.

- `ToolpathType` trait (D08 §1 without the 3-D-only inputs), `registry()`, `StrategyInput`, `SetupGeometry` (bodies in W with ids), `CamError`.
- Chains: selections → curves in W → projected 2-D segments (lines; arcs for circles about Z; biarcs otherwise) → chained within the model tolerance.
- `layout_even`, `layout_fixed`, `levels(top, bottom, step | count)`.
- Linking: entry from clearance; retract `full` / `minimum` / `stay_down`; arc and linear leads; ramps `plunge`, `zigzag`, `helix`, `profile` with fallback helix → zigzag → plunge (`cam.linking.ramp_fallback`); feeds per D08 §9; final retract to clearance.
- Output: `output_arcs = false` linearizes; `max_segment_length` splits lines.
- [x] **Step 1:** Tests: layout and levels; chaining open/closed/reversed curves; each link policy; each lead and ramp, with the fallback; continuity of every result (`ToolpathResult::new`).
- [x] **Step 2:** Implement; pass; commit.

### Task 6: `contour_2d`

**Files:** Create `crates/fluut-cam/src/strategy/contour_2d.rs` and tests.

- Sides `inside`/`outside`/`left`/`right`/`on`; `climb`/`conventional`; compensation `computer`/`control`/`wear` (pass `comp` side; offset by `stock_to_leave` only for `control`).
- Depths (`step_down` or `depth_count`, last level `bottom + stock_to_leave_axial`); finishing passes with `finish_every_level`; spring passes; `start_point`; `order`.
- Tabs `count`/`distance`/`points` with exact ends and ramps; merge of overlapping tabs (`cam.contour_2d.tabs.overlap`); `cam.contour_2d.tabs.moved`.
- Drive curves (WireBody, edges, planar faces) and drive faces (vertical walls: bottom edges).
- `cam.contour_2d.offset_empty`; `param.geometry.drive_faces.kind` for non-wall faces.
- [x] **Step 1:** Tests: orientation per side and direction; offsets and finishing pass radii; level Zs; tab Z profile and exact tab ends; control compensation path and `comp`; walls and edges give the same path; endpoint test (D08 §11).
- [x] **Step 2:** Implement; pass; commit.

### Task 7: `pocket_2d`

**Files:** Create `crates/fluut-cam/src/strategy/pocket_2d.rs` and tests.

- Regions from closed drive chains (nesting → islands) and planar floor faces (loops → region, Z → `selection`).
- Offset pattern (rings at −(R + stl) − k·s, `inside_out`/`outside_in`); raster pattern (layout by `pass_layout`, zig-zag, final contour); `direction`; `stock_to_leave`, `stock_to_leave_axial`.
- `cam.pocket_2d.region.open`, `cam.pocket_2d.region.unreachable`.
- [x] **Step 1:** Tests: ring count and spacing ≤ stepover (D08 §11 stepover test); raster line ends on the region boundary; island kept; floor Z; ramp helix fits or falls back.
- [x] **Step 2:** Implement; pass; commit.

### Task 8: `fluut-core` session, graph and jobs

**Files:** Create `crates/fluut-core/{Cargo.toml, src/lib.rs, src/store.rs, src/frame.rs, src/graph.rs, src/session.rs, src/post.rs}` and tests.

- `ModelStore`: load STEP sources relative to the project file, with a file hash check (warning on mismatch) and ids.
- `SetupFrame`: transform to W (orientation, zero), stock box, clearance, `HeightEnv`.
- `Graph`: nodes `GeometrySource → SetupFrame → ToolpathResult → PostOutput` with SHA-256 hashes (D01 §4); stale = hash not in the cache.
- `Session::calculate(targets) -> Vec<JobReport>`, sequential, single process; `Session::post(post_id) -> PostOutput`, with the `PostJob` filled from the P fields.
- [x] **Step 1:** Tests: graph hashes (a P edit keeps the result node and changes the post node); cache hit on recalculation; missing tool and missing file diagnostics.
- [x] **Step 2:** Implement; pass; commit.

### Task 9: `fluut-cli`, golden projects and end-to-end tests

**Files:** Create `crates/fluut-cli/{Cargo.toml, src/main.rs, tests/cli.rs, tests/golden.rs, tests/support/step.rs, tests/projects/…}`.

- `fluut calc <project> [--tools <file>]…`; `fluut post <project> --post <id> -o <file> [--date] [--time]`. Exit code 1 on error diagnostics.
- Hand-written STEP inputs from a small test writer: a rounded-rectangle boss (planes and cylinders) and a block with a pocket and a square island. The files are committed and checked against the writer.
- Golden G-code per type (`fluut/linuxcnc`), refreshed with `FLUUT_BLESS=1`.
- Translation invariance: the same project with the model and the zero moved by (0.123457, −0.987651, 0.31).
- [x] **Step 1:** Tests; bless and review the golden output by eye.
- [x] **Step 2:** Pass; commit.

### Task 10: Close

- [x] **Step 1:** Mark this plan done with deviations; README row; D-doc updates (D06 field set, D01 hash, D15 clarifications found while building). Commit and push `m3-vertical-slice`.

**Done.** `fluut post` takes the reference projects in `crates/fluut-cli/tests/projects/` from
STEP to G-code, with golden files for `fluut/linuxcnc` and `fluut/grbl`. The
translation-invariance test moves model and zero by (0.123457, −0.987651, 0.31), with the zero
as a point and from the stock. The G-code is identical, and every coordinate is within 1e-6 mm.
`parallel` has its parameters, validation and registry slot; `generate` reports
`cam.type.unavailable` until S2 merges.

Deviations from this plan:
- Tasks 5–7 landed in one commit: the registry ties the framework to both strategies.
- `fluut-post` changed outside the plan. Each toolpath now starts with a rapid to its first
  segment's start, so the first move never runs from wherever the tool change left the tool.
  The post golden files were re-blessed, and D10 §4.7 records the rule.
- The D15 §3.4 example had one unaligned line (`tabs`). It was aligned, so that every example
  round-trips through the canonical writer.
- Offset pockets add a ring half a stepover in wherever the region runs out between rings.
  Ring merges at an angle θ can still leave a corner up to (s/2)/sin(θ/2) from a pass. The
  test checks that bound (D09 §2.2).
- `minimum` retract stays down for clear links up to one tool diameter. `profile` ramps
  replace the lead-in, and fall back to plunge over tabs (D15 §2.6, D08 §8.2).
- `tabs.width` is the length of the bridge. The flat on the tool-centre path is width + D
  (D15 §3.2).
- New codes are listed in D15 §6: `param.<scope>.<key>.range` for ranges without their own
  code, `param.geometry.rest_from.unsupported`, `cam.geometry.drive.empty` and
  `cam.type.unavailable`.
- The tests run on STEP parts from a small writer (`crates/fluut-cam/tests/support/step.rs`)
  and on WireBodies built in code. No corpus files were needed.
- The † constants of D15 (decision 15) were not tuned: there were no real cuts in this
  session.

Follow-ups:
- `parallel` after S2.
- Link ordering across areas: offset rings alternate between separate areas of a level.
- Profile ramps over tabs.
- Machine definitions for the machine rules.
- The disk result cache (M4).
