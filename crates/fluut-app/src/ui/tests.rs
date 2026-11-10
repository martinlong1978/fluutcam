// SPDX-License-Identifier: MIT OR Apache-2.0

//! The window without a GPU, through `egui_kittest`: widgets are found by
//! their accessible labels, as a screen reader would (D13 §10).

use super::*;
use crate::dialogs::DialogKind;
use crate::session::tests::project;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use fluut_model::project::DisplayUnit;
use std::time::Duration;

mod editing;
mod guide;
mod setup_edit;

/// The app is built on the first frame, from the harness's context.
type State = Option<FluutApp>;

pub(crate) fn harness() -> Harness<'static, State> {
    Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_ui_state(
            |ui, app: &mut State| {
                app.get_or_insert_with(|| FluutApp::new(ui.ctx(), None, None, Instant::now()))
                    .frame(ui);
            },
            None,
        )
}

pub(crate) fn app<'a>(h: &'a mut Harness<'static, State>) -> &'a mut FluutApp {
    h.state_mut().as_mut().expect("built on the first frame")
}

/// Steps the UI until no job runs (the worker runs on its own thread) and
/// no dialog waits.
pub(crate) fn settle(h: &mut Harness<'static, State>) {
    for _ in 0..2000 {
        h.step();
        if !app(h).jobs.is_busy() && !app(h).dialogs.is_open() {
            h.step();
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("job did not finish");
}

/// Clicks Open with the dialog answering `path`.
pub(crate) fn open(h: &mut Harness<'static, State>, path: PathBuf) {
    scripted(h).answer(DialogKind::Open, path);
    h.get_by_label("Open").click();
    settle(h);
}

/// Replaces the native dialogs by scripted ones.
pub(crate) fn scripted<'a>(h: &'a mut Harness<'static, State>) -> &'a mut Dialogs {
    let a = app(h);
    if !a.dialogs.is_scripted() {
        a.dialogs = Dialogs::scripted();
    }
    &mut a.dialogs
}

fn enabled(h: &Harness<'static, State>, label: &str) -> bool {
    !h.get_by_label(label).accesskit_node().is_disabled()
}

/// Clicks a stage tab, by its upper-case label.
pub(crate) fn stage(h: &mut Harness<'static, State>, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(2);
}

#[test]
fn ribbon_without_a_project() {
    let mut h = harness();
    h.run_steps(2);
    assert!(enabled(&h, "Open"));
    assert!(!enabled(&h, "Cancel"));
    stage(&mut h, "TOOLPATHS");
    assert!(!enabled(&h, "Calculate all"));
    stage(&mut h, "POST");
    assert!(!enabled(&h, "Post"));
    stage(&mut h, "MODEL");
    h.get_by_label("Open a project to see its Setups and toolpaths.");
    h.get_by_label("Ready");
}

#[test]
fn open_select_and_edit_through_the_window() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    h.get_by_label("Opened block.fluut");
    stage(&mut h, "TOOLPATHS");
    assert!(enabled(&h, "Calculate all"));
    // The browser lists the toolpaths with their status.
    h.get_by_label("Pocket floor raster").click();
    h.run_steps(2);
    assert_eq!(app(&mut h).ui.selection, Selection::Toolpath("t2".into()));
    // The inspector shows the Passes tab with the raster angle.
    h.get_by_label("Pass angle");
    assert!(h.query_by_label("Offset order").is_none());
    // Switching the pattern to offset swaps the fields.
    h.get_by_label("offset").click();
    h.step();
    let snap = app(&mut h).jobs.wait_snapshot().unwrap();
    assert_eq!(
        snap.project
            .toolpath("t2")
            .unwrap()
            .1
            .params
            .get("passes.pattern"),
        Some(&fluut_model::PValue::Enum("offset".into()))
    );
    h.run_steps(2);
    h.get_by_label("Offset order");
}

#[test]
fn about_dialog_shows_the_logo() {
    let mut h = harness();
    h.step();
    h.get_by_label("About FluutCAM").click();
    h.run_steps(2);
    h.get_by_label("Open-source CAM for CNC milling");
    h.get_by_label(&format!("Version {}", env!("CARGO_PKG_VERSION")));
    h.get_by_label("Close").click();
    h.run_steps(2);
    assert!(!app(&mut h).ui.show_about);
}

#[test]
fn theme_switch() {
    let mut h = harness();
    h.step();
    assert!(app(&mut h).ui.dark);
    h.get_by_label("Light theme").click();
    h.run_steps(2);
    assert!(!app(&mut h).ui.dark);
    h.get_by_label("Dark theme");
}

#[test]
fn simulation_tab_steps_and_shows_the_rest_legend() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    app(&mut h).jobs.simulate("s1");
    settle(&mut h);
    let a = app(&mut h);
    let tab = a.dock.find_tab(&Tab::Simulation).expect("a Simulation tab");
    a.dock.set_active_tab(tab).unwrap();
    h.run_steps(3);
    let end = app(&mut h).ui.playback.t;
    assert!(end > 0.0);
    // Back one move, then forward again.
    assert!(!enabled(&h, "Forward"));
    h.get_by_label("Back").click();
    h.run_steps(2);
    let t = app(&mut h).ui.playback.t;
    assert!(t < end);
    h.get_by_label("Forward").click();
    h.run_steps(2);
    assert_eq!(app(&mut h).ui.playback.t, end);
    // By toolpath: back to the end of the one before the last.
    h.get_by_label("Toolpath").click();
    h.get_by_label("Back").click();
    h.run_steps(2);
    let snap = app(&mut h).jobs.snapshot.clone().unwrap();
    let clips = &snap.sims["s1"].clips;
    assert_eq!(app(&mut h).ui.playback.t, clips[clips.len() - 2].end);
    // The rest colouring and its legend.
    assert!(h.query_by_label("Above tolerance").is_none());
    h.get_by_label("Rest material").click();
    h.run_steps(2);
    assert_eq!(app(&mut h).ui.stock_colour, StockColour::Rest);
    for l in [
        "Above tolerance",
        "Within tolerance",
        "Gouge",
        "No part below",
    ] {
        h.get_by_label(l);
    }
    h.get_by_label("By toolpath").click();
    h.run_steps(2);
    h.get_by_label("Not cut");
    assert!(h.query_by_label("Above tolerance").is_none());
}

// --- the ribbon ------------------------------------------------------------

#[test]
fn each_stage_renders_its_commands_in_both_themes() {
    let cases: [(&str, &[&str]); 6] = [
        (
            "MODEL",
            &[
                "New project",
                "Open",
                "Save as",
                "Import model",
                "Re-import",
            ],
        ),
        (
            "SETUP",
            &[
                "New 3-axis Setup",
                "New Rotary Setup",
                "New Indexed Setup",
                "New Setup based on selected",
                "Stock",
                "Work zero",
            ],
        ),
        (
            "TOOLPATHS",
            &[
                "2D optimised load",
                "Drill",
                "3D optimised load",
                "Steep and shallow",
                "Rotary spiral",
                "Manual NC",
                "Calculate selected",
                "Calculate all",
            ],
        ),
        (
            "SIMULATE",
            &["Simulate", "Play or pause", "Step back", "Step forward"],
        ),
        ("POST", &["Post"]),
        ("LIBRARY", &["Tools", "Machines", "Posts"]),
    ];
    let mut h = harness();
    h.step();
    for dark in [true, false] {
        app(&mut h).ui.dark = dark;
        for (tab, labels) in cases {
            stage(&mut h, tab);
            for l in labels {
                h.get_by_label(l);
            }
            h.get_by_label("Cancel");
        }
    }
}

#[test]
fn switching_stage_leaves_the_viewport_alone() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    let a = app(&mut h);
    a.view.camera.target = glam::Vec3::new(1.0, 2.0, 3.0);
    let camera = format!("{:?}", a.view.camera);
    let selection = a.ui.selection.clone();
    for tab in ["SETUP", "TOOLPATHS", "SIMULATE", "POST", "LIBRARY", "MODEL"] {
        stage(&mut h, tab);
        assert_eq!(format!("{:?}", app(&mut h).view.camera), camera);
        assert_eq!(app(&mut h).ui.selection, selection);
    }
}

#[test]
fn commands_that_need_a_project_are_disabled_without_one() {
    let mut h = harness();
    h.run_steps(2);
    assert!(enabled(&h, "New project") && enabled(&h, "Open"));
    for l in [
        "Save",
        "Save as",
        "Import model",
        "Re-import",
        "Undo",
        "Redo",
    ] {
        assert!(!enabled(&h, l), "{l}");
    }
    stage(&mut h, "SETUP");
    for l in ["New 3-axis Setup", "New Rotary Setup", "Stock", "Work zero"] {
        assert!(!enabled(&h, l), "{l}");
    }
    // Only the Library has no operations yet.
    stage(&mut h, "LIBRARY");
    for l in ["Tools", "Machines", "Posts"] {
        assert!(!enabled(&h, l), "{l}");
    }
}

#[test]
fn the_ribbon_has_no_path_fields() {
    let mut h = harness();
    h.run_steps(2);
    for st in ["MODEL", "POST"] {
        stage(&mut h, st);
        assert!(
            h.query_all_by_role(egui::accesskit::Role::TextInput)
                .next()
                .is_none(),
            "{st}"
        );
    }
}

#[test]
fn calculate_selected_calculates_the_selection() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    h.get_by_label("Pocket floor raster").click();
    h.run_steps(2);
    stage(&mut h, "TOOLPATHS");
    h.get_by_label("Calculate selected").click();
    h.run_steps(2);
    settle(&mut h);
    let snap = app(&mut h).jobs.snapshot.clone().unwrap();
    // `t2` needs the stock `t1` leaves; `t3` comes after it.
    assert!(snap.paths.contains_key("t2"));
    assert!(!snap.paths.contains_key("t3"));
}

#[test]
fn the_gallery_follows_the_setup_kind() {
    let mut h = harness();
    h.step();
    stage(&mut h, "TOOLPATHS");
    h.get_by_label("Rotary spiral");
    h.get_by_label("Drill");
    stage(&mut h, "MODEL");
    open(&mut h, project("shaft"));
    stage(&mut h, "TOOLPATHS");
    h.get_by_label("Rotary spiral");
    assert!(h.query_by_label("Drill").is_none());
}

#[test]
fn ribbon_playback_moves_the_playhead() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    app(&mut h).jobs.simulate("s1");
    settle(&mut h);
    stage(&mut h, "SIMULATE");
    let end = app(&mut h).ui.playback.t;
    assert!(end > 0.0);
    h.get_by_label("Step back").click();
    h.run_steps(2);
    assert!(app(&mut h).ui.playback.t < end);
    h.get_by_label("Play or pause").click();
    h.run_steps(1);
    assert!(app(&mut h).ui.playback.playing);
}

/// Writes a picture of each stage in both themes, to `FLUUT_SHOTS` or the
/// system temporary folder. Needs a GPU adapter, so it is not in the normal
/// run: `cargo test -p fluut-app ribbon_screenshots -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; writes PNG files"]
fn ribbon_screenshots() {
    let dir = std::env::var_os("FLUUT_SHOTS").map_or_else(std::env::temp_dir, PathBuf::from);
    std::fs::create_dir_all(&dir).unwrap();
    for scale in [1.0, 2.0] {
        shots_at(&dir, scale);
    }
}

fn shots_at(dir: &std::path::Path, scale: f32) {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_pixels_per_point(scale)
        .wgpu()
        .build_ui_state(
            |ui, app: &mut State| {
                app.get_or_insert_with(|| FluutApp::new(ui.ctx(), None, None, Instant::now()))
                    .frame(ui);
            },
            None,
        );
    h.step();
    open(&mut h, project("block"));
    for dark in [true, false] {
        let a = app(&mut h);
        a.ui.dark = dark;
        let theme = if dark { "dark" } else { "light" };
        h.run_steps(1);
        brand::apply_theme(&h.ctx, dark);
        for tab in ["MODEL", "SETUP", "TOOLPATHS", "SIMULATE", "POST", "LIBRARY"] {
            stage(&mut h, tab);
            let image = h.render().expect("render");
            let (w, h) = (1440 * scale as u32, 260 * scale as u32);
            let top = image::imageops::crop_imm(&image, 0, 0, w, h).to_image();
            let name = format!("ribbon-{}-{theme}-{scale}x.png", tab.to_lowercase());
            top.save(dir.join(name)).unwrap();
        }
    }
}

// --- the update check ------------------------------------------------------

mod update_check {
    use super::*;
    use crate::updates::{Checker, UpdateState, Updates, now};
    use fluut_core::settings::Settings;
    use fluut_core::update::{Offer, Outcome};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn offer() -> Offer {
        Offer {
            version: "2.0.0".into(),
            date: "2026-12-01".into(),
            notes_url: "https://example.org/notes".into(),
            download_page: "https://example.org/downloads".into(),
            artefacts: Vec::new(),
        }
    }

    /// A window with the update check on, a fake network and no stored
    /// settings.
    fn harness_with(result: Outcome) -> (Harness<'static, State>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let n = Arc::clone(&calls);
        let checker: Checker = Arc::new(move || {
            n.fetch_add(1, Ordering::SeqCst);
            Ok(result.clone())
        });
        let mut updates = Some(Updates::with(
            Settings::default(),
            None,
            checker,
            now,
            Arc::new(|| {}),
            true,
        ));
        updates.as_mut().unwrap().set_has_key(true);
        let h = Harness::builder()
            .with_size(egui::vec2(1440.0, 900.0))
            .build_ui_state(
                move |ui, app: &mut State| {
                    app.get_or_insert_with(|| {
                        FluutApp::new(ui.ctx(), None, None, Instant::now())
                            .with_updates(updates.take().unwrap())
                    })
                    .frame(ui);
                },
                None,
            );
        (h, calls)
    }

    /// Steps until the check thread has reported.
    fn until_checked(h: &mut Harness<'static, State>) {
        for _ in 0..2000 {
            h.step();
            if !app(h).updates.is_checking() {
                h.step();
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("check did not end");
    }

    #[test]
    fn a_window_without_the_check_does_not_ask() {
        let mut h = harness();
        h.run_steps(2);
        assert!(
            h.query_by_label("Check for updates automatically?")
                .is_none()
        );
    }

    #[test]
    fn first_start_asks_and_dismissing_means_no() {
        let (mut h, calls) = harness_with(Outcome::UpToDate);
        h.run_steps(2);
        h.get_by_label("Check for updates automatically?");
        h.get_by_label("Yes");
        h.get_by_label("No");
        assert!(app(&mut h).updates.needs_prompt());
        // Nothing is checked while the question is open.
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(!app(&mut h).updates.needs_prompt());
        assert!(!app(&mut h).updates.auto_check());
        assert!(
            h.query_by_label("Check for updates automatically?")
                .is_none()
        );
        h.run_steps(3);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn no_is_the_same_as_dismissing() {
        let (mut h, calls) = harness_with(Outcome::UpToDate);
        h.run_steps(2);
        h.get_by_label("No").click();
        h.run_steps(3);
        assert!(!app(&mut h).updates.needs_prompt());
        assert!(!app(&mut h).updates.auto_check());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn yes_checks_and_the_status_bar_offers_the_update() {
        let (mut h, calls) = harness_with(Outcome::Available(offer()));
        h.run_steps(2);
        h.get_by_label("Yes").click();
        h.run_steps(2);
        until_checked(&mut h);
        assert!(app(&mut h).updates.auto_check());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        h.get_by_label("Version 2.0.0 is available");
        // Details opens Settings with the links; nothing is downloaded.
        h.get_by_label("Details").click();
        h.run_steps(2);
        h.get_by_label("Release notes:");
        h.get_by_label("https://example.org/notes");
        h.get_by_label("https://example.org/downloads");
        h.get_by_label("Version 2.0.0 is available (2026-12-01).");
    }

    #[test]
    fn settings_toggle_and_check_now_work_when_automatic_checks_are_off() {
        let (mut h, calls) = harness_with(Outcome::UpToDate);
        h.run_steps(2);
        h.get_by_label("No").click();
        h.run_steps(2);
        h.get_by_label("Settings").click();
        h.run_steps(2);
        h.get_by_label("Check for updates automatically");
        h.get_by_label("Check now").click();
        h.run_steps(2);
        until_checked(&mut h);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(*app(&mut h).updates.state(), UpdateState::UpToDate);
        h.get_by_label("FluutCAM is up to date.");
        assert!(!app(&mut h).updates.auto_check());
        // The toggle turns the automatic check on.
        h.get_by_label("Check for updates automatically").click();
        h.run_steps(2);
        assert!(app(&mut h).updates.auto_check());
    }
}

/// The ribbon button of a label that the inspector also shows.
fn button<'a>(h: &'a Harness<'static, State>, label: &'a str) -> egui_kittest::Node<'a> {
    h.get_by_role_and_label(egui::accesskit::Role::Button, label)
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fluut-app-ribbon-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn boss_step() -> PathBuf {
    project("boss").with_file_name("boss.step")
}

/// A fixed header time, so posted files are the same on every run.
fn fixed_clock() -> (String, String) {
    ("2026-11-05".into(), "12:00:00".into())
}

fn snapshot(h: &mut Harness<'static, State>) -> std::sync::Arc<crate::session::Snapshot> {
    app(h).jobs.snapshot.clone().unwrap()
}

#[test]
fn ribbon_adds_setups_and_toolpaths_and_undoes() {
    let d = temp("ops");
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::New(DisplayUnit::Mm), d.join("ops"));
    h.get_by_label("New project").click();
    settle(&mut h);
    h.get_by_label("Millimetres").click();
    settle(&mut h);
    h.get_by_label("ops.fluut");
    scripted(&mut h).answer(DialogKind::Import, boss_step());
    h.get_by_label("Import model").click();
    settle(&mut h);
    h.get_by_label("ops.fluut *");
    // Setup > New 3-axis Setup.
    stage(&mut h, "SETUP");
    h.get_by_label("New 3-axis Setup").click();
    settle(&mut h);
    assert_eq!(app(&mut h).ui.selection, Selection::Setup("s1".into()));
    h.get_all_by_label("Setup 1").next().unwrap();
    // New Setup based on the selected one.
    h.get_by_label("New Setup based on selected").click();
    settle(&mut h);
    let s = snapshot(&mut h);
    assert_eq!(s.project.setups.len(), 2);
    assert_eq!(s.project.setups[1].based_on.as_deref(), Some("s1"));
    // Toolpaths > Parallel, on the selected Setup.
    stage(&mut h, "TOOLPATHS");
    h.get_by_label("Parallel").click();
    settle(&mut h);
    assert_eq!(app(&mut h).ui.selection, Selection::Toolpath("t1".into()));
    h.get_all_by_label("Parallel 1").next().unwrap();
    // The Undo button undoes the toolpath; Redo, by Ctrl+Y, brings it back.
    h.get_by_label("Undo").click();
    settle(&mut h);
    let s = snapshot(&mut h);
    assert!(s.project.toolpath("t1").is_none() && s.can_redo);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Y);
    settle(&mut h);
    let s = snapshot(&mut h);
    assert!(s.project.toolpath("t1").is_some() && !s.can_redo);
    // Ctrl+Z undoes it again, Ctrl+Shift+Z redoes.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    settle(&mut h);
    assert!(snapshot(&mut h).project.toolpath("t1").is_none());
    h.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::Z,
    );
    settle(&mut h);
    assert!(snapshot(&mut h).project.toolpath("t1").is_some());
    // Ctrl+S saves: the star goes.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    settle(&mut h);
    h.get_by_label("ops.fluut");
    assert!(!snapshot(&mut h).dirty);
}

#[test]
fn reimport_runs_from_the_ribbon() {
    let d = temp("reimport");
    let mut h = harness();
    h.step();
    app(&mut h).jobs.new_project(d.join("re"), DisplayUnit::Mm);
    settle(&mut h);
    assert!(!enabled(&h, "Re-import"));
    app(&mut h).jobs.import_model(boss_step());
    settle(&mut h);
    h.get_by_label("Re-import").click();
    settle(&mut h);
    assert_eq!(
        app(&mut h).jobs.last,
        Some((
            JobKind::Reimport,
            Outcome::Reimported {
                sources: 1,
                unmatched: 0
            }
        ))
    );
}

#[test]
fn stock_and_work_zero_select_the_setup_and_point_at_its_row() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    stage(&mut h, "SETUP");
    app(&mut h).ui.selection = Selection::None;
    button(&h, "Stock").click();
    h.run_steps(2);
    assert_eq!(app(&mut h).ui.selection, Selection::Setup("s1".into()));
    assert_eq!(app(&mut h).ui.inspector_focus, Some("setup-stock"));
    button(&h, "Work zero").click();
    h.run_steps(2);
    assert_eq!(app(&mut h).ui.inspector_focus, Some("setup-zero"));
}

#[test]
fn shortcuts_ask_the_dialogs() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    let n0 = scripted(&mut h).asked().len();
    for (mods, key) in [
        (egui::Modifiers::COMMAND, egui::Key::N),
        (egui::Modifiers::COMMAND, egui::Key::O),
        (
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::S,
        ),
        (egui::Modifiers::COMMAND, egui::Key::I),
    ] {
        h.key_press_modifiers(mods, key);
        settle(&mut h);
    }
    let kinds: Vec<_> = scripted(&mut h).asked()[n0..]
        .iter()
        .map(|a| a.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            DialogKind::New(DisplayUnit::Mm),
            DialogKind::Open,
            DialogKind::SaveAs,
            DialogKind::Import
        ]
    );
}

#[test]
fn a_dirty_project_asks_before_it_is_replaced() {
    let d = temp("discard");
    let mut h = harness();
    h.step();
    app(&mut h).jobs.new_project(d.join("one"), DisplayUnit::Mm);
    settle(&mut h);
    app(&mut h).jobs.import_model(boss_step());
    settle(&mut h);
    assert!(snapshot(&mut h).dirty);
    // Keep editing: nothing is opened.
    scripted(&mut h).answer(DialogKind::Open, project("block"));
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::O);
    settle(&mut h);
    h.run_steps(2);
    h.get_by_label("Unsaved changes");
    h.get_by_label("Keep editing").click();
    settle(&mut h);
    assert!(h.query_by_label("Unsaved changes").is_none());
    assert_eq!(snapshot(&mut h).project.name, "one");
    // Discard: the project is opened.
    scripted(&mut h).answer(DialogKind::Open, project("block"));
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::O);
    settle(&mut h);
    h.run_steps(2);
    h.get_by_label("Discard").click();
    settle(&mut h);
    h.get_by_label("Opened block.fluut");
}

#[test]
fn the_post_dropdown_lists_the_store_and_posting_asks_for_a_file() {
    let d = temp("post");
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    app(&mut h).ui.clock = fixed_clock;
    stage(&mut h, "POST");
    h.query_all_by_role(egui::accesskit::Role::ComboBox)
        .find(|n| n.value().as_deref() == Some("fluut/linuxcnc"))
        .unwrap()
        .click();
    h.run_steps(2);
    for id in ["fluut/gcode", "fluut/linuxcnc-4x"] {
        h.get_by_label(id);
    }
    h.get_by_label("fluut/grbl").click();
    h.run_steps(2);
    assert_eq!(app(&mut h).ui.post_id, "fluut/grbl");
    // The dialog offers the project name and the post's extension.
    let out = d.join("chosen.gcode");
    scripted(&mut h).answer(DialogKind::PostOutput, out.clone());
    h.get_by_label("Post").click();
    settle(&mut h);
    let asked = scripted(&mut h).asked().last().cloned().unwrap();
    assert_eq!(asked.kind, DialogKind::PostOutput);
    assert_eq!(asked.name, "block.gcode");
    assert!(out.exists());
}

/// The prototype loop, by clicks alone: a new project, a model, a Setup, a
/// toolpath, Calculate all and Post, with the file dialogs answered by
/// script.
#[test]
fn new_import_setup_toolpath_calculate_post_by_clicking() {
    let d = temp("e2e");
    let mut h = harness();
    h.step();
    app(&mut h).ui.clock = fixed_clock;
    scripted(&mut h).answer(DialogKind::New(DisplayUnit::Mm), d.join("loop"));
    h.get_by_label("New project").click();
    settle(&mut h);
    h.get_by_label("Millimetres").click();
    settle(&mut h);
    scripted(&mut h).answer(DialogKind::Import, boss_step());
    h.get_by_label("Import model").click();
    settle(&mut h);
    stage(&mut h, "SETUP");
    h.get_by_label("New 3-axis Setup").click();
    settle(&mut h);
    stage(&mut h, "TOOLPATHS");
    h.get_by_label("Parallel").click();
    settle(&mut h);
    // The browser lists the Setup and its toolpath.
    h.get_all_by_label("Setup 1").next().unwrap();
    h.get_all_by_label("Parallel 1").next().unwrap();
    h.get_by_label("Calculate all").click();
    settle(&mut h);
    let s = snapshot(&mut h);
    assert!(s.paths.contains_key("t1"), "calculated");
    stage(&mut h, "POST");
    let out = d.join("loop.ngc");
    scripted(&mut h).answer(DialogKind::PostOutput, out.clone());
    h.get_by_label("Post").click();
    settle(&mut h);
    h.get_by_label(&format!("Wrote {}", out.display()));
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(
        text.contains("G1") && text.contains("(Project: loop)"),
        "{text}"
    );
    // The dialogs offered the project's folder and names.
    let asked = scripted(&mut h).asked().to_vec();
    let kinds: Vec<_> = asked.iter().map(|a| a.kind).collect();
    assert_eq!(
        kinds,
        [
            DialogKind::New(DisplayUnit::Mm),
            DialogKind::Import,
            DialogKind::PostOutput
        ]
    );
    assert_eq!(asked[2].name, "loop.ngc");
    assert_eq!(asked[2].dir.as_deref(), Some(d.as_path()));
}
