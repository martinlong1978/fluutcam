# Index Clearance Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every A turn between Setups safe without the user computing anything. Today the post turns A wherever the last toolpath left the tool, at the Setup clearance ([stock in child Setups](2026-11-01-stock-child-setups.md), "Indexing clearance"). A plane clearance (stock top + offset) clears the stock top, not its corners while the part turns, so a turn can swing material into the tool. `flipside` works around it with a hand-chosen clearance cylinder of radius 35. Nothing checks it.

**Architecture:**
- `fluut-core`:
  - `frame.rs`: `SetupFrame::turn_radius`, the largest distance from the rotary axis of the stock and the bodies, and `SetupFrame::turn_clearance`, that radius + the clearance offset. A rotary Setup whose clearance cylinder does not clear its turn radius is refused (`setup.clearance.index_unsafe`).
  - `session.rs`: the post gets the turn height of each Setup that turns A at its start; a turn height above the machine's Z travel is an error (`setup.clearance.index_travel`). `Session::simulate` checks the turn into an indexed Setup (`sim.index_turn`).
- `fluut-post`: `SetupJob::turn_z`; before `rotary_index`, and at the start of a rotary Setup, the emitter rapids to it in Z when the tool has moved.
- `fluut-sim`: `verify::index_turn`, a conservative check of the material turning about X against the tool assembly.
- `fluut-cli`: `flipside` uses the default plane clearance.

**Tech Stack:** as M7. No new dependencies.

**Design:** [D08](../design/D08-toolpath-framework.md) §7, §8.2; [D11](../design/D11-simulation.md) §5; spec 03 §3.9.2, §3.9.5; spec 05 §5.4.1, §5.6; [CH-15](../design/challenges.md); [stock in child Setups](2026-11-01-stock-child-setups.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Test first.
- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask check-layers` and `cargo deny check`.
- Determinism (D01 §6): `libm` for transcendental functions; `BTreeMap` wherever iteration order reaches output.
- `fluut-cam`'s `waterline.rs` and `cutter/` are being changed in another branch: not touched here.
- Re-bless golden G-code only where the change is provably safer; list each file below.
- No DeskProto-derived content.

## Scope decisions

- **Retract, then turn (option A, the default).** The post emits the retract; the toolpaths are unchanged, so their results and cache keys stay valid and the safe height follows the Setups, not one toolpath. Validation alone (option B) would leave the user to compute a radius, which the goal rules out. Option B is kept only where the post cannot add a move: inside a rotary Setup, where A turns on rapids at the clearance cylinder.
- **What turns.** Everything on the table lies inside the cylinder about the rotary axis through the farthest corner of the stock box (or the stock cylinder) and of the bodies' box. The in-process stock is never larger than the stock, so this holds at any moment. The swept volume of any turn lies inside the same cylinder. For an indexed `block` stock the box is taken at A = 0, before it is turned, since its radius does not change with the turn.
- **Turn radius and turn height.** `turn_radius` = that cylinder's radius (rotary and indexed Setups; none for `three_axis`). `turn_clearance` = `turn_radius` + the Setup's clearance offset (the plane `offset`, default 10 mm; 10 mm for a cylinder clearance). It mirrors the plane clearance, which keeps the same offset above the stock top. The height of a turn into Setup k is the larger of the `turn_clearance` of Setup k and of the Setup before it: the work on the table is one of the two.
- **Why Z alone is enough.** Indexed and rotary Setups have their zero on the axis (spec 03 §3.9.3), so after the Setup's work offset Z is the height above the axis line. A tool tip at Z ≥ h, with the tool pointing up, has every point of the assembly at distance ≥ h from the axis, whatever its X and Y.
- **Where.** The retract comes after `setup_start` (the new work offset) and before `rotary_unclamp` and `rotary_index` in an indexed Setup; at the start of a rotary Setup, before its first move, which turns A with a rapid. It is emitted only once the tool has moved in the program: the first index assumes the machine starts clear, as the first rapid of every program does. It is a `rapid` event with only `[Z]`.
- **Rotary Setups.** Rapids at the clearance cylinder turn A. A clearance cylinder not larger than the turn radius is an error, `setup.clearance.index_unsafe`. The default (stock radius + 10) passes.
- **Machine limits.** With `travel.z` and a rotary axis in the machine definition, the highest Z in W is `travel.z[1] − centre.z`. A turn height above it fails the post with `setup.clearance.index_travel`. Travel limits are read as limits of the tool tip with length compensation, as for any other Z word.
- **Simulation.** Before an indexed Setup, `Session::simulate` takes the tool where the post leaves it for the turn: the end of the previous Setup's last toolpath, raised to the turn height. `verify::index_turn` revolves the Setup's start stock (tri-dexel) about X: per Z ray, the largest radius of its material, plus the ray spacing for the material between rays. The tool assembly is a vertical cylinder of its largest radius (cutter, shank, holder) from the tip up. A turn is clear when, over the tool's X range, no material radius reaches the tool's distance from the axis. Otherwise the error `sim.index_turn` is reported on the Setup's first toolpath.

## Review Focus

1. **Safe by construction:** after the retract, every point of the tool is outside the cylinder that holds everything that turns.
2. **Automatic:** no project names a radius; `flipside` uses the default clearance.
3. **The simulation check** finds a turn with the tool at the plane clearance and passes the post's height.
4. **Golden files** change only by the retract and the clearance named below.

---

### Task 1: Plan

- [x] **Step 1:** This plan; README row. Commit.

### Task 2: Retract before a turn in the post

**Files:** `crates/fluut-post/src/{job.rs, emit.rs, emit/tests.rs}`.

- [ ] **Step 1:** Tests: two indexed Setups with `turn_z` give `G0 Z…` after the first Setup's moves and before the second `A`, not before the first; a rotary Setup after an indexed one retracts before its first move; no retract without `turn_z`.
- [ ] **Step 2:** Implement; pass. Commit.

### Task 3: Turn radius and turn height

**Files:** `crates/fluut-core/src/{frame.rs, session.rs}`, `tests/rotary_frame.rs`.

- [ ] **Step 1:** Tests: the turn radius of an indexed block (at 0° and 30°), of an indexed cylinder and of a rotary Setup; a rotary clearance cylinder inside the stock is refused; `flipside` posts a retract to the turn height before `A180`; a machine whose Z travel is too short fails with `setup.clearance.index_travel`.
- [ ] **Step 2:** Implement; pass; re-bless and list. Commit.

### Task 4: Simulation check

**Files:** `crates/fluut-sim/src/{verify.rs, verify/tests.rs}`; `crates/fluut-core/src/session.rs`, `tests/simulate.rs`.

- [ ] **Step 1:** Tests: a block turned with the tool above its corner radius is clear; with the tool at its top + 10 it is not; a holder wider than the cutter is caught; `flipside` simulates clean.
- [ ] **Step 2:** Implement; pass. Commit.

### Task 5: `flipside` and docs

**Files:** `crates/fluut-cli/tests/projects/flipside/flipside.fluut`, `golden/flipside-linuxcnc-4x.ngc`; D08 §7, D11 §5, spec 03 §3.9.2 and §3.9.5, spec 05 §5.4.1 and §5.6, CH-15.

- [ ] **Step 1:** `flipside` drops its clearance cylinder; re-bless and list. Docs. Merge `main`, run all checks, mark done, README row. Commit.

## Re-blessed golden files

To be filled in.
