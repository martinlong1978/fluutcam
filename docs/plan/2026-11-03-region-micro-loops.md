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
- **Tolerances tied to the region tolerances.** `EPS` (1e-9 mm) is the point-on-boundary tolerance and the existing clean-up (`cleaned`, booleans) works at 1e-7 mm. The clean-up of offsets needed 1e-5 mm (the kinks reach a few 1e-6 mm), still two orders below G-code resolution (1e-3 mm). It changes only short features (at most 1e-3 mm), so long edges keep their G-code points.
- **Crash reports only for crashes.** A panic caught by a region `guard` is either retried successfully or becomes a `RegionFailure` diagnostic; neither is a crash.

## Review Focus

1. The clean-up removes only features below its tolerance; golden G-code and the determinism test show what else moved.
2. Vertex and loop counts after chained offsets stay bounded (test with the fuzz shape).
3. A panic outside a region `guard` still writes a crash report.

---

### Task 1: Plan

- [x] **Step 1:** This plan. Commit.

### Task 2: Reproduce and find the cause

**Files:** `crates/fluut-cam/src/region/tests.rs`.

- [x] **Step 1:** Regression test with the exact class 3 input (five chained offsets). Fails in debug.
- [x] **Step 2:** Test: chained offsets of the same shape keep a sane vertex and loop count. Fails.
- [x] **Step 3:** Find where the tiny features come from; record below.

### Task 3: Clean offsets

**Files:** `crates/fluut-cam/src/region.rs`, maybe `third_party/cavalier_contours/{src/…, README-FLUUTCAM.md}`.

- [x] **Step 1:** Implement the clean-up; tests of Task 2 pass; golden G-code, determinism and CLI simulation checked. Commit.

### Task 4: Crash reports for caught panics

**Files:** `crates/fluut-cam/src/region.rs`, `crates/fluut-core/{src/crash.rs, tests/crash.rs}`.

- [x] **Step 1:** Test: a panic caught by a region retry writes no report; a panic outside still does. Fails.
- [x] **Step 2:** Implement; pass. Commit.

### Task 5: Fuzz

**Files:** `fuzz/{README.md, fuzz_targets/region.rs}`.

- [ ] **Step 1:** Raise or remove the 500-vertex stop; run with debug assertions and in release; record executions and findings below; fix what it finds the same way. Commit.

### Task 6: Finish

- [ ] **Step 1:** Merge `main`, run all checks, mark done. Commit.

## Root cause

The class 3 ring (`ring_with_a_sliver_bump`: a disc minus a disc that a third disc overlaps by 2e-6 mm) has a sliver bump 2e-6 mm high and 6e-3 mm long on its hole. Its offsets keep that bump as a notch of nanometre-flat arcs, which is a fair answer at the precision `cavalier_contours` works at. Two things in the vendored crate, both in the FluutCAM patch that closes open offsets, then turn it into self-overlap:

1. **A retraced tail.** At a notch, `parallel_offset` of a closed loop came back open and ran on past its start: its last two vertices repeat its first two (second offset of the chain). The patch closed it as it was, so the loop retraced its first segment.
2. **Stray slices.** It can also return extra open pieces of two vertices, single slivers along one segment (third offset). The patch closed them into sliver loops of area 1e-10 mm², and the shape stitching (`slice_join_eps` 1e-4) joined them to the main loop as spikes that turn back on themselves.

Each self-overlap made the next offset worse; at the fifth, a line–circle intersection on a retraced spike failed the assertion in `parametric_from_point` (release builds carried on, with a shrink larger than its input, area 122.8 to 140.4). Even with both fixed, every offset leaves nanometre kinks and arcs (vertices 1e-6 mm off a line, arcs with a sagitta of 1e-8 mm), and repeated offsets multiply them: with no clean-up the chain of `vertex_counts_stay_bounded_under_chained_offsets` did not finish in ten minutes; with the clean-up below but without the arc flattening it went from 8 vertices to 45 in ten offsets, then 1 771 and 471 838 in two more (the fuzz timeouts). Without the kink removal it blew up as well (149 vertices and four loops after eleven offsets, then an assertion).

**Fix.** In the vendored crate (README entry): the closing patch removes a tail that repeats the start, and drops open pieces no wider than `pos_equal_eps` once closed. In `fluut-cam`: every offset loop is tidied (`Loop2::tidied`): vertices within `TIDY` (1e-5 mm) merged, spikes (a vertex where the loop turns back, within `TIDY` of its neighbours' line) removed, kinks within `TIDY` of a straight line removed where a segment is at most `TIDY_SHORT` (1e-3 mm) long, short arcs with a sagitta under `TIDY_FLAT` (1e-6 mm) turned into lines, and loops narrower than `TIDY` dropped. The length limit keeps every vertex on long straight edges: without it, collinear points vanished from the `rough3d` G-code, and flattening arcs up to 1e-5 mm sagitta changed one link in `plate2d` (both harmless, but not needed). The chain now stays at 5 to 12 vertices.

**Crash reports.** `region::guard` runs through `region::catch_quiet`, which marks the thread; the crash hook (`fluut-core`) skips a panic while the mark is set (no report, no message). The panic message still travels in the `RegionFailure` if every retry fails.

## Fuzz results

(Task 5.)

## Re-blessed golden files

None. Every golden G-code file, the determinism test and the CLI simulation tests are unchanged. Earlier versions of the clean-up did change them, which set its limits (see Root cause): removing every vertex between collinear lines dropped points on straight edges from `rough3d-linuxcnc.ngc` and a 0.013 mm arc from `dome-linuxcnc.ngc`, and flattening arcs up to 1e-5 mm sagitta turned one retract in `plate2d-linuxcnc.ngc` into a stay-down move.
