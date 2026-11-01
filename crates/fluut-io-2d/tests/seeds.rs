// SPDX-License-Identifier: MIT OR Apache-2.0

//! The fuzz seeds in `fuzz/seeds/` read without errors or warnings, so the
//! fuzzer starts from files that reach every entity and element handler.

use fluut_io_2d::{Options, read_dxf, read_svg};
use std::path::PathBuf;

fn seeds(kind: &str) -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/seeds")
        .join(kind);
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            (p.display().to_string(), std::fs::read(&p).unwrap())
        })
        .collect();
    out.sort();
    assert!(!out.is_empty(), "{}", dir.display());
    out
}

#[test]
fn dxf_seeds_read() {
    let opts = Options {
        scale: Some(1.0),
        ..Options::default()
    };
    for (name, bytes) in seeds("dxf") {
        let r = read_dxf(&bytes, &opts).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            !r.wires.is_empty() && r.report.warnings.is_empty(),
            "{name}: {:?}",
            r.report
        );
        assert_eq!(r.report.dropped, 0, "{name}");
    }
}

#[test]
fn svg_seeds_read() {
    for (name, bytes) in seeds("svg") {
        let r = read_svg(&bytes, &Options::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            !r.wires.is_empty() && r.report.warnings.is_empty(),
            "{name}: {:?}",
            r.report
        );
        assert_eq!(r.report.dropped, 0, "{name}");
    }
}
