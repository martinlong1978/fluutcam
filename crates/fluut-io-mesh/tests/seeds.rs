// SPDX-License-Identifier: MIT OR Apache-2.0

//! The fuzz seeds in `fuzz/seeds/stl/` read without errors.

use fluut_io_mesh::{Options, read_stl};
use std::path::PathBuf;

#[test]
fn stl_seeds_read() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds/stl");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        let m = read_stl(&std::fs::read(&p).unwrap(), &Options::default())
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        assert!(!m.body.tris.is_empty(), "{}", p.display());
        n += 1;
    }
    assert!(n > 0);
}
