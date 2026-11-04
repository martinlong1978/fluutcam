# Simulation Rest Colour by Closest Distance Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Colour the stock mesh by the *true* distance from each vertex to the part, not by the vertical distance at the Z rays around it. The [playback plan](2026-11-02-sim-playback.md) left walls with the class of the columns beside them, so a 0.1 mm skin of stock against a vertical wall read as several millimetres of rest. Target: a thin skin on a wall gets the class its thickness deserves, a flat floor is unchanged, a 45° wall is judged by its perpendicular distance, and a mesh update costs no more than 20 % over today's.

**Architecture:** Gouge findings, the rest volumes and the Z-ray class (`PartHeights::class`) are unchanged. The part gains an optional owned *clearance* query, `Clearance::clear_height(x, y, r)`: the lowest height at which a ball of radius `r` over `(x, y)` is clear of the part, so a point is at least `r` from the part (and outside it) when it is at or above it. `Part::clearance()` returns it; the verification keeps it in `PartHeights`. At vertex level `Paint::vertex` refines the class of `combine`: a vertex the columns call *rest* is rest only if it is clear of the part by the tolerance, else within. Vertical distance is an upper bound of the true distance, so a vertex within tolerance vertically needs no query, and a vertex farther than the tolerance from every sampled part height in the 5 x 5 columns around it is rest without one. Only the rest candidates left query, and only in blocks that are rebuilt. The core part answers by a ball-nosed drop query (D07) of radius `r` on the exact faces: the ball at `p` is clear exactly when `p.z` is at or above the ball's centre at its highest contact, cached by `(x, y)`. This is the closest-point distance for a part a three-axis tool can reach (the same limit as the gouge check).

**Tech Stack:** as M7. No new dependencies.

**Design:** [`../design/D11-simulation.md`](../design/D11-simulation.md) §5, §10.

## Global Constraints

All earlier Global Constraints apply. In addition:

- Findings and the CLI simulation results do not change; only the colour class of a vertex does.
- Determinism: a vertex's class depends on its position and the part only. No `HashMap`; no thread-dependent order.
- A part without a clearance query (`Part::clearance` is `None`) keeps the vertical method.
- Golden files: none expected to change (no rest-colour snapshots exist).

## Review Focus

1. **The shortcuts must not change the answer:** a vertex within tolerance vertically is within; a vertex farther than the tolerance from every part height in its 5 x 5 columns is rest (heights are samples, so a part feature narrower than a ray spacing between samples is not seen, as with the vertical method).
2. **Owned query geometry:** `QueryGeometry` holds its body as a `Cow` so the part can keep a copy of the body that outlives the Setup's geometry.

---

### Task 1: Tests first

**Files:** `crates/fluut-sim/src/player/tests.rs`.

- [x] **Step 1:** Tests: a vertical wall with a skin thinner than the tolerance is within (the vertical method says rest); a skin thicker than it is rest; a flat floor is unchanged; a wall at 45° is judged by perpendicular distance (rest vertically, within truly, and the reverse above 1.41 × tolerance); one thread and many give identical tags.

### Task 2: Clearance in the sim and the core part

**Files:** `crates/fluut-sim/src/{verify.rs, mesh.rs, player.rs, lib.rs}`, `crates/fluut-cam/src/cutter/geom.rs` (owned bodies), `crates/fluut-core/src/sim.rs`.

- [x] **Step 1:** Fail, implement, pass; ball-drop clearance tested against a plane at 45° and a wall in the core.

### Task 3: Measurements and documents

**Files:** this plan, D11 §5 and §10, the playback plan, `docs/plan/README.md`.

- [x] **Step 1:** Re-run m7 before and after; record; workspace checks; commit.

**Done.** Windows, 12 threads, release, `cargo run -p fluut-core --example m7 --release -- all`. The machine was shared with other builds (projects with no part moved by up to 25 % between runs), so the final numbers are medians of 7 alternating runs of the old binary (before this plan) and the new; mesh update after 300 moves from the middle of the last toolpath, ms:

| Case | res (mm) | Mesh update before to after | First mesh of a `Player` (s) | Step back worst (ms) |
|---|---|---|---|---|
| `dome` | 0.1 | 20.2 to 21.8 (x1.08) | 0.08 to 0.09 | 10.3 to 12.9 |
| `dome` | 0.05 | 166 to 189 (x1.14) | 0.38 to 0.46 | 15.9 to 16.1 |
| `flipside` s1 | 0.1 / 0.05 | 8.1 to 8.8 / 59.6 to 56.6 | 0.06 to 0.05 / 0.30 to 0.30 | 16.1 to 14.7 / 69.9 to 56.8 |
| `flipside` s2 | 0.1 / 0.05 | 7.9 to 8.3 / 42.6 to 43.5 | 0.06 / 0.32 to 0.36 | 4.3 to 4.6 / 17.1 to 17.1 |
| `holes` | 0.1 | 4.1 to 5.7 (+1.6 ms) | 0.03 to 0.03 | 52.8 to 55.1 |
| `holes` | 0.05 | 15.9 to 18.8 (x1.18) | 0.21 to 0.22 | 197 to 206 |
| `boss`, `ridge`, `rough3d`, others | | within noise | within noise | within noise |

The first version took `holes` from 3.9 to 297 ms. Measured fixes, in order of effect (each is in D11 section 10):
1. Vertices below the part's lowest nearby height (the stock's bottom and sides inside the part) are within without a query: most queries.
2. The queries themselves: a ball drop over a hole sampled the whole hole edge a quarter ball radius apart (`Seeding::split_edges`), searched face rims (`DropOptions::rims` off), and tested trim containment against every segment (`Trim` keeps runs of 16 segments with their range). A query went from about 1 ms to a few microseconds.
3. A block refines all its vertices in parallel (`Paint::vertices`).
Tried and not enough: a coarser or 3D cache key; the cull by the highest nearby height alone.

Deviations:
- `holes` at 0.1 mm is +1.6 ms (+39 %) on a base of 4 ms; the rest are inside about 20 % or the noise between runs.
- The cache is keyed by `(x, y)` of the vertex and holds the offset height, not a yes or no per vertex, so the vertices up a wall share one query.
- Distance is by a ball-nosed drop, not a closest-point solver: the faces have no closest-point query with trimming, and a drop is exact for what a three-axis tool reaches. `fluut-cam` gained `Seeding::split_edges`, `DropOptions::rims` and the `Trim` runs; the default behaviour of every other drop is unchanged.
- No golden file changed (there are no rest-colour snapshots); findings and CLI simulation results are untouched.
