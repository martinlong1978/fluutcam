// SPDX-License-Identifier: MIT OR Apache-2.0

//! **Import post** in the window: two scripted dialogs, the worker, the
//! result window, and Use this post (design D10 §6.1).

use super::*;
use crate::vm::post_import::Phase;
use crate::vm::post_import::tests::{fixture, fixture_named, temp};

/// Steps until the import has left its dialogs and its worker.
fn settle_import(h: &mut Harness<'static, State>) {
    for _ in 0..2000 {
        h.step();
        let a = app(h);
        if !a.ui.post_import.is_running()
            && !a.ui.post_import.awaiting_target()
            && !a.dialogs.is_open()
        {
            h.run_steps(3);
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("import did not finish");
}

/// Clicks Import post… on the Library tab.
fn start(h: &mut Harness<'static, State>) {
    stage(h, "LIBRARY");
    h.get_by_label("Import post…").click();
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

/// Steps until the snapshot satisfies `done` (edits run on the worker).
fn until(h: &mut Harness<'static, State>, done: impl Fn(&crate::session::Snapshot) -> bool) {
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

fn has_post(s: &crate::session::Snapshot) -> bool {
    s.posts.iter().any(|p| p.id == "vectric/grbl-like")
}

#[test]
fn the_ribbon_asks_for_the_file_then_for_the_post_and_shows_the_result() {
    let d = temp("ui-ok");
    let out = d.join("shop.post.toml");
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::ImportPostSource, fixture());
    scripted(&mut h).answer(DialogKind::ImportPostTarget, out.clone());
    start(&mut h);
    assert_eq!(
        kinds(&mut h),
        [DialogKind::ImportPostSource, DialogKind::ImportPostTarget]
    );
    let second = scripted(&mut h).asked()[1].clone();
    assert_eq!(second.name, "grbl-like.post.toml");
    // The id, the counts and the warning count.
    h.get_by_label_contains("Wrote post vectric/grbl-like to ");
    h.get_by_label("Post id: vectric/grbl-like");
    h.get_by_label_contains("Post name: FluutCAM test");
    h.get_by_label_contains("Settings mapped: ");
    h.get_by_label_contains("Blocks mapped: ");
    h.get_by_label_contains("Words written: ");
    h.get_by_label("Warnings: 0");
    h.get_by_label("No warnings.");
    // No project: the result says how to use the post.
    h.get_by_label("Open a project to use the post.");
    assert!(h.query_by_label("Use this post").is_none());
    assert!(out.is_file());
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(h.query_by_label("Post id: vectric/grbl-like").is_none());
    assert!(app(&mut h).ui.post_import.is_free());
    // The ribbon still has no path fields.
    assert!(
        h.query_all_by_role(egui::accesskit::Role::TextInput)
            .next()
            .is_none()
    );
}

#[test]
fn a_file_with_warnings_lists_each_with_its_severity_and_code() {
    let d = temp("ui-warn");
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::ImportPostSource, fixture_named("comma-inch.pp"));
    scripted(&mut h).answer(DialogKind::ImportPostTarget, d.join("out.post.toml"));
    start(&mut h);
    h.get_by_label("Post id: vectric/comma-inch");
    h.get_by_label("Warnings: 1");
    h.get_by_label("Warnings");
    h.get_by_label("Warning");
    h.get_by_label("[import.pp.decimal_char]");
    h.get_by_label_contains("words use both");
    assert!(h.query_by_label("No warnings.").is_none());
}

#[test]
fn cancelling_the_file_dialog_does_nothing() {
    let mut h = harness();
    h.step();
    scripted(&mut h);
    start(&mut h);
    assert_eq!(kinds(&mut h), [DialogKind::ImportPostSource]);
    assert!(matches!(app(&mut h).ui.post_import.phase, Phase::Idle));
    assert!(h.query_by_label("Nothing was imported.").is_none());
}

#[test]
fn cancelling_the_save_dialog_writes_nothing() {
    let d = temp("ui-cancel");
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::ImportPostSource, fixture());
    start(&mut h);
    assert_eq!(
        kinds(&mut h),
        [DialogKind::ImportPostSource, DialogKind::ImportPostTarget]
    );
    assert!(matches!(app(&mut h).ui.post_import.phase, Phase::Idle));
    assert!(h.query_by_label("Nothing was imported.").is_none());
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
    // It can be started again.
    scripted(&mut h).answer(DialogKind::ImportPostSource, fixture());
    scripted(&mut h).answer(DialogKind::ImportPostTarget, d.join("again"));
    h.get_by_label("Import post…").click();
    settle_import(&mut h);
    h.get_by_label("Post id: vectric/grbl-like");
    // A name without an extension gets .post.toml.
    assert!(d.join("again.post.toml").is_file());
}

#[test]
fn a_cps_file_is_answered_with_the_cli_refusal() {
    let d = temp("ui-cps");
    let src = d.join("shop.cps");
    std::fs::write(&src, "function onOpen() {}").unwrap();
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::ImportPostSource, src);
    start(&mut h);
    // No Save dialog: nothing can be written.
    assert_eq!(kinds(&mut h), [DialogKind::ImportPostSource]);
    h.get_by_label("Nothing was imported.");
    h.get_by_label("[import.format.unsupported]");
    h.get_by_label_contains("Autodesk .cps posts are programs");
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(h.query_by_label("Nothing was imported.").is_none());
}

#[test]
fn a_bad_file_shows_its_errors_and_writes_nothing() {
    let d = temp("ui-bad");
    let src = d.join("broken.pp");
    std::fs::write(&src, "just some text\n").unwrap();
    let out = d.join("broken.post.toml");
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::ImportPostSource, src);
    scripted(&mut h).answer(DialogKind::ImportPostTarget, out.clone());
    start(&mut h);
    h.get_by_label("Nothing was imported.");
    h.get_by_label("Error");
    h.get_by_label("[import.file.invalid]");
    assert!(h.query_by_label_contains("Post id:").is_none());
    assert!(!out.exists());
}

#[test]
fn an_existing_file_is_replaced_only_when_the_save_dialog_confirmed() {
    let d = temp("ui-exists");
    let out = d.join("p.post.toml");
    std::fs::write(&out, "keep").unwrap();
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::ImportPostSource, fixture());
    scripted(&mut h).answer(DialogKind::ImportPostTarget, out.clone());
    start(&mut h);
    h.get_by_label("[import.output.exists]");
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "keep");
    h.get_by_label("Close").click();
    h.run_steps(3);
    // The dialog's own question was answered yes.
    scripted(&mut h).script_confirms_overwrite(true);
    scripted(&mut h).answer(DialogKind::ImportPostSource, fixture());
    scripted(&mut h).answer(DialogKind::ImportPostTarget, out.clone());
    h.get_by_label("Import post…").click();
    settle_import(&mut h);
    h.get_by_label("Post id: vectric/grbl-like");
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("fluut.post/1")
    );
}

#[test]
fn the_source_is_never_replaced() {
    let d = temp("ui-same");
    let src = d.join("grbl-like.pp");
    std::fs::copy(fixture(), &src).unwrap();
    let mut h = harness();
    h.step();
    scripted(&mut h).script_confirms_overwrite(true);
    scripted(&mut h).answer(DialogKind::ImportPostSource, src.clone());
    scripted(&mut h).answer(DialogKind::ImportPostTarget, src.clone());
    start(&mut h);
    h.get_by_label("[import.output.is_input]");
    assert_eq!(
        std::fs::read(&src).unwrap(),
        std::fs::read(fixture()).unwrap()
    );
}

#[test]
fn a_post_in_the_project_posts_folder_is_found_and_chosen_without_restarting() {
    let d = temp("ui-project");
    copy_dir(project("block").parent().unwrap(), &d);
    std::fs::create_dir_all(d.join("posts")).unwrap();
    let proj = d.join("block.fluut");
    let mut h = harness();
    h.step();
    open(&mut h, proj);
    app(&mut h).ui.clock = fixed_clock;
    assert!(!has_post(&snapshot(&mut h)));
    scripted(&mut h).answer(DialogKind::ImportPostSource, fixture());
    scripted(&mut h).answer(
        DialogKind::ImportPostTarget,
        d.join("posts/grbl-like.post.toml"),
    );
    start(&mut h);
    // The Save dialog started in the project's posts folder.
    let second = scripted(&mut h).asked().last().cloned().unwrap();
    assert_eq!(second.kind, DialogKind::ImportPostTarget);
    assert_eq!(second.dir, Some(d.join("posts")));
    h.get_by_label("Post id: vectric/grbl-like");
    h.get_by_label_contains("so the project finds it whenever it is opened");
    assert_eq!(app(&mut h).ui.post_id, "fluut/linuxcnc");
    h.get_by_label("Use this post").click();
    until(&mut h, has_post);
    h.get_by_label("Post now uses this post.");
    assert_eq!(app(&mut h).ui.post_id, "vectric/grbl-like");
    // Nothing was added to the project.
    assert!(!snapshot(&mut h).dirty);
    // The Post stage offers it and posts with it, in the file type it names.
    h.get_by_label("Close").click();
    h.run_steps(3);
    stage(&mut h, "POST");
    h.query_all_by_role(egui::accesskit::Role::ComboBox)
        .find(|n| n.value().as_deref() == Some("vectric/grbl-like"))
        .unwrap()
        .click();
    h.run_steps(2);
    h.get_by_label("fluut/gcode");
    h.get_by_label("fluut/grbl");
    h.get_by_label("vectric/grbl-like").click();
    h.run_steps(2);
    let out = d.join("chosen");
    scripted(&mut h).answer(DialogKind::PostOutput, out);
    h.get_by_label("Post").click();
    settle(&mut h);
    let asked = scripted(&mut h).asked().last().cloned().unwrap();
    assert_eq!(asked.kind, DialogKind::PostOutput);
    assert_eq!(asked.name, "block.gcode");
    let text = std::fs::read_to_string(d.join("chosen.gcode")).unwrap();
    assert!(text.contains("G21"), "{text}");
    assert!(
        app(&mut h)
            .jobs
            .last
            .as_ref()
            .is_some_and(|(_, o)| matches!(o, crate::jobs::Outcome::Posted { .. }))
    );
}

#[test]
fn a_post_elsewhere_cannot_be_chosen_and_the_window_says_why() {
    let d = temp("ui-elsewhere");
    copy_dir(project("block").parent().unwrap(), &d.join("jobs/one"));
    let proj = d.join("jobs/one/block.fluut");
    let mut h = harness();
    h.step();
    open(&mut h, proj);
    std::fs::create_dir_all(d.join("shared")).unwrap();
    scripted(&mut h).answer(DialogKind::ImportPostSource, fixture());
    scripted(&mut h).answer(DialogKind::ImportPostTarget, d.join("shared/p.post.toml"));
    start(&mut h);
    h.get_by_label("Post id: vectric/grbl-like");
    h.get_by_label_contains("The post is outside this project's posts folder");
    assert!(h.query_by_label("Use this post").is_none());
    assert!(!has_post(&snapshot(&mut h)));
    assert_eq!(app(&mut h).ui.post_id, "fluut/linuxcnc");
    // Without a posts folder the dialog starts in the project's folder.
    let second = scripted(&mut h).asked().last().cloned().unwrap();
    assert_eq!(second.dir, Some(d.join("jobs/one")));
    // A post in the project folder itself, not in posts/, is not found either.
    h.get_by_label("Close").click();
    h.run_steps(3);
    scripted(&mut h).answer(DialogKind::ImportPostSource, fixture());
    scripted(&mut h).answer(
        DialogKind::ImportPostTarget,
        d.join("jobs/one/loose.post.toml"),
    );
    h.get_by_label("Import post…").click();
    settle_import(&mut h);
    h.get_by_label_contains("The post is outside this project's posts folder");
}

#[test]
fn use_this_post_is_not_offered_without_a_project() {
    let d = temp("ui-noproject");
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::ImportPostSource, fixture());
    scripted(&mut h).answer(DialogKind::ImportPostTarget, d.join("p.post.toml"));
    start(&mut h);
    h.get_by_label("Post id: vectric/grbl-like");
    h.get_by_label("Open a project to use the post.");
    assert!(h.query_by_label("Use this post").is_none());
}

#[test]
fn the_command_waits_while_a_job_runs() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    stage(&mut h, "LIBRARY");
    assert!(enabled(&h, "Import post…"));
    app(&mut h).jobs.calculate(crate::jobs::CalcTargets::All);
    h.run_steps(1);
    assert!(!enabled(&h, "Import post…"));
    settle(&mut h);
}
