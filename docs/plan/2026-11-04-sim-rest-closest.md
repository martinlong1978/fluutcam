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

**Done.** Windows, 12 threads, release, `cargo run -p fluut-core --example m7 --release -- all`; before to after (mesh update after 300 moves from the middle of the last toolpath, ms):

| Case | res (mm) | Mesh update | First mesh of a `Player` (s) |
|---|---|---|---|
| `dome` | 0.1 | 19.9 to 27.4 | 0.09 to 0.28 |
| `dome` | 0.05 | 143 to 161 | 0.38 to 1.17 |
| `flipside` s1 | 0.1 / 0.05 | 8.1 to 10.3 / 56 to 52 | 0.05 to 0.09 / 0.32 to 0.37 |
| `holes` | 0.1 / 0.05 | 3.9 to 297 / 15.7 to 385 | 0.03 to 0.13 / 0.19 to 0.39 |
| other projects, stand-ins | | unchanged within noise | unchanged |

Deviations:
- The per-step budget (+20 %) is met on `dome` at 0.05 mm and on `flipside`, missed on `dome` at 0.1 mm (+38 %) and by far on `holes`, where a ball drop on cylindrical faces costs about 0.3 ms (25 us on average elsewhere). Not fixed here: open issue in D11 section 10.
- The cache is keyed by `(x, y)` of the vertex and holds the offset height, not a yes or no per vertex, so the vertices up a wall share one query.
- Distance is by a ball-nosed drop, not a closest-point solver: the faces have no closest-point query with trimming, and a drop is exact for what a three-axis tool reaches.
- Two earlier versions were slower: a cull by the highest part height around the vertex only, and a cache by 3D position (`holes` update 585 ms).
- No golden file changed (there are no rest-colour snapshots); findings and CLI simulation results are untouched.
