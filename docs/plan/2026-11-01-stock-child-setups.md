# Stock in Child Setups Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the gap left by M6 part 3 ([plan](2026-11-01-m6-roughing-rotary.md), "Not done: stock carried into child Setups") and the open issue of the [simulation findings](2026-11-01-sim-findings.md) ("the rapid floor ignores the in-process stock"):
- a `from_previous` child Setup starts from its parent's in-process stock, and goes stale when the parent's toolpaths change;
- `indexed` Setups calculate, with the parent's stock re-sliced in the child's frame (CH-15);
- the linker rapids down to the in-process material + safety distance where the stock is known;
- the simulation of a child starts from the parent's simulated stock.

**Architecture:**
- `fluut-cam`:
  - `stock.rs`: `LayeredStock::top_near` (the highest material within a radius of a point), `LayeredStock::reframed` (D08 §7) and `LayeredStock::revolved` (planar slices of a solid of revolution).
  - `link.rs`: a `StockFloor` from the in-process stock; `link_over` and `rapid_floor_at` take it. `link` keeps its signature (3-D finishing types call it and are changed elsewhere).
  - The 2-D types, the 3-D roughing types, `drill` and `bore` pass the in-process stock to the linker.
- `fluut-model`: `ToolpathTypeId::links_over_stock`: the types whose links read the in-process stock.
- `fluut-core`:
  - the frame of an `indexed` Setup (spec 03 §3.9.3): the rotary alignment, then `R_index(θ)`;
  - in-process stock carried across Setups: the parent's stock after its last toolpath, reframed;
  - the graph: a toolpath whose links read the stock depends on the earlier stock-updating toolpaths of its Setup and of its `from_previous` ancestors; a child's simulation depends on its parent's;
  - the post gets the index angle;
  - `Session::simulate` starts a child from its parent's final simulated stock, turned by the index angle difference.
- `fluut-sim`: `TriDexel::turned_quarter` (a quarter turn about X, exact on the ray grid) and `Simulation::run_from` (a run from a given stock).
- `fluut-cli`: the golden `flipside` project (indexed, two-sided), its invariance test, and `fluut sim` in the reference test.

**Tech Stack:** as M7. No new dependencies.

**Design:** [D08](../design/D08-toolpath-framework.md) §7, §8.2; [D11](../design/D11-simulation.md) §2; spec 03 §3.9.2–§3.9.6; spec 04 §4.12.6; [CH-14, CH-15](../design/challenges.md); M6 part 3 [plan](2026-11-01-m6-roughing-rotary.md); [simulation findings](2026-11-01-sim-findings.md); M7 [simulation plan](2026-11-01-m7-simulation.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Test first.
- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask check-layers` and `cargo deny check`.
- Determinism (D01 §6): `libm` for transcendental functions; `BTreeMap` wherever iteration order reaches output.
- `fluut-cam`'s `waterline.rs`, `follow.rs` and the 3-D finishing strategies (`contour`, `horizontal`, `spiral`, `scallop`, `steep_and_shallow`) are being changed in another branch: not touched here.
- Re-bless golden G-code only where the change is provably safe; list each file below.
- No DeskProto-derived content.

## Scope decisions

- **Which frames.** Spec 03 §3.9.4 lets a `from_previous` child change only its kind, index angle, region and clearance. Parent and child therefore share the rotary axis line (X of W) and the work zero, and the child's W is the parent's turned about X by the difference of their index angles. A `rotary` parent counts as angle 0 and has cylindrical stock.
- **Carried stock.** The child's initial in-process stock is the parent's after its last enabled stock-updating toolpath:

  | Parent | Child | Child's initial stock |
  |---|---|---|
  | planar | planar, same angle | the parent's |
  | planar | planar, turned by φ | `reframed(φ)` |
  | cylindrical | planar (indexed) | `revolved`: each slab's outer loops give an X range at the slab's radius |
  | cylindrical | cylindrical | the parent's |
  | planar | cylindrical | the child's own stock cylinder (conservative) |

  When the parent cannot be calculated, the child starts from its own stock (its outline, as before).
- **`reframed(φ)`.** D08 §7: the stock as prisms between levels, turned, re-sliced.
  - φ = 0: unchanged. φ = 180°: exact; slab (z₀, z₁] with region R becomes slab [−z₁, −z₀) with R mirrored in Y.
  - Other angles: the child's levels are an even grid of at most 1 mm † over the turned stock's Z range. For each child slab (a, b] and parent slab (z₀, z₁] with region R, the parent points that land in the child slab lie in the parallelogram P = {z₀ ≤ z ≤ z₁, a ≤ y sin φ + z cos φ ≤ b} of the YZ plane. R is cut to the Y range of P; each outer loop of the result gives a rectangle: its X range by the range of y′ = y cos φ − z sin φ over P within the loop's Y range. Holes are ignored. The result contains every material point of the parent, so it is never smaller than the real material.
  - Sines and cosines of multiples of 90° are exact.
- **`revolved`** slices a stepped solid of revolution (pieces x₀…x₁ at radius ρ) at an even grid of at most 1 mm †: in slab (a, b] a piece is the rectangle [x₀, x₁] × [−w, w] with w = √(ρ² − m²), m the smallest |z| in the slab. An indexed Setup with cylinder stock starts from it too.
- **Rapid floor from the stock** (D08 §8.2). Where a toolpath has the in-process stock, the rapid down to an entry at p stops at `max(feed, top_near(p, R) + safety_distance)`, where `top_near` is the key of the highest slab whose region comes within the tool radius R of p (no material: the feed height). The material in that slab lies at or below its key, so the tool stays `safety_distance` above everything under it. This is never higher than the old floor. The drill's R plane and `bore`'s approach use the same floor per hole. Moves between holes at the R plane still check against the stock top.
- **Which toolpaths have the stock.** The 2-D types, `adaptive_3d`, `pocket_3d`, `drill` and `bore` (`links_over_stock`), plus every toolpath with `rest_from = "previous"`, as before. Their result hash includes the earlier stock-updating toolpaths (the same rule as `rest_from` in M6). Calculating one of them calculates the earlier toolpaths of its Setup first, and the stock-updating toolpaths of its `from_previous` ancestors. 3-D finishing and rotary types are unchanged.
- **Indexed Setups.**
  - Frame: `p_W = R_index(θ) · (R_align · S · p_M − z)`, with `R_align` and the zero as for a rotary Setup (axis along X, Y and Z of the zero on it).
  - Stock: a `block` is built in the aligned frame (A = 0), as for `three_axis`, and its box in W is the box of the turned block; a `cylinder` is coaxial. Clearance: a plane at stock top + offset, or Z = radius for a cylinder.
  - Heights are Z values, as for `three_axis`. Toolpath types allowed: the 3-axis types (spec 03 §3.9.2).
  - The post gets the index angle (`INDEX_A`); `setup.kind.unsupported` is gone.
- **Simulation of a child.** `Session::simulate` simulates the parent first (cached), and turns its final tri-dexel stock by the angle difference when that is a multiple of 90°: a quarter turn about X maps the ray grid onto itself when the grid's origin is the image of the far corner of the parent's grid. Other angles, and a rotary parent (not simulated), start from the child's full stock with the warning `sim.stock.full`. The child's simulation node depends on the parent's.

## Review Focus

1. **Conservative:** the reframed and revolved stock contains every material point of the parent; the floor never goes below material within the tool radius.
2. **Invalidation:** editing a parent toolpath makes the child's linking toolpaths and simulation stale; editing a 3-D finishing toolpath does not.
3. **Simulation stays clean** on every reference project.
4. **Indexed frames:** a part turned 180° machines its other side; the post emits the A index.

---

### Task 1: Plan

- [ ] **Step 1:** This plan; README row. Commit.

### Task 2: Stock queries and reframing

**Files:** `crates/fluut-cam/src/stock.rs`, `stock/tests.rs`.

- [ ] **Step 1:** Tests: `top_near` inside, beside and away from material; `reframed(0)` is the identity; `reframed(180°)` is exact on a pocketed block; at 90° and 30° every sampled material point lands in the child stock and the child's top is the turned box top; `revolved` contains sampled points of the cylinder.
- [ ] **Step 2:** Implement; pass. Commit.

### Task 3: Rapid floor from the in-process stock

**Files:** `crates/fluut-cam/src/{link.rs, strategy/*.rs}`; `crates/fluut-model/src/params/meta.rs`; `crates/fluut-core/src/{graph.rs, session.rs}`.

- [ ] **Step 1:** Tests: a floor pass after a pocket rapids to its floor + safety, but feeds where an earlier toolpath left material within the tool radius; a drill hole inside a pocket gets its R plane above the pocket floor; result hashes of linking toolpaths follow the earlier stock-updating toolpaths, those of 3-D finishing do not.
- [ ] **Step 2:** Implement; pass; re-bless and list. The simulation test stays clean. Commit.

### Task 4: Stock carried into `from_previous` children

**Files:** `crates/fluut-core/src/{graph.rs, session.rs}`, `tests/session.rs`.

- [ ] **Step 1:** Tests: a child's rest pocket starts from the parent's stock; editing a parent toolpath makes the child's toolpath stale; calculating only the child calculates the parent.
- [ ] **Step 2:** Implement; pass. Commit.

### Task 5: Indexed Setups

**Files:** `crates/fluut-core/src/{frame.rs, session.rs}`, `tests/rotary_frame.rs`.

- [ ] **Step 1:** Tests: the frame of an indexed Setup at 0°, 90° and 180°; a 180° child of an indexed parent gets the parent's stock mirrored; the post emits `A180`.
- [ ] **Step 2:** Implement; pass. Commit.

### Task 6: Simulation of child Setups

**Files:** `crates/fluut-sim/src/{dexel.rs, run.rs}`; `crates/fluut-core/src/{session.rs, graph.rs}`.

- [ ] **Step 1:** Tests: a quarter turn four times is the identity and maps volumes; a child simulation starts from the parent's final stock; the child's simulation goes stale with the parent's.
- [ ] **Step 2:** Implement; pass. Commit.

### Task 7: Golden two-sided project

**Files:** `crates/fluut-cam/tests/support/step.rs`; `crates/fluut-cli/tests/projects/flipside/`; `crates/fluut-cli/tests/{cli.rs, golden/flipside-linuxcnc-4x.ngc}`.

- `flipside`: an 80 × 50 × 16 plate on the X axis with a pocket on each side and a through window. Setup 1, indexed at 0°: the top pocket and the window to below half depth. Setup 2, `from_previous` at 180°: the bottom pocket and a rest pocket of the window, which stops where Setup 1 broke through.
- [ ] **Step 1:** Golden G-code, invariance along X, `fluut sim` clean. Commit.

### Task 8: Close

- [ ] **Step 1:** D08 §7, §8.2; CH-14, CH-15; spec 03 §3.9.6. Merge `main`, run all checks, mark done, README row. Commit.

## Re-blessed golden files

(Filled in as the work lands.)
