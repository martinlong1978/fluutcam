# Simulation Playback Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make scrubbing and playback interactive, and show where rest material is. The M7 simulation ([plan](2026-11-01-m7-simulation.md), deviations) re-meshes the whole stock on every frame (0.2–11 s), sends triangle soup, reports rest material as totals only, and has no step controls. Target: a scrub step of a few hundred moves re-meshes in under 50 ms on the S5 relief at 0.2 mm.

**Architecture:** `fluut-sim` meshes the stock in square blocks of Z-ray columns (`MeshTiles`). Each block is an indexed mesh with shared vertices, built from the rays of the block and a margin around it, so a block never depends on another block's output. After a scrub, the old and new stock are compared tile by tile (`Arc::ptr_eq` first, then ray by ray), the changed rays mark the blocks they reach, and only those blocks are rebuilt, in parallel. A `Player` keeps the stock of the last frame and cuts forward from it when it can, instead of from the checkpoint. Each vertex carries a tag: its rest class against the part (from the part heights the verification already queries) and the toolpath that last cut near it. The app worker owns the `Player`; a frame carries the list of block meshes (`Arc`), and the renderer uploads only the blocks whose `Arc` changed. The shader colours the stock by the tag and a mode uniform, so switching the colouring needs no upload.

**Tech Stack:** as M7. No new dependencies.

**Design:** [`../design/D11-simulation.md`](../design/D11-simulation.md) §4, §5, §10; [`../design/D13-ui-architecture.md`](../design/D13-ui-architecture.md) §5, §7; CH-22.

## Global Constraints

All earlier Global Constraints apply. In addition:

- Determinism: the block meshes are bit-identical however the scrubs before them went, on any thread count. No `HashMap` reaches an output.
- The incremental mesh equals a full rebuild, block by block, tested on random scrub sequences.
- UI thread (D13): no meshing, no part queries and no colour computation on it; it uploads changed blocks only. Scrubs stay coalesced (one in flight from the UI, the latest served by the worker).
- Colours are colour-blind safe (Okabe–Ito) and have a legend.

## Review Focus

1. **Block independence:** a block's mesh depends only on the rays within its margin, so dirty marking by changed rays is complete.
2. **Forward cutting:** cutting chords `a..b` then `b..c` gives the same stock as `a..c` (span subtraction is a set difference), so the player may continue from the last frame.
3. **Rest classes:** gouge and rest use the part heights of the four Z rays around a vertex, so walls next to the part are not painted as gouges.

---

### Task 1: Block meshes in `fluut-sim`

**Files:** `crates/fluut-sim/src/{mesh.rs, mesh/tests.rs}`.

- `MeshTiles::new(&TriDexel, &Paint)`: blocks of `BLOCK × BLOCK` lattice columns. A block owns the crossing edges whose lower lattice point is in it; it computes cell vertices (mass point of the crossings, as before) and area-weighted vertex normals from the edges within a margin of two lattice points.
- `MeshTiles::update(&TriDexel, &Paint) -> Vec<usize>`: the blocks a changed ray reaches, rebuilt in parallel.
- `surface()` stays, as the blocks joined.
- [ ] **Step 1:** Tests: the joined blocks are closed with the block's volume; the slot volume; incremental after random scrubs equals a new `MeshTiles` block by block; a cut marks only the blocks it reaches; the same blocks on 1 and 4 threads.
- [ ] **Step 2:** Fail, implement, pass.

### Task 2: Player, step times and paint

**Files:** `crates/fluut-sim/src/{run.rs, run/tests.rs, verify.rs, player.rs}`.

- `Simulation::part`: the part heights at the Z rays and the tolerance, kept from the verification.
- `Player::seek(&Simulation, n)`: the stock after `n` chords, cut forward from the last frame within a checkpoint interval; the changed blocks.
- Vertex tags: rest class (off the part, above tolerance, within, gouge) and the toolpath that last cut the Z rays around the vertex.
- `Simulation::step(t, unit, forward)`: the next or previous end of a move, pass or toolpath.
- [ ] **Step 1:** Tests: forward cutting equals `stock_after`; step times on a hand-made program; the classes of a stock above, on and below a flat part; tags by toolpath.
- [ ] **Step 2:** Fail, implement, pass.

### Task 3: App

**Files:** `crates/fluut-app/src/{session.rs, jobs.rs, vm/playback.rs, ui/simulation.rs, ui/viewport.rs, viewport/render.rs, viewport/shader.wgsl}`; `crates/fluut-i18n/locales/en/app.ftl`.

- The worker keeps a `Player` per Setup; a frame carries the block meshes.
- The renderer keeps a vertex and index buffer per block and uploads the blocks whose `Arc` changed.
- Simulation tab: step back and forward by move, pass or toolpath; colour by none, rest material or toolpath; a legend.
- [ ] **Step 1:** Tests: `Playback` steps; a frame after a step changes few blocks; a headless render of rest colours; the legend through `egui_kittest`.
- [ ] **Step 2:** Fail, implement, pass.

### Task 4: Measurements and documents

**Files:** `crates/fluut-core/examples/m7.rs`; D11 §10, CH-22, the M7 plan, `docs/plan/README.md`, this plan.

- [ ] **Step 1:** Measure full and incremental meshing on the relief and the reference projects; record; workspace checks; commit.
