// SPDX-License-Identifier: MIT OR Apache-2.0

//! Bytes that parse go on to the schema, units, census and mapping
//! (including p-curves).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(m) = fluut_io_step::parse(data) {
        let _ = fluut_io_step::schema(&m);
        let _ = fluut_io_step::census(&m);
        let _ = fluut_io_step::map_bodies(&m);
    }
});
