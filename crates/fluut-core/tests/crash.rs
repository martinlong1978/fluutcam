// SPDX-License-Identifier: MIT OR Apache-2.0

//! The panic hook writes a report file, except for panics that a region
//! retry catches. The hook is process-wide, so this file has a single test.

use fluut_core::crash;

#[test]
fn a_panic_leaves_a_report_unless_a_region_retry_catches_it() {
    let dir = std::env::temp_dir().join(format!("fluut-hook-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    crash::install_in("fluut-test", dir.clone());

    // A panic caught inside a region operation (and retried there) is not a
    // crash: no report.
    let quiet = fluut_cam::region::catch_quiet(|| panic!("caught by a region retry"));
    assert!(quiet.is_err());
    assert!(
        crash::pending(&dir).is_empty(),
        "{:?}",
        crash::pending(&dir)
    );

    let caught = std::panic::catch_unwind(|| {
        let v: Vec<u32> = Vec::new();
        let _ = v[3];
    });
    assert!(caught.is_err());

    let reports = crash::pending(&dir);
    assert_eq!(reports.len(), 1, "{reports:?}");
    let text = std::fs::read_to_string(&reports[0]).unwrap();
    assert!(text.contains("Application: fluut-test"), "{text}");
    assert!(text.contains("index out of bounds"), "{text}");
    assert!(text.contains("crash.rs:"), "location of the panic: {text}");
    assert!(text.contains("Backtrace:\n"), "{text}");
    std::fs::remove_dir_all(&dir).unwrap();
}
