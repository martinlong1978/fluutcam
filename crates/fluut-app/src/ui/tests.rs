// SPDX-License-Identifier: MIT OR Apache-2.0

//! The window without a GPU, through `egui_kittest`: widgets are found by
//! their accessible labels, as a screen reader would (D13 §10).

use super::*;
use crate::session::tests::project;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use std::time::Duration;

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

/// Steps the UI until no job runs (the worker runs on its own thread).
pub(crate) fn settle(h: &mut Harness<'static, State>) {
    for _ in 0..2000 {
        h.step();
        if !app(h).jobs.is_busy() {
            h.step();
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("job did not finish");
}

fn enabled(h: &Harness<'static, State>, label: &str) -> bool {
    !h.get_by_label(label).accesskit_node().is_disabled()
}

#[test]
fn ribbon_without_a_project() {
    let mut h = harness();
    h.run_steps(2);
    assert!(enabled(&h, "Open"));
    assert!(!enabled(&h, "Calculate all"));
    assert!(!enabled(&h, "Post"));
    assert!(!enabled(&h, "Cancel"));
    h.get_by_label("Open a project to see its Setups and toolpaths.");
    h.get_by_label("Ready");
}

#[test]
fn open_select_and_edit_through_the_window() {
    let mut h = harness();
    h.step();
    app(&mut h).ui.open_path = project("block").display().to_string();
    h.get_by_label("Open").click();
    settle(&mut h);
    h.get_by_label("Opened block.fluut");
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
    app(&mut h).ui.open_path = project("block").display().to_string();
    h.get_by_label("Open").click();
    settle(&mut h);
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

#[test]
fn file_menu_adds_setups_and_toolpaths_and_undoes() {
    let d = std::env::temp_dir().join(format!("fluut-app-menu-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let mut h = harness();
    h.step();
    // Without a project the operations that need one are off.
    h.get_all_by_label("File").next().unwrap().click();
    h.run_steps(2);
    assert!(enabled(&h, "New project (mm)..."));
    assert!(!enabled(&h, "Import model..."));
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    app(&mut h)
        .jobs
        .new_project(d.join("menu"), fluut_model::project::DisplayUnit::Mm);
    settle(&mut h);
    app(&mut h)
        .jobs
        .import_model(project("boss").with_file_name("boss.step"));
    settle(&mut h);
    h.get_by_label("menu.fluut *");
    // Add Setup > 3-axis.
    h.get_all_by_label("File").next().unwrap().click();
    h.run_steps(2);
    h.get_by_label("Add Setup ⏵").click();
    h.run_steps(2);
    h.get_by_label("3-axis").click();
    settle(&mut h);
    assert_eq!(app(&mut h).ui.selection, Selection::Setup("s1".into()));
    h.get_all_by_label("Setup 1").next().unwrap();
    // Add toolpath > Parallel, on the selected Setup.
    h.get_all_by_label("File").next().unwrap().click();
    h.run_steps(2);
    h.get_by_label("Add toolpath ⏵").click();
    h.run_steps(2);
    h.get_by_label("Parallel").click();
    settle(&mut h);
    assert_eq!(app(&mut h).ui.selection, Selection::Toolpath("t1".into()));
    h.get_all_by_label("Parallel 1").next().unwrap();
    // Ctrl+Z undoes the toolpath.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    settle(&mut h);
    let s = app(&mut h).jobs.snapshot.clone().unwrap();
    assert!(s.project.setups[0].toolpaths.is_empty() && s.can_redo);
    // Ctrl+S saves: the star goes.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    settle(&mut h);
    h.get_by_label("menu.fluut");
    assert!(!app(&mut h).jobs.snapshot.as_ref().unwrap().dirty);
}
