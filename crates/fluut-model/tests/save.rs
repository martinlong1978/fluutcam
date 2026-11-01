// SPDX-License-Identifier: MIT OR Apache-2.0

//! Atomic save and load (D06 §7.1–§7.2).

use fluut_model::document::Document;
use fluut_model::edit::Edit;
use fluut_model::persist::{backup_path, load, save};
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn every_reference_project_saves_byte_identically() {
    let d = dir("save-reference");
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../fluut-cli/tests/projects");
    let mut n = 0;
    for name in ["block", "boss", "plate", "ridge"] {
        let src = PathBuf::from(root).join(name).join(format!("{name}.fluut"));
        let original = std::fs::read(&src).unwrap();
        let (l, bytes) = load(&src).unwrap();
        assert_eq!(bytes, original);
        let out = d.join(format!("{name}.fluut"));
        let saved = save(&out, &l.project).unwrap();
        assert_eq!(saved.bytes, original, "{name}");
        assert_eq!(std::fs::read(&out).unwrap(), original, "{name}");
        n += 1;
    }
    assert_eq!(n, 4);
}

#[test]
fn save_keeps_the_previous_file_as_bak() {
    let d = dir("save-bak");
    let path = d.join("p.fluut");
    let p0 = fluut_model::project::Project::new("First");
    save(&path, &p0).unwrap();
    assert!(!backup_path(&path).exists());
    let first = std::fs::read(&path).unwrap();
    let mut doc = Document::new(p0);
    doc.apply(Edit::SetName("Second".into())).unwrap();
    save(&path, doc.project()).unwrap();
    assert_eq!(std::fs::read(backup_path(&path)).unwrap(), first);
    let (l, _) = load(&path).unwrap();
    assert_eq!(l.project.name, "Second");
    // A third save replaces the old backup.
    doc.apply(Edit::SetName("Third".into())).unwrap();
    let second = std::fs::read(&path).unwrap();
    save(&path, doc.project()).unwrap();
    assert_eq!(std::fs::read(backup_path(&path)).unwrap(), second);
    // No temporary file is left behind.
    let names: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
}

#[test]
fn load_errors_name_the_file() {
    let d = dir("save-errors");
    let e = load(&d.join("none.fluut")).unwrap_err();
    assert!(e.to_string().contains("none.fluut"), "{e}");
    std::fs::write(d.join("bad.fluut"), "format = \"fluut.project/9\"").unwrap();
    let e = load(&d.join("bad.fluut")).unwrap_err();
    assert!(e.to_string().contains("not supported"), "{e}");
}
