// SPDX-License-Identifier: MIT OR Apache-2.0

//! The three S3 test shapes.

use crate::Region2;

/// A: a 100 × 60 rectangle with a Ø20 island in the middle.
pub fn plate() -> Region2 {
    Region2::rect(0.0, 0.0, 100.0, 60.0).with_hole(&Region2::circle(50.0, 30.0, 10.0))
}

/// B: two 40 × 40 rooms joined by a 20 mm long, 12 mm wide neck.
pub fn rooms() -> Region2 {
    Region2::polygon(&[
        (0.0, 0.0),
        (40.0, 0.0),
        (40.0, 14.0),
        (60.0, 14.0),
        (60.0, 0.0),
        (100.0, 0.0),
        (100.0, 40.0),
        (60.0, 40.0),
        (60.0, 26.0),
        (40.0, 26.0),
        (40.0, 40.0),
        (0.0, 40.0),
    ])
}

/// C: a Ø80 disc with a 10 mm slot cut in from the edge to the centre.
pub fn slotted_disc() -> Region2 {
    Region2::circle(0.0, 0.0, 40.0).subtract(&Region2::rect(-5.0, 0.0, 5.0, 45.0))
}

pub fn all() -> [(&'static str, Region2); 3] {
    [
        ("A plate", plate()),
        ("B rooms", rooms()),
        ("C slotted disc", slotted_disc()),
    ]
}
