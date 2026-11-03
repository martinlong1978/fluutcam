# Region Micro-Loops Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the open issues of the [cavalier panic plan](2026-11-03-cavalier-panic.md): region offsets keep tiny self-overlapping features (spikes, notches and loops a few nanometres wide, `offset_raw` drops loops under 1e-9 mm² only), chained offsets multiply them (5 loops / 843 vertices became 87 loops / 29 460 vertices in one offset), and they cause fuzz class 3 (a debug assertion in `parametric_from_point`, from `line_circle_intr`). Also stop the crash hook from writing a report for every panic that the region retries catch.

**Architecture:**
- `fluut-cam`:
  - `region.rs`: a clean-up of every offset result before it leaves `offset_raw`, at a tolerance far below any machining tolerance but far above rounding: near-duplicate vertices merged, arcs flatter than the tolerance turned into lines, zero-width spikes (a vertex where the loop turns back on itself) removed, and loops narrower than the tolerance dropped. The cause is found first; the vendored `cavalier_contours` is patched only if it lies there.
  - `region.rs`: `guard` marks the thread while it runs a `cavalier_contours` call, and the crash hook asks before writing a report (`region::guard_active()`).
- `fluut-core`: `crash.rs`, the hook skips panics caught by a region `guard` (no report, no message; the panic message travels in the `RegionFailure` when the retries all fail).
- `fuzz/`: the `region` target's 500-vertex stop is raised or removed.

**Tech Stack:** as M7. No new dependencies.

**Design:** [D08](../design/D08-toolpath-framework.md), [D14](../design/D14-build-test-release.md) §6 (crash reports); spec 06 §6.3, §6.4; [S3 report](spikes/s3-region-2d.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Test first.
- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, `cargo xtask check-layers` and `cargo deny check`.
- Determinism (D01 §6): `libm` for transcendental functions; `BTreeMap` wherever iteration order reaches output.
- Golden G-code changes only where a fix provably corrects the output; list each file below. The CLI simulation test over the reference projects stays clean with no new `ALLOWED_FINDINGS`.
- No DeskProto-derived content.

## Scope decisions

- **Clean in `fluut-cam`, not in `cavalier_contours`,** unless the cause is a bug there. The offset of a loop with a nanometre notch is a loop with a nanometre spike; that is a correct answer at the precision `cavalier_contours` works at, but no use to a CAM program, and it is the region's job to keep its loops clean.
- **One tolerance, tied to the region tolerances.** `EPS` (1e-9 mm) is the point-on-boundary tolerance and the existing clean-up (`cleaned`, booleans) works at 1e-7 mm. The clean-up of offsets uses the same 1e-7-scale constant unless the reduced cases need more; whatever it is, it stays well below 1e-4 mm (G-code resolution).
- **Crash reports only for crashes.** A panic caught by a region `guard` is either retried successfully or becomes a `RegionFailure` diagnostic; neither is a crash.

## Review Focus

1. The clean-up removes only features below its tolerance; golden G-code and the determinism test show what else moved.
2. Vertex and loop counts after chained offsets stay bounded (test with the fuzz shape).
3. A panic outside a region `guard` still writes a crash report.

---

### Task 1: Plan

- [ ] **Step 1:** This plan. Commit.

### Task 2: Reproduce and find the cause

**Files:** `crates/fluut-cam/src/region/tests.rs`.

- [ ] **Step 1:** Regression test with the exact class 3 input (five chained offsets). Fails in debug.
- [ ] **Step 2:** Test: chained offsets of the same shape keep a sane vertex and loop count. Fails.
- [ ] **Step 3:** Find where the tiny features come from; record below.

### Task 3: Clean offsets

**Files:** `crates/fluut-cam/src/region.rs`, maybe `third_party/cavalier_contours/{src/…, README-FLUUTCAM.md}`.

- [ ] **Step 1:** Implement the clean-up; tests of Task 2 pass; golden G-code, determinism and CLI simulation checked. Commit.

### Task 4: Crash reports for caught panics

**Files:** `crates/fluut-cam/src/region.rs`, `crates/fluut-core/{src/crash.rs, tests/crash.rs}`.

- [ ] **Step 1:** Test: a panic caught by a region retry writes no report; a panic outside still does. Fails.
- [ ] **Step 2:** Implement; pass. Commit.

### Task 5: Fuzz

**Files:** `fuzz/{README.md, fuzz_targets/region.rs}`.

- [ ] **Step 1:** Raise or remove the 500-vertex stop; run with debug assertions and in release; record executions and findings below; fix what it finds the same way. Commit.

### Task 6: Finish

- [ ] **Step 1:** Merge `main`, run all checks, mark done. Commit.

## Root cause

(Task 2.)

## Fuzz results

(Task 5.)

## Re-blessed golden files

(Task 3.)
