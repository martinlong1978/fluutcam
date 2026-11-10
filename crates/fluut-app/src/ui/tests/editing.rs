// SPDX-License-Identifier: MIT OR Apache-2.0

//! Editing in the window: the Setup editor, tools, heights, picking and the
//! browser's actions.

use super::*;
use crate::jobs::{CalcTargets, Jobs};
use crate::vm::pick::PickMode;
use egui::accesskit::Role;
use fluut_model::project::{SetupKind, StockSpec, ZeroSpec};
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

/// A window with a real viewport, or `None` without a `wgpu` adapter.
fn gpu_harness() -> Option<Harness<'static, State>> {
    crate::viewport::render::headless()?;
    // Primary backends only, as `render::headless` explains.
    let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = wgpu::Backends::PRIMARY;
    let render = egui_kittest::wgpu::create_render_state(
        eframe::egui_wgpu::WgpuSetup::CreateNew(setup),
        eframe::egui_wgpu::RendererOptions::PREDICTABLE,
    );
    let mut render = Some(render);
    Some(
        Harness::builder()
            .with_size(egui::vec2(1440.0, 900.0))
            .build_ui_state(
                move |ui, app: &mut State| {
                    app.get_or_insert_with(|| {
                        FluutApp::new(ui.ctx(), render.take(), None, Instant::now())
                    })
                    .frame(ui);
                },
                None,
            ),
    )
}

fn press_at(h: &mut Harness<'static, State>, pos: egui::Pos2) {
    h.hover_at(pos);
    h.run_steps(2);
    h.drag_at(pos);
    h.run_steps(1);
    h.drop_at(pos);
    h.run_steps(2);
}

#[test]
fn hovering_and_clicking_in_the_viewport_picks_faces() {
    let Some(mut h) = gpu_harness() else {
        eprintln!("no wgpu adapter: hovering_and_clicking skipped");
        return;
    };
    h.step();
    let id = pocket_on_plate(&mut h);
    h.get_all_by_label("Pick").next().unwrap().click();
    h.run_steps(2);
    let centre = h.get_by_label("3-D viewport").rect().center();
    h.hover_at(centre);
    h.run_steps(3);
    let hover = app(&mut h).ui.pick.as_ref().unwrap().hover.clone();
    let Some(Sel::Face(face)) = hover else {
        panic!("the middle of the view is a face: {hover:?}");
    };
    let r = snapshot(&mut h).revision;
    press_at(&mut h, centre);
    newer(&mut h, r);
    assert_eq!(drive_faces(&mut h, &id), [Sel::Face(face)]);
    assert!(app(&mut h).ui.pick.is_some(), "a click stays in pick mode");
    // A second click takes it out again.
    let r = snapshot(&mut h).revision;
    press_at(&mut h, centre);
    newer(&mut h, r);
    assert!(drive_faces(&mut h, &id).is_empty());
}

#[test]
fn a_curve_field_hovers_edges_in_the_viewport() {
    let Some(mut h) = gpu_harness() else {
        eprintln!("no wgpu adapter: a_curve_field skipped");
        return;
    };
    h.step();
    open(&mut h, project("plate2d"));
    app(&mut h)
        .jobs
        .add_toolpath("s1", ToolpathTypeId::Contour2d);
    settle(&mut h);
    let Selection::Toolpath(id) = app(&mut h).ui.selection.clone() else {
        panic!();
    };
    app(&mut h).ui.pick = Some(PickMode::new(
        ToolpathTypeId::Contour2d,
        &id,
        "geometry.drive_curves",
    ));
    h.run_steps(2);
    let rect = h.get_by_label("3-D viewport").rect();
    // Sweep across the view until the pointer meets an edge.
    let mut x = rect.left() + 10.0;
    let mut found = None;
    while x < rect.right() - 10.0 && found.is_none() {
        h.hover_at(egui::pos2(x, rect.center().y));
        h.run_steps(2);
        found = app(&mut h)
            .ui
            .pick
            .as_ref()
            .unwrap()
            .hover
            .clone()
            .filter(|s| matches!(s, Sel::Edge(_)));
        x += 4.0;
    }
    let edge = found.expect("an edge crosses the middle of the view");
    assert!(edge.id().starts_with("e-588."), "{edge:?}");
}

fn toolpath_ids(h: &mut Harness<'static, State>) -> Vec<String> {
    snapshot(h).project.setups[0]
        .toolpaths
        .iter()
        .map(|t| t.id.clone())
        .collect()
}

/// Opens the context menu of the browser row `label`.
fn row_menu(h: &mut Harness<'static, State>, label: &str) {
    h.get_all_by_label(label).next().unwrap().click_secondary();
    h.run_steps(3);
}

#[test]
fn f2_renames_and_esc_cancels() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    h.get_by_label("Pocket floor raster").click();
    h.run_steps(2);
    h.key_press(egui::Key::F2);
    h.run_steps(3);
    assert!(app(&mut h).ui.renaming.is_some());
    let field = "Pocket floor raster";
    h.query_all_by_role(Role::TextInput)
        .find(|n| n.value().as_deref() == Some(field))
        .unwrap()
        .focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.query_all_by_role(Role::TextInput)
        .find(|n| n.value().as_deref() == Some(field))
        .unwrap()
        .type_text("Floor");
    h.run_steps(2);
    let r = snapshot(&mut h).revision;
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    newer(&mut h, r);
    assert!(app(&mut h).ui.renaming.is_none());
    assert_eq!(
        snapshot(&mut h).project.toolpath("t2").unwrap().1.name,
        "Floor"
    );
    // Esc leaves the name alone.
    h.key_press(egui::Key::F2);
    h.run_steps(3);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(app(&mut h).ui.renaming.is_none());
    assert_eq!(
        snapshot(&mut h).project.toolpath("t2").unwrap().1.name,
        "Floor"
    );
}

#[test]
fn del_removes_the_selection_and_undo_restores_it() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    h.get_by_label("Cut out").click();
    h.run_steps(2);
    let r = snapshot(&mut h).revision;
    h.key_press(egui::Key::Delete);
    h.run_steps(2);
    newer(&mut h, r);
    assert_eq!(toolpath_ids(&mut h), ["t1", "t2"]);
    // The selection moved to something that exists.
    assert_eq!(app(&mut h).ui.selection, Selection::Setup("s1".into()));
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(toolpath_ids(&mut h), ["t1", "t2", "t3"]);
}

#[test]
fn del_in_a_text_field_stays_in_the_field() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    h.run_steps(2);
    h.get_by_role_and_label(Role::TextInput, "Name").focus();
    h.run_steps(2);
    h.key_press(egui::Key::Delete);
    h.run_steps(3);
    assert_eq!(snapshot(&mut h).project.setups.len(), 1);
}

#[test]
fn the_context_menu_moves_duplicates_disables_and_deletes() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    row_menu(&mut h, "Cut out");
    let r = snapshot(&mut h).revision;
    h.get_by_label("Move up").click();
    h.run_steps(2);
    newer(&mut h, r);
    assert_eq!(toolpath_ids(&mut h), ["t1", "t3", "t2"]);
    // Duplicate selects the copy, after the original.
    row_menu(&mut h, "Cut out");
    h.get_by_label("Duplicate").click();
    h.run_steps(2);
    settle(&mut h);
    let ids = toolpath_ids(&mut h);
    assert_eq!(ids.len(), 4);
    assert_eq!(
        app(&mut h).ui.selection,
        Selection::Toolpath(ids[2].clone())
    );
    h.get_all_by_label("Cut out copy").next().unwrap();
    // Disable, then enable again from the same place.
    row_menu(&mut h, "Cut out copy");
    let r = snapshot(&mut h).revision;
    h.get_by_label("Disable").click();
    h.run_steps(2);
    newer(&mut h, r);
    assert!(
        !snapshot(&mut h)
            .project
            .toolpath(&ids[2])
            .unwrap()
            .1
            .is_enabled()
    );
    row_menu(&mut h, "Cut out copy");
    h.get_by_label("Enable");
    let r = snapshot(&mut h).revision;
    h.get_by_label("Delete").click();
    h.run_steps(2);
    newer(&mut h, r);
    assert_eq!(toolpath_ids(&mut h), ["t1", "t3", "t2"]);
}

#[test]
fn deleting_a_parent_setup_is_refused_in_the_status_line() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    app(&mut h).jobs.add_setup(SetupKind::ThreeAxis, Some("s1"));
    settle(&mut h);
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    h.run_steps(2);
    let r = snapshot(&mut h).revision;
    h.key_press(egui::Key::Delete);
    h.run_steps(2);
    newer(&mut h, r);
    assert_eq!(snapshot(&mut h).project.setups.len(), 2);
    assert!(snapshot(&mut h).edit_error.is_some());
}

#[test]
fn dragging_a_row_moves_it() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    h.run_steps(2);
    let from = h.get_by_label("Cut out").rect().center();
    let to = h.get_by_label("Pocket").rect().center();
    let r = snapshot(&mut h).revision;
    h.hover_at(from);
    h.run_steps(2);
    h.drag_at(from);
    h.run_steps(2);
    h.hover_at(from + egui::vec2(0.0, -8.0));
    h.run_steps(2);
    h.hover_at(to);
    h.run_steps(2);
    h.drop_at(to);
    h.run_steps(2);
    newer(&mut h, r);
    assert_eq!(toolpath_ids(&mut h), ["t3", "t1", "t2"]);
}

#[test]
fn a_new_project_asks_for_its_unit() {
    let d = temp("units");
    let mut h = harness();
    h.step();
    scripted(&mut h).answer(DialogKind::New(DisplayUnit::Mm), d.join("inches"));
    h.get_by_label("New project").click();
    settle(&mut h);
    assert!(app(&mut h).jobs.snapshot.is_none(), "nothing yet");
    h.get_by_label("Which unit does this project use?");
    h.get_by_label("Inches").click();
    settle(&mut h);
    assert_eq!(snapshot(&mut h).project.units, DisplayUnit::Inch);
    assert!(
        h.query_by_label("Which unit does this project use?")
            .is_none()
    );
    // Cancel makes nothing.
    scripted(&mut h).answer(DialogKind::New(DisplayUnit::Inch), d.join("other"));
    h.get_by_label("New project").click();
    settle(&mut h);
    h.get_all_by_label("Cancel").last().unwrap().click();
    settle(&mut h);
    assert!(app(&mut h).ui.new_project.is_none());
    assert!(!d.join("other.fluut").exists());
}

#[test]
fn the_unit_badge_changes_the_project_unit_and_undoes() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    let r = snapshot(&mut h).revision;
    h.get_by_role_and_label(Role::Button, "mm").click();
    h.run_steps(2);
    newer(&mut h, r);
    assert_eq!(snapshot(&mut h).project.units, DisplayUnit::Inch);
    h.get_by_role_and_label(Role::Button, "inch");
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(snapshot(&mut h).project.units, DisplayUnit::Mm);
}

/// The project written by hand: what the end-to-end test below builds by
/// clicking.
const REFERENCE: &str = r#"format = "fluut.project/1"
name   = "e2e"
units  = "mm"

[[geometry]]
id   = "g1"
path = "plate2d.step"
kind = "step"

  [[geometry.body]]
  id   = "b-588"
  name = "plate"

[[setup]]
id       = "s1"
name     = "Setup 1"
kind     = "three_axis"
machine  = "fluut/generic-3axis-router@1"
material = "MDF"
stock    = { kind = "block", margin = [7.0, 2, 1] }
zero     = { method = "stock_top_centre" }

  [[setup.toolpath]]
  id   = "t1"
  name = "2D pocket 1"
  type = "pocket_2d"
  tool = "fluut/starter#flat-12"

    [setup.toolpath.geometry]
    drive_faces = [{ face = "f-588.14" }]

    [setup.toolpath.heights]
    retract = { ref = "stock_top", offset = 7.5 }
    top     = { ref = "model_top" }
"#;

/// The offset entry of the `n`th height shown.
fn offset_entry(h: &mut Harness<'static, State>, n: usize, text: &str) {
    let focus = |h: &Harness<'static, State>| {
        h.query_all_by_role_and_label(Role::TextInput, "+")
            .nth(n)
            .unwrap()
            .focus();
    };
    focus(h);
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.query_all_by_role_and_label(Role::TextInput, "+")
        .nth(n)
        .unwrap()
        .type_text(text);
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
}

/// The prototype's whole promise: everything by clicking and picking, no
/// project file edited, and the G-code of a hand-written project. Undo then
/// walks back through every state.
#[test]
fn a_pocket_from_new_project_to_gcode_matches_a_hand_written_project() {
    let d = temp("pick-e2e");
    let mut h = harness();
    h.step();
    app(&mut h).ui.clock = fixed_clock;
    let mut states = Vec::new();
    scripted(&mut h).answer(DialogKind::New(DisplayUnit::Mm), d.join("e2e"));
    h.get_by_label("New project").click();
    settle(&mut h);
    h.get_by_label("Millimetres").click();
    settle(&mut h);
    states.push(snapshot(&mut h).project.clone());
    let step = project("plate2d").with_file_name("plate2d.step");
    scripted(&mut h).answer(DialogKind::Import, step.clone());
    h.get_by_label("Import model").click();
    settle(&mut h);
    states.push(snapshot(&mut h).project.clone());
    stage(&mut h, "SETUP");
    h.get_by_label("New 3-axis Setup").click();
    settle(&mut h);
    states.push(snapshot(&mut h).project.clone());
    // Stock and zero in the Setup editor.
    let r = snapshot(&mut h).revision;
    enter(&mut h, "Margin X, each side", "7");
    newer(&mut h, r);
    states.push(snapshot(&mut h).project.clone());
    let r = snapshot(&mut h).revision;
    choose(&mut h, "Location", "Stock top, centre");
    newer(&mut h, r);
    states.push(snapshot(&mut h).project.clone());
    // A pocket, its floor picked on the model.
    stage(&mut h, "TOOLPATHS");
    h.get_by_label("2D pocket").click();
    settle(&mut h);
    states.push(snapshot(&mut h).project.clone());
    app(&mut h).ui.inspector_tab = 1;
    h.run_steps(3);
    h.get_all_by_label("Pick").next().unwrap().click();
    h.run_steps(2);
    click_entity(&mut h, &Sel::Face("f-588.14".into()));
    states.push(snapshot(&mut h).project.clone());
    h.get_by_label("Done").click();
    h.run_steps(2);
    // The tool.
    app(&mut h).ui.inspector_tab = 0;
    h.run_steps(3);
    let r = snapshot(&mut h).revision;
    choose(
        &mut h,
        "Tool",
        "Starter tools: Flat 12 mm, 12 mm flat end mill",
    );
    newer(&mut h, r);
    states.push(snapshot(&mut h).project.clone());
    // Heights: order is clearance, retract, feed, top, bottom.
    app(&mut h).ui.inspector_tab = 2;
    h.run_steps(3);
    let r = snapshot(&mut h).revision;
    offset_entry(&mut h, 1, "7.5");
    newer(&mut h, r);
    states.push(snapshot(&mut h).project.clone());
    let r = snapshot(&mut h).revision;
    choose(&mut h, "Top", "Model top");
    newer(&mut h, r);
    states.push(snapshot(&mut h).project.clone());
    // Calculate and Post.
    h.get_by_label("Calculate all").click();
    settle(&mut h);
    let s = snapshot(&mut h);
    assert!(s.paths.contains_key("t1"), "{:?}", s.diagnostics("t1"));
    stage(&mut h, "POST");
    let out = d.join("e2e.ngc");
    scripted(&mut h).answer(DialogKind::PostOutput, out.clone());
    button(&h, "Post").click();
    settle(&mut h);
    let got = std::fs::read_to_string(&out).unwrap();
    // The same project by hand.
    let by_hand = d.join("reference");
    std::fs::create_dir_all(&by_hand).unwrap();
    std::fs::copy(&step, by_hand.join("plate2d.step")).unwrap();
    let file = by_hand.join("e2e.fluut");
    std::fs::write(&file, REFERENCE).unwrap();
    let mut j = Jobs::start(|| {});
    j.open(file);
    j.wait();
    let ref_out = by_hand.join("e2e.ngc");
    let (date, time) = fixed_clock();
    j.post(crate::jobs::PostRequest {
        post: "fluut/linuxcnc".into(),
        output: ref_out.clone(),
        date,
        time,
    });
    assert!(
        matches!(j.wait(), Some(Outcome::Posted { .. })),
        "{:?}",
        j.last
    );
    let want = std::fs::read_to_string(&ref_out).unwrap();
    assert!(got.lines().count() > 20, "{got}");
    assert_eq!(got, want);
    // Undo returns to each state before it.
    for want in states.iter().rev().skip(1) {
        h.get_by_label("Undo").click();
        settle(&mut h);
        assert_eq!(snapshot(&mut h).project, *want);
    }
}

/// A selection holds persistent ids: re-import finds them again.
#[test]
fn reimport_keeps_a_picked_selection() {
    let d = temp("pick-reimport");
    let mut h = harness();
    h.step();
    app(&mut h).jobs.new_project(d.join("re"), DisplayUnit::Mm);
    settle(&mut h);
    let step = project("plate2d").with_file_name("plate2d.step");
    app(&mut h).jobs.import_model(step);
    settle(&mut h);
    app(&mut h).jobs.add_setup(SetupKind::ThreeAxis, None);
    settle(&mut h);
    app(&mut h)
        .jobs
        .add_toolpath("s1", ToolpathTypeId::Pocket2d);
    settle(&mut h);
    let Selection::Toolpath(id) = app(&mut h).ui.selection.clone() else {
        panic!();
    };
    let path = "geometry.drive_faces";
    app(&mut h).ui.pick = Some(PickMode::new(ToolpathTypeId::Pocket2d, &id, path));
    click_entity(&mut h, &Sel::Face("f-588.14".into()));
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
    assert_eq!(drive_faces(&mut h, &id), [Sel::Face("f-588.14".into())]);
}

/// Curves of a drawing: an edge, its closed loop offered and taken, and a
/// contour from it.
#[test]
fn a_contour_of_wire_curves_from_a_picked_loop() {
    let mut h = harness();
    h.step();
    open(&mut h, project("plate"));
    let setup = snapshot(&mut h).project.setups[0].id.clone();
    app(&mut h)
        .jobs
        .add_toolpath(&setup, ToolpathTypeId::Contour2d);
    settle(&mut h);
    let Selection::Toolpath(id) = app(&mut h).ui.selection.clone() else {
        panic!();
    };
    let path = "geometry.drive_curves";
    let s = snapshot(&mut h);
    let scene = s.scenes[&setup].clone();
    let first = scene.edge_loops[0][0] as usize;
    app(&mut h).ui.pick = Some(PickMode::new(ToolpathTypeId::Contour2d, &id, path));
    click_entity(&mut h, &Sel::Edge(scene.edge_ids[first].clone()));
    let s = snapshot(&mut h);
    let r = s.revision;
    let a = app(&mut h);
    let e = a.ui.pick.as_mut().unwrap().accept_offer(&s).unwrap();
    a.jobs.edit(e);
    newer(&mut h, r);
    let n = crate::vm::pick::selected(&snapshot(&mut h), &id, path).len();
    assert_eq!(n, scene.edge_loops[0].len());
    app(&mut h).ui.pick = None;
    app(&mut h)
        .jobs
        .calculate(CalcTargets::Toolpaths(vec![id.clone()]));
    settle(&mut h);
    let s = snapshot(&mut h);
    assert!(s.paths.contains_key(&id), "{:?}", s.diagnostics(&id));
}
