# S3 Follow-ups Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the three limits the S3 report found before M3 builds on the region code: platform trigonometry inside `cavalier_contours` (report §3), the all-pairs segment test and the "first matching piece" stitching in the region booleans (report §2, §7).

**Architecture:** `cavalier_contours` 0.9.0 is vendored under `third_party/cavalier_contours` and replaces the crates.io copy through `[patch.crates-io]`. Only its trigonometric calls change; they go through `libm`. The vendored crate is not a workspace member, so the workspace lints and the layer check do not apply to it. `fluut-cam/src/region/boolean.rs` gets a spatial index for the segment pairs and a turn-angle rule for stitching.

**Tech Stack:** as S3; `static_aabb2d_index` through the re-export `cavalier_contours::static_aabb2d_index`, so no new dependency.

**Design:** [`spikes/s3-region-2d.md`](spikes/s3-region-2d.md) §2, §3, §7; [`../design/challenges.md`](../design/challenges.md) CH-01, CH-16.

## Global Constraints

All S3 Global Constraints apply. In addition:

- The vendored crate differs from 0.9.0 only in its trigonometric calls and the files that say so (`README-FLUUTCAM.md`). Its licence files stay.
- No new behaviour in `cavalier_contours`: only the bits of results may change.

## Review Focus

1. **Patch completeness**: no call in the vendored library code (outside its own tests) reaches `num_traits` or `std` trigonometry.
2. **Junctions**: where several kept pieces start at one point, stitching takes the sharpest left turn, so each loop bounds one piece of interior and loops never cross at a touching point.
3. **Pinned bits**: the determinism test fails on any last-bit change of offset or boolean results.

---

### Task 1: Plan

**Files:** Create `docs/plan/2026-10-31-s3-followups.md`; modify `docs/plan/README.md`.

- [x] **Step 1:** Write this plan and add it to the plans table.

### Task 2: Deterministic `cavalier_contours`

**Files:** Create `third_party/cavalier_contours/` (copy of 0.9.0 plus `README-FLUUTCAM.md`), `crates/fluut-cam/tests/determinism.rs`; modify `Cargo.toml`, `deny.toml` or `xtask` only if they need it.

- `Real` gets `libm_*` methods (`sin_cos`, `tan`, `atan`, `atan2`, `acos`) implemented with `libm` for `f32` and `f64`; library code calls those instead of the `num_traits::Real` methods.
- [x] **Step 1:** Test: offsets and booleans of fixed shapes, with `f64::to_bits` of areas and of a few vertices pinned. Run it against the crates.io copy, record the values.
- [x] **Step 2:** Vendor, patch, point the workspace at it; the test still passes on Linux (glibc and `libm` agree on these inputs or the pins are updated once, with the reason in the commit). `cargo deny check` and `cargo xtask check-layers` pass.

### Task 3: Booleans robustness

**Files:** Modify `crates/fluut-cam/src/region/boolean.rs`, `crates/fluut-cam/src/region/tests.rs`.

- Segment pairs: build a `StaticAABB2DIndex` over the other region's segment boxes and query it per segment. Classifying a piece queries the same index: only segments whose box meets the point or the ray to its right can change the boundary distance test or the winding number.
- Piece lookup while stitching: piece starts sorted by x and searched by bisection instead of a linear scan.
- Stitching: collect the pieces that start at the current end point; take the sharpest left turn, i.e. the first piece clockwise from the way back along the incoming piece, measured with exact end tangents of lines and arcs; equal tangents are ordered by curvature. A loop that still passes through one point twice (a hole touching its outer loop) is split there, so every output loop is simple.
- Nearly collinear edges (found while re-running the S3 measurements): a vertex within `JOIN` of the other region's segment cuts it, and a piece counts as lying on the other boundary only if its ends do too. `examples/s3_booleans.rs` checks the area identities on every union adaptive clearing builds.
- [x] **Step 1:** Tests: two squares sharing one corner (union is two simple loops); a square minus a disc tangent to it inside, and a square and a disc tangent outside; a disc tangent to a hole; two pairs of nearly collinear stadiums from an adaptive pass; the existing proptest, now also checking that every loop is simple; a new proptest on grid-aligned rectangles and discs, where edges coincide and corners touch often.
- [x] **Step 2:** Fail, implement, pass.

### Task 4: Measurements and report

**Files:** Modify `docs/plan/spikes/s3-region-2d.md`, `docs/design/challenges.md`, `docs/plan/README.md`.

- [ ] **Step 1:** Re-run `cargo run -p fluut-cam --example s3 --release`; add a dated "Follow-ups" section with before and after numbers.
- [ ] **Step 2:** Update CH-01 and CH-16 where the results change them; mark this plan done.
