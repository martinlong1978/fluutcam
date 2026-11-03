# Cavalier Panic Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `optimised_load_3d` on the `dome` reference part (Ø6 flat end mill, 0.5 mm stock to leave, 4 mm step-down) panics in the vendored `cavalier_contours` (`pline_view.rs:363`, a debug assertion in `PlineViewData::create`). Fix the cause, and make sure any failure left in region offsets and booleans reaches the user as a diagnostic on the toolpath instead of a crash (spec 06 §6.3, 01 §1.8.1: algorithms return errors, never panic).

**Architecture:**
- `third_party/cavalier_contours`: the smallest fix at the root, listed in `README-FLUUTCAM.md`.
- `fluut-cam`:
  - `region.rs`: `RegionFailure` (which operation failed, and the panic message), the typed result of a `cavalier_contours` call that still fails after the existing retries. `Region2::offset` and the booleans keep their signatures and raise it as a panic payload (`resume_unwind`, so no second panic hook call).
  - `toolpath.rs`: `calculate` runs the strategy under `catch_unwind` and turns a `RegionFailure` into `CamError::Invalid` with `cam.region.offset_failed` or `cam.region.boolean_failed` on the toolpath. Other panics are bugs and are re-raised.
  - A `cfg(test)` hook makes every offset or boolean on the current thread fail, to test the path without leaving a real failing case unfixed.
- `fuzz/`: a `region` target for offsets and booleans of random small polylines.

**Tech Stack:** as M7. No new dependencies.

**Design:** [D08](../design/D08-toolpath-framework.md), [D14](../design/D14-build-test-release.md) (`panic = "unwind"` in release); spec 06 §6.3, 01 §1.8.1; [S3 report](spikes/s3-region-2d.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Test first.
- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask check-layers` and `cargo deny check`.
- Determinism (D01 §6): `libm` for transcendental functions; `BTreeMap` wherever iteration order reaches output.
- Golden G-code changes only where a fix provably corrects the output; list each file below.
- No DeskProto-derived content.

## Scope decisions

- **Root fix in the vendored crate,** as small as possible, with a regression test in `region/tests.rs` that uses the exact coordinates.
- **Typed failure through a panic payload.** About 180 call sites use `offset`, `union`, `intersect` and `subtract` as infallible. Turning them all into `Result` is a large change for a failure that should not happen; instead the failure travels as a typed panic payload to `calculate`, the one place where every strategy is called, and becomes an error there. `catch_unwind` needs `panic = "unwind"`, which the release profile sets (D14).
- **A failed shrink is an error too.** `offset` used to return an empty region when a shrink still failed after its retries, which silently drops material from a toolpath. It now raises the failure like a growth.
- **Only region failures become diagnostics.** Other panics in a strategy are bugs; they keep crashing (with the crash report) so they are found.

## Review Focus

1. The `cavalier_contours` change is the smallest that removes the cause, and changes no other result (golden files, determinism test).
2. A region failure anywhere in a strategy, including inside `rayon`, reaches `calculate` and names the toolpath.
3. No test leaves a real failing case unfixed; the forced failure is `cfg(test)` only.

---

### Task 1: Plan

- [x] **Step 1:** This plan. Commit.

### Task 2: Reproduce, reduce, fix

**Files:** `third_party/cavalier_contours/{src/…, README-FLUUTCAM.md}`, `crates/fluut-cam/src/region/tests.rs`, `crates/fluut-cam/tests/rough3d.rs`.

- [x] **Step 1:** Integration test: `optimised_load_3d` on the dome. Fails.
- [x] **Step 2:** Reduce to the smallest offset that panics; unit test with exact coordinates. Fails.
- [x] **Step 3:** Root cause and fix; README entry; check upstream. Pass. Commit.

### Task 3: Panic guard

**Files:** `crates/fluut-cam/src/{region.rs, region/tests.rs, toolpath.rs, strategy/contour_2d.rs}`.

- [x] **Step 1:** Tests with the forced failure: `Region2::try_offset` and the booleans return `RegionFailure`; `calculate`'s guard turns it into `cam.region.offset_failed` / `cam.region.boolean_failed` on the toolpath.
- [x] **Step 2:** Implement; pass. Commit.

### Task 4: Fuzz

**Files:** `fuzz/{Cargo.toml, README.md, fuzz_targets/region.rs}`.

- [x] **Step 1:** Target: offsets and booleans of small regions built from rectangles, circles and triangles; run it; fix what it finds the same way; record executions and crashes below. Commit.

### Task 5: Finish

- [x] **Step 1:** Golden G-code and CLI simulation check; merge `main`, run all checks, mark done, README row. Commit.

## Root cause

A Z-level section of the dome holds a sliver: a triangle 0.86 mm long and 3e-5 mm wide (`offset_of_a_sliver_triangle`). Its raw offset has two steep, almost collinear segments that meet end to end. `line_line_intr` took them as parallel (their cross product is under `eps`) and measured their overlap along X, because the second segment's X extent (3e-5) was just over `eps`. Dividing by it gave an overlap 0.11 mm along the segment, a slice started there, off its segment, and `PlineViewData::create` failed its debug assertion. The fix measures along the larger component, as `parametric_from_point` already does. Upstream `master` has the same code. Release builds skip the assertion, so they built the bad slice silently.

## Fuzz results

`fuzz/fuzz_targets/region.rs`, run on Windows (nightly 2026-11-02, ASan) for about 50 minutes in all, about 315 000 executions:

- **With debug assertions (`-a`):** three classes of `cavalier_contours` assertion, each within a minute or two.
  1. An offset of a closed loop came back open where slices meet almost tangentially (a micrometre bump left by a circle that nearly touches an edge), and the shape offset took it as a loop. Fixed: close it (`offsets_at_near_tangent_contacts`). Release results unchanged.
  2. The shape offset closed a loop with a second vertex on top of the first, a zero-length arc that a later boolean rejected. Fixed: remove every repeated closing vertex (`offset_of_loops_touching_at_a_vertex`).
  3. Open: after five chained offsets of a ring left by tangent circles, the offset holds self-overlapping micrometre loops, and `parametric_from_point` (from `line_circle_intr`) fails its assertion. Input: a circle (5.250001, 5.250001) r 7.500001 minus a circle (1e-6, 5.250001) r 2.6250015, union a circle (5.250001, 5.250001) r 2.6250005, then offsets −1.34375, −1.34375, +3.5625, −1.375, −1.34375. Debug builds now report `cam.region.offset_failed`; release builds skip the assertion.
- **Release (no `-a`, what users run):** 289 949 executions, no panic. Three timeouts: repeated offsets of the same kind of shape multiply micrometre loops (5 loops and 843 vertices to 87 loops and 29 460 vertices in one offset). The target now stops above 500 vertices. Open.

## Re-blessed golden files

None. Every golden G-code file and the CLI simulation tests are unchanged.

**Done.** The `optimised_load_3d` dome case calculates (`optimised_load_3d_on_the_dome`, cuts clear of the part by R + stock to leave). Any `cavalier_contours` panic left in an offset or boolean becomes `cam.region.offset_failed` or `cam.region.boolean_failed` on the toolpath. Deviations from this plan:
- **Two more fixes** from the fuzz target (above), both in `shape_algorithms/mod.rs`.
- **Open-path offsets** (`contour_2d::offset_open`) go through the same guard.
- **Open issues:** fuzz class 3 and the loop blow-up above; both come from micrometre loops that the offset keeps (`offset_raw` drops loops under 1e-9 mm² only). The application's crash hook still writes a report for each panic the region retries catch, though not for the `RegionFailure` that leaves a strategy (`resume_unwind` skips the hook). On Windows the ASan runtime DLL must be on `PATH` to run the fuzz targets.
