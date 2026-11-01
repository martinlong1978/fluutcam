// SPDX-License-Identifier: MIT OR Apache-2.0

//! Any bytes into the SVG reader, with small limits so that runs are not
//! spent on legitimately large drawings.

#![no_main]

use fluut_io_2d::{Limits, Options, read_svg};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let opts = Options {
        scale: None,
        limits: Limits {
            max_curves: 10_000,
            max_items: 100_000,
            max_points: 10_000,
            ..Limits::default()
        },
        ..Options::default()
    };
    let _ = read_svg(data, &opts);
});
