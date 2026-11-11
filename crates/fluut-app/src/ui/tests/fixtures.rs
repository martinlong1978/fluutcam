// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Fixtures group of the Setup inspector and the index angle dial, in
//! the window.

use super::editing::{choose, enter, gpu_harness, newer};
use super::setup_edit;
use super::*;
use crate::vm::handles::dial;
use crate::vm::handles::{HandleKind, handles};
use fluut_model::project::{AngleMode, Fixture};

/// `flipside` with `setup` selected and its Fixtures group in view (the
/// inspector scrolls to a group that is pointed at).
fn flipside(h: &mut Harness<'static, State>, setup: &str) {
    open(h, project("flipside"));
    stage(h, "SETUP");
    select(h, setup);
}

/// Chooses `kind` in the Add list and waits for the edit.
fn add(h: &mut Harness<'static, State>, kind: &str) {
    let r = snapshot(h).revision;
    choose(h, "Add", kind);
    newer(h, r);
}

/// Clicks the button `label` (the first one) and waits for the edit.
fn press(h: &mut Harness<'static, State>, label: &str) {
    let r = snapshot(h).revision;
    h.get_all_by_label(label).next().unwrap().click();
    newer(h, r);
}

fn select(h: &mut Harness<'static, State>, setup: &str) {
    app(h).ui.selection = Selection::Setup(setup.into());
    app(h).ui.inspector_focus = Some("setup-fixtures");
    h.run_steps(4);
}

fn fixtures(h: &mut Harness<'static, State>, setup: &str) -> Option<Vec<Fixture>> {
    snapshot(h).project.setup(setup).unwrap().fixtures.clone()
}

fn index_angle(h: &mut Harness<'static, State>, setup: &str) -> (AngleMode, f64) {
    let a = snapshot(h)
        .project
        .setup(setup)
        .unwrap()
        .index_angle
        .clone()
        .unwrap();
    (a.mode, a.value.v)
}

#[test]
fn fixtures_are_added_edited_removed_and_undone_in_the_inspector() {
    let mut h = harness();
    h.step();
    flipside(&mut h, "s1");
    h.get_by_label("Fixtures");
    h.get_by_label("None");
    // Add a chuck by choosing it in the Add list.
    let r = snapshot(&mut h).revision;
    choose(&mut h, "Add", "Chuck");
    newer(&mut h, r);
    assert!(matches!(
        fixtures(&mut h, "s1").unwrap()[..],
        [Fixture::Chuck { .. }]
    ));
    h.get_by_label("Chuck 1");
    h.get_by_label("1 fixture");
    // A size is typed and checked as the project file checks it.
    let r = snapshot(&mut h).revision;
    enter(&mut h, "Diameter", "75");
    newer(&mut h, r);
    let Fixture::Chuck { diameter, .. } = fixtures(&mut h, "s1").unwrap()[0].clone() else {
        panic!()
    };
    assert_eq!(diameter.v, 75.0);
    let r = snapshot(&mut h).revision;
    enter(&mut h, "Length", "0");
    h.run_steps(3);
    h.get_by_label("Must be above 0.");
    assert_eq!(snapshot(&mut h).revision, r);
    // A clamp has two corners of three numbers; a maximum below its
    // minimum is refused.
    add(&mut h, "Clamp");
    h.get_by_label("Clamp 2");
    enter(&mut h, "Max Y", "1");
    h.run_steps(3);
    h.get_by_label("Each maximum must be above its minimum.");
    // Remove the chuck with its button; then everything.
    press(&mut h, "Remove");
    assert!(matches!(
        fixtures(&mut h, "s1").unwrap()[..],
        [Fixture::Clamp { .. }]
    ));
    press(&mut h, "Remove all fixtures");
    assert_eq!(fixtures(&mut h, "s1"), None);
    // Each edit is one undo step: five get back to the start.
    for _ in 0..5 {
        press(&mut h, "Undo");
    }
    assert_eq!(fixtures(&mut h, "s1"), None);
    h.get_by_label("None");
}

#[test]
fn a_based_on_setup_shows_the_inherited_fixtures_and_can_replace_or_reset_them() {
    let mut h = harness();
    h.step();
    flipside(&mut h, "s1");
    add(&mut h, "Tailstock");
    select(&mut h, "s2");
    h.get_all_by_label("Inherited from Top").next().unwrap();
    h.get_by_label("Tailstock 1");
    assert_eq!(fixtures(&mut h, "s2"), None);
    // Replace the inherited list: a local copy, then a field of it.
    press(&mut h, "Make a local copy");
    assert_eq!(fixtures(&mut h, "s2"), fixtures(&mut h, "s1"));
    assert!(h.query_by_label("Make a local copy").is_none());
    let r = snapshot(&mut h).revision;
    enter(&mut h, "Length", "12");
    newer(&mut h, r);
    let Fixture::Tailstock { length, .. } = fixtures(&mut h, "s2").unwrap()[0].clone() else {
        panic!()
    };
    assert_eq!(length.v, 12.0);
    // Reset to inherited drops it again; the parent never changed.
    let r = snapshot(&mut h).revision;
    h.get_all_by_label("Reset to inherited")
        .last()
        .unwrap()
        .click();
    newer(&mut h, r);
    assert_eq!(fixtures(&mut h, "s2"), None);
    h.get_all_by_label("Inherited from Top").next().unwrap();
    assert_eq!(fixtures(&mut h, "s1").unwrap().len(), 1);
}

#[test]
fn a_three_axis_setup_has_no_fixtures_in_the_inspector() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    stage(&mut h, "SETUP");
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    h.run_steps(3);
    assert!(h.query_by_label("Fixtures").is_none());
    assert!(h.query_by_label("Add a fixture...").is_none());
}

fn dial_ring(h: &mut Harness<'static, State>, setup: &str) -> dial::Ring {
    dial::ring(&snapshot(h), setup).expect("an indexed Setup has a ring")
}

#[test]
fn dragging_the_dial_pointer_sets_the_index_angle_in_one_step() {
    let Some(mut h) = gpu_harness() else {
        eprintln!("no wgpu adapter: the dial drag is skipped");
        return;
    };
    h.step();
    open(&mut h, project("flipside"));
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    h.run_steps(4);
    let ring = dial_ring(&mut h, "s1");
    let pointer = setup_edit::mark(&mut h, HandleKind::Dial);
    let at0 = setup_edit::on_screen(&mut h, ring.point(0.0));
    assert!((pointer - at0).length() < 1.0, "{pointer:?} {at0:?}");
    // To 90 degrees: one edit, which undoes.
    let r = snapshot(&mut h).revision;
    let to = setup_edit::on_screen(&mut h, ring.point(90.0));
    setup_edit::drag(&mut h, pointer, to);
    newer(&mut h, r);
    assert_eq!(index_angle(&mut h, "s1"), (AngleMode::Absolute, 90.0));
    // The pointer is at the new angle.
    let moved = setup_edit::mark(&mut h, HandleKind::Dial);
    let at90 = setup_edit::on_screen(&mut h, ring.point(90.0));
    assert!((moved - at90).length() < 1.5, "{moved:?} {at90:?}");
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(index_angle(&mut h, "s1"), (AngleMode::Absolute, 0.0));
    // A drag to 47 degrees takes the 5 degree step below: 45.
    let r = snapshot(&mut h).revision;
    let to = setup_edit::on_screen(&mut h, ring.point(47.0));
    setup_edit::drag(&mut h, pointer, to);
    newer(&mut h, r);
    assert_eq!(index_angle(&mut h, "s1"), (AngleMode::Absolute, 45.0));
    // Past half way round it carries on: 200 degrees, not -160.
    let r = snapshot(&mut h).revision;
    let from = setup_edit::mark(&mut h, HandleKind::Dial);
    let to = setup_edit::on_screen(&mut h, ring.point(200.0));
    setup_edit::drag(&mut h, from, to);
    newer(&mut h, r);
    assert_eq!(index_angle(&mut h, "s1"), (AngleMode::Absolute, 200.0));
    // The handle exists only for this Setup's own selection.
    assert!(
        handles(&snapshot(&mut h), "s1")
            .iter()
            .any(|k| k.kind == HandleKind::Dial)
    );
}

#[test]
fn a_dial_drag_to_where_it_already_is_changes_nothing() {
    let Some(mut h) = gpu_harness() else {
        eprintln!("no wgpu adapter: the dial drag is skipped");
        return;
    };
    h.step();
    open(&mut h, project("flipside"));
    app(&mut h).ui.selection = Selection::Setup("s2".into());
    h.run_steps(4);
    let ring = dial_ring(&mut h, "s2");
    let from = setup_edit::mark(&mut h, HandleKind::Dial);
    let r = snapshot(&mut h).revision;
    let near = setup_edit::on_screen(&mut h, ring.point(181.0));
    setup_edit::drag(&mut h, from, near);
    h.run_steps(4);
    assert_eq!(snapshot(&mut h).revision, r);
    assert_eq!(index_angle(&mut h, "s2"), (AngleMode::Absolute, 180.0));
}

#[test]
fn a_ring_seen_edge_on_still_follows_the_pointer() {
    let Some(mut h) = gpu_harness() else {
        eprintln!("no wgpu adapter: the dial drag is skipped");
        return;
    };
    h.step();
    open(&mut h, project("flipside"));
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    h.run_steps(4);
    // Looking along +Y at the level: the plane of the ring is a line.
    {
        let cam = &mut app(&mut h).view.camera;
        cam.yaw = std::f32::consts::FRAC_PI_2;
        cam.pitch = 0.0;
    }
    h.run_steps(3);
    let ring = dial_ring(&mut h, "s1");
    let from = setup_edit::mark(&mut h, HandleKind::Dial);
    let r = snapshot(&mut h).revision;
    // The pointer goes down the line to the level of 60 degrees.
    let to = setup_edit::on_screen(&mut h, ring.point(60.0));
    setup_edit::drag(&mut h, from, to);
    newer(&mut h, r);
    let (_, v) = index_angle(&mut h, "s1");
    // 60 or its mirror, 300, and always a step of 5.
    assert!(
        (v - 60.0).abs() < 10.0 || (v - 300.0).abs() < 10.0,
        "the angle is {v}"
    );
    assert!(v % 5.0 == 0.0, "{v}");
}

#[test]
fn fixtures_reach_the_viewport_without_a_problem() {
    let Some(mut h) = gpu_harness() else {
        eprintln!("no wgpu adapter: the fixture drawing is skipped");
        return;
    };
    h.step();
    open(&mut h, project("flipside"));
    select(&mut h, "s1");
    let before = snapshot(&mut h).scenes["s1"].clone();
    for kind in ["Chuck", "Tailstock", "Clamp"] {
        add(&mut h, kind);
    }
    h.run_steps(4);
    let after = snapshot(&mut h).scenes["s1"].clone();
    assert!(after.edges.points.len() > before.edges.points.len());
}
