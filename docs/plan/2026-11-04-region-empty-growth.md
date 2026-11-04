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

- [ ] **Step 1:** This plan. Commit.

### Task 2: Reproduce and find the causes

**Files:** `crates/fluut-cam/src/region/tests.rs`.

- [ ] **Step 1:** Regression tests with the exact inputs of both open issues. Fail.
- [ ] **Step 2:** Find the causes; record below.

### Task 3: Fix in `cavalier_contours`

**Files:** `third_party/cavalier_contours/{src/polyline/internal/raw_pline_offset.rs, README-FLUUTCAM.md}`.

- [ ] **Step 1:** Both fixes; tests of Task 2 pass. Golden files, determinism and CLI simulation checked. Commit.

### Task 4: Containment guard

**Files:** `crates/fluut-cam/src/{region.rs, region/tests.rs}`, `crates/fluut-cam/tests/optimised_load_2d.rs`.

- [ ] **Step 1:** Tests: a forced wrong offset (test hook) is retried and then raises `RegionFailure`; the `optimised_load_2d` scenarios need no containment retry. Fail.
- [ ] **Step 2:** Implement; pass. Commit.

### Task 5: Golden G-code review

**Files:** golden files that change, this plan.

- [ ] **Step 1:** For each changed golden file: what changed, why the new output is correct (cut area or simulated stock before and after, CLI simulation clean). Re-bless only those. Commit.

### Task 6: Fuzz

- [ ] **Step 1:** `region` target for at least 10 minutes with and without debug assertions; record below; fix what it finds. Commit.

### Task 7: Finish

- [ ] **Step 1:** Merge `main`, run all checks, mark done. Commit.

## Root causes

(Filled in by Task 2.)

## Golden G-code

(Filled in by Task 5.)

## Fuzz results

(Filled in by Task 6.)
