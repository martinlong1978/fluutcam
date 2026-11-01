// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared helpers of the script tests.

#![allow(dead_code)]

use fluut_core::Session;
use fluut_script::{Console, ErrorKind, Limits, Permissions, RunOptions, RunOutcome, run};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// A fresh folder under the target's temp dir.
pub fn tmp(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Small limits that hostile scripts reach quickly.
pub fn small() -> Limits {
    Limits {
        operations: 200_000,
        call_levels: 32,
        expr_depth: 32,
        fn_expr_depth: 16,
        string_len: 10_000,
        array_len: 10_000,
        map_len: 1_000,
        variables: 200,
        modules: 4,
        memory_cells: 50_000,
        script_time: Duration::from_secs(5),
    }
}

pub fn opts(perm: Permissions, limits: Limits) -> RunOptions {
    RunOptions {
        name: "test.rhai".into(),
        limits,
        permissions: perm,
        date: "2026-11-01".into(),
        time: "12:00:00".into(),
        ..RunOptions::default()
    }
}

/// Runs without a project; the console and the outcome.
pub fn bare(src: &str, limits: Limits) -> (RunOutcome, Arc<Console>) {
    let c = Arc::new(Console::new());
    let o = run(src, None, opts(Permissions::default(), limits), c.clone());
    (o, c)
}

pub fn kind(o: &RunOutcome) -> Option<ErrorKind> {
    o.result.as_ref().err().map(|e| e.kind)
}

/// A reference project copied into a fresh folder (so tests may write).
pub fn reference(name: &str, test: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fluut-cli/tests/projects")
        .join(name);
    let dst = tmp(test).join(name);
    copy_dir(&src, &dst);
    dst
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

/// Opens a copied reference project; the session, its folder and the
/// permissions of a run on it.
pub fn open(name: &str, test: &str) -> (Session, PathBuf, Permissions) {
    let dir = reference(name, test);
    let s = Session::open(&dir.join(format!("{name}.fluut")), &[]).unwrap();
    assert!(s.warnings.iter().all(|w| !w.is_error()), "{:?}", s.warnings);
    let perm = Permissions {
        project_dir: Some(dir.clone()),
        ..Permissions::default()
    };
    (s, dir, perm)
}

/// Runs `src` on a session with default limits.
pub fn on(src: &str, s: Session, perm: Permissions) -> (RunOutcome, Arc<Console>) {
    let c = Arc::new(Console::new());
    let o = run(src, Some(s), opts(perm, Limits::default()), c.clone());
    (o, c)
}

/// Panics with the error and console when the run failed.
pub fn ok(o: &RunOutcome, c: &Console) {
    if let Err(e) = &o.result {
        panic!("{e:?}\nconsole:\n{}", c.text());
    }
}
