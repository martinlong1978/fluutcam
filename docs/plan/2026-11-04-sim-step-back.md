# Simulation Step-Back Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make stepping *backwards* through a simulation fast enough to scrub. The [playback plan](2026-11-02-sim-playback.md) left it open: a back step re-cut the stock from the checkpoint before it (up to 32 768 chords), 58 ms on the S5 relief at 0.2 mm, 1.1 s on the rotary wrap. Target: under 16 ms per back step on the reference projects, well under 100 ms on the rotary wrap, memory bounded and stated.

**Architecture:** The `Player` keeps copy-on-write snapshots of the stock (`TriDexel` clones; tiles are `Arc`s, so a snapshot holds only the tiles changed since the one before it) keyed by chord count. A seek starts from the latest of the stock shown (if at or before the target), a snapshot and a checkpoint, and cuts on from there, so a step back cuts less than one *stride* of chords. Snapshots are left at the strides passed on the way: every stride in the last 32 strides before the target, and before that every 8th stride. The stride is chosen from the measured cutting speed so that it takes about 3 ms (the cost per chord differs 100 times between projects). A byte budget (default 64 MiB) bounds the snapshots; over it, the ones above the stock shown go first, then the farthest below it.

**Tech Stack:** as M7. No new dependencies.

**Design:** [`../design/D11-simulation.md`](../design/D11-simulation.md) §4, §10; CH-22.

## Global Constraints

All earlier Global Constraints apply. In addition:

- Determinism: the stock after a seek is bit-identical to cutting forward from the start, whatever seeks came before and whatever snapshots exist (span subtraction is a set difference). Only the *time* of a seek depends on the snapshots (and on the measured speed). No `HashMap`; snapshots are a `BTreeMap`.
- Rest colours and findings are unchanged: paint depends on the stock and the checkpoints only. No golden file changes.
- Memory is bounded by `Limits::budget` (an upper bound: tiles shared with the neighbouring snapshot or checkpoint are not counted again).

## Review Focus

1. **Accounting is an upper bound:** each snapshot is charged the bytes of its tiles that are not the same allocation as the tile at the same place in the snapshot or checkpoint before it, plus its tile lists. A tile shared by two snapshots is charged to the earlier one only. Removing a snapshot can make its successor cost more, so after a batch of evictions all charges are worked out again. Spilled spans are estimated at 20 % (S5), not counted ray by ray.
2. **The speed estimate cannot affect results**, only the stride; it quickly follows faster cuts and slowly follows slower ones, since copying tiles after a snapshot is not the cut.

---

### Task 1: Measure

**Files:** `crates/fluut-core/examples/m7.rs`.

- [x] **Step 1:** m7 prints 20 single-move back steps from the middle of the last toolpath (average / worst) and the snapshot MB. Baseline recorded below.

### Task 2: Snapshots in the Player

**Files:** `crates/fluut-sim/src/{player.rs, player/tests.rs, dexel.rs, run.rs, lib.rs}`.

- `Limits { stride, budget }` (`stride` 0 = adaptive), `Player::with_limits`, `Player::new` uses the defaults; `Player::{last_cut, stride, snapshot_bytes}`.
- `TriDexel::{bytes_since, unique_bytes}`, `Simulation::checkpoint_at`.
- [x] **Step 1:** Tests first: for 60 random seek sequences (anywhere, a few chords either way, runs of back steps), on five limit settings (default, tiny stride, tight budgets, a budget too small for any snapshot), the stock after each seek has the hash of a fresh forward cut from the start, the blocks equal a new player's, and `snapshot_bytes() <= budget`; after the first step back, a step back cuts less than a stride; snapshots share tiles (far below one stock copy each) and a smaller budget evicts down to it.
- [x] **Step 2:** Fail, implement, pass.

### Task 3: Measurements and documents

**Files:** this plan, D11 §10, CH-22, the playback plan, `docs/plan/README.md`.

- [x] **Step 1:** Re-run m7; record; workspace checks; commit.

**Done.** Windows, 12 threads, release, `cargo run -p fluut-core --example m7 --release -- all`. A back step is one move, from the middle of the last toolpath, after a forward step of 300 moves and the jump back (so the snapshots of a scrub exist). Times include the mesh update.

| Case | res (mm) | Back step before: avg / worst (ms) | after: avg / worst (ms) | Snapshots (MB) | 300 moves back before → after (ms) |
|---|---|---|---|---|---|
| S5 relief | 0.2 | 58.6 / 68.6 | 3.0 / 4.3 | 63 | 79 → 147 |
| S5 relief | 0.1 | 212 / 221 | 15.4 / 21.5 | 46 | 449 → 1 290 |
| S5 rotary wrap | 0.2 | 1 101 / 1 202 | 3.5 / 5.6 | 61 | 1 129 → 1 433 |
| S5 rotary wrap | 0.1 | 4 282 / 4 538 | 25.9 / 56.6 | 57 | 4 382 → 14 744 |
| `block` | 0.2 | 11.1 / 17.3 | 6.0 / 13.1 | 29 | 63 → 75 |
| `boss` | 0.2 | 6.7 / 10.0 | 2.1 / 4.0 | 9 | 26 → 30 |
| `dome` | 0.2 | 44.5 / 48.3 | 1.3 / 5.0 | 19 | 67 → 77 |
| `flipside` s1 / s2 | 0.2 | 14.4 / 8.8 | 2.2 / 2.7 | 21 / 10 | 21 → 34 / 17 → 22 |
| `holes` | 0.2 | 21.3 / 50.8 | 7.0 / 68 | 37 | 4 → 4 |
| `plate` | 0.2 | 14.5 / 31.8 | 8.7 / 51.8 | 38 | 197 → 143 |
| `plate2d` | 0.2 | 29.5 / 62.3 | 15.8 / 126 | 41 | 201 → 177 |
| `ridge` | 0.2 | 7.3 / 8.5 | 1.0 / 3.6 | 7 | 23 → 19 |
| `rough3d` | 0.2 | 6.8 / 9.5 | 0.9 / 5.3 | 22 | 30 → 34 |
| `vcarve` | 0.2 | 209.9 / 219.9 | 2.8 / 4.7 | 21 | 239 → 243 |

(Error 0.1 mm gives res 0.2 mm; the 0.05 mm rows are in the m7 output and behave alike.)

Memory: at most `Limits::budget` (default 64 MiB, `DEFAULT_BUDGET`) of snapshot tiles per `Player`, on top of the simulation's checkpoints. The numbers above are the upper bound the player accounts; the stock itself is 49-473 MB on the S5 cases. `Limits::stride` (0 = adaptive, about 3 ms of cutting; `MIN_STRIDE` 8 to `MAX_STRIDE` 4096 chords) is the other knob. The app uses the defaults.

Notes and open issues:
- A step back costs the cut (less than a stride, 1.5 ms on average) plus the mesh update for that step. `plate2d` and `plate` stay near or over 16 ms because one *forward* move there already needs 16 ms / 6 ms of mesh update (long moves touching most blocks); that is the mesh, not the cut.
- The first step back after a jump (slider drag to an earlier time) cuts from the checkpoint, as before (the worst-case column), and now also leaves snapshots on the way, which costs time: a jump back of 300 moves is 1.2 to 3 times slower than before (the relief at 0.1 mm 0.45 to 1.3 s, the rotary wrap at 0.1 mm 4.4 to 14.7 s: the 64 MiB budget holds only a few snapshots of a 200 to 470 MB stock, and each costs a copy of the tiles it changed). The cost comes back as fast steps; a bigger budget helps large stocks at the price of memory.
- Past the 32 strides kept finely, the next back step cuts from the nearest coarse snapshot (at most 8 strides, about 24 ms, then rebuilds the fine ones).
- Cutting speed is measured with a clock: the stride (so the time, never the result) varies between runs.
