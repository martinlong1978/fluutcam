# S3 — 2-D Geometry Spike Report

Date: 2026-10-31. Plan: [`../2026-10-31-s3-region-2d.md`](../2026-10-31-s3-region-2d.md).
Code: `crates/fluut-cam/src/region*` (kept), `crates/fluut-cam/src/spike/` (prototypes),
`crates/fluut-cam/examples/s3.rs` (measurements).

All numbers: release build, Windows 11, one thread, `cargo run -p fluut-cam --example s3 --release`.
Tool radius R = 3 mm.

Test shapes:

- **A**: a 100 × 60 rectangle with a Ø20 island.
- **B**: two 40 × 40 rooms joined by a neck 20 mm long and 12 mm wide.
- **C**: a Ø80 disc with a 10 mm slot cut in from the edge to the centre.

## Summary

| Question | Answer |
|---|---|
| Can `cavalier_contours` carry the D08 §3 region work? | **Yes, with two additions and one patch.** Multi-loop offsets work as needed. Booleans had to be written on top (§2). The library must be patched to use `libm` (§3). |
| Adaptive clearing without a grid (CH-16)? | **Works on all 3 shapes**, with exact arc engagement and no grid. Path length is close to ideal. It is still 15–40× too slow; the causes are known (§5). |
| Exit criterion: "working adaptive pocket on 3 test shapes" | Met. |

## 1. Region type and offsets

`Region2` stores closed loops in the bulge form of `cavalier_contours`. Outer loops run counter-clockwise and holes clockwise. Area, containment, support and bounding box are exact over arcs and are implemented in `fluut-cam`.

`Region2::offset` uses `Shape::parallel_offset`, which offsets the outer loops and holes together. The tests cover:
- a hole growing into the outer boundary until the region splits in two;
- offsets that empty the region;
- correct orientation after each of these.

Offset pocket (D09 §2.2), stepover 2.4 mm:

| Shape | Area mm² | Rings | Path mm | Ring offsets | Uncut mm² |
|---|---|---|---|---|---|
| A | 5685.8 | 16 | 2106.3 | 0.17 ms | 7.7257 |
| B | 3440.0 | 14 | 1197.7 | 0.14 ms | 15.4513 |
| C | 4627.6 | 8 | 1702.6 | 0.06 ms | 5.8948 |

The uncut area is exactly the material a round tool cannot reach in sharp convex corners: R²(1 − π/4) per 90° corner. A has 4 such corners, B has 8, and C has 2 non-right corners. The rings leave **no slivers**: the swept union minus the region matches the corner residue to 4 decimals.

Offsets are fast enough. They will not be the bottleneck.

## 2. Booleans: `cavalier_contours` combines two single loops only

`Polyline::boolean` works on one loop against one loop. D08 §3 needs booleans between regions that have several outer loops and holes, such as clear ∖ cleared or region ∩ boundary.

**What was built.** `region/boolean.rs`, about 200 lines, uses split and classify:
1. Split every segment where it meets the other region, using `cavalier_contours::pline_seg_intr`.
2. Keep or drop each piece by testing its midpoint against the other region. Coincident pieces are decided by a side probe at 1e-5.
3. Stitch the kept pieces into loops.

Orientation carries through without extra work.

**Tests.** A proptest checks `area(A ∪ B) + area(A ∩ B) = area(A) + area(B)` and `area(A ∖ B) = area(A) − area(A ∩ B)`. It runs on random unions and differences of rectangles and discs, and passes 2 000 cases. Hand-written cases cover coincident edges, touching regions, containment and disjoint regions.

**Limits found.**
- Segment pairs are tested with an all-pairs bounding-box filter, which costs O(n·m).
- Stitching takes the first matching piece at each junction. Regions that touch at a single point can therefore stitch into a figure-eight loop. It did not happen in any test, but the risk is real.

Both limits must be fixed before M3 uses this code in production:
- a spatial index, for which `cavalier_contours`' `StaticAABB2DIndex` can be reused;
- a turn-angle rule at junctions.

Both are done; see [Follow-ups](#follow-ups-2026-10-31). The figure-eight risk was real: a proptest on grid-aligned shapes finds it at once.

## 3. Determinism: `cavalier_contours` uses platform trigonometry

**Finding.** The crate calls `tan`, `atan`, `atan2`, `sin`, `cos` and `acos` through `num_traits::Real`, which resolves to the platform libm. There are 36 call sites. They are in `raw_pline_offset.rs`, `pline_intersects.rs`, `pline_seg.rs`, `base_math.rs` and `traits.rs`, so offsets and intersections are affected.

This breaks the CH-01 resolution: results must be bit-identical on all platforms, and only `libm` is allowed.

**Decision.** Use a patched copy of `cavalier_contours`. The licence is MIT OR Apache-2.0, so this is allowed. The patch routes these calls to `libm`: either a vendored crate under `third_party/`, or a git fork referenced through `[patch.crates-io]`. The patch is small and mechanical. We will also offer it upstream as an optional `libm` feature.

This must happen before any `fluut-cam` output is golden-tested, which means early in M3.

Done as a vendored copy; see [Follow-ups](#follow-ups-2026-10-31).

## 4. Line clipping

`Region2::clip_line` returns exact intervals from line–line and line–circle intersections. Tangent points are dropped. A collinear edge counts as inside, because the region is closed. This is what raster pass layout needs (D08 §3).

## 5. Adaptive clearing (CH-16)

**Prototype** (`spike/adaptive.rs`):
- **Entry:** a helix at the deepest point of the tool-centre region. The cleared region K starts as that helix's disc.
- **Each pass:**
  - It starts at the nearest point half a radius inside K's edge where the tool engages enough material.
  - Every step of 0.75 mm (R/4) picks its heading by bisection, so that engagement equals the 60° target with the material on the right.
  - The pass ends when it leaves the tool-centre region or runs out of material.
  - Then K ∪= the pass's swept region, built as a union of stadiums.

**Engagement** (`spike/engagement.rs`) is exact. It finds the arcs of the tool circle that lie in the uncut region by cutting the circle at its exact intersections with every boundary segment. It then subtracts the arcs within R of the tool's earlier positions in the current pass.

Tests:
- Analytic half-plane cases match to 1e-12.
- Arc regions match a 200 000-point numerical integration within 1e-4 rad. That is the integration's own resolution, so the plan's 1e-6 target could not be checked this way.

**Results**, target 60°:

| Shape | Passes | Steps | Path mm | Ideal ≈ area / 1.5 mm | RMS error | Steps > 10° off | Uncut mm² (corner residue) | Time |
|---|---|---|---|---|---|---|---|---|
| A | 229 | 4640 | 3480 | 3790 | 12.1° | 270 (5.8 %) | 17.48 (7.73) | 29.7 s |
| B | 209 | 2173 | 1630 | 2290 | 15.0° | 195 (9.0 %) | 24.53 (15.45) | 12.7 s |
| C | 201 | 3842 | 2882 | 3085 | 16.5° | 350 (9.1 %) | 16.28 (5.89) | 18.6 s |

Path length is 8–29 % below the straight-wall estimate (area / 1.5 mm). The estimate ignores curved walls and steps above the target engagement, and the gap was not looked into further. The "max error 60°" seen on every shape comes from the final step of a pass, where no material is left.

**Slivers.** About 9–10 mm² per shape is left beyond the unreachable corners, roughly 0.2 % of the area. D09 §2.3 already plans a final contour pass, and that pass removes this.

**First attempt.** Engagement there was measured against K alone, so a pass re-cut its own sweep. Shape A then needed a 45 m path and 648 s. Subtracting the pass's own earlier positions fixed this. Keep that rule in the real implementation.

**Speed.** 12–30 s per level, when the goal is well under 1 s. K stays small (236–342 segments), so the time goes into engagement evaluations: about 50 per step, each scanning every segment and computing arc geometry with `atan2` each time. Expected fixes, together worth 20–50×:
- Cache the arc centre, radius and angles per segment.
- Use a spatial index for circle–segment and containment queries.
- Use 12 bisection iterations instead of 30, which is already well under 0.1°.
- Update the uncut region incrementally instead of recomputing `clear ∖ K` for every pass.

**Too many passes** (200+ per shape, so many links). Passes end when material runs out in a corner or neck. A real implementation should continue along the K boundary while staying down (D09 §2.3: "link (stay down if inside K)"). This affects machining time, not correctness.

## 6. Questions not prototyped

- **CH-13 (B-spline boundaries).** Nothing found changes the plan. `Region2` keeps no per-segment source reference yet; M3 adds it, and refinement onto the exact curve follows D08 §3.1.
- **CH-17 (medial axis).** `cavalier_contours` has nothing for it. The workaround in CH-17, our own Voronoi of line and arc sites, stands. The split and classify code and the exact circle intersections here can be reused for the bisector clipping. Estimate: 3–5 weeks for a robust version.
- **CH-18 (scallop).** The library offsets by uniform distances only, as expected. The workaround stands.
- **CH-19 (unrolled rotary offsets).** Scaling θ maps arcs to ellipses, which a line-and-arc region cannot hold. The anisotropic offset must therefore work on loops linearized at `tol.machining / 4` before scaling, then refit with biarcs (D02 §7) after unscaling. This costs an extra fit step, but the workaround still works.

## 7. Decisions

1. Keep `cavalier_contours` (OQ-06 confirmed), with a `libm` patch (§3).
2. Keep the multi-loop booleans in `fluut-cam`. Before M3 adds a spatial index and a junction rule.
3. Build adaptive clearing on exact arc engagement, as designed. No grid fallback is needed. The prototype's speed issues have known fixes. Do the performance work as its own M6 task, with a target of < 1 s per level on these shapes.

## Follow-ups (2026-10-31)

Plan: [`../2026-10-31-s3-followups.md`](../2026-10-31-s3-followups.md). These numbers come from a different machine from the rest of this report: a Linux cloud container, release build, one thread. "Before" is `main` measured on that machine.

### Deterministic `cavalier_contours`

`third_party/cavalier_contours` is 0.9.0 with its trigonometry routed to `libm`, used through `[patch.crates-io]`; its `README-FLUUTCAM.md` lists the changes. All the crate's library trigonometry goes through 7 call sites; the other matches behind the "36 sites" count are its own tests and doc examples, which the workspace does not build. The crate's own 287 tests pass with the patch. `cargo deny` and `xtask check-layers` needed no change; `third_party` is in `workspace.exclude` so the copy keeps its own lints and its tests run in place.

`crates/fluut-cam/tests/determinism.rs` pins the bits of offset, boolean and arc-helper results. Platform maths already makes a difference on one machine: with the unpatched crate, glibc and `libm` give different bits for `arcs_to_approx_lines`. The adaptive runs also changed with the patch alone (shape B 209 → 213 passes, C 201 → 184). So last-bit differences in trigonometry do reach toolpaths, which is what CH-01 feared.

### Booleans

- Segment pairs and point classification go through a `StaticAABB2DIndex` per region. Containment looks only at segments near the point or on the ray to its right.
- Stitching takes the sharpest left turn where several pieces start at one point, using exact end tangents with ties broken by curvature. Loops that still pass a point twice, such as a hole touching its outer loop, are split there.
- The old stitching does make figure-eights: a new proptest on grid-aligned rectangles and discs fails at once with it. The new code passes 200 000 cases, and both proptests now check that every loop is simple.
- Re-running the measurements exposed a third problem. Consecutive adaptive steps give stadiums with nearly collinear sides. A sliver that came within 1e-7 of the other edge at its middle but not at its end was treated as coincident, so both sides dropped it. Vertices within 1e-7 of the other region's segments now cut them, and a piece counts as coincident only if its ends are on the other boundary too. `examples/s3_booleans.rs` replays every union of the adaptive runs and checks the area identities of §2.

| | Before | After |
|---|---|---|
| Star polygons, 4 × 500 segments: ∪, ∩, ∖ | 30 ms | 3.8 ms |
| Star polygons, 4 × 2 000 segments | 701 ms | 54 ms |
| Star polygons, 4 × 8 000 segments | 31.5 s | 1.9 s |
| Adaptive unions that break an area identity by > 1e-6 mm² | 65 of 10 725 | 0 of 10 423 (worst 4.5e-7) |
| Sweep and K ∪ sweep inside the adaptive runs, A / B / C | 745 / 209 / 399 ms | 131 / 64 / 85 ms |
| Swept union of the offset-pocket rings, A / B / C | 0.7 / 0.9 / 0.3 ms | 0.4 / 0.6 / 0.3 ms |

The two adaptive rows were measured with the old boolean code on the patched crate, so their paths differ slightly from `main`'s (see above).

The stars are a hard case for the index: their spikes point outwards, so near the x axis a ray to the right meets many segment boxes. At 4 × 8 000 segments that averages 356 boxes per ray, against 74 at 4 × 2 000. Areas are the same before and after. The offset pocket is unchanged: same rings, length and uncut area.

### Adaptive clearing, re-run

| Shape | Passes | Steps | RMS error | Steps > 10° off | Uncut mm² | Time |
|---|---|---|---|---|---|---|
| A, before | 229 | 4640 | 12.1° | 270 | 17.48 | 17.1 s |
| A, after | 229 | 4640 | 12.1° | 270 | 17.48 | 16.9 s |
| B, before | 209 | 2173 | 15.0° | 195 | 24.53 | 7.4 s |
| B, after | 216 | 2207 | 18.9° | 293 | 28.45 | 8.6 s |
| C, before | 201 | 3842 | 16.5° | 350 | 16.28 | 10.8 s |
| C, after | 215 | 3576 | 17.5° | 340 | 15.66 | 14.4 s |

A gives the same path as before. B and C give different paths, because the prototype samples its start points along K's offset from each loop's first vertex. Changing only where loops start moves B between 14.9° and 18.5° RMS. These statistics are therefore noisy by a few degrees, and better start-point selection belongs in the M6 work.

Sweeps and unions now take under 1 % of a level. The time is engagement, as §5 said. B and C are slower per step after the change because their paths run past uncut regions with more segments (338 against 244 per pass on B), and engagement scans every segment. On A, segments per pass are unchanged (449). The §5 fixes still apply unchanged.
