# M7 — Simulation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The simulation part of M7. Turn the S5 `fluut-sim` spike into the product feature of D11: stock from a Setup, removal of whole toolpath results with progress and cancel, verification, the time estimate, a simulation node and job in `fluut-core`, and Simulate, playback and findings in `fluut-app`. Exit criterion from the roadmap: the D11 tests pass.

**Architecture:** `fluut-sim` (layer 4) stays independent of `fluut-model`: it takes stock shapes, tool assemblies, feeds, machine kinematics and a part height query as plain values and traits. `fluut-core` maps the Setup frame, tools, feeds and machine onto them, implements the part query with the exact drop query of `fluut-cam` (D07), and caches a simulation per Setup under a graph node. `fluut-app` runs the job on its session worker and scrubs on a second thread.

**Tech Stack:** as M5 and the S5 follow-ups. No new dependencies.

**Design:** [`../design/D11-simulation.md`](../design/D11-simulation.md) §2–§7, [`../design/D01-workspace-architecture.md`](../design/D01-workspace-architecture.md) §3–§5, [`../design/D13-ui-architecture.md`](../design/D13-ui-architecture.md) §5.1, §7; spec 04 §4.10, spec 08 §8.7. S5: [`spikes/s5-simulation.md`](spikes/s5-simulation.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Determinism (D01 §6): the stock, the findings and the time estimate are bit-identical on any thread count. `libm` for transcendental functions; `BTreeMap` for anything that reaches an output.
- Simulation never feeds a toolpath (DC-02).
- Tests first: analytic dexel lengths, thread-count determinism, a positive and a negative case per finding, a hand-computed time estimate, graph invalidation, the playback view model.

## Review Focus

1. **Copy-on-write tiles:** checkpoints share untouched tiles, and a cut never changes a checkpoint.
2. **Attribution:** every finding names the toolpath, the segment index (in `ToolpathResult::segments` order) and a position in the Setup's work frame.
3. **Invalidation:** the simulation node goes stale when a toolpath result, a tool, a feed, the machine or the settings change, and not otherwise.

---

### Task 1: `fluut-sim` product API

**Files:** `crates/fluut-sim/src/{dexel.rs, sim.rs, tool.rs, sweep.rs, run.rs, run/tests.rs, lib.rs}`.

- Grids stored as copy-on-write tiles of `ROWS` rows (`Arc`), so a checkpoint is a cheap clone.
- `Resolution`: from an error target (S5: the maximum error is about half the ray spacing on sharp detail) and the memory budget of D11 §2.
- Cone tools (V, drill, taper) through the GJK fallback.
- `Simulation::run`: whole `ToolpathResult`s, each with its tool assembly and feeds, in order; checkpoint after each toolpath; progress per batch; cancel between batches.
- `Simulation::stock_at(t)`: the latest checkpoint before `t`, plus the chords up to `t`.
- [x] **Step 1:** Tests: a slot of a flat tool through a block gives exact dexel lengths in all three families; same stock hash on 1 and 4 threads through `Simulation::run`; checkpoints unchanged by later cuts; `stock_at` at the end equals the final stock; cancel stops the run; resolution from error and budget.
- [x] **Step 2:** Fail, implement, pass. Keep the spike modules and their tests.

### Task 2: Verification

**Files:** `crates/fluut-sim/src/{verify.rs, verify/tests.rs, run.rs}`.

- During removal: rapid moves that cut material; material inside the holder or a shank wider than the tool; material above the flute length (warning).
- After removal, with a `Part` height query: gouges (stock below the part by more than the tolerance at a Z ray and on a ring of that radius around it), attributed to the first chord that cut below the part; rest material on and off the part.
- [x] **Step 1:** Tests, each with a positive and a negative case.
- [x] **Step 2:** Fail, implement, pass.

### Task 3: Time estimate

**Files:** `crates/fluut-sim/src/{time.rs, time/tests.rs}`; `crates/fluut-core/src/session.rs`; `crates/fluut-cli/src/main.rs`.

- D11 §6: programmed feed, axis-limited speed, junction deviation, trapezoidal profiles, centripetal limit on arcs, dwells, correction factor. Without an acceleration, time is length over speed.
- `Session::time_estimate(toolpath)`; `fluut calc` prints it.
- [x] **Step 1:** Tests: a hand-computed program (a straight line, a split straight line, a right-angle corner, an arc, a dwell); without acceleration; the CLI line.
- [x] **Step 2:** Fail, implement, pass.

### Task 4: `fluut-core` simulation node and job

**Files:** `crates/fluut-core/src/{graph.rs, session.rs, sim.rs}`, `crates/fluut-core/tests/simulate.rs`; `crates/fluut-cli/src/main.rs`.

- `NodeId::Simulation(setup)`, hashed from the Setup's result and post hashes, tool assemblies, machine and settings.
- `Session::simulate(setup, settings, monitor)`; findings as diagnostics on their toolpaths; the result cached by node hash.
- `fluut sim <project>`.
- [x] **Step 1:** Tests: graph invalidation; simulate on a reference project; a cached second run; cancel.
- [x] **Step 2:** Fail, implement, pass.

### Task 5: `fluut-app`

**Files:** `crates/fluut-app/src/{jobs.rs, session.rs, scene.rs, vm/playback.rs, vm/ribbon.rs, ui/*.rs, viewport/render.rs}`; `crates/fluut-i18n/locales/en/app.ftl`.

- Simulate command (calculates what is stale first); a Simulation tab with play, pause, a scrub slider, toolpath clips and the findings; clicking a finding centres the view on it.
- Stock mesh by dual contouring on the tri-dexel (D11 §4), built off the UI thread.
- [x] **Step 1:** Tests: playback view model (play, pause, tick, seek, end, clips); simulate job through the worker; the ribbon command.
- [x] **Step 2:** Fail, implement, pass.

### Task 6: Measurements and documents

**Files:** `crates/fluut-sim/examples/m7.rs`; `docs/design/D11-simulation.md`, `docs/design/challenges.md` (CH-22), `docs/plan/README.md`, this plan.

- Memory, time per move and full simulation time on the reference projects and the S5 stand-ins; what needs a real GPU.
- [x] **Step 1:** Measure; record; workspace checks; commit.

**Done.** Implementation notes and measurements are in [D11 §10](../design/D11-simulation.md#10-m7-implementation-2026-11-01); CH-22 updated. Deviations from this plan and from D11:
- Tasks 1 and 2 were committed together, and the time estimate's `fluut-sim` part with them; its session and CLI part went with task 4.
- Gouges are measured as vertical depth below the exact part top (a drop query) with a ring test, not as closest-point distance to the faces; a closest-point query on trimmed faces does not exist yet.
- Checkpoints are taken every 32 768 chords within a toolpath as well as before each one, and they cost 3–5× the bare stock, so the memory budget counts 115 B per ray instead of 24.
- The stock mesh is rebuilt whole on every scrub, not per dirty tile, and has no deviation colours; it takes 0.2–3 s at 0.2 mm, so scrubbing is not interactive on large programs.
- Body stock, region-of-interest refinement and the rest heat map are not built. Cylinder stock works in `fluut-sim` only, since no rotary Setup is calculated yet.
- Machine definitions have no acceleration, correction factor or tool-change time, so the session's estimate is length over speed; the estimator supports them.
- The app's Simulation tab is a minimal timeline: a slider, toolpath clips and the findings list; no tool-change or index markers.
- Findings on the reference projects, left for their owners: `block` t2 rapids down to 2 mm above its floor through the 0.2 mm of wall stock t1 left; `holes` t2 has one rapid in stock; `plate2d` t2 (score) and t4 (adaptive pocket) cut below the model, by design of that project's depths.
