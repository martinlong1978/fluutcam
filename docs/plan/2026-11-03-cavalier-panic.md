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

- [ ] **Step 1:** This plan. Commit.

### Task 2: Reproduce, reduce, fix

**Files:** `third_party/cavalier_contours/{src/…, README-FLUUTCAM.md}`, `crates/fluut-cam/src/region/tests.rs`, `crates/fluut-cam/tests/rough3d.rs`.

- [ ] **Step 1:** Integration test: `optimised_load_3d` on the dome. Fails.
- [ ] **Step 2:** Reduce to the smallest offset that panics; unit test with exact coordinates. Fails.
- [ ] **Step 3:** Root cause and fix; README entry; check upstream. Pass. Commit.

### Task 3: Panic guard

**Files:** `crates/fluut-cam/src/{region.rs, region/tests.rs, toolpath.rs}`.

- [ ] **Step 1:** Tests with the forced failure: `Region2::try_offset` and the booleans return `RegionFailure`; `calculate`'s guard turns it into `cam.region.offset_failed` / `cam.region.boolean_failed` on the toolpath.
- [ ] **Step 2:** Implement; pass. Commit.

### Task 4: Fuzz

**Files:** `fuzz/{Cargo.toml, README.md, fuzz_targets/region.rs}`.

- [ ] **Step 1:** Target: offsets and booleans of random small polylines; run it; fix what it finds the same way; record executions and crashes below. Commit.

### Task 5: Finish

- [ ] **Step 1:** Golden G-code and CLI simulation check; merge `main`, run all checks, mark done, README row. Commit.
