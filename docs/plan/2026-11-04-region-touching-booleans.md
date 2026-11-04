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
