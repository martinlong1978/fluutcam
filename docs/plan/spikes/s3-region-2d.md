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

## 3. Determinism: `cavalier_contours` uses platform trigonometry

**Finding.** The crate calls `tan`, `atan`, `atan2`, `sin`, `cos` and `acos` through `num_traits::Real`, which resolves to the platform libm. There are 36 call sites. They are in `raw_pline_offset.rs`, `pline_intersects.rs`, `pline_seg.rs`, `base_math.rs` and `traits.rs`, so offsets and intersections are affected.

This breaks the CH-01 resolution: results must be bit-identical on all platforms, and only `libm` is allowed.

**Decision.** Use a patched copy of `cavalier_contours`. The licence is MIT OR Apache-2.0, so this is allowed. The patch routes these calls to `libm`: either a vendored crate under `third_party/`, or a git fork referenced through `[patch.crates-io]`. The patch is small and mechanical. We will also offer it upstream as an optional `libm` feature.

This must happen before any `fluut-cam` output is golden-tested, which means early in M3.

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
