// SPDX-License-Identifier: MIT OR Apache-2.0

//! Any text as a script, run on a small project with small limits and no
//! file access (spec 06 §6.3, D12 §6). A run may fail; it must not panic,
//! hang or run out of memory.

#![no_main]

use fluut_core::{ModelStore, Session, builtin_library};
use fluut_model::project::read;
use fluut_script::{Console, Limits, Permissions, RunOptions, run};
use libfuzzer_sys::fuzz_target;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const PROJECT: &str = include_str!("../../crates/fluut-cli/tests/projects/block/block.fluut");

fuzz_target!(|data: &[u8]| {
    let src = String::from_utf8_lossy(data);
    let project = read(PROJECT).expect("block.fluut reads").project;
    let session = Session::new(project, PathBuf::new(), ModelStore::new(), builtin_library());
    let opts = RunOptions {
        name: "fuzz.rhai".into(),
        limits: Limits {
            operations: 100_000,
            call_levels: 32,
            expr_depth: 32,
            fn_expr_depth: 16,
            string_len: 10_000,
            array_len: 10_000,
            map_len: 1_000,
            variables: 200,
            modules: 4,
            memory_cells: 100_000,
            script_time: Duration::from_secs(2),
        },
        permissions: Permissions::default(),
        ..RunOptions::default()
    };
    let _ = run(&src, Some(session), opts, Arc::new(Console::new()));
});
