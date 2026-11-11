// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tool library references of a project (D06 §7.2.1): the file key, the
//! edit, and loading the referenced files.

use fluut_model::document::Document;
use fluut_model::edit::Edit;
use fluut_model::library::{LayerKind, Library, check_tool_ref, resolve_tool_ref, tool_ref_for};
use fluut_model::project::{Project, read, write};
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn lib(id: &str, version: &str, tool: &str) -> String {
    format!(
        "format = \"fluut.tools/1\"\nid = \"{id}\"\nversion = \"{version}\"\n\
         [[tool]]\nid = \"{tool}\"\nname = \"{tool}\"\nnumber = 1\nshape = \"flat\"\n\
         diameter = 6\nflute_length = 20\n"
    )
}

const BASE: &str = "format = \"fluut.project/1\"\nname   = \"P\"\nunits  = \"mm\"\n";

#[test]
fn a_project_without_references_is_written_as_before() {
    let l = read(BASE).unwrap();
    assert!(l.project.tool_libraries.is_empty());
    assert!(l.warnings.is_empty());
    assert_eq!(write(&l.project), BASE);
}

#[test]
fn references_round_trip_byte_for_byte_as_minor_one() {
    let text = "format         = \"fluut.project/1.1\"\n\
                name           = \"P\"\n\
                units          = \"mm\"\n\
                tool_libraries = [\"../shared/fusion.toml\", \"C:/shop/tools.toml\"]\n";
    let l = read(text).unwrap();
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    assert_eq!(
        l.project.tool_libraries,
        ["../shared/fusion.toml", "C:/shop/tools.toml"]
    );
    assert_eq!(write(&l.project), text);
}

#[test]
fn the_key_is_read_from_a_file_that_says_minor_zero() {
    let l = read(&format!("{BASE}tool_libraries = [\"a.toml\"]\n")).unwrap();
    assert_eq!(l.project.tool_libraries, ["a.toml"]);
    assert!(l.warnings.is_empty());
    // And is written as minor one.
    assert!(write(&l.project).starts_with("format         = \"fluut.project/1.1\""));
}

#[test]
fn a_newer_minor_still_warns() {
    let l = read("format = \"fluut.project/1.2\"\nname = \"P\"\n").unwrap();
    assert_eq!(l.warnings.len(), 1);
}

#[test]
fn bad_values_are_load_errors() {
    for (v, msg) in [
        ("\"a.toml\"", "expected an array of strings"),
        ("[1]", "expected an array of strings"),
        ("[\"\"]", "is empty"),
        ("[\"a.toml\", \"  \"]", "is empty"),
    ] {
        let e = read(&format!("{BASE}tool_libraries = {v}\n")).unwrap_err();
        assert!(e.0.contains(msg), "{v}: {e}");
        assert!(e.0.contains("tool_libraries"), "{v}: {e}");
    }
}

#[test]
fn references_are_paths_only() {
    for ok in [
        "a.toml",
        "../shared/a.toml",
        "C:\\shop\\a.toml",
        "C:/shop/a.toml",
        "/home/me/a.toml",
        "my libs/a b.toml",
    ] {
        assert_eq!(check_tool_ref(ok), Ok(()), "{ok}");
    }
    for bad in [
        "",
        "  ",
        "https://example.com/a.toml",
        "http://x/a.toml",
        "file:///etc/a.toml",
        "file:a.toml",
        "ftp://x/a.toml",
        "\\\\server\\share\\a.toml",
        "//server/share/a.toml",
        "a\0b.toml",
        "a\nb.toml",
    ] {
        assert!(check_tool_ref(bad).is_err(), "{bad:?}");
    }
    assert!(check_tool_ref(&"a".repeat(2000)).is_err());
}

#[test]
fn references_are_made_relative_when_a_folder_is_shared() {
    let root = dir("toolrefs-rel");
    let proj = root.join("jobs/one");
    let shared = root.join("shared/fusion.toml");
    assert_eq!(tool_ref_for(&proj, &shared), "../../shared/fusion.toml");
    assert_eq!(
        tool_ref_for(&proj, &proj.join("tools/own.toml")),
        "tools/own.toml"
    );
    assert_eq!(
        tool_ref_for(&proj, &proj.join("../one/./x/../a.toml")),
        "a.toml"
    );
    // The round trip names the same file.
    for f in [&shared, &proj.join("tools/own.toml")] {
        let r = tool_ref_for(&proj, f);
        let back = resolve_tool_ref(&proj, &r);
        assert!(back.starts_with(&proj) || back.is_absolute(), "{back:?}");
        assert!(!r.contains('\\'), "{r}");
    }
}

#[test]
#[cfg(windows)]
fn a_file_on_another_drive_is_written_absolute() {
    use std::path::Path;
    let r = tool_ref_for(Path::new("C:\\jobs\\one"), Path::new("D:\\shared\\a.toml"));
    assert_eq!(r, "D:/shared/a.toml");
    let r = tool_ref_for(
        Path::new("C:\\jobs\\one"),
        Path::new("c:\\JOBS\\two\\a.toml"),
    );
    assert_eq!(r, "../two/a.toml");
}

fn codes(d: &[fluut_model::Diagnostic]) -> Vec<&str> {
    d.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn references_load_after_tools_folder_in_order_and_later_wins() {
    let d = dir("toolrefs-order");
    std::fs::create_dir_all(d.join("tools")).unwrap();
    std::fs::create_dir_all(d.join("shared")).unwrap();
    std::fs::write(d.join("tools/own.toml"), lib("project/own", "1.0", "a")).unwrap();
    std::fs::write(
        d.join("shared/one.toml"),
        lib("project/dup", "1.0", "from-one"),
    )
    .unwrap();
    std::fs::write(
        d.join("shared/two.toml"),
        lib("project/dup", "1.0", "from-two"),
    )
    .unwrap();
    let mut l = Library::new();
    let mut w = l.load_dir(&d, LayerKind::Project);
    w.extend(l.load_tool_references(
        &d,
        &["shared/one.toml".into(), "shared/two.toml".into()],
        LayerKind::Project,
    ));
    // The second file replaces the first with the same id and version, and says so.
    assert_eq!(codes(&w), ["project.tools.replaces"], "{w:?}");
    let tools = l.tools();
    assert!(tools.get("project/own#a").is_some());
    assert!(tools.get("project/dup#from-two").is_some());
    assert!(tools.get("project/dup#from-one").is_none());
}

#[test]
fn a_missing_file_is_a_warning_and_the_rest_still_load() {
    let d = dir("toolrefs-missing");
    std::fs::write(d.join("ok.toml"), lib("project/ok", "1.0", "a")).unwrap();
    let mut l = Library::new();
    let w = l.load_tool_references(
        &d,
        &["gone.toml".into(), "ok.toml".into(), "sub".into()],
        LayerKind::Project,
    );
    assert_eq!(
        codes(&w),
        ["project.tools.missing", "project.tools.missing"]
    );
    assert!(w[0].message.contains("gone.toml"));
    assert_eq!(w[0].severity, fluut_model::Severity::Warning);
    assert!(l.tools().get("project/ok#a").is_some());
}

#[test]
fn urls_network_paths_and_repeats_are_not_loaded() {
    let d = dir("toolrefs-rejected");
    std::fs::write(d.join("ok.toml"), lib("project/ok", "1.0", "a")).unwrap();
    let mut l = Library::new();
    let w = l.load_tool_references(
        &d,
        &[
            "https://example.com/x.toml".into(),
            "\\\\server\\share\\x.toml".into(),
            "ok.toml".into(),
            "./ok.toml".into(),
            "sub/../ok.toml".into(),
        ],
        LayerKind::Project,
    );
    assert_eq!(
        codes(&w),
        [
            "project.tools.rejected",
            "project.tools.rejected",
            "project.tools.duplicate",
            "project.tools.duplicate"
        ]
    );
    assert!(l.tools().get("project/ok#a").is_some());
}

#[test]
fn a_bad_library_file_is_reported_and_left_out() {
    let d = dir("toolrefs-bad");
    std::fs::write(d.join("bad.toml"), "this is = not [valid").unwrap();
    let mut l = Library::new();
    let w = l.load_tool_references(&d, &["bad.toml".into()], LayerKind::Project);
    assert_eq!(codes(&w), ["model.tools.load"]);
}

#[test]
fn remove_tool_libs_drops_only_that_layer() {
    let d = dir("toolrefs-remove");
    std::fs::write(d.join("a.toml"), lib("project/a", "1.0", "a")).unwrap();
    let mut l = Library::new();
    l.load_tools_file(&d.join("a.toml"), LayerKind::User);
    l.load_tool_references(&d, &["a.toml".into()], LayerKind::Project);
    l.remove_tool_libs(LayerKind::Project);
    assert!(l.tools().get("project/a#a").is_some());
    l.remove_tool_libs(LayerKind::User);
    assert!(l.tools().get("project/a#a").is_none());
}

#[test]
fn the_edit_sets_the_list_is_one_undo_step_and_checks_it() {
    let mut doc = Document::new(Project::new("P"));
    doc.apply(Edit::SetToolLibraries(vec![
        "a.toml".into(),
        "b.toml".into(),
    ]))
    .unwrap();
    assert_eq!(doc.project().tool_libraries, ["a.toml", "b.toml"]);
    doc.apply(Edit::SetToolLibraries(vec!["a.toml".into()]))
        .unwrap();
    assert_eq!(doc.project().tool_libraries, ["a.toml"]);
    assert!(doc.undo().is_some());
    assert_eq!(doc.project().tool_libraries, ["a.toml", "b.toml"]);
    assert!(doc.undo().is_some());
    assert!(doc.project().tool_libraries.is_empty());
    assert!(doc.redo().is_some());
    assert_eq!(doc.project().tool_libraries, ["a.toml", "b.toml"]);
    assert_eq!(
        Edit::SetToolLibraries(vec![]).label(),
        "Change tool libraries"
    );
    for bad in [
        vec!["https://x/a.toml".to_owned()],
        vec!["a.toml".to_owned(), "a.toml".to_owned()],
        vec![String::new()],
    ] {
        let before = doc.project().clone();
        assert!(doc.apply(Edit::SetToolLibraries(bad)).is_err());
        assert_eq!(*doc.project(), before);
    }
}
