// SPDX-License-Identifier: MIT OR Apache-2.0

//! Linked models through the window: the notice, Update and its report, the
//! link in the Model panel, a broken toolpath and Pick again, and opening a
//! `.craftx` file from the command line or from Import model.

use super::*;
use crate::session::tests::floor_gone;
use fluut_core::watch::Due;
use fluut_model::project::DisplayUnit;

/// `<tmp>/design/block.craftx` (revision 1) and a saved project
/// `<tmp>/cam/p.fluut` that links it, with toolpath `t1` "Pocket" on the
/// floor (`F2:floor`).
fn workspace(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fluut-app-link-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("design")).unwrap();
    std::fs::create_dir_all(d.join("cam")).unwrap();
    std::fs::write(d.join("design/block.craftx"), crate::craftx::rev1().bytes()).unwrap();
    let mut s = fluut_core::Session::new_project(&d.join("cam/p.fluut"), DisplayUnit::Mm).unwrap();
    s.import_model(&d.join("design/block.craftx")).unwrap();
    let setup = s
        .add_setup(fluut_model::project::SetupKind::ThreeAxis, None, "Top")
        .unwrap();
    s.add_toolpath(&setup, fluut_model::ToolpathTypeId::Pocket2d, "Pocket")
        .unwrap();
    let stem = s.project.geometry[0].bodies[0]
        .id
        .strip_prefix("b-")
        .unwrap()
        .to_owned();
    s.apply(fluut_model::edit::Edit::set_toolpath_field(
        "t1",
        "geometry.drive_faces",
        Some(&format!("[{{ face = \"f-{stem}.10\" }}]")),
    ))
    .unwrap();
    s.save().unwrap();
    d
}

/// Checks the linked files at once, as gaining focus does.
fn check(h: &mut Harness<'static, State>) {
    let files = app(h).jobs.snapshot.as_ref().unwrap().linked_files.clone();
    app(h).ui.links.check_now(&files, Due::Focus);
    h.run_steps(2);
}

#[test]
fn the_model_panel_shows_the_link_and_its_parameters() {
    let d = workspace("panel");
    let mut h = harness();
    h.step();
    open(&mut h, d.join("cam/p.fluut"));
    h.get_by_label("Linked model block.craftx");
    h.get_by_label("From SolidCraft 0.12.0");
    h.get_by_label("Revision 1");
    assert!(
        h.query_all_by_label_contains("Saved 20").next().is_some(),
        "the saved time shows"
    );
    h.get_by_label("Design parameters (read only)").click();
    h.run_steps(2);
    h.get_by_label("wall");
    // Nothing has changed: no notice.
    check(&mut h);
    assert!(h.query_by_label_contains("Linked model changed").is_none());
}

#[test]
fn a_changed_file_raises_a_notice_and_update_shows_every_outcome() {
    let d = workspace("notice");
    let mut h = harness();
    h.step();
    open(&mut h, d.join("cam/p.fluut"));
    std::fs::write(d.join("design/block.craftx"), floor_gone().bytes()).unwrap();
    check(&mut h);
    h.get_by_label("Linked model changed: block.craftx");
    h.get_by_label("The file has changed since the project took it.");
    // Nothing is recalculated or re-read on its own.
    assert_eq!(
        app(&mut h).jobs.snapshot.as_ref().unwrap().links[0].revision,
        "1"
    );
    // Later hides the notice until the file changes again.
    h.get_by_label("Later").click();
    h.run_steps(2);
    assert!(h.query_by_label_contains("Linked model changed").is_none());
    std::fs::write(
        d.join("design/block.craftx"),
        crate::craftx::split().bytes(),
    )
    .unwrap();
    check(&mut h);
    h.get_by_label("Linked model changed: block.craftx");
    std::fs::write(d.join("design/block.craftx"), floor_gone().bytes()).unwrap();
    check(&mut h);
    // Update: one report with the outcome, the toolpath and plain words.
    h.get_all_by_label("Update").next().unwrap().click();
    settle(&mut h);
    h.get_by_label("Model updated");
    h.get_by_label("block.craftx: revision 1 → 7");
    h.get_by_label("Missing");
    // The report row and the browser row.
    assert!(h.get_all_by_label("Pocket").count() >= 2);
    h.get_by_label(
        "The face F2:floor is not in the model any more. The toolpath is broken until you pick it again.",
    );
    h.get_by_label("1 picks; 1 toolpaths broken");
    assert!(h.query_by_label_contains("Linked model changed").is_none());
    // The toolpath list shows it broken.
    let snap = app(&mut h).jobs.snapshot.clone().unwrap();
    assert_eq!(snap.status("t1"), crate::session::ToolpathStatus::Broken);
    h.get_by_label("Broken: a picked face or edge is not in the model any more");
    // Show selects it; the inspector says what is missing.
    h.get_by_label("Show").click();
    h.run_steps(2);
    assert_eq!(app(&mut h).ui.selection, Selection::Toolpath("t1".into()));
    h.get_by_label("Close").click();
    h.run_steps(2);
    assert!(app(&mut h).ui.update_report.is_none());
    h.get_by_label("This toolpath is broken");
    // Pick again takes the lost pick out and picks the field on the model.
    h.get_by_label("Pick again").click();
    for _ in 0..400 {
        h.step();
        if app(&mut h)
            .jobs
            .snapshot
            .as_ref()
            .is_some_and(|s| s.broken.is_empty())
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let pick = app(&mut h).ui.pick.clone().unwrap();
    assert_eq!(
        (pick.toolpath.as_str(), pick.path.as_str()),
        ("t1", "geometry.drive_faces")
    );
    let snap = app(&mut h).jobs.snapshot.clone().unwrap();
    assert!(snap.broken.is_empty());
    assert!(crate::vm::pick::selected(&snap, "t1", "geometry.drive_faces").is_empty());
}

#[test]
fn calculate_refuses_a_broken_toolpath_with_the_missing_pick() {
    let d = workspace("refuse");
    let mut h = harness();
    h.step();
    open(&mut h, d.join("cam/p.fluut"));
    std::fs::write(d.join("design/block.craftx"), floor_gone().bytes()).unwrap();
    // Opened again on the changed file: broken before any update.
    open(&mut h, d.join("cam/p.fluut"));
    stage(&mut h, "TOOLPATHS");
    h.get_by_label("Calculate all").click();
    settle(&mut h);
    let snap = app(&mut h).jobs.snapshot.clone().unwrap();
    assert_eq!(snap.status("t1"), crate::session::ToolpathStatus::Broken);
    assert!(
        snap.diagnostics("t1")
            .iter()
            .any(|x| x.code == "model.pick.missing"),
        "{:?}",
        snap.diagnostics("t1")
    );
}

#[test]
fn importing_a_file_of_a_linked_design_offers_to_update_instead() {
    let d = workspace("offer");
    std::fs::write(d.join("sent.craftx"), crate::craftx::hole().bytes()).unwrap();
    let mut h = harness();
    h.step();
    open(&mut h, d.join("cam/p.fluut"));
    scripted(&mut h).answer(DialogKind::Import, d.join("sent.craftx"));
    h.get_by_label("Import model").click();
    settle(&mut h);
    h.get_by_label(
        "This project already links the design in sent.craftx. Update it from this file instead?",
    );
    // Only from the command line is a new project offered.
    assert!(h.query_by_label("Open in a new project").is_none());
    h.get_by_label("Update from this file").click();
    settle(&mut h);
    h.get_by_label("Model updated");
    // The heading names the file the source links now.
    h.get_by_label("sent.craftx: revision 1 → 2");
}

#[test]
fn a_craftx_file_from_the_command_line_opens_in_a_new_project() {
    let d = std::env::temp_dir().join(format!("fluut-app-link-{}-launch", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let file = d.join("bracket.craftx");
    std::fs::write(&file, crate::craftx::rev1().bytes()).unwrap();
    let (launch, answer) = (file.clone(), d.join("bracket.fluut"));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_ui_state(
            move |ui, app: &mut State| {
                app.get_or_insert_with(|| {
                    let mut a = FluutApp::new(ui.ctx(), None, None, Instant::now())
                        .open_linked(launch.clone());
                    a.dialogs = Dialogs::scripted();
                    a.dialogs
                        .answer(DialogKind::New(DisplayUnit::Mm), answer.clone());
                    a
                })
                .frame(ui);
            },
            None,
        );
    settle(&mut h);
    settle(&mut h);
    // The dialog offered the file's folder and name.
    let asked = app(&mut h).dialogs.asked().to_vec();
    assert_eq!(asked[0].name, "bracket.fluut");
    assert_eq!(asked[0].dir.as_deref(), Some(d.as_path()));
    h.get_by_label("Millimetres").click();
    settle(&mut h);
    settle(&mut h);
    let snap = app(&mut h).jobs.snapshot.clone().unwrap();
    assert_eq!(snap.file_name(), "bracket.fluut");
    assert_eq!(
        snap.project.geometry[0].kind,
        fluut_model::project::SourceKind::Craftx
    );
    assert_eq!(snap.project.geometry[0].path, "bracket.craftx");
    h.get_by_label("Linked model bracket.craftx");
}

#[test]
fn a_craftx_file_from_the_command_line_offers_to_update_the_project_given_with_it() {
    let d = workspace("launch-update");
    std::fs::write(d.join("sent.craftx"), crate::craftx::hole().bytes()).unwrap();
    let (project, launch) = (d.join("cam/p.fluut"), d.join("sent.craftx"));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_ui_state(
            move |ui, app: &mut State| {
                app.get_or_insert_with(|| {
                    let mut a =
                        FluutApp::new(ui.ctx(), None, Some(project.clone()), Instant::now())
                            .open_linked(launch.clone());
                    a.dialogs = Dialogs::scripted();
                    a
                })
                .frame(ui);
            },
            None,
        );
    settle(&mut h);
    settle(&mut h);
    h.get_by_label("Open in a new project");
    h.get_by_label("Update from this file").click();
    settle(&mut h);
    h.get_by_label("Model updated");
    let snap = app(&mut h).jobs.snapshot.clone().unwrap();
    assert_eq!(snap.project.geometry[0].path, "../sent.craftx");
    assert_eq!(snap.links[0].revision, "2");
}
