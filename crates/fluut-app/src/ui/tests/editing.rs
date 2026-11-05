// SPDX-License-Identifier: MIT OR Apache-2.0

//! Editing in the window: the Setup editor, tools, heights, picking and the
//! browser's actions.

use super::*;
use crate::vm::pick::PickMode;
use egui::accesskit::Role;
use fluut_model::project::{StockSpec, ZeroSpec};
use fluut_model::{Num, Sel, ToolpathTypeId};

/// Types into the text field labelled `label` and commits with Enter.
pub(crate) fn enter(h: &mut Harness<'static, State>, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get_by_role_and_label(Role::TextInput, label)
        .type_text(text);
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
}

/// Chooses `option` in the dropdown labelled `label`.
pub(crate) fn choose(h: &mut Harness<'static, State>, label: &str, option: &str) {
    h.get_by_role_and_label(Role::ComboBox, label).click();
    h.run_steps(2);
    h.get_by_label(option).click();
    h.run_steps(2);
}

/// Steps until a snapshot newer than `after` has arrived.
pub(crate) fn newer(h: &mut Harness<'static, State>, after: u64) {
    for _ in 0..2000 {
        h.step();
        if snapshot(h).revision > after {
            h.run_steps(2);
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("no new snapshot");
}

#[test]
fn the_setup_editor_edits_stock_and_zero_in_the_window() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    stage(&mut h, "SETUP");
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    button(&h, "Stock").click();
    h.run_steps(2);
    let r = snapshot(&mut h).revision;
    enter(&mut h, "Margin X, each side", "9");
    newer(&mut h, r);
    let s = snapshot(&mut h);
    assert_eq!(
        s.project.setup("s1").unwrap().stock,
        Some(StockSpec::Block {
            margin: Some([Num::float(9.0), Num::int(5), Num::int(0)]),
            size: None
        })
    );
    let r = s.revision;
    choose(&mut h, "Location", "Stock top, centre");
    newer(&mut h, r);
    let zero =
        |h: &mut Harness<'static, State>| snapshot(h).project.setup("s1").unwrap().zero.clone();
    assert_eq!(zero(&mut h), Some(ZeroSpec::StockTopCentre));
    // Undo goes back one edit at a time.
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_ne!(zero(&mut h), Some(ZeroSpec::StockTopCentre));
}

/// Opens `plate2d` with a new pocket selected, on the inspector's Geometry
/// tab. The id of the pocket.
fn pocket_on_plate(h: &mut Harness<'static, State>) -> String {
    open(h, project("plate2d"));
    app(h).jobs.add_toolpath("s1", ToolpathTypeId::Pocket2d);
    settle(h);
    let Selection::Toolpath(id) = app(h).ui.selection.clone() else {
        panic!("the new toolpath is selected");
    };
    app(h).ui.inspector_tab = 1;
    h.run_steps(3);
    id
}

/// Picks `sel` as a click in the viewport would.
fn click_entity(h: &mut Harness<'static, State>, sel: &Sel) {
    let r = snapshot(h).revision;
    let s = snapshot(h);
    let a = app(h);
    let e = a.ui.pick.as_mut().unwrap().hit(&s, sel).unwrap();
    a.jobs.edit(e);
    newer(h, r);
}

fn drive_faces(h: &mut Harness<'static, State>, id: &str) -> Vec<Sel> {
    crate::vm::pick::selected(&snapshot(h), id, "geometry.drive_faces")
}

#[test]
fn pick_mode_runs_from_the_inspector() {
    let mut h = harness();
    h.step();
    let id = pocket_on_plate(&mut h);
    assert!(app(&mut h).ui.pick.is_none());
    h.get_all_by_label("Pick").next().unwrap().click();
    h.run_steps(2);
    let mode = app(&mut h).ui.pick.clone().unwrap();
    assert_eq!(mode.toolpath, id);
    h.get_by_label("Done");
    let path = mode.path.clone();
    let a = Sel::Face("f-588.14".into());
    let b = Sel::Face("f-588.0".into());
    let picked =
        |h: &mut Harness<'static, State>| crate::vm::pick::selected(&snapshot(h), &id, &path);
    click_entity(&mut h, &a);
    click_entity(&mut h, &b);
    h.get_by_label("2 selected");
    h.get_by_label("f-588.14");
    assert_eq!(picked(&mut h), [a.clone(), b.clone()]);
    // Undo takes back one pick.
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(picked(&mut h), std::slice::from_ref(&a));
    click_entity(&mut h, &b);
    // Remove one, then Clear the rest.
    let r = snapshot(&mut h).revision;
    h.get_all_by_label("Remove").next().unwrap().click();
    newer(&mut h, r);
    assert_eq!(picked(&mut h), std::slice::from_ref(&b));
    let r = snapshot(&mut h).revision;
    h.get_by_label("Clear").click();
    newer(&mut h, r);
    assert!(picked(&mut h).is_empty());
    // Done leaves.
    h.get_by_label("Done").click();
    h.run_steps(2);
    assert!(app(&mut h).ui.pick.is_none());
}

#[test]
fn esc_and_a_new_selection_leave_pick_mode() {
    let mut h = harness();
    h.step();
    let _ = pocket_on_plate(&mut h);
    h.get_all_by_label("Pick").next().unwrap().click();
    h.run_steps(2);
    assert!(app(&mut h).ui.pick.is_some());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(app(&mut h).ui.pick.is_none());
    h.get_all_by_label("Pick").next().unwrap().click();
    h.run_steps(2);
    assert!(app(&mut h).ui.pick.is_some());
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    h.run_steps(2);
    assert!(app(&mut h).ui.pick.is_none());
}

#[test]
fn the_loop_on_offer_is_a_button() {
    let mut h = harness();
    h.step();
    open(&mut h, project("plate2d"));
    app(&mut h)
        .jobs
        .add_toolpath("s1", ToolpathTypeId::Contour2d);
    settle(&mut h);
    let Selection::Toolpath(id) = app(&mut h).ui.selection.clone() else {
        panic!();
    };
    app(&mut h).ui.inspector_tab = 1;
    h.run_steps(3);
    app(&mut h).ui.pick = Some(PickMode::new(
        ToolpathTypeId::Contour2d,
        &id,
        "geometry.drive_curves",
    ));
    h.run_steps(2);
    click_entity(&mut h, &Sel::Edge("e-588.16".into()));
    let n = app(&mut h).ui.pick.as_ref().unwrap().offer.len() + 1;
    assert!(n > 1);
    let r = snapshot(&mut h).revision;
    h.get_by_label(&format!("Add closed loop ({n} edges)"))
        .click();
    newer(&mut h, r);
    let items = crate::vm::pick::selected(&snapshot(&mut h), &id, "geometry.drive_curves");
    assert_eq!(items.len(), n);
    assert!(drive_faces(&mut h, &id).is_empty());
}
