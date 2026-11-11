// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tool libraries outside the project's `tools/` folder, in the window:
//! the Library ribbon command, the list in the browser, and undo (design
//! D06 §7.2.1).

use super::*;
use crate::session::Snapshot;
use crate::vm::tool_import::tests::temp;
use std::path::Path;

const LIB: &str = "format = \"fluut.tools/1\"\nid = \"project/shared\"\n\
    [[tool]]\nid = \"flat-3\"\nname = \"Shared 3 mm flat\"\nnumber = 1\nshape = \"flat\"\n\
    diameter = 3\nflute_length = 12\n";

/// `<d>/jobs/one/block.fluut` (a copy of the reference project) and
/// `<d>/shared/shared.toml`.
fn workspace(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let d = temp(name);
    let job = d.join("jobs/one");
    std::fs::create_dir_all(&job).unwrap();
    let from = project("block");
    for e in std::fs::read_dir(from.parent().unwrap()).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_file() {
            std::fs::copy(e.path(), job.join(e.file_name())).unwrap();
        }
    }
    std::fs::create_dir_all(d.join("shared")).unwrap();
    let lib = d.join("shared/shared.toml");
    std::fs::write(&lib, LIB).unwrap();
    (d, job.join("block.fluut"), lib)
}

fn until(h: &mut Harness<'static, State>, done: impl Fn(&Snapshot) -> bool) {
    for _ in 0..2000 {
        h.step();
        if app(h).jobs.snapshot.as_deref().is_some_and(&done) {
            h.run_steps(3);
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the snapshot did not change");
}

fn has_tool(s: &Snapshot) -> bool {
    s.tools.get("project/shared#flat-3").is_some()
}

fn snap(h: &mut Harness<'static, State>) -> std::sync::Arc<Snapshot> {
    app(h).jobs.snapshot.clone().unwrap()
}

fn add(h: &mut Harness<'static, State>, lib: &Path) {
    scripted(h).answer(DialogKind::AddToolLibrary, lib);
    stage(h, "LIBRARY");
    h.get_by_label("Add tool library…").click();
    h.run_steps(3);
}

#[test]
fn the_command_needs_an_open_project() {
    let mut h = harness();
    h.step();
    stage(&mut h, "LIBRARY");
    assert!(!enabled(&h, "Add tool library…"));
    stage(&mut h, "MODEL");
    open(&mut h, project("block"));
    stage(&mut h, "LIBRARY");
    assert!(enabled(&h, "Add tool library…"));
}

#[test]
fn a_library_is_picked_listed_loaded_and_undone() {
    let (_, proj, lib) = workspace("ui-tl-add");
    let mut h = harness();
    h.step();
    open(&mut h, proj.clone());
    assert!(!has_tool(&snap(&mut h)));
    h.get_by_label("No libraries listed.");
    add(&mut h, &lib);
    // The dialog started in the project's folder.
    let asked = scripted(&mut h).asked().last().cloned().unwrap();
    assert_eq!(asked.kind, DialogKind::AddToolLibrary);
    assert_eq!(asked.dir, Some(proj.parent().unwrap().to_path_buf()));
    until(&mut h, has_tool);
    let s = snap(&mut h);
    assert_eq!(s.project.tool_libraries, ["../../shared/shared.toml"]);
    assert!(s.dirty);
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    assert_eq!(s.undo_label.as_deref(), Some("Change tool libraries"));
    // The browser lists it.
    h.get_by_label("Tool libraries");
    h.get_by_label("shared.toml");
    assert!(h.query_by_label("No libraries listed.").is_none());
    // Saving writes the reference, and a reopened project still has it.
    stage(&mut h, "MODEL");
    h.get_by_label("Save").click();
    settle(&mut h);
    let text = std::fs::read_to_string(&proj).unwrap();
    assert!(text.contains("tool_libraries"), "{text}");
    assert!(text.starts_with("format         = \"fluut.project/1.1\""));
    stage(&mut h, "MODEL");
    open(&mut h, proj.clone());
    assert!(has_tool(&snap(&mut h)));
    assert!(!snap(&mut h).dirty);
    // Undo removes it again.
    add(&mut h, &lib);
    h.get_by_label("Undo").click();
    until(&mut h, |s| s.project.tool_libraries.len() == 1);
}

#[test]
fn undo_takes_a_library_back_out() {
    let (_, proj, lib) = workspace("ui-tl-undo");
    let mut h = harness();
    h.step();
    open(&mut h, proj);
    add(&mut h, &lib);
    until(&mut h, has_tool);
    stage(&mut h, "LIBRARY");
    h.get_by_label("Undo").click();
    until(&mut h, |s| !has_tool(s));
    let s = snap(&mut h);
    assert!(s.project.tool_libraries.is_empty());
    assert!(!s.dirty);
    h.get_by_label("Redo").click();
    until(&mut h, has_tool);
}

#[test]
fn remove_is_one_undo_step() {
    let (_, proj, lib) = workspace("ui-tl-remove");
    let mut h = harness();
    h.step();
    open(&mut h, proj);
    add(&mut h, &lib);
    until(&mut h, has_tool);
    h.get_by_label("Remove").click();
    until(&mut h, |s| !has_tool(s));
    assert!(snap(&mut h).project.tool_libraries.is_empty());
    assert_eq!(
        snap(&mut h).undo_label.as_deref(),
        Some("Change tool libraries")
    );
    h.get_by_label("No libraries listed.");
    stage(&mut h, "LIBRARY");
    h.get_by_label("Undo").click();
    until(&mut h, has_tool);
}

#[test]
fn a_missing_file_is_shown_in_the_list_and_the_project_still_works() {
    let (d, proj, lib) = workspace("ui-tl-missing");
    let mut h = harness();
    h.step();
    open(&mut h, proj);
    add(&mut h, &lib);
    until(&mut h, has_tool);
    std::fs::remove_file(&lib).unwrap();
    // Reading the libraries again finds it gone.
    app(&mut h)
        .jobs
        .edit(crate::jobs::Edit::ReloadToolLibraries);
    until(&mut h, |s| !has_tool(s));
    h.get_by_label_contains("The file was not found");
    let s = snap(&mut h);
    assert!(s.warnings.iter().any(|w| w.code == "project.tools.missing"));
    assert_eq!(s.project.tool_libraries.len(), 1);
    // The file comes back: reading again finds it.
    std::fs::write(d.join("shared/shared.toml"), LIB).unwrap();
    app(&mut h)
        .jobs
        .edit(crate::jobs::Edit::ReloadToolLibraries);
    until(&mut h, has_tool);
    assert!(
        h.query_by_label_contains("The file was not found")
            .is_none()
    );
}

#[test]
fn a_cancelled_dialog_and_a_repeated_library_change_nothing() {
    let (_, proj, lib) = workspace("ui-tl-cancel");
    let mut h = harness();
    h.step();
    open(&mut h, proj);
    // Cancelled: no answer is scripted.
    scripted(&mut h);
    stage(&mut h, "LIBRARY");
    h.get_by_label("Add tool library…").click();
    h.run_steps(3);
    assert!(snap(&mut h).project.tool_libraries.is_empty());
    assert!(!snap(&mut h).dirty);
    add(&mut h, &lib);
    until(&mut h, has_tool);
    // The same file again says so and adds nothing.
    add(&mut h, &lib);
    h.get_by_label("This project already lists that library.");
    assert_eq!(snap(&mut h).project.tool_libraries.len(), 1);
}

#[test]
fn no_path_text_box_anywhere_for_libraries() {
    let (_, proj, lib) = workspace("ui-tl-notext");
    let mut h = harness();
    h.step();
    open(&mut h, proj);
    add(&mut h, &lib);
    until(&mut h, has_tool);
    stage(&mut h, "LIBRARY");
    // Any text field there is belongs to something else (a parameter).
    for n in h.query_all_by_role(egui::accesskit::Role::TextInput) {
        let label = n
            .accesskit_node()
            .label()
            .unwrap_or_default()
            .to_lowercase();
        assert!(
            !label.contains("librar") && !label.contains("path") && !label.contains("file"),
            "{label}"
        );
    }
}
