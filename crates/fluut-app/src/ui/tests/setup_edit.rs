// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Setup editor's kind, bodies, orientation and handles in the window.

use super::editing::{choose, click_entity, gpu_harness, newer};
use super::*;
use crate::jobs::JobKind;
use crate::jobs::Outcome;
use crate::vm::pick::{PickMode, SetupPick};
use egui::accesskit::Role;
use fluut_model::Sel;
use fluut_model::project::{AxisDir, AxisPick, Orientation, SetupKind, StockSpec};

/// `plate` (two drawing bodies) with its Setup selected.
fn plate_setup(h: &mut Harness<'static, State>) {
    open(h, project("plate"));
    stage(h, "SETUP");
    app(h).ui.selection = Selection::Setup("s1".into());
    h.run_steps(3);
}

fn bodies(h: &mut Harness<'static, State>) -> Option<Vec<String>> {
    snapshot(h)
        .project
        .setup("s1")
        .unwrap()
        .geometry
        .as_ref()
        .and_then(|g| g.bodies.clone())
}

#[test]
fn the_kind_is_chosen_in_the_inspector_and_undone() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    stage(&mut h, "SETUP");
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    h.run_steps(3);
    let r = snapshot(&mut h).revision;
    choose(&mut h, "Kind", "Rotary");
    newer(&mut h, r);
    let s = snapshot(&mut h);
    let st = s.project.setup("s1").unwrap();
    assert_eq!(st.kind, Some(SetupKind::Rotary));
    assert!(matches!(st.stock, Some(StockSpec::Cylinder { .. })));
    h.get_all_by_label("Radius").next().unwrap();
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(
        snapshot(&mut h).project.setup("s1").unwrap().kind,
        Some(SetupKind::ThreeAxis)
    );
}

#[test]
fn bodies_are_checked_and_picked_on_the_model() {
    let mut h = harness();
    h.step();
    plate_setup(&mut h);
    h.get_by_label("POCKET (b-2)");
    // A check box is one edit.
    let r = snapshot(&mut h).revision;
    h.get_by_role_and_label(Role::CheckBox, "POCKET (b-2)")
        .click();
    newer(&mut h, r);
    assert_eq!(bodies(&mut h), Some(vec!["b-1".to_owned()]));
    // Pick mode: the first Pick button is the bodies'.
    h.get_all_by_label("Pick").next().unwrap().click();
    h.run_steps(2);
    let m = app(&mut h).ui.pick.clone().unwrap();
    assert_eq!(m.setup, Some(("s1".to_owned(), SetupPick::Bodies)));
    h.get_by_label("Done");
    click_entity(&mut h, &Sel::Edge("e-2.0".into()));
    assert_eq!(bodies(&mut h), None);
    assert!(app(&mut h).ui.pick.is_some(), "a body pick stays open");
    click_entity(&mut h, &Sel::Edge("e-1.0".into()));
    assert_eq!(bodies(&mut h), Some(vec!["b-2".to_owned()]));
    // Undo takes back one pick, and Esc ends the mode.
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(bodies(&mut h), None);
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(app(&mut h).ui.pick.is_none());
}

#[test]
fn stock_from_a_body_is_chosen_and_picked() {
    let mut h = harness();
    h.step();
    plate_setup(&mut h);
    h.get_by_role_and_label(Role::CheckBox, "POCKET (b-2)")
        .click();
    settle(&mut h);
    let r = snapshot(&mut h).revision;
    choose(&mut h, "Type", "Body");
    newer(&mut h, r);
    let stock =
        |h: &mut Harness<'static, State>| snapshot(h).project.setup("s1").unwrap().stock.clone();
    assert_eq!(stock(&mut h), Some(StockSpec::Body { body: "b-2".into() }));
    // Pick the other body as the stock: a single value ends the pick.
    let n = app(&mut h).jobs.snapshot.clone().unwrap().revision;
    app(&mut h).ui.pick = Some(PickMode::for_setup("s1", SetupPick::StockBody));
    click_entity(&mut h, &Sel::Edge("e-1.0".into()));
    h.run_steps(2);
    assert!(snapshot(&mut h).revision > n);
    assert_eq!(stock(&mut h), Some(StockSpec::Body { body: "b-1".into() }));
    assert!(app(&mut h).ui.pick.is_none());
}

#[test]
fn the_orientation_is_chosen_picked_and_undone() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    stage(&mut h, "SETUP");
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    h.run_steps(3);
    let orientation = |h: &mut Harness<'static, State>| {
        snapshot(h).project.setup("s1").unwrap().orientation.clone()
    };
    let r = snapshot(&mut h).revision;
    choose(&mut h, "X axis", "+Y of the model");
    newer(&mut h, r);
    assert_eq!(
        orientation(&mut h),
        Some(Orientation::ThreeAxis {
            tool_axis: AxisPick::Axis(AxisDir::PosZ),
            x_axis: AxisPick::Axis(AxisDir::PosY),
        })
    );
    // A face of the model is the tool axis: the Pick button beside it.
    let tool = app(&mut h).jobs.snapshot.clone().unwrap();
    assert!(tool.scenes["s1"].faces.iter().any(|f| f == "f-494.10"));
    let picks: Vec<_> = h.get_all_by_label("Pick").collect();
    assert!(
        picks.len() >= 2,
        "Pick buttons for bodies and each direction"
    );
    h.get_all_by_label("Pick").nth(1).unwrap().click();
    h.run_steps(2);
    assert_eq!(
        app(&mut h).ui.pick.as_ref().and_then(|p| p.setup.clone()),
        Some(("s1".to_owned(), SetupPick::ToolAxis))
    );
    click_entity(&mut h, &Sel::Face("f-494.10".into()));
    h.run_steps(2);
    assert!(app(&mut h).ui.pick.is_none(), "one value, one pick");
    assert_eq!(
        orientation(&mut h),
        Some(Orientation::ThreeAxis {
            tool_axis: AxisPick::Face("f-494.10".into()),
            x_axis: AxisPick::Axis(AxisDir::PosY),
        })
    );
    h.get_by_label("Undo").click();
    settle(&mut h);
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(orientation(&mut h), None);
}

#[test]
fn reimport_keeps_body_stock_and_orientation_picks() {
    let d = temp("setup_reimport");
    let mut h = harness();
    h.step();
    app(&mut h).jobs.new_project(d.join("re"), DisplayUnit::Mm);
    settle(&mut h);
    app(&mut h)
        .jobs
        .import_model(project("plate2d").with_file_name("plate2d.step"));
    settle(&mut h);
    app(&mut h).jobs.add_setup(SetupKind::ThreeAxis, None);
    settle(&mut h);
    let s = snapshot(&mut h);
    let edit = |h: &mut Harness<'static, State>, key: &str, text: &str| {
        let s = snapshot(h);
        let e = crate::vm::setup::setup_edit(&s, "s1", key, text).unwrap();
        let r = s.revision;
        app(h).jobs.edit(crate::jobs::Edit::Model(e));
        newer(h, r);
    };
    assert_eq!(s.project.geometry[0].bodies[0].id, "b-588");
    edit(&mut h, "bodies.all", "false");
    edit(&mut h, "orientation.x_axis", "face:f-588.14");
    app(&mut h).jobs.reimport();
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
    assert_eq!(bodies(&mut h), Some(vec!["b-588".to_owned()]));
    let o = snapshot(&mut h)
        .project
        .setup("s1")
        .unwrap()
        .orientation
        .clone();
    assert!(matches!(
        o,
        Some(Orientation::ThreeAxis { x_axis: AxisPick::Face(ref f), .. }) if f == "f-588.14"
    ));
}

#[test]
fn a_body_pick_draws_the_whole_model_and_hovers_in_the_viewport() {
    let Some(mut h) = gpu_harness() else {
        eprintln!("no wgpu adapter: the body pick drawing is skipped");
        return;
    };
    h.step();
    open(&mut h, project("block"));
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    app(&mut h).ui.pick = Some(PickMode::for_setup("s1", SetupPick::Bodies));
    h.run_steps(3);
    let centre = h.get_by_label("3-D viewport").rect().center();
    h.hover_at(centre);
    h.run_steps(3);
    let hover = app(&mut h).ui.pick.as_ref().unwrap().hover.clone();
    assert!(
        matches!(hover, Some(Sel::Face(_) | Sel::Edge(_))),
        "the model is under the pointer: {hover:?}"
    );
}

/// Where `world` (in W) is on the screen of the harness.
fn on_screen(h: &mut Harness<'static, State>, world: [f32; 3]) -> egui::Pos2 {
    let rect = h.get_by_label("3-D viewport").rect();
    let q = app(h)
        .view
        .camera
        .project(
            glam::Vec3::from(world),
            glam::Vec2::new(rect.width(), rect.height()),
        )
        .unwrap();
    rect.min + egui::vec2(q.x, q.y)
}

fn mark(h: &mut Harness<'static, State>, kind: crate::vm::handles::HandleKind) -> egui::Pos2 {
    app(h)
        .view
        .handle_marks
        .iter()
        .find(|(k, _)| *k == kind)
        .unwrap_or_else(|| panic!("no handle {kind:?}"))
        .1
}

fn drag(h: &mut Harness<'static, State>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.run_steps(2);
    h.drag_at(from);
    h.run_steps(2);
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run_steps(2);
    }
    h.drop_at(to);
    h.run_steps(2);
}

#[test]
fn dragging_the_zero_handle_snaps_to_the_top_centre_and_undoes() {
    use crate::vm::handles::HandleKind;
    let Some(mut h) = gpu_harness() else {
        eprintln!("no wgpu adapter: the handle drag is skipped");
        return;
    };
    h.step();
    open(&mut h, project("block"));
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    h.run_steps(4);
    let zero =
        |h: &mut Harness<'static, State>| snapshot(h).project.setup("s1").unwrap().zero.clone();
    let zero_at = mark(&mut h, HandleKind::Zero);
    // A drag to nowhere changes nothing.
    let r = snapshot(&mut h).revision;
    drag(&mut h, zero_at, egui::pos2(zero_at.x, zero_at.y - 400.0));
    h.run_steps(3);
    assert_eq!(snapshot(&mut h).revision, r);
    // A drag to the top centre drops the zero there.
    let centre = crate::vm::handles::zero_targets(&snapshot(&mut h), "s1")
        .into_iter()
        .find(|(k, _)| k == "top_centre")
        .unwrap()
        .1;
    let to = on_screen(&mut h, centre);
    drag(&mut h, zero_at, to);
    newer(&mut h, r);
    assert_eq!(
        zero(&mut h),
        Some(fluut_model::project::ZeroSpec::StockTopCentre)
    );
    // One undo step.
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert!(matches!(
        zero(&mut h),
        Some(fluut_model::project::ZeroSpec::StockTopCorner(_))
    ));
}

#[test]
fn dragging_a_margin_handle_changes_the_margin_in_one_step() {
    use crate::vm::handles::HandleKind;
    let Some(mut h) = gpu_harness() else {
        eprintln!("no wgpu adapter: the handle drag is skipped");
        return;
    };
    h.step();
    open(&mut h, project("block"));
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    h.run_steps(4);
    let from = mark(&mut h, HandleKind::Margin(2));
    // About 6 mm up the Z axis on the screen.
    let top = crate::vm::handles::handles(&snapshot(&mut h), "s1")
        .into_iter()
        .find(|k| k.kind == HandleKind::Margin(2))
        .unwrap()
        .at;
    let to = on_screen(&mut h, [top[0], top[1], top[2] + 6.0]);
    let r = snapshot(&mut h).revision;
    drag(&mut h, from, to);
    newer(&mut h, r);
    let margin =
        |h: &mut Harness<'static, State>| match snapshot(h).project.setup("s1").unwrap().stock {
            Some(StockSpec::Block {
                margin: Some(m), ..
            }) => m,
            ref s => panic!("{s:?}"),
        };
    let m = margin(&mut h);
    assert!(
        (m[2].v - 6.0).abs() < 1.0,
        "the top margin follows the drag: {}",
        m[2].v
    );
    assert_eq!(m[0].v, 5.0);
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(margin(&mut h)[2].v, 0.0);
}
