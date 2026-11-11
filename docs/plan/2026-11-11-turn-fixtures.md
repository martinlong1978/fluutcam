# Turn Fixtures and Tool Length Warning Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Close two follow-ups of the [index clearance](2026-11-02-index-clearance.md) and [index fixtures](2026-11-03-index-fixtures.md) plans. First, a chuck, a tailstock and clamps that turn with A must raise the retract before an A turn, and the simulation should find a toolpath that meets one. Today `frame::turning_fixture_radius` is a hook that returns `None`. Second, a tool without an `overall_length` is checked at its tip only, silently, when the Z travel is checked before a turn, and its holder is silently dropped from the simulation. The user must see a warning.

**Architecture:**
- `fluut-model`: `Fixture` (chuck, tailstock, clamp) and the Setup key `fixtures` (inherited as a whole list, written in the project file, set from scripts); `ResolvedSetup::fixtures`; the warning `setup.fixtures.ignored` in a `three_axis` Setup.
- `fluut-core`:
  - `fixture.rs` (new): `FixtureShape`, the fixtures in W (a solid of revolution, or a box), their radius over an X range, and the tool test.
  - `frame.rs`: the hook is replaced by the shapes in `SetupFrame::fixtures`. `turn_radius` counts them over the stock's X range; `turn_radius_reaching(reach)` and `turn_clearance_reaching(reach)` widen the range by the tool's reach.
  - `session.rs`: `turn_heights` uses the reach of the Setup's tools. `Session::simulate` reports `sim.fixture`. The warning `cam.tool.length_missing`.
- `fluut-script`: `setup.fixtures`.
- `fluut-app`: unchanged (see "Not done").

**Tech Stack:** as M7. No new dependencies.

**Design:** [D08](../design/D08-toolpath-framework.md) §7; [D11](../design/D11-simulation.md) §5; spec 03 §3.9.5, §3.9.8 (new); [index clearance](2026-11-02-index-clearance.md); [index fixtures](2026-11-03-index-fixtures.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Test first.
- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask check-layers`, `cargo deny check` and `cargo xtask wiki-reference --check`.
- Determinism (D01 §6): `libm` for transcendental functions (`sin_cos_deg`); `BTreeMap` wherever iteration order reaches output.
- Re-bless golden G-code only where the change is provably correct; list each file below.
- No DeskProto-derived content.
- Test strings avoid hyphens (the i18n duplicate-id check).

## Scope decisions

- **Where fixtures live.** On the Setup, not the machine. The design documents describe no fixture model: spec 03 had only fixture *bodies* imported with the model (`excludedFaces`), and the earlier plan left a hook. Which fixtures hold the part is a property of the way the part is held, so it belongs to the Setup, which already carries the rotary axis and the work zero. The key is inheritable as a whole list, like `geometry`: a child Setup (the other side of the same part, `from_previous`) shares the rotary and its fixtures. `fixtures = []` says "none". A machine-level fixture (a chuck bolted to the machine's rotary) would need a machine-definition change; not done, and a Setup can list it.
- **Kinds.** `chuck { diameter, length }` is a cylinder about the axis against the stock's start face (lowest X), reaching `length` outward. `tailstock { diameter, length, tip_diameter? }` is a cylinder, or a cone, against the stock's end face (highest X): `diameter` at the far end, `tip_diameter` (default `diameter`) at the stock. `clamp { min, max }` is a box between two corners in W at A = 0. The positions of chuck and tailstock follow the stock, so changing the stock length moves them; the stock "start" is its lowest X in W.
- **Fixtures that do not turn.** Not modelled (fixed to the table, outside the rotary). Import them as bodies of the model; the existing excluded-faces and body rules apply.
- **Clamps in an indexed Setup.** Given at A = 0, they turn with the Setup: the radius from the axis does not change with the turn; the box in W (used by the tool test) is the bounding box of the turned box. A child Setup with another index angle therefore inherits the clamps correctly.
- **"Along the X range the tool visits".** Only fixtures within the X range the tool can reach raise the turn. The range is the stock's X range widened at both ends by the **largest cutter radius** of the Setup's enabled toolpaths (`Session::tool_reach`). It is known before the toolpaths exist and does not depend on their results, so cache keys are not affected. A chuck that only touches the stock's end face does not count for a tool that stays inside the stock (`SetupFrame::turn_radius`); it counts for a tool that overhangs the end by its radius (`turn_radius_reaching`). Per fixture the radius within that range is used (a cone: the largest of its radii at the ends of the clipped range). A fixture smaller than the part never lowers the radius. A rotary Setup uses the stock's own range (reach 0) for `setup.clearance.index_unsafe`, since its rapids go round at the clearance cylinder.
- **Turn height.** Unchanged: the larger of the turn clearances of the Setup and the one before, each at its own reach. The gauge-line Z travel check follows without change (`setup.clearance.index_travel` fires for a chuck high enough).
- **Simulation.** `Session::simulate` checks an indexed Setup's toolpaths against `SetupFrame::fixtures` and reports `sim.fixture` (an error on the toolpath). The tool is a vertical cylinder of its cutter radius (or a wider shank) from the tip up, and the holder one from the overall length up. A solid of revolution is tested by the tool's nearest distance to the axis against the fixture's radius over the tool's X range; a box by its bounding box. Segments are tested at both ends, lines every millimetre. Arcs and helices only at their ends (their intermediate points are within a tool radius of the ends in practice, but this is not proven). So the check errs towards a hit in space and may miss a mid-arc contact. Rotary Setups are not simulated at all, so there the fixtures only affect the turn height. `sim.index_turn` is unchanged: the turning stock is measured; a fixture on the turning part is covered by the turn height.
- **Tool length warning.** A warning `cam.tool.length_missing` (code in the `cam` family, which the generated Diagnostics page knows) is put on the first calculation report of a toolpath that uses the tool, once per tool per calculation, in two cases: (1) the machine has a Z travel and a rotary axis, a Setup turns A, and the tool in the spindle at that turn has no `overall_length` (the check is then at the tip only); (2) the tool has a holder but no overall length (the holder starts at the overall length and is dropped from the simulation). `Session::simulate` gives case 2 once per tool as well. The text names the Setups and says what is not checked. Cached toolpaths get it too, since it is added to the report, not to the cache. A tool without a holder in a project without a Z travel gets no warning: nothing needs the length.
- **Project file.** `fixtures` is the last Setup key. The writer puts the list on one line, so a hand-written multi-line array is rewritten when saved. No format change for projects without fixtures: no golden or reference project changes.
- **Window.** The Setup inspector has no field for lists of tables. The inspector is being changed by another branch; the fixtures are in the project file and in scripts, and `Known-Limits` says so.

## Review Focus

1. **Fixtures only count where the tool can go.** A chuck beyond the tool's reach and a fixture smaller than the part change nothing; the golden file of `flipside` does not change.
2. **No invented geometry:** a chuck is a plain cylinder at the stock face; the bounding-box clamps and the plain-cylinder tool are conservative and documented.
3. **One warning per tool**, not per toolpath; none when the length is set, or nothing needs it.

---

### Task 1: Plan

- [x] **Step 1:** This plan; README row. Commit.

### Task 2: Fixture model

**Files:** `crates/fluut-model/src/{project.rs, project/read.rs, project/write.rs, project/tests.rs, setup.rs}`, `tests/inherit.rs`; `crates/fluut-script/src/api/setup.rs`, `tests/api.rs`.

- [x] **Step 1:** Tests: every form round-trips through the writer; the list is inherited, replaced and reset by an empty list; bad kinds and sizes are load errors; fixtures in a `three_axis` Setup warn; a script sets and reads them.
- [x] **Step 2:** Implement; pass. Commit.

### Task 3: Turn radius

**Files:** `crates/fluut-core/src/{fixture.rs, frame.rs, session.rs, lib.rs}`, `tests/fixtures.rs`; `crates/fluut-cli/tests/cli.rs`.

- [x] **Step 1:** Tests: a chuck, a tailstock cone and a clamp bigger than the part raise the radius only over the X range; smaller fixtures and fixtures out of range change nothing; a clamp is bounded after the index turn; a rotary Setup counts a clamp over its stock and refuses a clearance cylinder inside it; end to end, `flipside` with a chuck of radius 50 posts `G0 Z60` before `G0 A180` (without: `Z41.321`), and fixtures inside the part leave the whole program unchanged; the Z travel check fires for the higher turn.
- [x] **Step 2:** Implement; pass. Commit.

### Task 4: Simulation

- [x] **Step 1:** Tests: a clamp across the stock gives `sim.fixture` on the toolpaths of `flipside` `s1`; the clamp out of the way gives none.
- [x] **Step 2:** Implement `fixture_finding`; pass. Commit.

### Task 5: Tool length warning

- [x] **Step 1:** Tests: with a Z travel and no overall length, one `cam.tool.length_missing` for four toolpaths, in the calculation and in the post's reports; none when the length is set or the machine has no travel; a holder without a length warns in calculate and in the simulation, once.
- [x] **Step 2:** Implement; advice entries; `cargo xtask wiki-reference`. Commit.

### Task 6: Docs

**Files:** spec 03 §3.9.8 and §3.9.5, D08 §7, D11 §5, wiki `Known-Limits`, `Setups-Stock-and-Zero`, `Machines-and-Posts`, `Diagnostics` (generated), UAT-SET-34 to 36, UAT-SIM-13 and 14, UAT-PST-16; this plan; README row.

- [x] **Step 1:** Docs. Merge `main`, run all checks, mark done. Commit.

## Re-blessed golden files

None. No reference project has `fixtures`, and with none the turn radius, the retract and the posted program are as before (`golden_two_sided` passes unchanged). Projects without fixtures have the same file text and the same Setup hash.

**Done.** A rotary or indexed Setup lists chuck, tailstock and clamp fixtures (`fixtures` in the project file, or from a script). The retract before an A turn rises to the largest radius of the stock, the part and any fixture within the stock's X range widened by the tool radius, plus the clearance offset; the Z travel check follows. `Session::simulate` reports `sim.fixture` for an indexed Setup. A tool without an overall length gets one `cam.tool.length_missing` warning, in the cases above. Deviations from this plan: none.

## Not done

- The Setup inspector in the window has no fixture fields (a list of tables; the inspector is being changed in parallel). Known-Limits says so.
- Fixtures in rotary Setups are not simulated (rotary Setups are not simulated at all).
- A machine-level fixture, and fixtures that do not turn with A, are not modelled.
- The X range is the stock's widened by the largest tool radius, not the X range of the actual toolpath results; a tool that never goes near the stock's end still counts the fixture at that end.
- `sim.fixture` checks arcs and helices at their ends only, and a box as its bounding box.
