// SPDX-License-Identifier: MIT OR Apache-2.0

//! `engrave` (D15b §2.4): parameters and drive chains are checked; the
//! V-carve itself waits for the exact medial axis (CH-17).

mod support;

use support::cam::*;

const W: &str = "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n";

fn stock() -> Stock {
    Stock::new([-20.0, -20.0, 0.0], [60.0, 60.0, 20.0])
}

#[test]
fn reports_unavailable_after_checking_its_input() {
    let square = wire(&[
        [0.0, 0.0, 20.0],
        [10.0, 0.0, 20.0],
        [10.0, 10.0, 20.0],
        [0.0, 10.0, 20.0],
        [0.0, 0.0, 20.0],
    ]);
    let t = tp("engrave", "v90", W);
    assert_eq!(run_err(&square, &t, stock()), ["cam.type.unavailable"]);
    // Parameter errors come first: not a V tool.
    let t = tp("engrave", "f6", W);
    assert!(run_err(&square, &t, stock()).contains(&"param.engrave.tool_shape".into()));
    // Open chains cannot bound a shape.
    let line = wire(&[[0.0, 0.0, 20.0], [10.0, 0.0, 20.0]]);
    let t = tp("engrave", "v90", W);
    assert!(run_err(&line, &t, stock()).contains(&"cam.pocket_2d.region.open".into()));
}
