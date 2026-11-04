# Region Empty Growth Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the two open issues of the [region micro-loops plan](2026-11-03-region-micro-loops.md): growths that `cavalier_contours` returns empty (1) when an arc's offset radius comes within about 1e-5 mm of zero, which `optimised_load::shrink` hits 267 times in the `optimised_load_2d` tests alone by growing the holes of a cleared region by R − 1e-6, so those holes vanish from the shrunk cleared region; and (2) next to short, nearly collinear lines, with a debug assertion in `line_line_join` on the way. Then make sure a wrong offset can never leave the region code silently.

**Architecture:**
- `third_party/cavalier_contours`: the smallest fixes at the root, in `raw_pline_offset.rs`, listed in `README-FLUUTCAM.md`.
- `fluut-cam`, `region.rs`: a containment guard in `Region2::try_offset`. A growth must contain its input and a shrink must lie inside it, within a tolerance; a result that fails is retried like a panic (the existing retries, a hair deeper or shallower), and if every retry fails the offset returns a `RegionFailure`, which the toolpath shows as `cam.region.offset_failed` (cavalier panic plan).
- `optimised_load::shrink` keeps its R − 1e-6: the hair is wanted (tool centres on the boundary of the cleared region at exactly R count as safe), so the degenerate radius is legitimate input and the offset must handle it.

**Tech Stack:** as M7. No new dependencies.

**Design:** [D08](../design/D08-toolpath-framework.md); spec 06 §6.3; [S3 report](spikes/s3-region-2d.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Test first.
- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, `cargo xtask check-layers` and `cargo deny check`.
- Determinism (D01 §6): `libm` for transcendental functions; `BTreeMap` wherever iteration order reaches output.
- Golden G-code changes only where a fix provably corrects the output; each changed file is explained below. The CLI simulation test over the reference projects stays clean with no new `ALLOWED_FINDINGS`.
- No DeskProto-derived content.

## Scope decisions

- **Fix in `cavalier_contours`.** Both causes are tolerance bugs in the raw offset of the vendored crate (found below), not in FluutCAM's use of it. The patches change only the cases that failed: an arc with an offset radius at most `pos_equal_eps`, and an inner line–line join that `line_line_intr` took as parallel although the current end lies off the next line.
- **The guard is cheap.** `try_offset` runs very often (`optimised_load` offsets in every pass). The check is linear in the vertex count: the signed area (a growth does not lose area, a shrink does not gain any, within `TIDY` times the perimeter) and, for one vertex of each loop, a point-in-region test (each input loop's first vertex lies in a growth; each result loop's first vertex lies in the input of a shrink; both within `10 · TIDY` of the boundary counting as inside). It catches an empty growth, a lost loop and a stray loop. A full boolean check (input minus growth empty) would double the cost of every offset.
- **A failed check is a failure, not an empty region.** As for panics: retry, then `RegionFailure`.
- **A counter for the regression test.** `region::containment_retries()` (hidden from the docs) counts offsets whose first result failed the check. The `optimised_load_2d` regression test asserts that it stays at 0 over the integration scenarios (it was 267 lost holes before the fixes).

## Review Focus

1. The vendored patches change no result outside the two degenerate cases (determinism test, golden files).
2. The guard never lets a wrong growth or shrink out silently, and costs little.
3. Every changed golden file is explained and shown to be correct.

---

### Task 1: Plan

- [x] **Step 1:** This plan. Commit.

### Task 2: Reproduce and find the causes

**Files:** `crates/fluut-cam/src/region/tests.rs`.

- [x] **Step 1:** Regression tests with the exact inputs of both open issues. Fail.
- [x] **Step 2:** Find the causes; record below.

### Task 3: Fix in `cavalier_contours`

**Files:** `third_party/cavalier_contours/{src/polyline/internal/raw_pline_offset.rs, README-FLUUTCAM.md}`.

- [x] **Step 1:** Both fixes; tests of Task 2 pass. Golden files, determinism and CLI simulation checked. Commit.

### Task 4: Containment guard

**Files:** `crates/fluut-cam/src/{region.rs, region/tests.rs}`, `crates/fluut-cam/tests/optimised_load_2d.rs`.

- [x] **Step 1:** Tests: a forced wrong offset (test hook) is retried and then raises `RegionFailure`; the `optimised_load_2d` scenarios need no containment retry. Fail.
- [x] **Step 2:** Implement; pass. Commit.

### Task 5: Golden G-code review

**Files:** golden files that change, this plan.

- [x] **Step 1:** For each changed golden file: what changed, why the new output is correct (cut area or simulated stock before and after, CLI simulation clean). Re-bless only those. Commit.

### Task 6: Fuzz

- [ ] **Step 1:** `region` target for at least 10 minutes with and without debug assertions; record below; fix what it finds. Commit.

### Task 7: Finish

- [ ] **Step 1:** Merge `main`, run all checks, mark done. Commit.

## Root causes

Both are tolerance mismatches in the raw offset (`raw_pline_offset.rs`) of the vendored `cavalier_contours`, which in both cases marked a join next to a short or tiny segment as invalid, and a closed raw offset with an invalid segment and no self-intersections is dropped whole (`into_validated_slice_set`). Release builds have always given these empty results.

1. **Collapsing arc.** An offset arc counts as collapsed (`RawOffsetSeg::Collapsed`, which keeps the source arc's tangents for its joins) only when its offset radius is below the default fuzzy epsilon, 1e-8. Between 1e-8 and `pos_equal_eps` (1e-5) it stays an ordinary arc, and `join_without_intersection` takes its tangent as `(point − centre).perp()`, a vector as long as the offset radius. The check for a zero-length segment (tangent shorter than `pos_equal_eps`) then fires, and both joins of the arc become `InvalidDegenerateGap`. The loop of the open issue has two concave arcs of radius 3 (one 7.9e-5 mm long), so grown by 2.999999 their offset radius is 1e-6: no loop. 2.99999 (radius 1e-5) failed too; 2.9999 and 3.0 (radius 0, within 1e-8: collapsed) worked. `optimised_load::shrink` grows the holes of the cleared region by R − 1e-6, and those holes are bounded by the arcs of the tool's sweep, radius R, which is exactly this case. The − 1e-6 itself is right (tool centres on the boundary of the cleared region at exactly R are safe), so the fix is in the offset: such an arc uses the source arc's tangent at the shared vertex, as a collapsed arc does. Its trimmed span, shorter than `pos_equal_eps`, is then dropped as before, and the joins are the round corners of an exact offset. The areas now grow smoothly through the collapse (36.91434 at 2.9999 to 36.91672 at 3.00001).
2. **Short, nearly collinear lines.** `line_line_intr` tests for parallel lines by comparing the perp product of the segment vectors, |v| |u| sin θ, with its epsilon (`pos_equal_eps`), and for collinear ones in the same way. For segments 0.022 and 0.042 mm long that is sin θ < 0.011: the offsets of the loop's lines, 2.9e-3 rad apart at a concave corner, count as overlapping. `line_line_join` then asks `parametric_from_point` for the parameter of the current end on the next line, but at an offset of 3.5625 that end lies d (1 − cos θ) = 1.5e-5 mm off it: the debug assertion. Release builds used the parameter anyway, the middle line's offset came out reversed, and the validation rejected the whole offset. The join now checks first whether the current end lies on the next line; if not, the lines are not parallel at the offset's precision (the join was already classified as inner, at more than 1e-8 rad), and the join is their crossing when it lies on both segments. `line_line_intr`, which the booleans also use, is unchanged.

Both patches change only the cases that failed: arcs with an offset radius at most `pos_equal_eps`, and inner line joins that `line_line_intr` called overlapping (or not intersecting) although they are not. The determinism test, which pins offsets bit by bit, is unchanged.

The containment guard (`Region2::offset_holds`) would have caught both: without the vendored fixes, `shrunk_cleared_regions_keep_their_holes` counts 153 retried offsets in its three runs (404 with the rest of `optimised_load_2d`'s tests running alongside); with them, none. The retries alone also rescue the collapsing arc (one of them lands at radius 0).

## A third cause: holes left alone

The guard found one more wrong offset, in `rest_after_roughing_cuts_little` (`optimised_load_3d` rest roughing of `pocket_block`): a frame and a ring round a square hole, both about 0.01 mm wide, shrunk by 0.02. `cavalier_contours` returned the grown hole alone, a region of area −100.8 mm² (the right answer is empty). The shape offset keeps the offset of a hole that crosses no other offset without checking it against the shape's other loops. Before the guard this negative region went on silently; with the guard every retry failed and the toolpath reported `cam.region.offset_failed`. `offset_raw` now drops holes that lie outside every outer loop of the result (`shrinking_a_thin_ring_around_a_hole_leaves_nothing`, exact input). This is in `fluut-cam`, not the vendored crate: the result's loops are already sorted into outer loops and holes there, and the check is one point-in-region test per hole.

## Golden G-code

Two files changed, both through `optimised_load` (2-D and 3-D), whose shrunk cleared regions (`shrink(cleared, R − 1e-6)`, the safe regions for entries, stay-down links and sliver moves) now keep the holes they used to lose. Every other golden file, the determinism test and `reference_projects_simulate_clean` (no new `ALLOWED_FINDINGS`) are unchanged. Evidence below from `fluut sim` (old binary: `main`; new: this branch) and a script that replays each file's optimised-load section at each cut level, sweeping a disc of R 3 along every move in G-code order, and reports the area each move removes that no earlier move at that level removed.

- **`plate2d-linuxcnc.ngc`, t4 "Optimised load pocket".** Cut 2328.9 → 2453.9 mm, rapid 122.9 → 120.4 mm, time 1:18 → 1:24; the other three toolpaths are identical. Simulated rest: 0.0 mm³ on the part and 35 120.5 mm³ off it, before and after; findings: the two allowed `sim.gouge` of t2 and t3, unchanged. Area removed at z −9: 1 856.001 → 1 855.995 mm². The first changes (lines 128 and 167) are two stay-down moves from a pass's end to the next pass's start that the old file made because the start lay in the (too large) shrunk cleared region; they cut 7.6 and 7.2 mm² of uncut material (5 mm wide in a 6 mm tool), so that region was wrong there. The new file makes the same cut as the pass's planned entry, after a lift and a plunge onto cleared material. The new file's stay-down links that the old one did not have (lines 330, 507) remove 0.002 mm² (the replay's polygon error). Coordinates then move by 1 µm here and there (lines 375–573: offsets near collapsing arcs), and from line 848 the clearing order diverges, as it does for any change of the cleared region. Engagement does not get worse: moves removing more than 4 mm × their length 73 → 69, more than 5 mm 31 → 27.
- **`rough3d-linuxcnc.ngc`, t1 "Optimised load rough".** Cut 6592.4 → 6707.3 mm, rapid 159.4 → 187.2 mm, time 2:44 → 2:51; t2 (rest roughing from the in-process stock) has the same length and time. Simulated rest: 2 188.2 mm³ on the part (up to 1.431 mm) and 949.4 mm³ off it, before and after; findings: the allowed `cam.linking.ramp_fallback` warning only. Area removed at each of the five levels identical to 1e-3 mm² (2 540.799 at the four lower levels, 3 195.813 at the top). The changes (from line 1 273, then at each level) are in the sliver moves: the old file linked from the end of one sliver move (the tool position at the sliver) straight to the start of the next, because the old shrunk cleared region had lost the hole round the sliver and so held that end. The new file first goes back along the sliver move to its start (removing nothing) and links from there, inside the corrected safe region; 12 more lifts where no such route stays inside it.

Both changes follow from the corrected safe regions; neither cuts more or less material, and the simulation is as clean as before. Re-blessed: these two files only.

## Fuzz results

(Filled in by Task 6.)
