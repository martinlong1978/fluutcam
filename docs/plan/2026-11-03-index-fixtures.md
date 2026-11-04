# Index Fixtures and Z Travel Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the two follow-ups of the [index clearance plan](2026-11-02-index-clearance.md). First, fixtures that turn with A (chuck, tailstock, clamps on the rotary) must count in the turn clearance. Second, the Z travel check (`setup.clearance.index_travel`) compares the tool *tip* with `travel.z`. Machine travel limits apply to the spindle, so a long tool or holder can pass the check and still hit the top of Z travel.

**Architecture:**
- `fluut-core`:
  - `frame.rs`: `turning_fixture_radius`, the hook that gives the largest distance from the axis of the fixtures that turn with A. The turn radius is the largest of the stock, the bodies and that hook. Today the hook returns `None` (see "Fixtures" below).
  - `session.rs`: `turn_heights` checks the **spindle gauge line**, at the turn height + the gauge length of the tool in the spindle, against `travel.z[1] − centre.z`. The diagnostic gives both numbers.
- `fluut-sim`: unchanged. The simulation has no fixtures to check.

**Tech Stack:** as M7. No new dependencies.

**Design:** [D08](../design/D08-toolpath-framework.md) §7; spec 02 §2.3, §2.4; spec 03 §3.9.5; [index clearance](2026-11-02-index-clearance.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Test first.
- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask check-layers` and `cargo deny check`.
- Determinism (D01 §6): `libm` for transcendental functions; `BTreeMap` wherever iteration order reaches output.
- `fluut-cam`'s `region*` and `third_party/cavalier_contours` are being changed in another branch: not touched here.
- Re-bless golden G-code only where the change is provably correct; list each file below.
- No DeskProto-derived content.

## Scope decisions

- **Fixtures.** The project model has no fixture model. Spec 03 and `fluut-model` define no chuck, tailstock, vice or clamp, and the simulation has none. The only fixtures FluutCAM knows are fixture bodies imported with the model (spec 03 §3.10 `excludedFaces`). These are bodies of the Setup, and their box already counts in the turn radius. So this plan adds no fixture model. It adds one named hook, `frame::turning_fixture_radius`, which the turn radius of rotary and indexed Setups includes. It returns `None` until a fixture model exists. A fixture fixed to the table outside the rotary does not turn, so it must not reach the hook. `sim.index_turn` is unchanged: it turns the simulated stock and has no fixtures.
- **What `travel.z` limits (decision).** Spec 03 §3.9.5 read `travel.z` as "the tool tip's range with length compensation". That gives the tip a range, but tool length compensation (G43) moves the machine's Z by the tool's length and does not change its limits. We follow the usual CNC meaning: travel limits are machine coordinates of the **spindle gauge line**, the point where a tool of zero length would have its tip. LinuxCNC and Fanuc soft limits work this way. The axis `centre` is in the same machine coordinates. So in W, with the zero on the axis, the gauge line is at tip Z + gauge length, and it must stay at or below `travel.z[1] − centre.z`.
- **Gauge length.** The gauge length is the tool's `overall_length` + its holder's `length`, from the tip to the spindle nose. The simulation uses the same model: the holder starts at the overall length (D11 §5). With no holder, it is `overall_length`. That is conservative: part of the shank is in the collet. With no `overall_length` it is 0, and the diagnostic says so. This is the same tip-only check as before, because nothing better is known.
- **Which tool.** The turn happens before the Setup's first tool change, so the tool in the spindle is the tool of the last enabled toolpath before the Setup, in project order. The first turn of a program emits no retract, so it has no check.
- **Text.** `Setup `s`: A turns with the tool tip at Z 41.321 and the spindle gauge line at Z 101.321 (tool `flat-6`, gauge length 60.000); the machine's Z travel ends at Z 80.000`.

## Review Focus

1. **The travel reference:** the gauge line, not the tip, is compared with `travel.z`; both numbers are in the message.
2. **No invented fixture model:** one hook, documented, included in the turn radius.
3. **Golden files** do not change. No reference project has a `travel.z` together with a rotary axis.

---

### Task 1: Plan

- [x] **Step 1:** This plan; README row. Commit.

### Task 2: Z travel at the gauge line, fixture hook

**Files:** `crates/fluut-core/src/{frame.rs, session.rs}`, `tests/rotary_frame.rs`; `crates/fluut-cli/tests/cli.rs`.

- [x] **Step 1:** Tests: a gauge length from the overall length and the holder, from the overall length alone, and 0 when unknown; `flipside` with a 60 mm tool fails a Z travel that clears its tip (`setup.clearance.index_travel`, and the message gives both Zs), and passes a longer travel.
- [x] **Step 2:** Implement; pass. Commit.

### Task 3: Docs

**Files:** spec 02 §2.3, spec 03 §3.9.5, D08 §7; this plan; README row.

- [x] **Step 1:** Docs. Merge `main`, run all checks, mark done. Commit.

## Re-blessed golden files

None. No reference project has a `travel.z` together with a rotary axis, so no program changes.

**Done.** Before an A turn, the Z travel check compares the spindle gauge line with `travel.z`. The gauge line is at the turn height + the gauge length (`Tool::gauge_length`: overall length + holder length) of the tool in the spindle. The message gives the tip's Z and the gauge line's. Turning fixtures count in the turn radius through `frame::turning_fixture_radius`, which is `None` because the project model has no fixtures. Deviations from this plan: none.

Follow-ups:
- A fixture model (chuck, tailstock, clamps on the rotary, each marked as turning or fixed) that feeds `turning_fixture_radius`, and fixtures in `sim.index_turn`.
- A tool without `overall_length` is checked at the tip only, without a warning. A warning when the machine has `travel.z` would make this visible.
- In a rotary Setup, `setup.clearance.index_unsafe` still compares the clearance cylinder with the stock and the bodies only, not with the fixture hook.
