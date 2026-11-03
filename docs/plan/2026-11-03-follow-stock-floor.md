# Follow Stock Floor Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make finishing after roughing stop wasting time in long air moves. The 2-D and roughing types rapid down to the in-process stock ([stock in child Setups](2026-11-01-stock-child-setups.md)), but the 3-D finishing linker (`follow.rs`: `parallel`, `contour`, `spiral`, `scallop`, `steep_and_shallow`, and `horizontal` through the 2-D linker) lifts to and descends from the original stock top + safety distance, even where roughing has cleared everything above the model.

**Architecture:**
- `fluut-cam`:
  - `stock.rs`: `LayeredStock::top_along(a, b, r)`, the key of the highest slab with material within `r` of a segment: `top_near` sampled along it, conservative.
  - `link.rs`: `stock_rapid_floor`, the rapid floor over a `StockFloor`, shared by `rapid_floor_at` and the finishing linker.
  - `follow.rs`: `Linker::floor`; lifts between runs and descents to an entry use the stock when the toolpath has it. `parallel` and `contour` (so `spiral`, `scallop`, `steep_and_shallow`) pass `StockFloor::of(input)`; `horizontal` passes it to `link_over`.
- `fluut-model`: `links_over_stock` includes the types that follow the surface, so their result hash includes the earlier stock-updating toolpaths and `Session` hands them the stock.

**Tech Stack:** as M7. No new dependencies.

**Design:** [D08](../design/D08-toolpath-framework.md) §7, §8.2; [CH-14](../design/challenges.md); [sim findings](2026-11-01-sim-findings.md), [stock in child Setups](2026-11-01-stock-child-setups.md), [waterline robustness](2026-11-01-waterline-robust.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Test first.
- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask check-layers` and `cargo deny check`.
- Determinism (D01 §6): `libm` for transcendental functions; `BTreeMap` wherever iteration order reaches output.
- `fluut-core`, `fluut-app` and `fluut-cli` source belong to another branch (update check): not touched here. Only a `fluut-core` test that pinned the old hash rule changes.
- Re-bless golden G-code only where the change is provably safer; list each file below.
- No DeskProto-derived content.

## Scope decisions

- **Lifts.** A lift between runs goes `max(geometry bound, material over the link) + safety_distance`, capped at `retract` for `minimum` and `stay_down` as before. The material over the link is the highest slab with material within the tool radius of the straight segment from the end of one run (after its lead-out) to the start of the next (before its lead-in), and never more than the plain material top. The retract at the first end, the rapid across and the descent at the second end all lie inside that swept stadium, so the lift is never lower than any material the tool passes over. `full` retracts are the retract height, as before.
- **Conservative sampling.** `top_along` samples the segment at most `r / 2` apart and queries each sample with `r` plus half the sample spacing. Every point of the stadium lies within that reach of a sample, so the answer is at or above the exact one (the reach is at most 25 % wider than the tool). An exact stadium test was not worth the region operations per link.
- **Descents.** The rapid down to an entry stops at `max(feed, top_near(entry, R) + safety)`, never higher than the plain floor: the same rule as `link.rs` (`stock_rapid_floor`). A finishing `top` is near the model top and `feed` is `top + 2` by default, so this seldom goes below the plain floor; the gain is in the lifts, after which the tool is already below the floor and plunges from the lift height.
- **No stock, no change.** Without in-process stock (`StrategyInput::in_process` is `None`, or cylindrical stock) the linkers behave exactly as before. When a toolpath has no earlier roughing, the stock is the Setup's block; links inside it are unchanged.
- **Hash.** The result hash of the finishing types now includes the earlier stock-updating toolpaths of the Setup and of its `from_previous` ancestors, as for the 2-D types. Calculating one calculates those first.
- **`horizontal`.** It links with the 2-D linker, which lifts to the plain material top; only its descents use the stock. Lowering the lifts of `link.rs` would change the golden files of every 2-D type and is left open.
- **Material above the last level.** Slabs are whole levels: roughing that leaves material above its last level over the model (a ridge crest) keeps the key of the slab above it, so links near the model stay at the stock top. The floor drops only where roughing cleared everything above (the flat around a boss).

## Review Focus

1. **Never lower than the material:** the floor is a maximum over the whole swept stadium of a link.
2. **Nothing without the stock:** `in_process = None` gives the same passes.
3. **Finishing cuts are the same** with and without the stock; only rapids and links change.

---

### Task 1: Plan

- [x] **Step 1:** This plan; README row. Commit.

### Task 2: `top_along` and the shared floor

**Files:** `crates/fluut-cam/src/{stock.rs, stock/tests.rs, link.rs}`.

- [x] **Step 1:** Test: along a pocket, across its wall, ending within reach of the wall, a point link, past the stock edge, a 0.2 mm island crossed between samples, cylindrical stock.
- [x] **Step 2:** Implement; pass. Commit.

### Task 3: The finishing linker

**Files:** `crates/fluut-cam/src/{follow.rs, strategy/{parallel,contour,horizontal}.rs}`, `tests/follow_floor.rs`.

- [x] **Step 1:** Tests on the dome: without the stock every rapid is at or above the stock top + 1; with the full stock the cuts are the same and rapids clear it; after a block roughed down to the block top, lifts are lower and never below the material (dense reference check); two thin posts of stock left standing at the part's edge are cleared by the links that cross them; descents stop above the material within reach; after a real `pocket_3d` roughing the lifts are lower (rapid height times length below 0.8 of the plain one).
- [x] **Step 2:** Implement; pass. Commit.

### Task 4: Hash and docs

**Files:** `crates/fluut-model/src/params/meta.rs`, `tests/stock_links.rs`; `crates/fluut-core/tests/session.rs`; D08 §8.2, CH-14, `docs/plan/README.md`.

- [x] **Step 1:** Test: every type that follows the surface links over the stock; rotary types and `manual_nc` do not; a finishing toolpath's result depends on an earlier roughing's parameters.
- [x] **Step 2:** Implement; docs; merge `main`, run all checks, mark done, README row. Commit.

## Re-blessed golden files

None. Every reference project that finishes (`dome`, `ridge`) has no roughing, so its stock is the Setup's block and every link stays within reach of it; their golden G-code is unchanged, and so are the cutting moves of every project. The CLI simulation test of every reference project stays clean with no new allowed finding, and translation invariance holds.

**Done.** With in-process stock, the 3-D finishing linker lifts and descends no lower than `safety_distance` above the highest in-process material within the tool's reach of the link, and never higher than before. Deviations from this plan:
- **No reference project.** Extending `rough3d` with a finishing toolpath gave no lower links: roughing leaves the slab above its last level over the ridge, so the stock near the model keeps the stock top key, and the links stay within reach of it. The measurable case is the integration test on the dome (a boss on a block) with a real `pocket_3d` roughing. No golden file changes.
- **Hash test.** Two `fluut-core` session tests pinned that `parallel` does not depend on earlier stock updates; they now say it does.
- **`horizontal`** takes only the descent floor (above).

Follow-ups:
- Lifts of the 2-D linker (`link.rs`) still go to the plain material top; the same swept test would lower them and re-bless many 2-D golden files.
- Finishing toolpaths do not update the stock (CH-14), so a second finishing sees the roughed stock.
- `optimised_load_3d` on the dome (Ø6 flat, 0.5 mm to leave, 4 mm step-down) panics in `cavalier_contours` (`UpdatedStartNotOnSegment`, `pline_view.rs:363`); found while writing the integration test, which uses `pocket_3d` instead.
