// SPDX-License-Identifier: MIT OR Apache-2.0

//! Posts, machines and templates beside the project load again on request
//! (`Session::reload_definitions`), so a post written while the project is
//! open is found without opening the project again.

use fluut_core::Session;
use fluut_model::DefRef;
use fluut_model::library::DefKind;
use std::path::PathBuf;

fn post(id: &str, name: &str) -> String {
    format!(
        "format = \"fluut.post/1\"\nid = \"{id}\"\nversion = \"1.0\"\nname = \"{name}\"\n\
         extends = \"fluut/gcode@1\"\n"
    )
}

fn workspace(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("posts")).unwrap();
    std::fs::write(
        dir.join("p.fluut"),
        "format = \"fluut.project/1.1\"\nname = \"P\"\nunits = \"mm\"\n",
    )
    .unwrap();
    dir
}

fn has(s: &Session, id: &str) -> bool {
    s.library
        .find(DefKind::Post, &DefRef::parse(id).unwrap())
        .is_some()
}

#[test]
fn a_post_added_while_the_project_is_open_is_found_after_a_reload() {
    let dir = workspace("definitions-add");
    let mut s = Session::open(&dir.join("p.fluut"), &[]).unwrap();
    assert!(!has(&s, "project/new"));
    std::fs::write(dir.join("posts/new.post.toml"), post("project/new", "New")).unwrap();
    assert!(!has(&s, "project/new"));
    s.reload_definitions();
    assert!(has(&s, "project/new"));
    // The built-in posts stay.
    assert!(has(&s, "fluut/gcode"));
    // A second reload does not add it twice.
    s.reload_definitions();
    let n = s
        .library
        .entries()
        .iter()
        .filter(|e| e.kind == DefKind::Post && e.def.id == "project/new")
        .count();
    assert_eq!(n, 1);
}

#[test]
fn a_changed_or_removed_post_and_its_findings_follow_the_files() {
    let dir = workspace("definitions-change");
    std::fs::write(dir.join("posts/a.post.toml"), post("project/a", "One")).unwrap();
    std::fs::write(dir.join("posts/bad.post.toml"), "not toml [").unwrap();
    let mut s = Session::open(&dir.join("p.fluut"), &[]).unwrap();
    assert!(has(&s, "project/a"));
    assert!(s.warnings.iter().any(|d| d.code == "def.load"));
    std::fs::write(dir.join("posts/a.post.toml"), post("project/a", "Two")).unwrap();
    std::fs::remove_file(dir.join("posts/bad.post.toml")).unwrap();
    s.reload_definitions();
    let name = s
        .library
        .find(DefKind::Post, &DefRef::parse("project/a").unwrap())
        .unwrap()
        .def
        .table
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    assert_eq!(name.as_deref(), Some("Two"));
    assert!(s.warnings.iter().all(|d| d.code != "def.load"));
    std::fs::remove_file(dir.join("posts/a.post.toml")).unwrap();
    s.reload_definitions();
    assert!(!has(&s, "project/a"));
}
