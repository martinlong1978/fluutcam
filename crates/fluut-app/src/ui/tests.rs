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
