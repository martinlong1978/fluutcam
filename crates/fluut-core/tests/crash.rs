// SPDX-License-Identifier: MIT OR Apache-2.0

//! The panic hook writes a report file. The hook is process-wide, so this
//! file has a single test.

use fluut_core::crash;

#[test]
fn a_panic_leaves_a_report_with_message_location_and_backtrace() {
    let dir = std::env::temp_dir().join(format!("fluut-hook-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    crash::install_in("fluut-test", dir.clone());

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
