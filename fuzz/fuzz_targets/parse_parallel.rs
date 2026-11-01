// SPDX-License-Identifier: MIT OR Apache-2.0

//! Parallel parsing against serial parsing: the first byte sets the chunk
//! size (1 to 256 bytes), the rest is the file. Both must give the same
//! model, or the same error at the same position.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&c, src)) = data.split_first() else {
        return;
    };
    let serial = fluut_io_step::parse_serial(src);
    let chunked = fluut_io_step::parse_chunked(src, c as usize + 1);
    assert!(chunked == serial, "chunk size {}: {chunked:?} != {serial:?}", c as usize + 1);
});
