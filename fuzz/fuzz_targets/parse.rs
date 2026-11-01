// SPDX-License-Identifier: MIT OR Apache-2.0

//! Any bytes into the Part 21 parser: errors are fine, panics are not.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = fluut_io_step::parse(data);
});
