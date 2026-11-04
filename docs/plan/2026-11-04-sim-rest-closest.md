# Simulation Rest Colour by Closest Distance Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Colour the stock mesh by the *true* distance from each vertex to the part, not by the vertical distance at the Z rays around it. The [playback plan](2026-11-02-sim-playback.md) left walls with the class of the columns beside them, so a 0.1 mm skin of stock against a vertical wall read as several millimetres of rest. Target: a thin skin on a wall gets the class its thickness deserves, a flat floor is unchanged, a 45° wall is judged by its perpendicular distance, and a mesh update costs no more than 20 % over today's.

**Architecture:** Gouge findings, the rest volumes and the Z-ray class (`PartHeights::class`) are unchanged. The part gains an optional owned *clearance* query, `Clearance::clear(p, r)`: no part surface lies within `r` of the point `p` (outside the part). `Part::clearance()` returns it; the verification keeps it in `PartHeights`. At vertex level `Paint::vertex` refines the class of `combine`: a vertex the columns call *rest* is rest only if it is clear of the part by the tolerance, else within. Vertical distance is an upper bound of the true distance, so a vertex within tolerance vertically needs no query, and a vertex far above every part height around it is rest without one. Only the rest candidates left query, and only in blocks that are rebuilt. The core part answers by a ball-nosed drop query (D07) of radius `r` on the exact faces: the ball at `p` is clear exactly when `p.z` is at or above the ball's centre at its highest contact. This is the closest-point distance for a part a three-axis tool can reach (the same limit as the gouge check).

**Tech Stack:** as M7. No new dependencies.

**Design:** [`../design/D11-simulation.md`](../design/D11-simulation.md) §5, §10.

## Global Constraints

All earlier Global Constraints apply. In addition:

- Findings and the CLI simulation results do not change; only the colour class of a vertex does.
- Determinism: a vertex's class depends on its position and the part only. No `HashMap`; no thread-dependent order.
- A part without a clearance query (`Part::clearance` is `None`) keeps the vertical method.
- Golden files: none expected to change (no rest-colour snapshots exist).

## Review Focus

1. **The shortcuts must not change the answer:** a vertex within tolerance vertically is within; a vertex above every part height in its 3 × 3 columns by more than the tolerance is rest (heights are samples, so a part feature narrower than a ray spacing between samples is not seen, as with the vertical method).
2. **Owned query geometry:** `QueryGeometry` holds its body as a `Cow` so the part can keep a copy of the body that outlives the Setup's geometry.

---

### Task 1: Tests first

**Files:** `crates/fluut-sim/src/player/tests.rs`.

- [ ] **Step 1:** Tests: a vertical wall with a skin thinner than the tolerance is within (the vertical method says rest); a skin thicker than it is rest; a flat floor is unchanged; a wall at 45° is judged by perpendicular distance (rest vertically, within truly, and the reverse above 1.41 × tolerance); one thread and many give identical tags.

### Task 2: Clearance in the sim and the core part

**Files:** `crates/fluut-sim/src/{verify.rs, mesh.rs, player.rs, lib.rs}`, `crates/fluut-cam/src/cutter/geom.rs` (owned bodies), `crates/fluut-core/src/sim.rs`.

- [ ] **Step 1:** Fail, implement, pass; ball-drop clearance tested against a plane at 45° and a wall in the core.

### Task 3: Measurements and documents

**Files:** this plan, D11 §5 and §10, the playback plan, `docs/plan/README.md`.

- [ ] **Step 1:** Re-run m7 before and after; record; workspace checks; commit.
