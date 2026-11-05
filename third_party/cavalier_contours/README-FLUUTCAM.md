# `cavalier_contours` 0.9.0, patched for FluutCAM

This is a copy of [`cavalier_contours`](https://github.com/jbuckmccready/cavalier_contours)
0.9.0 as published on crates.io (upstream commit `22887a824d22cd8ab50968a551f34cdb4508127f`).
It is licensed under MIT OR Apache-2.0, like the original; see `LICENSE-MIT` and
`LICENSE-APACHE`, taken from the same upstream commit because the published crate does not
include them. The workspace uses it in place of the crates.io crate through
`[patch.crates-io]` in the root `Cargo.toml`.

## Why

FluutCAM results must be bit-identical on Windows, macOS and Linux (design D01 §6,
challenge CH-01). The original crate calls `sin`, `cos`, `tan`, `atan`, `atan2` and `acos`
through `num_traits::real::Real`, which uses the platform maths library. Those functions can
differ in the last bit between platforms. See the S3 report, `docs/plan/spikes/s3-region-2d.md`
§3.

## What changed

The trigonometric calls, and nine fixes for nearly degenerate offsets and intersections. Everything else is as
published.

- `src/core/math/line_line_intersect.rs`, `line_line_intr`: for collinear segments, the
  overlap is measured along the larger component of the second segment. The original tested
  whether its X component was within `eps` of zero and otherwise divided by it. A steep
  segment with an X extent just over `eps` (3e-5 for 0.75 mm of Y) then gave parameters
  scaled by the tiny X differences: two segments that only share an end were reported to
  overlap 0.11 mm along the second. The offset built a slice starting at that point, off its
  segment, which failed `validate_for_source` (a debug assertion in `PlineViewData::create`).
  It happened when growing a sliver triangle from a section of the dome
  (`offset_of_a_sliver_triangle` and `steep_lines_meeting_end_to_end_do_not_overlap` in
  `crates/fluut-cam/src/region/tests.rs`). Upstream (`master`, November 2026) has the same
  code.
- `src/shape_algorithms/mod.rs`, `IndexedPolyline::parallel_offset_for_shape`: an offset that
  comes back open is closed. Where offset slices meet almost tangentially (a micrometre bump
  left by a boolean of a circle and an edge it nearly touches), the stitching can leave the
  offset of a closed loop open. The shape offset takes every offset as a closed loop, and an
  open one failed a debug assertion in `PlineViewData::from_slice_points`. Release builds
  wrapped the indexes anyway and gave the same results as with the loop closed (within 1e-8
  mm²). Found by the `region` fuzz target (`offsets_at_near_tangent_contacts` in
  `crates/fluut-cam/src/region/tests.rs`). The stitching itself is not changed.
  The open offset can also run on past its start and retrace its first segments (the end
  vertices repeat the first ones); that tail is removed before closing, or the closed loop
  overlaps itself. And an open piece no wider than `pos_equal_eps` once closed (a stray slice
  along one segment, two vertices) is dropped: closed, it became a sliver loop that the shape
  stitching joined to the main loop as a spike. Both made chained offsets multiply loops and
  vertices, and led to a debug assertion in `parametric_from_point`
  (`chained_offsets_of_a_ring_with_a_sliver_bump` and
  `vertex_counts_stay_bounded_under_chained_offsets`; plan
  `docs/plan/2026-11-03-region-micro-loops.md`).
  Before any piece is closed, open pieces that end where another starts (within
  `pos_equal_eps`) are joined into one, which may then close. The offset of one loop can come
  back as several open pieces that together form the loop: shrinking a shape by half the width
  of a bar, so that the bar collapses to a line, gave three. Closed one by one, the first was
  closed by a chord across the region and the others were dropped, a wrong region of
  plausible area (`shrink_where_a_bar_collapses_to_a_line`; plan
  `docs/plan/2026-11-04-region-touching-booleans.md`).
- `src/shape_algorithms/mod.rs`, `Shape::stitch_slices_together`: when a loop is closed,
  every last vertex on top of the first is removed, not only one. A last slice that ends with
  a collapsed segment left a second one, so the closed loop had a zero-length segment, and a
  later boolean failed a debug assertion in `seg_arc_radius_and_center`. Found by the
  `region` fuzz target (`offset_of_loops_touching_at_a_vertex`).

- `src/core/math/line_circle_intersect.rs`, `line_circle_intr`: a line whose X extent is within
  the default epsilon of zero but not zero is written as dy·x − dx·y + C = 0 instead of being
  taken as vertical at its mean X. The vertical shortcut put intersection points far along a
  short, nearly vertical line (2 700 line lengths for 6e-10 mm of X over 1.07e-3 mm of Y) off
  it by more than the 1e-9 epsilon of the booleans, which failed the debug assertion in
  `parametric_from_point`. Exactly vertical lines and all others are unchanged. Found by the
  `region` fuzz target (`intersection_with_a_nearly_vertical_line`). Upstream (`master`,
  November 2026) has the same code.
- `src/polyline/pline_view.rs`, `PlineViewData::create_on_single_segment`: returns `None`
  when the end point lies within `VALIDATION_EPS` of the segment's start vertex. Such a slice
  is collapsed, but it was built anyway and failed `validate_for_source`, which panicked in
  debug builds. It happened when an inward offset reached exactly half a slot's width
  (`offset_to_exactly_half_the_width` in `crates/fluut-cam/src/region/tests.rs`).

- `src/polyline/internal/raw_pline_offset.rs`, `RawOffsetBuilder::join_without_intersection`
  (and the new `join_tangent`): an arc whose offset radius is at most `pos_equal_eps` uses the
  source arc's tangent at the shared vertex to classify its joins. An arc counts as collapsed
  only when its offset radius is below the default epsilon (1e-8); between that and
  `pos_equal_eps` (1e-5) it stayed an arc, and its tangent, as long as its radius, was taken for
  a zero-length segment: every join next to it became an invalid gap and the whole closed offset
  was dropped. It happened when growing a loop with a concave arc of radius R by R − 1e-6, as
  `optimised_load::shrink` grows holes of a cleared region (`growth_where_a_concave_arc_collapses_to_a_point`;
  plan `docs/plan/2026-11-04-region-empty-growth.md`). Arcs with a larger offset radius are
  unchanged.
- `src/polyline/internal/raw_pline_offset.rs`, `RawOffsetBuilder::join_without_intersection`: a
  join whose tangents are nearly opposite, within `pos_equal_eps / |offset|` rad, is a reversal
  (bridged by a round connector), not only within the default epsilon. At a spike tip where two
  arcs meet back to back, rounding can leave the tangents turning back a micro-radian on the
  inner side; the join was then an inner join, the offsets on either side of a tip do not cross,
  the join became an invalid gap and every growth of the shape came back empty. Found by the
  `region` fuzz target through FluutCAM's containment check
  (`growths_of_a_shape_with_spike_tips`).
- `src/core/math/line_line_intersect.rs`, `line_line_intr`: for segments with |v| |u| < 1 that
  are not points, the parallel test compares the perp product of the segment vectors with
  `eps · |v| |u|` (an angle of `eps`) and the collinear tests with `eps · |v|` and `eps · |u|` (a
  distance of `eps`). The original compared these products, which scale with the lengths, with
  `eps` itself, so short segments at a clear angle counted as parallel and then as overlapping:
  sin θ up to 0.011 for lines 0.022 and 0.042 mm long, 0.16 rad for 6.6e-3 mm. In an offset's inner
  line join (`line_line_join`) that asked `parametric_from_point` for a point 1.5e-5 mm off the
  line (a debug assertion; release builds reversed the join and dropped the whole growth,
  `growth_of_short_nearly_collinear_lines`), and in the self-intersections of a shrunk thin
  triangle it gave a slice end off its segment (a debug assertion in `PlineViewData::create`,
  found by the `region` fuzz target, `shrink_of_a_thin_triangle_to_near_its_inradius`).
  Segments with |v| |u| >= 1, and point-like ones, are unchanged.

The trigonometric changes:

- `src/core/traits/real.rs`: the `Real` trait has five new methods, `libm_sin_cos`,
  `libm_tan`, `libm_atan`, `libm_atan2` and `libm_acos`, implemented for `f32` and `f64`
  with the pure-Rust `libm` crate.
- Library code calls these instead of the `num_traits` methods:
  - `src/core/math/base_math.rs`: `angle`, `point_on_circle`, `bulge_from_angle`,
    `angle_from_bulge`;
  - `src/core/math/vector2.rs`: `Vector2::rotate_about`;
  - `src/polyline/pline_seg.rs`: `dist_from_segment_start` (arc lengths);
  - `src/polyline/traits.rs`: `arcs_to_approx_lines`.

  Most of the crate's trigonometry goes through the four `base_math` helpers, so these seven
  call sites cover the offset, intersection and boolean code.
- `Cargo.toml` and `Cargo.toml.orig`: `libm = "0.2"` added to the dependencies.

The crate's own unit tests, doc examples and benchmarks still call the `num_traits` methods.
They are not built by the FluutCAM workspace and do not affect its results.

`crates/fluut-cam/tests/determinism.rs` pins offset, boolean and arc-helper results bit by bit.
With the unpatched crate, `arcs_to_approx_lines` already gives different bits on Linux (glibc)
than with `libm`.

## Updating

Copy the new release over this directory, then repeat the changes above. Search `src/` for
`sin`, `cos`, `tan`, `atan`, `atan2`, `acos`, `asin`, `exp`, `ln` and `powf` outside tests to
find new call sites. The change could also go upstream as an optional `libm` feature.
