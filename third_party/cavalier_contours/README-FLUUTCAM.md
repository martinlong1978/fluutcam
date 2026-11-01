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

The trigonometric calls, and one fix for collapsed offset slices. Everything else is as
published.

- `src/polyline/pline_view.rs`, `PlineViewData::create_on_single_segment`: returns `None`
  when the end point lies within `VALIDATION_EPS` of the segment's start vertex. Such a slice
  is collapsed, but it was built anyway and failed `validate_for_source`, which panicked in
  debug builds. It happened when an inward offset reached exactly half a slot's width
  (`offset_to_exactly_half_the_width` in `crates/fluut-cam/src/region/tests.rs`).

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
