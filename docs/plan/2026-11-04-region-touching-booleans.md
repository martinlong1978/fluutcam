# Region Touching Booleans Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the open issues of the [region empty-growth plan](2026-11-04-region-empty-growth.md): (1) about one fuzzed boolean in 10 000 with touching or nearly coincident boundaries still gives an impossible area after all retries, and (2) growths of a loop pinched to a narrow channel go wrong in `cavalier_contours` from an offset of about 2.95 mm. Both are reported as `cam.region.boolean_failed` / `cam.region.offset_failed` today; they should give the right region. If cheap, make the plausibility guards check more than one point per loop.

**Architecture:**
- `fluut-cam`, `region/boolean.rs`: the split-and-classify boolean. Fixes in its classification of coincident pieces and its stitching where boundaries touch, or pre-processing (snapping nearly coincident vertices and edges together within the region tolerance) before the boolean.
- `third_party/cavalier_contours`: the smallest fixes at the root for the offset class, listed in `README-FLUUTCAM.md`, or pre-processing in `region.rs` where the input is degenerate (a pinch narrower than the tolerance).
- `region.rs`: the guards (`offset_holds`, `guarded`) unchanged in what they accept as right; perhaps a few more sample points per loop.

**Tech Stack:** as M7. No new dependencies.

**Design:** [D08](../design/D08-toolpath-framework.md); spec 06 §6.3; [S3 report](spikes/s3-region-2d.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Test first: each class becomes a regression test with the exact coordinates of a reduced fuzz input before it is fixed.
- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, `cargo xtask check-layers` and `cargo deny check`.
- Determinism (D01 §6): `libm` for transcendental functions; `BTreeMap` wherever iteration order reaches output. The determinism test stays unchanged.
- Golden G-code changes only where a fix provably corrects the output; each changed file is explained below. The CLI simulation test over the reference projects stays clean with no new `ALLOWED_FINDINGS`.
- No DeskProto-derived content.

## Scope decisions

- **Regenerate the fuzz inputs.** The artifacts of the empty-growth plan were lost. A release run of the `region` target (lenient, `-fork`) gives fresh ones; a small replay tool (outside the repository) turns each into the exact sequence of constructors and operations, which is reduced by hand to a test.
- **Fix where the cause is.** The boolean is FluutCAM's own code, so its fixes go there. Offsets go through `cavalier_contours`; a fix there is a vendored patch, unless the input itself is degenerate below the region tolerance, in which case pre-processing in `region.rs` is the right layer.
- **Guards stay plausibility checks.** An exact check (the symmetric difference of input and result) would double the cost of every offset and boolean. More sample points are added only if the geometry benchmarks show no measurable slowdown.

## Review Focus

1. Every fix changes only nearly degenerate cases (touching or coincident boundaries within a few micrometres, pinches narrower than the tolerance): determinism test, golden files.
2. The regression tests use exact fuzz coordinates and fail before the fixes.
3. Fuzz numbers before and after.

---

### Task 1: Plan

- [ ] **Step 1:** This plan. Commit.

### Task 2: Reproduce

**Files:** `crates/fluut-cam/src/region/tests.rs`.

- [ ] **Step 1:** A release fuzz run (lenient, `-fork`) to regenerate failing inputs; replay and group them by cause.
- [ ] **Step 2:** Reduced regression tests with exact coordinates, one per cause. Fail.

### Task 3: Fix the booleans

**Files:** `crates/fluut-cam/src/region/{boolean.rs, tests.rs}`.

- [ ] **Step 1:** Root causes recorded below; fixes; tests pass. Golden files, determinism and CLI simulation checked. Commit.

### Task 4: Fix the pinched growths

**Files:** `third_party/cavalier_contours/…` and `README-FLUUTCAM.md`, or `crates/fluut-cam/src/region.rs`.

- [ ] **Step 1:** Root cause recorded below; fix; tests pass, including `growth_of_a_loop_pinched_at_a_tangent_hole_is_never_wrong` without its failure branch. Commit.

### Task 5: Guard

**Files:** `crates/fluut-cam/src/region.rs`.

- [ ] **Step 1:** If cheap, more sample points per loop; benchmark numbers before and after recorded below. Commit.

### Task 6: Golden G-code review

- [ ] **Step 1:** For each changed golden file: what changed and why the new output is correct. Re-bless only those. Commit.

### Task 7: Fuzz

- [ ] **Step 1:** `region` target for at least 15 minutes with and without debug assertions after the fixes; record below; fix what it finds. Commit.

### Task 8: Finish

- [ ] **Step 1:** Merge `main`, run all checks, mark done. Commit.

## Reproduction

A 9-minute release run of the `region` target (lenient, `-fork=8 -ignore_crashes=1`, ASan) on the merged `main`: 934 693 executions, 45 saved inputs, all guard failures: 21 unions with an impossible area and 24 offsets that do not hold their input (the empty-growth plan reported 1 in 10 000; this run found about 1 in 20 000 executions). A replay tool outside the repository turned each input into its constructors and operations; the regression tests below use those exact coordinates and check every result against a point oracle on a 60 × 60 grid (inside or outside the exact union, intersection, difference, growth or shrink, away from the boundaries) as well as the guards. All 45 inputs pass after the fixes.

## Root causes

Five causes, two in the boolean, three in the offset.

1. **Coincident pieces decided differently from each side** (`region/boolean.rs`). A piece lay on the other boundary only when its midpoint and both ends came within `JOIN` (1e-7) of it, and coincident pieces of `b` were then dropped as "decided on the `a` side". Two boundaries that run together but drift apart by more than 1e-7 (two circles of equal radius with centres 1 µm apart, `union_with_a_circle_moved_by_a_micrometre`) were coincident seen from one region and not from the other: the piece of `a` was `In` and dropped, the piece of `b` was coincident and dropped too, and the union lost its loop (area 0). A piece whose ends both lie within `SNAP` (below) of the other boundary and whose midpoint lies within `PROBE` (1e-5) of it is now classified by two probes, `PROBE` to either side: both in, both out, or one in (coincident: `Same` or `Opposite`), the same test from both sides. When both probes agree the midpoint itself decides, so a piece inside a slot thinner than the probes is still `In`. A piece with an end off the other boundary is classified as before, at a point away from it: the first version probed every piece within `PROBE`, and a piece leaving the other boundary at a shallow crossing (neighbouring tool sweeps in `optimised_load`) became `Same` and broke every boolean of two such sweeps.
2. **Touching boundaries cut at different points** (`region/boolean.rs`). Where two boundaries touch or cross nearly tangentially, each region was cut at its own vertex or at the crossing, 1e-7 to 3e-6 apart; stitching (`JOIN`, `LOOSE` = 1e-6) could not join the pieces and lost the loop (a triangle along an edge to the strip's corner, `union_with_a_triangle_along_an_edge_to_its_corner`). Now, before splitting, vertices of `b` within `SNAP` (5e-6) of a vertex of `a` move onto it; a crossing within `SNAP` of an end of either segment is that end; a vertex within `SNAP` of the other region's segment cuts it at its foot (was `JOIN`); and stitching bridges up to `SNAP` (`LOOSE`) when no piece starts within `JOIN`. All below `TIDY` (1e-5), the clean-up tolerance of offsets. Points within `JOIN` of each other are left as they were (splitting and stitching take them as one already), which keeps the bits of every ordinary result (determinism test).
3. **Overlapping offsets of parts that run together** (`region.rs`, `offset_raw`). Two parts of a region a few micrometres apart (a circle cut by a slot 3 µm wide; parts that touch at a vertex) grow along the same circles. `cavalier_contours` stitched both growths into one loop that passed a point twice, or left two overlapping outer loops, enclosing part of the growth twice. `contains` (winding > 0) still answered right, so the guard passed, but the area was too large and the next boolean, which assumes simple disjoint loops, went wrong (most of the 21 failed unions came from such a growth one step earlier). `offset_raw` now splits loops at repeated points and unites outer loops that overlap (a vertex or segment midpoint of one inside the other, holes counted).
4. **Open pieces closed one by one** (vendored, `IndexedPolyline::parallel_offset_for_shape`). Shrinking by half the width of a bar, so that it collapses to a line, `cavalier_contours` gave the loop's offset as three open pieces, each ending where the next starts. FluutCAM's earlier patch closed each piece on its own: the first got a chord across the region and the others were dropped. The region lay inside its input and had a plausible area, so the guard passed it (`shrink_where_a_bar_collapses_to_a_line`). Open pieces are now joined end to start before closing (README entry).
5. **Cusps and walls thinner than the tolerance** (the pinched class of the empty-growth plan, and a wall 5 µm thick between a hole and the outside). A circle cut by an internally tangent circle leaves a C shape whose tips are cusps 0.012 mm apart; growths go wrong where their round joins nearly coincide. Shrinking a square with a hole whose top lies 5 µm below the square's top, `cavalier_contours` keeps the slices along the wall, which lie within its `pos_equal_eps` (1e-5) of the right distance from both sides, and returns the outer loop alone. Neither is a single wrong line in the crate but its tolerance meeting input features below it. `try_offset` gets two more retries, both exact rather than approximate: **in two steps** (d/2 + d/2, then d/4 + 3d/4; growths and shrinks by discs compose exactly), after which the cusps are round ends; and **loop by loop**, per part (O grown) minus (each hole shrunk as a solid), or (O shrunk) minus (each hole grown), the parts united: single loops offset reliably, and the booleans now handle the coincident boundaries. `growth_of_a_loop_pinched_at_a_tangent_hole_is_never_wrong` now checks that the growths are right, not only that they fail cleanly.

## Guard

`offset_holds` also checks that the midpoint of every segment of the result lies within `|d| ± 10 · TIDY` of the input's boundary (a bounding-box index over the input, one query per segment). This caught cause 4, which the area and one-vertex checks passed.
