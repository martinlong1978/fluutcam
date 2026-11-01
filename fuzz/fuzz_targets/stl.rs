// SPDX-License-Identifier: MIT OR Apache-2.0

//! Any bytes into the STL reader, welding and adjacency included.

#![no_main]

use fluut_io_mesh::{Limits, Options, read_stl};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let opts = Options {
        scale: Some(1.0),
        limits: Limits {
            max_triangles: 100_000,
            ..Limits::default()
        },
        ..Options::default()
    };
    let _ = read_stl(data, &opts);
});
