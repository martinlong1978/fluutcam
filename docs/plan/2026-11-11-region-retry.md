# Region Growths That Failed Every Retry (reg-8)

**Goal:** the two growths left open by the [touching-booleans plan](2026-11-04-region-touching-booleans.md) (`growth_of_a_grown_strip_with_a_micrometre_slit` and `growth_after_a_shrink_of_unions_with_a_thin_triangle`, `#[ignore]`d, reported as `cam.region.offset_failed`) succeed with a correct result. Find the cause rather than add a retry.

**Where:** `third_party/cavalier_contours` (raw offset joins), `crates/fluut-cam/src/region.rs` (`offset_raw`), tests in `crates/fluut-cam/src/region/tests.rs`.

## Reproduction

Both inputs end in a single loop that `cavalier_contours` offsets to nothing: `Polyline::parallel_offset` on the loop alone returns no polyline, at every distance tried for the second case and above about 1.3 mm for the first. Every retry of `try_offset` (a hair deeper or shallower, two steps, loop by loop, tidied) offsets the same loop, so all of them failed too. The raw offsets showed one invalid segment each and no self-intersection, and `into_validated_slice_set` drops a closed raw offset with an invalid segment and no dissection points.

## Root causes

1. **Lines at a micro-radian angle are neither crossing nor collinear** (`cavalier_contours`, `line_line_intr` as used by `RawOffsetBuilder::line_line_join`). The first growth of the slit strip leaves the left edge turning by 8e-6 rad at a vertex between lines 1.5 and 0.5 mm long (the slit is 3 µm deep). At that inner join the two offset lines cross, but `line_line_intr` (with `pos_equal_eps` = 1e-5) takes them for parallel (an angle below 1e-5 for segments with |v| |u| < 1), and then for collinear only when the start of the long line lies within 1e-5 · |u| of the short one's line; 1.5 mm away it lies 1.2e-5 off. So `NoIntersect`, the join became an invalid connector (`InvalidInnerGap`), the offsets' ends lay `|offset| · angle` = 3.2e-5 apart (more than `pos_equal_eps`, so not a coincident join), the raw offset has no self-intersection there to cut the connector out, and the whole loop was dropped. It happens when a line longer than 1 mm meets a short one at an angle between `pos_equal_eps / length` and `pos_equal_eps`, for offsets above `pos_equal_eps / angle`.
2. **Tidying turned a self-touching loop into a self-crossing one** (`Region2::offset_raw`). Shrinking the union with the thin triangle by 3.75 leaves a corner of about 20° at (10.027, 4.0). `cavalier_contours` returns the shrink with a lobe 5e-5 mm wide beyond that tip: the loop passes the point P = (10.026848, 4.000001) twice, once between two lines along the triangle's edge and once between two lines along the bottom, with a clockwise sliver of 3e-9 mm² between (inside the input's tolerance band, so its slices were kept). `offset_raw` tidied each loop *before* splitting it where it touches itself: tidying removes a vertex between two lines in the same direction, which removed P both times, and the loop then crossed itself near the tip instead of touching there. The split found no repeated point, the shrink passed the guards (area and containment are fine), and the next growth of a self-crossing loop came back empty. With the loop split first, the sliver is a separate clockwise loop on the boundary of the main one and is dropped as a hole outside every outer loop, as before.

## Fix

1. `RawOffsetBuilder::line_line_join` (vendored patch, `README-FLUUTCAM.md`): on `NoIntersect`, the exact crossing of the two offset lines is used when it lies on both (within `pos_equal_eps`). An inner join is never between parallel lines (those are degenerate joins, classified before), so the crossing is well defined; its parameters come from the plain perp-product formula. `line_line_intr` itself is unchanged (it is also used by the booleans and self-intersection search), and so is every join it already resolves.
2. `Loop2::tidied_parts`, used by `offset_raw`: the loop is split where it passes the same point twice, then each part is tidied and split again (tidying can merge vertices into a new repeated point). A loop that does not touch itself gives exactly what tidying then splitting gave before.

No retry was added; both growths now succeed at the first attempt.

## Tests

- `growth_of_a_grown_strip_with_a_micrometre_slit` and `growth_after_a_shrink_of_unions_with_a_thin_triangle`: no longer ignored; every step is checked against the point oracle.
- `growth_across_a_vertex_turning_by_micro_radians` (cause 1, constructed): a 2 mm square whose left edge has a vertex 3 µm inward (turning by 9.4e-6 rad between lines 1.6 and 0.4 mm long) grown by 4. `cavalier_contours` returns one loop, no containment retry, oracle and area right. Fails before the fix (empty growth).
- `growths_across_vertices_turning_by_micro_radians` (cause 1, sweep): six edge splits × four dents (inward and outward, 1-8 µm) × five distances (shrinks included): oracle, area within the dent's share, and no containment retry. Fails before the fix.
- `tidied_parts_split_a_loop_where_it_touches_itself_before_tidying` (cause 2, constructed): a triangle with a lobe 5e-5 mm wide touching its tip between collinear lines on both sides; `tidied_parts` gives the 3-vertex triangle and no part touches itself, while tidying first loses the tip. Fails with the split-first step removed, as does the thin-triangle fixture.

## Results

RESULTS

## Open issues

- Other inner joins (line-arc, arc-arc) at near-tangent angles go through circle intersections with their own tolerances; no failure of that kind is known, but the same class (two primitives that cross, reported as not crossing) could exist there.
- The guards are still plausibility checks (see the touching-booleans plan). A self-crossing loop of plausible area passes them; it is now not produced by `offset_raw` from a self-touching one, but a boolean or the input could still give one.
