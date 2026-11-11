// SPDX-License-Identifier: MIT OR Apache-2.0

//! **Import tools** in the window: two scripted dialogs, the worker, and the
//! result window (design D16 §10.2).

use super::*;
use crate::vm::tool_import::Phase;
use crate::vm::tool_import::tests::{fixture, temp};

/// Steps until the import has left its dialogs and its worker.
fn settle_import(h: &mut Harness<'static, State>) {
    for _ in 0..2000 {
        h.step();
        let a = app(h);
        if !a.ui.tool_import.is_running()
            && !a.ui.tool_import.awaiting_target()
            && !a.dialogs.is_open()
        {
            h.run_steps(3);
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("import did not finish");
}

/// Clicks Import tools… on the Library tab.
fn start(h: &mut Harness<'static, State>) {
    stage(h, "LIBRARY");
    h.get_by_label("Import tools…").click();
    settle_import(h);
}

fn kinds(h: &mut Harness<'static, State>) -> Vec<DialogKind> {
    scripted(h).asked().iter().map(|a| a.kind).collect()
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), t).unwrap();
        }
    }
}

#[test]
fn the_ribbon_asks_for_the_file_then_for_the_library_and_shows_the_result() {
    let d = temp("ui-ok");
    let out = d.join("shop.toml");
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::ImportToolsSource, fixture());
    scripted(&mut h).answer(DialogKind::ImportToolsTarget, out.clone());
    start(&mut h);
    assert_eq!(
        kinds(&mut h),
        [DialogKind::ImportToolsSource, DialogKind::ImportToolsTarget]
    );
    let second = scripted(&mut h).asked()[1].clone();
    assert_eq!(second.name, "fusion-sample.toml");
    // Counts, library id and warnings with their codes.
    h.get_by_label_contains("Wrote 7 of 9 tools to ");
    h.get_by_label("Tools read: 9");
    h.get_by_label("Tools written: 7");
    h.get_by_label("Tools skipped: 2");
    h.get_by_label("Library id: fusion/fusion-sample");
    h.get_by_label_contains("Source keys not used");
    h.get_by_label("Warnings");
    h.get_by_label("[import.tool.unsupported_type]");
    // No project: the result says how to use the library.
    h.get_by_label_contains("Open a project to use the library");
    assert!(h.query_by_label("Reload project to use it").is_none());
    assert!(out.is_file());
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(h.query_by_label("Warnings").is_none());
    assert!(app(&mut h).ui.tool_import.is_free());
    // The ribbon still has no path fields.
    assert!(
        h.query_all_by_role(egui::accesskit::Role::TextInput)
            .next()
            .is_none()
    );
}

#[test]
fn cancelling_the_file_dialog_does_nothing() {
    let mut h = harness();
    h.step();
    scripted(&mut h);
    start(&mut h);
    assert_eq!(kinds(&mut h), [DialogKind::ImportToolsSource]);
    assert!(matches!(app(&mut h).ui.tool_import.phase, Phase::Idle));
    assert!(h.query_by_label("Nothing was imported.").is_none());
}

#[test]
fn cancelling_the_save_dialog_writes_nothing() {
    let d = temp("ui-cancel");
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::ImportToolsSource, fixture());
    start(&mut h);
    assert_eq!(
        kinds(&mut h),
        [DialogKind::ImportToolsSource, DialogKind::ImportToolsTarget]
    );
    assert!(matches!(app(&mut h).ui.tool_import.phase, Phase::Idle));
    assert!(h.query_by_label("Nothing was imported.").is_none());
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
    // It can be started again.
    scripted(&mut h).answer(DialogKind::ImportToolsSource, fixture());
    scripted(&mut h).answer(DialogKind::ImportToolsTarget, d.join("again.toml"));
    h.get_by_label("Import tools…").click();
    settle_import(&mut h);
    h.get_by_label("Tools written: 7");
    assert!(d.join("again.toml").is_file());
}

#[test]
fn a_vtdb_file_is_answered_with_the_unsupported_format_error() {
    let d = temp("ui-vtdb");
    let src = d.join("shop.vtdb");
    std::fs::write(&src, "SQLite format 3").unwrap();
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::ImportToolsSource, src);
    start(&mut h);
    // No Save dialog: nothing can be written.
    assert_eq!(kinds(&mut h), [DialogKind::ImportToolsSource]);
    h.get_by_label("Nothing was imported.");
    h.get_by_label("[import.format.unsupported]");
    h.get_by_label_contains("Vectric .vtdb tool databases cannot be imported yet");
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(h.query_by_label("Nothing was imported.").is_none());
}

#[test]
fn a_bad_json_file_shows_its_errors_and_writes_nothing() {
    let d = temp("ui-bad");
    let src = d.join("broken.json");
    std::fs::write(&src, "{ this is not json").unwrap();
    let out = d.join("broken.toml");
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::ImportToolsSource, src);
    scripted(&mut h).answer(DialogKind::ImportToolsTarget, out.clone());
    start(&mut h);
    h.get_by_label("Nothing was imported.");
    h.get_by_label("Error");
    assert!(h.query_by_label("Tools read: 9").is_none());
    assert!(!out.exists());
}

#[test]
fn an_existing_file_is_replaced_only_when_the_save_dialog_confirmed() {
    let d = temp("ui-exists");
    let out = d.join("lib.toml");
    std::fs::write(&out, "keep").unwrap();
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::ImportToolsSource, fixture());
    scripted(&mut h).answer(DialogKind::ImportToolsTarget, out.clone());
    start(&mut h);
    h.get_by_label("[import.output.exists]");
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "keep");
    h.get_by_label("Close").click();
    h.run_steps(3);
    // The dialog's own question was answered yes.
    scripted(&mut h).script_confirms_overwrite(true);
    scripted(&mut h).answer(DialogKind::ImportToolsSource, fixture());
    scripted(&mut h).answer(DialogKind::ImportToolsTarget, out.clone());
    h.get_by_label("Import tools…").click();
    settle_import(&mut h);
    h.get_by_label("Tools written: 7");
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("fluut.tools/1")
    );
}

#[test]
fn a_library_in_the_project_tools_folder_can_be_loaded_by_reloading_the_project() {
    let d = temp("ui-project");
    copy_dir(project("block").parent().unwrap(), &d);
    let proj = d.join("block.fluut");
    let mut h = harness();
    h.step();
    open(&mut h, proj.clone());
    let ids = |h: &mut Harness<'static, State>| -> Vec<String> {
        snapshot(h)
            .tools
            .libraries()
            .map(|l| l.id.clone())
            .collect()
    };
    assert!(!ids(&mut h).contains(&"fusion/fusion-sample".to_owned()));
    scripted(&mut h).answer(DialogKind::ImportToolsSource, fixture());
    scripted(&mut h).answer(DialogKind::ImportToolsTarget, d.join("tools/sample.toml"));
    start(&mut h);
    // The Save dialog started in the project's tools folder.
    let second = scripted(&mut h).asked().last().cloned().unwrap();
    assert_eq!(second.kind, DialogKind::ImportToolsTarget);
    assert_eq!(second.dir, Some(d.join("tools")));
    h.get_by_label("Tools written: 7");
    h.get_by_label_contains("so it loads when the project is opened");
    h.get_by_label("Reload project to use it").click();
    settle(&mut h);
    h.run_steps(3);
    assert!(ids(&mut h).contains(&"fusion/fusion-sample".to_owned()));
    assert!(h.query_by_label("Tools written: 7").is_none());
}

#[test]
fn a_library_elsewhere_says_a_project_cannot_refer_to_it_yet() {
    let d = temp("ui-elsewhere");
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    scripted(&mut h).answer(DialogKind::ImportToolsSource, fixture());
    scripted(&mut h).answer(DialogKind::ImportToolsTarget, d.join("lib.toml"));
    start(&mut h);
    h.get_by_label("Tools written: 7");
    h.get_by_label_contains("A project cannot refer to a library in another folder yet");
    assert!(h.query_by_label("Reload project to use it").is_none());
}

#[test]
fn the_command_waits_while_a_job_runs() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    stage(&mut h, "LIBRARY");
    assert!(enabled(&h, "Import tools…"));
    app(&mut h).jobs.calculate(crate::jobs::CalcTargets::All);
    h.run_steps(1);
    assert!(!enabled(&h, "Import tools…"));
    settle(&mut h);
}
