# Link Stock Lift Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make 2-D toolpaths after a roughing stop lifting to the original stock top. The 2-D linker (`link.rs`) already rapids *down* to each entry over the in-process stock, but the lift between passes (`minimum` / `stay_down`) still goes to `material_top + safety_distance`, even where an earlier toolpath cleared the material along the link. The 3-D finishing linker (`follow.rs`) already lifts over the stock.

**Architecture:**
- `fluut-cam`:
  - `link.rs`: `stock_lift_height` (next to `stock_rapid_floor`, sharing its "above the stock" rule) and `link_height_over`, which `link_over` calls for the lift from pass end `a` to next entry `b`.

**Tech Stack:** as M7. No new dependencies.

**Design:** [D08](../design/D08-toolpath-framework.md) §8.2; [follow stock floor](2026-11-03-follow-stock-floor.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Test first.
- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, `cargo xtask check-layers` and `cargo deny check`.
- Determinism (D01 §6): `libm` for transcendental functions; `BTreeMap` wherever iteration order reaches output.
- Re-bless golden G-code only where provably correct; list each file below. The CLI simulation test over the reference projects stays clean with no new `ALLOWED_FINDINGS`.
- No DeskProto-derived content.

## Scope decisions

- **Lift.** With a planar `StockFloor`, a `minimum` or `stay_down` lift goes to `max(feed, top_along(a, b, R) + safety_distance)`, the feed height when the tool meets no material. It is never higher than `link_height` gives without the stock, and never below the pass Z. `top_along` covers the retract at `a`, the rapid across and the descent at `b`.
- **Unchanged.** `full` retracts, stay-down links that remain at depth, cylindrical stock, and no stock at all.
- **One place.** `stock_rapid_floor` and `stock_lift_height` share the "safety above the stock, never above plain" rule; `follow.rs` keeps its own geometry-bound lift.
- **Not touched.** `bore` and `drill` call `link_height` themselves and keep the plain lift.
- **Feed height.** The lift cannot go below the feed height, so a toolpath whose `top` is the stock top (feed = top + 2) gains nothing over a stock cleared only below it; the gain is for toolpaths whose `top` is lower (floor passes, `horizontal`).

## Review Focus

1. **Never lower than the material** along the link, or than the pass.
2. **Nothing without the stock** and nothing for `full`.
3. **Cuts are the same**; only lifts change.

---

### Task 1: Plan

- [x] **Step 1:** This plan; README row. Commit.

### Task 2: Lift over the stock

**Files:** `crates/fluut-cam/src/link.rs`, `crates/fluut-cam/tests/follow_floor.rs`, D08 §8.2.

- [ ] **Step 1:** Unit tests in `link.rs`: a link across cleared stock lifts to the feed height; across an un-roughed island, above it (and 1 mm above a partly roughed one); material beside the link out of reach; cylindrical stock and no stock unchanged; `full` unchanged; never below the pass Z. Integration test: `horizontal` after a real `pocket_3d` roughing on the dome, same cuts, lower lifts, none below the material.
- [ ] **Step 2:** Implement; pass. D08. Merge `main`, all checks. Commit.

## Re-blessed golden files

None expected: the reference projects' toolpaths either have no earlier roughing or have a `top` at the stock top, so the lift stays at the feed height or above.
