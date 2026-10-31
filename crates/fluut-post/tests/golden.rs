// SPDX-License-Identifier: MIT OR Apache-2.0

//! Golden output of the built-in posts for the reference suite (spec 05
//! §5.11, D10 §9). Run with `FLUUT_BLESS=1` to rewrite the files after a
//! deliberate change, then review the diff by eye.

use fluut_model::{DefRef, MemStore};
use fluut_post::builtin::builtin_store;
use fluut_post::reference::{SuiteOptions, reference_job};
use fluut_post::{CompiledPost, Post, Provenance, post, resolve_post};
use std::path::PathBuf;

const ROTARY_LEAF: &str = "id = 'test/linuxcnc-rotary-a'\nversion = '1.0'\n\
    extends = 'fluut/linuxcnc@1'\nmixins = ['fluut/mixin/rotary-a@1']";

fn golden(name: &str, store: &MemStore, id: &str) {
    let r = resolve_post(store, &DefRef::parse(id).unwrap(), None).unwrap();
    let cp = CompiledPost::compile(Post::load(&r).unwrap()).unwrap();
    let job = reference_job(SuiteOptions::for_post(&cp), Provenance::of(&r, id));
    let out = post(&job, &cp).unwrap();
    assert!(out.warnings.is_empty(), "{name}: {:?}", out.warnings);
    assert_eq!(out.files.len(), 1);
    let text = &out.files[0];
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "golden",
        &format!("{name}.nc"),
    ]
    .iter()
    .collect();
    if std::env::var_os("FLUUT_BLESS").is_some() {
        std::fs::write(&path, text).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} missing: run with FLUUT_BLESS=1", path.display()));
    if *text != want {
        let line = text
            .lines()
            .zip(want.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        panic!(
            "{name}: output differs from {} at line {}:\n  got:  {:?}\n  want: {:?}",
            path.display(),
            line + 1,
            text.lines().nth(line),
            want.lines().nth(line)
        );
    }
}

#[test]
fn gcode() {
    golden("gcode", &builtin_store(), "fluut/gcode");
}

#[test]
fn grbl() {
    golden("grbl", &builtin_store(), "fluut/grbl");
}

#[test]
fn linuxcnc() {
    golden("linuxcnc", &builtin_store(), "fluut/linuxcnc");
}

#[test]
fn linuxcnc_rotary_a() {
    let mut s = builtin_store();
    s.insert_toml(ROTARY_LEAF).unwrap();
    golden("linuxcnc-rotary-a", &s, "test/linuxcnc-rotary-a");
}
