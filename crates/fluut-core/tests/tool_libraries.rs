// SPDX-License-Identifier: MIT OR Apache-2.0

//! A project that refers to tool libraries outside its `tools/` folder
//! (D06 §7.2.1): loading, precedence, live edits with undo, saving.

use fluut_core::Session;
use fluut_model::edit::Edit;
use std::path::PathBuf;

fn lib(id: &str, tool: &str, diameter: u32) -> String {
    format!(
        "format = \"fluut.tools/1\"\nid = \"{id}\"\n[[tool]]\nid = \"{tool}\"\nname = \"{tool}\"\n\
         number = 1\nshape = \"flat\"\ndiameter = {diameter}\nflute_length = 20\n"
    )
}

/// `<dir>/jobs/one/p.fluut` with `tools/own.toml`, and `<dir>/shared/*.toml`.
fn workspace(name: &str, refs: &str) -> (PathBuf, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    let job = dir.join("jobs/one");
    std::fs::create_dir_all(job.join("tools")).unwrap();
    std::fs::create_dir_all(dir.join("shared")).unwrap();
    std::fs::write(job.join("tools/own.toml"), lib("project/own", "a", 6)).unwrap();
    std::fs::write(
        dir.join("shared/fusion.toml"),
        lib("project/fusion", "b", 8),
    )
    .unwrap();
    std::fs::write(dir.join("shared/more.toml"), lib("project/more", "c", 10)).unwrap();
    let p = job.join("p.fluut");
    let key = if refs.is_empty() {
        String::new()
    } else {
        format!("tool_libraries = [{refs}]\n")
    };
    std::fs::write(
        &p,
        format!("format = \"fluut.project/1.1\"\nname = \"P\"\nunits = \"mm\"\n{key}"),
    )
    .unwrap();
    (dir, p)
}

fn has(s: &Session, tool: &str) -> bool {
    s.tools.get(tool).is_some()
}

fn codes(s: &Session) -> Vec<&str> {
    s.warnings.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn referenced_libraries_load_with_the_project() {
    let (_, p) = workspace(
        "plib-open",
        "\"../../shared/fusion.toml\", \"../../shared/more.toml\"",
    );
    let s = Session::open(&p, &[]).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    assert!(has(&s, "project/own#a"));
    assert!(has(&s, "project/fusion#b"));
    assert!(has(&s, "project/more#c"));
    assert!(!s.is_dirty());
}

#[test]
fn a_project_without_the_key_loads_as_before() {
    let (_, p) = workspace("plib-none", "");
    let s = Session::open(&p, &[]).unwrap();
    assert!(s.warnings.is_empty());
    assert!(has(&s, "project/own#a"));
    assert!(!has(&s, "project/fusion#b"));
    assert!(s.project.tool_libraries.is_empty());
}

#[test]
fn a_missing_file_warns_and_the_project_still_opens() {
    let (_, p) = workspace(
        "plib-missing",
        "\"../../shared/gone.toml\", \"../../shared/more.toml\"",
    );
    let s = Session::open(&p, &[]).unwrap();
    assert_eq!(codes(&s), ["project.tools.missing"]);
    assert!(has(&s, "project/more#c"));
    assert!(has(&s, "project/own#a"));
    // The reference is kept, so the file can be put back.
    assert_eq!(s.project.tool_libraries.len(), 2);
}

#[test]
fn tools_folder_then_references_then_command_line_and_later_wins() {
    let (dir, p) = workspace("plib-order", "\"../../shared/fusion.toml\"");
    // The same library id and version in tools/, in the reference and on the command line.
    std::fs::write(
        dir.join("jobs/one/tools/dup.toml"),
        lib("project/fusion", "from-tools", 1),
    )
    .unwrap();
    let extra = dir.join("extra.toml");
    std::fs::write(&extra, lib("project/fusion", "from-cli", 3)).unwrap();
    let s = Session::open(&p, &[]).unwrap();
    // The reference replaces tools/ (and says so).
    assert_eq!(codes(&s), ["project.tools.replaces"]);
    assert!(has(&s, "project/fusion#b"));
    assert!(!has(&s, "project/fusion#from-tools"));
    // `--tools` adds on top of both.
    let s = Session::open(&p, std::slice::from_ref(&extra)).unwrap();
    assert!(has(&s, "project/fusion#from-cli"));
    assert!(!has(&s, "project/fusion#b"));
}

#[test]
fn an_edit_loads_and_undo_unloads_a_library_and_it_is_one_step() {
    let (_, p) = workspace("plib-edit", "");
    let mut s = Session::open(&p, &[]).unwrap();
    assert!(!has(&s, "project/fusion#b"));
    s.apply(Edit::SetToolLibraries(vec![
        "../../shared/fusion.toml".into(),
    ]))
    .unwrap();
    assert!(has(&s, "project/fusion#b"));
    assert!(has(&s, "project/own#a"));
    assert!(s.is_dirty());
    assert_eq!(s.undo_label().as_deref(), Some("Change tool libraries"));
    s.undo().unwrap();
    assert!(!has(&s, "project/fusion#b"));
    assert!(has(&s, "project/own#a"));
    assert!(!s.is_dirty());
    s.redo().unwrap();
    assert!(has(&s, "project/fusion#b"));
}

#[test]
fn load_findings_follow_the_list() {
    let (_, p) = workspace("plib-findings", "");
    let mut s = Session::open(&p, &[]).unwrap();
    s.apply(Edit::SetToolLibraries(vec!["nothing.toml".into()]))
        .unwrap();
    assert_eq!(codes(&s), ["project.tools.missing"]);
    s.apply(Edit::SetToolLibraries(vec![])).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    s.undo().unwrap();
    assert_eq!(codes(&s), ["project.tools.missing"]);
}

#[test]
fn the_command_line_files_stay_on_top_after_an_edit() {
    let (dir, p) = workspace("plib-cli-edit", "");
    let extra = dir.join("extra.toml");
    std::fs::write(&extra, lib("project/fusion", "from-cli", 3)).unwrap();
    let mut s = Session::open(&p, std::slice::from_ref(&extra)).unwrap();
    s.apply(Edit::SetToolLibraries(vec![
        "../../shared/fusion.toml".into(),
    ]))
    .unwrap();
    assert!(has(&s, "project/fusion#from-cli"));
    assert!(!has(&s, "project/fusion#b"));
}

#[test]
fn saving_writes_the_key_and_a_reopen_keeps_it() {
    let (_, p) = workspace("plib-save", "");
    let mut s = Session::open(&p, &[]).unwrap();
    s.apply(Edit::SetToolLibraries(vec![
        "../../shared/fusion.toml".into(),
    ]))
    .unwrap();
    s.save().unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(
        text.starts_with("format         = \"fluut.project/1.1\""),
        "{text}"
    );
    assert!(text.contains("tool_libraries = [\"../../shared/fusion.toml\"]"));
    let s = Session::open(&p, &[]).unwrap();
    assert!(has(&s, "project/fusion#b"));
    // Removing the last reference writes the plain format line again.
    let mut s = s;
    s.apply(Edit::SetToolLibraries(vec![])).unwrap();
    s.save().unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.starts_with("format = \"fluut.project/1\"\n"), "{text}");
    assert!(!text.contains("tool_libraries"));
}

#[test]
fn save_as_in_another_folder_keeps_pointing_at_the_same_files() {
    let (dir, p) = workspace(
        "plib-saveas",
        "\"../../shared/fusion.toml\", \"../../shared/more.toml\"",
    );
    let mut s = Session::open(&p, &[]).unwrap();
    let abs = std::path::absolute(dir.join("shared/more.toml")).unwrap();
    s.apply(Edit::SetToolLibraries(vec![
        "../../shared/fusion.toml".into(),
        abs.to_string_lossy().replace('\\', "/"),
    ]))
    .unwrap();
    let other = dir.join("deeper/jobs/two/q.fluut");
    s.save_as(&other).unwrap();
    // The relative one is rebased; the absolute one is as it was.
    assert_eq!(s.project.tool_libraries[0], "../../../shared/fusion.toml");
    assert_eq!(
        s.project.tool_libraries[1],
        abs.to_string_lossy().replace('\\', "/")
    );
    assert!(!s.is_dirty());
    let t = Session::open(&other, &[]).unwrap();
    assert!(has(&t, "project/fusion#b"));
    assert!(has(&t, "project/more#c"));
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
}

#[test]
fn a_url_in_the_file_is_never_read_and_is_kept() {
    let (_, p) = workspace(
        "plib-url",
        "\"https://example.com/x.toml\", \"../../shared/more.toml\"",
    );
    let s = Session::open(&p, &[]).unwrap();
    assert_eq!(codes(&s), ["project.tools.rejected"]);
    assert!(has(&s, "project/more#c"));
    assert_eq!(s.project.tool_libraries.len(), 2);
}
