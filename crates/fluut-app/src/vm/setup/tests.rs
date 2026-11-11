// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::jobs::{Edit as JobEdit, Jobs};
use crate::session::tests::opened;
use crate::vm::Selection;
use crate::vm::inspector::InspectorVm;
use fluut_model::Sel;
use std::sync::Arc;

fn vm(s: &Snapshot, id: &str) -> SetupVm {
    match InspectorVm::new(Some(s), &Selection::Setup(id.into())) {
        InspectorVm::Setup(v) => v,
        other => panic!("{other:?}"),
    }
}

fn apply(j: &mut Jobs, key: &str, text: &str) -> Arc<Snapshot> {
    let s = j.snapshot.clone().unwrap();
    j.edit(JobEdit::Model(setup_edit(&s, "s1", key, text).unwrap()));
    j.wait_snapshot().unwrap()
}

fn undo(j: &mut Jobs) -> Arc<Snapshot> {
    j.undo();
    j.wait();
    j.snapshot.clone().unwrap()
}

#[test]
fn rows_show_the_setup_in_the_project_unit() {
    let j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let v = vm(&s, "s1");
    assert_eq!(v.title, "Top");
    assert_eq!(v.row("name").unwrap().value, "Top");
    let RowWidget::Choice(kinds) = &v.row("kind").unwrap().widget else {
        panic!()
    };
    assert_eq!(kinds.len(), 3);
    assert_eq!(v.row("kind").unwrap().value, "three_axis");
    assert_eq!(v.row("stock.kind").unwrap().value, "margin");
    assert_eq!(v.row("stock.margin.0").unwrap().value, "5");
    assert_eq!(v.row("zero.choice").unwrap().value, "top.front_left");
    assert_eq!(v.row("clearance.offset").unwrap().value, "10");
    assert!(v.row("clearance.radius").is_none());
    let ids: Vec<&str> = v.groups.iter().map(|g| g.id).collect();
    assert_eq!(
        ids,
        [
            "setup-general",
            "setup-bodies",
            "setup-orientation",
            "setup-stock",
            "setup-zero",
            "setup-clearance",
            "setup-geometry"
        ]
    );
}

#[test]
fn the_machine_list_comes_from_the_library_and_follows_the_kind() {
    let j = opened("block");
    let s = j.snapshot.clone().unwrap();
    assert!(s.machines.iter().any(|m| !m.rotary));
    let v = vm(&s, "s1");
    let RowWidget::Choice(o) = &v.row("machine").unwrap().widget else {
        panic!()
    };
    assert!(
        o.iter()
            .any(|(v, l)| v == "fluut/generic-3axis-router@1" && l == "Generic 3-axis router"),
        "{o:?}"
    );
    let j = opened("shaft");
    let s = j.snapshot.clone().unwrap();
    let id = s.project.setups[0].id.clone();
    let v = vm(&s, &id);
    let RowWidget::Choice(o) = &v.row("machine").unwrap().widget else {
        panic!()
    };
    assert!(o.iter().all(|(v, _)| !v.contains("3axis")), "{o:?}");
}

#[test]
fn stock_margin_changes_the_scene_and_undoes() {
    let mut j = opened("block");
    let before = j.snapshot.clone().unwrap();
    let b0 = before.scenes["s1"].stock.unwrap();
    let s = apply(&mut j, "stock.margin.0", "12");
    let b1 = s.scenes["s1"].stock.unwrap();
    assert!(((b1.max[0] - b1.min[0]) - (b0.max[0] - b0.min[0]) - 14.0).abs() < 1e-3);
    assert_eq!(vm(&s, "s1").row("stock.margin.0").unwrap().value, "12");
    assert_eq!(vm(&s, "s1").row("stock.margin.1").unwrap().value, "5");
    let s = undo(&mut j);
    assert_eq!(s.project, before.project);
    assert_eq!(s.scenes["s1"].stock.unwrap(), b0);
}

#[test]
fn switching_to_a_size_keeps_the_stock_box() {
    let mut j = opened("block");
    let b0 = j.snapshot.as_ref().unwrap().scenes["s1"].stock.unwrap();
    let s = apply(&mut j, "stock.kind", "size");
    let v = vm(&s, "s1");
    assert_eq!(v.row("stock.kind").unwrap().value, "size");
    let b1 = s.scenes["s1"].stock.unwrap();
    for i in 0..3 {
        assert!(((b1.max[i] - b1.min[i]) - (b0.max[i] - b0.min[i])).abs() < 1e-2);
    }
    let s = apply(&mut j, "stock.size.2", "40");
    let b = s.scenes["s1"].stock.unwrap();
    assert!((b.max[2] - b.min[2] - 40.0).abs() < 1e-3);
}

#[test]
fn work_zero_moves_the_scene() {
    let mut j = opened("block");
    let s = apply(&mut j, "zero.choice", "top_centre");
    let b = s.scenes["s1"].stock.unwrap();
    assert!((b.max[0] + b.min[0]).abs() < 1e-3, "{b:?}");
    assert!(b.max[2].abs() < 1e-3);
    let s = apply(&mut j, "zero.choice", "bottom.back_right");
    let b = s.scenes["s1"].stock.unwrap();
    assert!(b.max[0].abs() < 1e-3 && b.max[1].abs() < 1e-3 && b.min[2].abs() < 1e-3);
    let s = apply(&mut j, "zero.choice", "point");
    assert_eq!(vm(&s, "s1").row("zero.point.1").unwrap().value, "0");
    let s = apply(&mut j, "zero.point.1", "7");
    assert_eq!(vm(&s, "s1").row("zero.point.1").unwrap().value, "7");
}

#[test]
fn entries_take_expressions_and_refuse_nonsense() {
    let mut j = opened("block");
    let s = apply(&mut j, "clearance.offset", "1/4in");
    assert_eq!(vm(&s, "s1").row("clearance.offset").unwrap().value, "6.35");
    assert!(setup_edit(&s, "s1", "stock.margin.0", "wide").is_err());
    assert!(setup_edit(&s, "s1", "stock.margin.0", "-1").is_err());
    assert!(setup_edit(&s, "s1", "name", "  ").is_err());
    assert!(setup_edit(&s, "s1", "no.such", "1").is_err());
}

#[test]
fn name_machine_material_and_offset() {
    let mut j = opened("block");
    apply(&mut j, "name", "Flip");
    apply(&mut j, "machine", "fluut/generic-3axis-router@1");
    apply(&mut j, "work_offset", "G55");
    let s = apply(&mut j, "material", "");
    let st = s.project.setup("s1").unwrap();
    assert_eq!(st.name, "Flip");
    assert_eq!(st.machine.as_deref(), Some("fluut/generic-3axis-router@1"));
    assert_eq!(st.work_offset.as_deref(), Some("G55"));
    assert_eq!(st.material, None);
    // Each was one undo step.
    let s = undo(&mut j);
    assert_eq!(
        s.project.setup("s1").unwrap().material.as_deref(),
        Some("MDF")
    );
    let s = undo(&mut j);
    assert_eq!(s.project.setup("s1").unwrap().work_offset, None);
}

#[test]
fn a_child_shows_what_it_inherits_and_resets_it() {
    let mut j = opened("block");
    j.add_setup(SetupKind::ThreeAxis, Some("s1"));
    j.wait();
    let s = j.snapshot.clone().unwrap();
    let id = s.project.setups[1].id.clone();
    let v = vm(&s, &id);
    let r = v.row("zero.choice").unwrap();
    assert_eq!(r.origin.inherited.as_deref(), Some("Inherited from Top"));
    assert_eq!(r.origin.reset, None);
    // `from_previous` is offered to a child, and is what it starts with.
    assert_eq!(v.row("stock.kind").unwrap().value, "from_previous");
    let e = setup_edit(&s, &id, "zero.choice", "top_centre").unwrap();
    j.edit(JobEdit::Model(e));
    let s = j.wait_snapshot().unwrap();
    let v = vm(&s, &id);
    let r = v.row("zero.choice").unwrap();
    assert_eq!(r.origin.inherited, None);
    assert_eq!(r.origin.reset, Some("zero"));
    j.edit(JobEdit::Model(reset_edit(&id, "zero")));
    let s = j.wait_snapshot().unwrap();
    assert!(s.project.setup(&id).unwrap().zero.is_none());
}

#[test]
fn rotary_setups_offer_cylinder_and_axis_zeros() {
    let mut j = opened("shaft");
    let s = j.snapshot.clone().unwrap();
    let id = s.project.setups[0].id.clone();
    let v = vm(&s, &id);
    assert_eq!(v.row("stock.kind").unwrap().value, "cylinder");
    assert!(v.row("clearance.radius").is_some());
    assert!(v.row("clearance.offset").is_none());
    let RowWidget::Choice(o) = &v.row("stock.kind").unwrap().widget else {
        panic!()
    };
    assert!(o.iter().all(|(k, _)| k != "margin" && k != "size"), "{o:?}");
    let e = setup_edit(&s, &id, "zero.choice", "at").unwrap();
    j.edit(JobEdit::Model(e));
    let s = j.wait_snapshot().unwrap();
    assert!(vm(&s, &id).row("zero.at").is_some());
    let e = setup_edit(&s, &id, "stock.radius", "30").unwrap();
    j.edit(JobEdit::Model(e));
    let s = j.wait_snapshot().unwrap();
    assert_eq!(vm(&s, &id).row("stock.radius").unwrap().value, "30");
}

#[test]
fn a_refused_edit_shows_in_the_status_line() {
    let mut j = opened("block");
    j.edit(JobEdit::Model(Edit::set_setup_field(
        "nope",
        "name",
        Some("\"x\""),
    )));
    let s = j.wait_snapshot().unwrap();
    assert!(s.edit_error.is_some());
    let st = crate::vm::status::StatusVm::new(&j);
    assert!(st.failed && st.text.starts_with("Not changed:"), "{st:?}");
}

#[test]
fn a_kind_change_within_the_rotary_class_writes_only_the_kind() {
    let mut j = opened("shaft");
    let id = j.snapshot.as_ref().unwrap().project.setups[0].id.clone();
    let before = j.snapshot.clone().unwrap();
    let e = setup_edit(&before, &id, "kind", "indexed").unwrap();
    assert!(matches!(e, Edit::SetSetupField { .. }), "{e:?}");
    j.edit(JobEdit::Model(e));
    let s = j.wait_snapshot().unwrap();
    let st = s.project.setup(&id).unwrap();
    assert_eq!(st.kind, Some(SetupKind::Indexed));
    assert_eq!(st.stock, before.project.setup(&id).unwrap().stock);
    assert!(vm(&s, &id).row("index_angle").is_some());
    assert_eq!(undo(&mut j).project, before.project);
}

#[test]
fn a_kind_change_across_classes_is_one_undo_step_and_keeps_the_scene() {
    let mut j = opened("block");
    let before = j.snapshot.clone().unwrap();
    let s = apply(&mut j, "kind", "rotary");
    let st = s.project.setup("s1").unwrap();
    assert_eq!(st.kind, Some(SetupKind::Rotary));
    assert!(
        matches!(st.stock, Some(StockSpec::Cylinder { .. })),
        "{:?}",
        st.stock
    );
    assert!(matches!(st.zero, Some(ZeroSpec::OnAxis { .. })));
    let v = vm(&s, "s1");
    assert_eq!(v.row("stock.kind").unwrap().value, "cylinder");
    assert!(v.row("zero.choice").is_some() && v.row("clearance.radius").is_some());
    // The pocket is not a rotary type: it keeps its place and says why.
    assert!(
        s.diagnostics["t1"].iter().any(|d| d.is_error()),
        "{:?}",
        s.diagnostics["t1"]
    );
    // And back: a block with the default margin, then undo twice.
    let s = apply(&mut j, "kind", "three_axis");
    assert!(matches!(
        s.project.setup("s1").unwrap().stock,
        Some(StockSpec::Block {
            margin: Some(_),
            ..
        })
    ));
    assert!(!s.diagnostics["t1"].iter().any(|d| d.is_error()));
    undo(&mut j);
    let s = undo(&mut j);
    assert_eq!(s.project, before.project);
}

#[test]
fn a_rotary_kind_is_refused_for_a_machine_without_a_rotary_axis() {
    let mut j = opened("block");
    apply(&mut j, "machine", "fluut/generic-3axis-router@1");
    let s = j.snapshot.clone().unwrap();
    let e = setup_edit(&s, "s1", "kind", "rotary").unwrap_err();
    assert!(
        e.contains("Generic 3-axis router") && e.contains("rotary"),
        "{e}"
    );
    assert!(setup_edit(&s, "s1", "kind", "nonsense").is_err());
}

fn bounds_of(s: &Snapshot, id: &str) -> crate::scene::Bounds {
    s.scenes[id].stock.unwrap()
}

#[test]
fn bodies_are_checked_one_by_one_and_undone() {
    let mut j = opened("plate");
    let s = j.snapshot.clone().unwrap();
    let v = vm(&s, "s1");
    assert_eq!(v.row("bodies.all").unwrap().value, "true");
    assert_eq!(v.row("bodies.all").unwrap().pick, Some(SetupPick::Bodies));
    assert_eq!(v.row("bodies.b-2").unwrap().label, "POCKET (b-2)");
    let all_edges = s.scenes["s1"].edge_ids.len();
    let s = apply(&mut j, "bodies.b-2", "false");
    let g = s.project.setup("s1").unwrap().geometry.clone().unwrap();
    assert_eq!(g.bodies, Some(vec!["b-1".to_owned()]));
    assert!(s.scenes["s1"].edge_ids.len() < all_edges);
    let v = vm(&s, "s1");
    assert_eq!(v.row("bodies.all").unwrap().value, "false");
    assert_eq!(v.row("bodies.b-2").unwrap().value, "false");
    // The last body stays.
    let e = setup_edit(&s, "s1", "bodies.b-1", "false").unwrap_err();
    assert!(e.contains("one body"), "{e}");
    // Checking it again is "all bodies" again, with no geometry key.
    let s = apply(&mut j, "bodies.b-2", "true");
    assert!(s.project.setup("s1").unwrap().geometry.is_none());
    assert_eq!(s.scenes["s1"].edge_ids.len(), all_edges);
    undo(&mut j);
    let s = undo(&mut j);
    assert!(s.project.setup("s1").unwrap().geometry.is_none());
    // All bodies written out, and back to all.
    let s = apply(&mut j, "bodies.all", "false");
    assert_eq!(
        s.project
            .setup("s1")
            .unwrap()
            .geometry
            .as_ref()
            .unwrap()
            .bodies,
        Some(vec!["b-1".to_owned(), "b-2".to_owned()])
    );
    let s = apply(&mut j, "bodies.all", "true");
    assert!(s.project.setup("s1").unwrap().geometry.is_none());
}

#[test]
fn stock_from_a_body_is_the_bounds_of_a_body_the_setup_does_not_machine() {
    let mut j = opened("plate");
    apply(&mut j, "bodies.b-2", "false");
    let s = apply(&mut j, "stock.kind", "body");
    assert_eq!(
        s.project.setup("s1").unwrap().stock,
        Some(StockSpec::Body { body: "b-2".into() })
    );
    let v = vm(&s, "s1");
    assert_eq!(v.row("stock.kind").unwrap().value, "body");
    assert_eq!(v.row("stock.body").unwrap().value, "b-2");
    assert_eq!(
        v.row("stock.body").unwrap().pick,
        Some(SetupPick::StockBody)
    );
    // The scene still holds the machined body, and the stock is the other's box.
    assert!(!s.scenes["s1"].edge_ids.is_empty());
    assert!(
        s.scenes["s1"]
            .edge_ids
            .iter()
            .all(|e| e.starts_with("e-1."))
    );
    let pocket = s.all_bodies.as_ref().unwrap();
    let b = bounds_of(&s, "s1");
    assert!(b.max[0] - b.min[0] > 0.0 && pocket.bounds.is_some());
    // The stock body is never machined, even when the Setup takes all
    // bodies; choosing the other body in the dropdown moves the stock.
    let s = apply(&mut j, "bodies.all", "true");
    assert!(
        s.scenes["s1"]
            .edge_ids
            .iter()
            .all(|e| e.starts_with("e-1."))
    );
    let e = setup_edit(&s, "s1", "stock.body", "b-1").unwrap();
    j.edit(JobEdit::Model(e));
    let s2 = j.wait_snapshot().unwrap();
    assert_eq!(
        s2.project.setup("s1").unwrap().stock,
        Some(StockSpec::Body { body: "b-1".into() })
    );
    assert_ne!(bounds_of(&s2, "s1"), b);
    assert!(
        s2.scenes["s1"]
            .edge_ids
            .iter()
            .all(|e| e.starts_with("e-2."))
    );
    undo(&mut j);
    undo(&mut j);
    undo(&mut j);
    let s = undo(&mut j);
    assert!(matches!(
        s.project.setup("s1").unwrap().stock,
        Some(StockSpec::Block { .. })
    ));
}

#[test]
fn the_orientation_follows_the_axes_and_the_scene_turns_with_it() {
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let v = vm(&s, "s1");
    assert_eq!(v.row("orientation.tool_axis").unwrap().value, "+Z");
    assert_eq!(v.row("orientation.x_axis").unwrap().value, "+X");
    let b0 = bounds_of(&s, "s1");
    // X of the work frame along the model's Y: width and depth swap.
    let s = apply(&mut j, "orientation.x_axis", "+Y");
    assert_eq!(vm(&s, "s1").row("orientation.x_axis").unwrap().value, "+Y");
    let b1 = bounds_of(&s, "s1");
    assert!(((b1.max[0] - b1.min[0]) - (b0.max[1] - b0.min[1])).abs() < 1e-2);
    assert!(((b1.max[1] - b1.min[1]) - (b0.max[0] - b0.min[0])).abs() < 1e-2);
    // Turning the part over: the tool axis is the model's -Z.
    let s = apply(&mut j, "orientation.tool_axis", "-Z");
    assert_eq!(
        vm(&s, "s1").row("orientation.tool_axis").unwrap().value,
        "-Z"
    );
    assert!(
        s.diagnostics["t1"]
            .iter()
            .all(|d| !d.is_error() || d.code != "setup.orientation.pick")
    );
    // Two parallel directions are refused by the frame and say so.
    let s = apply(&mut j, "orientation.x_axis", "-Z");
    assert!(
        vm(&s, "s1")
            .diagnostics
            .iter()
            .any(|d| d.text.contains("orientation"))
            || s.scenes["s1"].faces.is_empty(),
        "{:?}",
        vm(&s, "s1").diagnostics
    );
    for _ in 0..3 {
        undo(&mut j);
    }
    assert_eq!(bounds_of(&j.snapshot.clone().unwrap(), "s1"), b0);
    assert!(setup_edit(&s, "s1", "orientation.tool_axis", "up").is_err());
    assert!(setup_edit(&s, "s1", "orientation.rotary_axis", "+X").is_err());
}

#[test]
fn a_picked_face_is_an_orientation_direction() {
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let floor = Sel::Face("f-494.10".into());
    let e = setup_pick_edit(&s, "s1", SetupPick::ToolAxis, &floor).unwrap();
    j.edit(JobEdit::Model(e));
    let s = j.wait_snapshot().unwrap();
    let v = vm(&s, "s1");
    let r = v.row("orientation.tool_axis").unwrap();
    assert_eq!(r.value, "face:f-494.10");
    let RowWidget::Choice(o) = &r.widget else {
        panic!()
    };
    assert!(
        o.iter()
            .any(|(v, l)| v == "face:f-494.10" && l == "Face f-494.10")
    );
    assert!(!s.scenes["s1"].faces.is_empty());
    // A body is no direction.
    assert!(setup_pick_edit(&s, "s1", SetupPick::XAxis, &Sel::Body("b-494".into())).is_err());
    let s = undo(&mut j);
    assert_eq!(
        vm(&s, "s1").row("orientation.tool_axis").unwrap().value,
        "+Z"
    );
}

#[test]
fn a_rotary_setup_orients_by_its_rotary_axis_and_the_a_zero_direction() {
    let mut j = opened("shaft");
    let id = j.snapshot.as_ref().unwrap().project.setups[0].id.clone();
    let s = j.snapshot.clone().unwrap();
    let v = vm(&s, &id);
    assert_eq!(
        v.row("orientation.rotary_axis").unwrap().value,
        "face:f-354.0"
    );
    assert_eq!(v.row("orientation.a_zero_toward").unwrap().value, "+X");
    assert!(v.row("orientation.tool_axis").is_none());
    let e = setup_edit(&s, &id, "orientation.a_zero_toward", "").unwrap();
    j.edit(JobEdit::Model(e));
    let s = j.wait_snapshot().unwrap();
    assert_eq!(
        vm(&s, &id).row("orientation.a_zero_toward").unwrap().value,
        ""
    );
    let e = setup_edit(&s, &id, "orientation.a_zero_toward", "-Y").unwrap();
    j.edit(JobEdit::Model(e));
    let s = j.wait_snapshot().unwrap();
    assert_eq!(
        vm(&s, &id).row("orientation.a_zero_toward").unwrap().value,
        "-Y"
    );
}

#[test]
fn scale_and_mirror_rows_show_the_model_transform_and_edit_it() {
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let v = vm(&s, "s1");
    let g = v.groups.iter().find(|g| g.id == "setup-geometry").unwrap();
    assert_eq!(g.rows.len(), 4, "scale and three mirrors");
    assert!(v.row("scale.0").is_none());
    assert_eq!(v.row("scale.all").unwrap().value, "1");
    assert_eq!(v.row("mirror.2").unwrap().value, "false");
    assert_eq!(v.row("mirror.0").unwrap().widget, RowWidget::Check);
    let b0 = bounds_of(&s, "s1");
    // A scale of 2 on every axis is one edit and doubles the model.
    let s = apply(&mut j, "scale.all", "2");
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    let k = s.project.setup("s1").unwrap().geometry.clone().unwrap();
    assert_eq!(k.scale.map(|k| k.map(|n| n.v)), Some([2.0; 3]));
    assert_eq!(k.mirror, None);
    let v = vm(&s, "s1");
    assert_eq!(v.row("scale.all").unwrap().value, "2");
    let b1 = bounds_of(&s, "s1");
    assert!(b1.max[0] - b1.min[0] > 1.5 * (b0.max[0] - b0.min[0]));
    // A mirror in Y keeps the scale; the model's shape is as it was.
    let s = apply(&mut j, "mirror.1", "true");
    let k = s.project.setup("s1").unwrap().geometry.clone().unwrap();
    assert_eq!(k.mirror, Some([false, true, false]));
    assert!(k.scale.is_some());
    assert_eq!(vm(&s, "s1").row("mirror.1").unwrap().value, "true");
    // Back to nothing: the field goes away again.
    apply(&mut j, "scale.all", "1");
    let s = apply(&mut j, "mirror.1", "false");
    assert!(s.project.setup("s1").unwrap().geometry.is_none());
    // Each was one undo step.
    undo(&mut j);
    undo(&mut j);
    let s = undo(&mut j);
    assert_eq!(
        s.project
            .setup("s1")
            .unwrap()
            .geometry
            .as_ref()
            .unwrap()
            .mirror,
        None
    );
    let s = undo(&mut j);
    assert!(s.project.setup("s1").unwrap().geometry.is_none());
}

#[test]
fn scale_entries_are_checked_and_bodies_keep_them() {
    let mut j = opened("plate");
    let s = j.snapshot.clone().unwrap();
    assert!(
        setup_edit(&s, "s1", "scale.0", "0")
            .unwrap_err()
            .contains("0")
    );
    assert!(setup_edit(&s, "s1", "scale.1", "abc").is_err());
    assert!(setup_edit(&s, "s1", "scale.3", "2").is_err());
    assert!(setup_edit(&s, "s1", "mirror.7", "true").is_err());
    // A mirror in X, then a body unchecked: the mirror stays.
    apply(&mut j, "mirror.0", "true");
    let s = apply(&mut j, "bodies.b-2", "false");
    let g = s.project.setup("s1").unwrap().geometry.clone().unwrap();
    assert_eq!(g.bodies, Some(vec!["b-1".to_owned()]));
    assert_eq!(g.mirror, Some([true, false, false]));
    // And a scale after it keeps the bodies.
    let s = apply(&mut j, "scale.2", "0.5");
    let g = s.project.setup("s1").unwrap().geometry.clone().unwrap();
    assert_eq!(g.bodies, Some(vec!["b-1".to_owned()]));
    assert_eq!(g.scale.map(|k| k[2].v), Some(0.5));
}

#[test]
fn a_rotary_setup_takes_a_body_as_its_stock() {
    let mut j = opened("plate");
    apply(&mut j, "machine", "shop/router-4x@1");
    apply(&mut j, "kind", "rotary");
    apply(&mut j, "bodies.b-2", "false");
    let s = j.snapshot.clone().unwrap();
    let v = vm(&s, "s1");
    let RowWidget::Choice(o) = &v.row("stock.kind").unwrap().widget else {
        panic!()
    };
    assert!(o.iter().any(|(k, _)| k == "body"), "{o:?}");
    let s = apply(&mut j, "stock.kind", "body");
    assert_eq!(
        s.project.setup("s1").unwrap().stock,
        Some(StockSpec::Body { body: "b-2".into() })
    );
    let v = vm(&s, "s1");
    assert_eq!(v.row("stock.body").unwrap().value, "b-2");
    assert_eq!(
        v.row("stock.body").unwrap().pick,
        Some(SetupPick::StockBody)
    );
    // The scene has the bounding cylinder of the body: centred on the axis.
    let b = bounds_of(&s, "s1");
    assert!(b.max[0] > b.min[0], "{b:?}");
    assert!((b.max[1] + b.min[1]).abs() < 1e-3 && (b.max[2] + b.min[2]).abs() < 1e-3);
    assert!(
        !s.scenes["s1"]
            .edge_ids
            .iter()
            .any(|e| e.starts_with("e-2."))
    );
    // Indexed takes it as a box.
    let s = apply(&mut j, "kind", "indexed");
    assert!(bounds_of(&s, "s1").max[0] > 0.0);
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
}

#[test]
fn different_scale_factors_are_shown_and_explained() {
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    // The window sets one factor; a file with one per axis shows them.
    let e = setup_edit(&s, "s1", "scale.0", "2").unwrap();
    j.edit(JobEdit::Model(e));
    let s = j.wait_snapshot().unwrap();
    let v = vm(&s, "s1");
    assert_eq!(v.row("scale.all").unwrap().value, "");
    assert_eq!(v.row("scale.0").unwrap().value, "2");
    assert_eq!(v.row("scale.1").unwrap().value, "1");
    // The solids take equal factors only: the Setup says to use one.
    assert!(
        s.warnings.iter().any(|w| w.message.contains("one factor")),
        "{:?}",
        s.warnings
    );
}

fn fixtures_of(s: &Snapshot, id: &str) -> Option<Vec<fluut_model::project::Fixture>> {
    s.project.setup(id).unwrap().fixtures.clone()
}

fn apply_to(j: &mut Jobs, id: &str, key: &str, text: &str) -> Arc<Snapshot> {
    let s = j.snapshot.clone().unwrap();
    j.edit(JobEdit::Model(setup_edit(&s, id, key, text).unwrap()));
    j.wait_snapshot().unwrap()
}

#[test]
fn a_three_axis_setup_has_no_fixtures_group() {
    let j = opened("block");
    let s = j.snapshot.clone().unwrap();
    assert!(vm(&s, "s1").groups.iter().all(|g| g.id != "setup-fixtures"));
    assert!(setup_edit(&s, "s1", "fixtures.add", "chuck").is_err());
}

#[test]
fn a_rotary_setup_lists_adds_edits_and_removes_fixtures() {
    use fluut_model::project::Fixture;
    let mut j = opened("shaft");
    let s = j.snapshot.clone().unwrap();
    let id = s.project.setups[0].id.clone();
    let v = vm(&s, &id);
    let g = v.groups.iter().find(|g| g.id == "setup-fixtures").unwrap();
    assert_eq!(g.rows[0].key, "fixtures");
    assert_eq!(g.rows[0].value, "None");
    assert_eq!(g.rows[0].origin, OriginVm::default());
    let RowWidget::Choice(add) = &v.row("fixtures.add").unwrap().widget else {
        panic!()
    };
    let kinds: Vec<&str> = add.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(kinds, ["", "chuck", "tailstock", "clamp"]);
    // Nothing to remove yet.
    assert!(v.row("fixtures.clear").is_none());
    let before = s.clone();
    let mut now = s;
    for kind in ["chuck", "tailstock", "clamp"] {
        now = apply_to(&mut j, &id, "fixtures.add", kind);
    }
    let list = fixtures_of(&now, &id).unwrap();
    assert_eq!(
        list.iter().map(Fixture::kind).collect::<Vec<_>>(),
        ["chuck", "tailstock", "clamp"]
    );
    // The clamp lies beside the stock, so a rotary Setup's clearance
    // cylinder (radius 25) is inside it: the model says so.
    assert!(
        now.warnings
            .iter()
            .any(|w| w.code == "setup.clearance.index_unsafe"),
        "{:?}",
        now.warnings
    );
    let v = vm(&now, &id);
    assert_eq!(v.row("fixtures").unwrap().value, "3 fixtures");
    assert_eq!(v.row("fixtures.0.remove").unwrap().label, "Chuck 1");
    assert_eq!(v.row("fixtures.1.remove").unwrap().label, "Tailstock 2");
    assert_eq!(v.row("fixtures.2.remove").unwrap().label, "Clamp 3");
    assert_eq!(v.row("fixtures.1.tip_diameter").unwrap().value, "");
    assert!(v.row("fixtures.2.max.2").is_some());
    assert!(v.row("fixtures.clear").is_some());
    // Fields.
    now = apply_to(&mut j, &id, "fixtures.0.diameter", "120");
    let Fixture::Chuck { diameter, .. } = &fixtures_of(&now, &id).unwrap()[0] else {
        panic!()
    };
    assert_eq!(diameter.v, 120.0);
    now = apply_to(&mut j, &id, "fixtures.1.tip_diameter", "8");
    assert_eq!(
        vm(&now, &id).row("fixtures.1.tip_diameter").unwrap().value,
        "8"
    );
    now = apply_to(&mut j, &id, "fixtures.1.tip_diameter", "");
    assert_eq!(
        vm(&now, &id).row("fixtures.1.tip_diameter").unwrap().value,
        ""
    );
    now = apply_to(&mut j, &id, "fixtures.2.max.1", "90");
    let Fixture::Clamp { max, .. } = &fixtures_of(&now, &id).unwrap()[2] else {
        panic!()
    };
    assert_eq!(max[1].v, 90.0);
    // Remove one, then all; with nothing left the field is gone.
    now = apply_to(&mut j, &id, "fixtures.1.remove", "true");
    assert_eq!(
        fixtures_of(&now, &id)
            .unwrap()
            .iter()
            .map(Fixture::kind)
            .collect::<Vec<_>>(),
        ["chuck", "clamp"]
    );
    now = apply_to(&mut j, &id, "fixtures.clear", "true");
    assert_eq!(fixtures_of(&now, &id), None);
    assert_eq!(
        now.project.setup(&id).unwrap(),
        before.project.setup(&id).unwrap()
    );
    // Every edit was one step: nine undos reach the start.
    for _ in 0..9 {
        undo(&mut j);
    }
    assert_eq!(j.snapshot.clone().unwrap().project, before.project);
}

#[test]
fn fixture_entries_follow_the_rules_of_the_project_file() {
    let mut j = opened("shaft");
    let id = j.snapshot.as_ref().unwrap().project.setups[0].id.clone();
    for kind in ["chuck", "tailstock", "clamp"] {
        apply_to(&mut j, &id, "fixtures.add", kind);
    }
    let s = j.snapshot.clone().unwrap();
    let bad = |key: &str, text: &str| setup_edit(&s, &id, key, text).unwrap_err();
    assert_eq!(bad("fixtures.0.diameter", "0"), "Must be above 0.");
    assert_eq!(bad("fixtures.0.length", "-3"), "Must be above 0.");
    assert_eq!(bad("fixtures.1.diameter", "0"), "Must be above 0.");
    assert_eq!(
        bad("fixtures.1.tip_diameter", "-1"),
        "Must not be negative."
    );
    assert!(bad("fixtures.0.diameter", "abc").contains("abc"));
    // A clamp's maximum stays above its minimum on every axis.
    assert_eq!(
        bad("fixtures.2.min.1", "500"),
        "Each maximum must be above its minimum."
    );
    assert_eq!(
        bad("fixtures.2.max.0", "-500"),
        "Each maximum must be above its minimum."
    );
    // A point tip is allowed.
    assert!(setup_edit(&s, &id, "fixtures.1.tip_diameter", "0").is_ok());
    // Keys that are not fields.
    for k in [
        "fixtures.9.remove",
        "fixtures.0.tip_diameter",
        "fixtures.0.colour",
        "fixtures.2.min.3",
        "fixtures.x.length",
        "fixtures.0",
    ] {
        assert_eq!(setup_edit(&s, &id, k, "1").unwrap_err(), NO_FIELD, "{k}");
    }
    assert_eq!(
        setup_edit(&s, &id, "fixtures.add", "vise").unwrap_err(),
        NO_FIELD
    );
    // A refused entry changed nothing.
    assert_eq!(j.snapshot.clone().unwrap().project, s.project);
}

#[test]
fn a_based_on_setup_shows_inherited_fixtures_and_replaces_or_resets_them() {
    use fluut_model::project::Fixture;
    let mut j = opened("flipside");
    let base = j.snapshot.clone().unwrap();
    // The parent has two fixtures; the child has none of its own.
    apply_to(&mut j, "s1", "fixtures.add", "chuck");
    let s = apply_to(&mut j, "s1", "fixtures.add", "tailstock");
    assert_eq!(fixtures_of(&s, "s2"), None);
    let v = vm(&s, "s2");
    let head = v.row("fixtures").unwrap();
    assert_eq!(head.value, "2 fixtures");
    assert_eq!(head.origin.inherited.as_deref(), Some("Inherited from Top"));
    assert_eq!(head.origin.reset, None);
    assert!(v.row("fixtures.local").is_some());
    // The parent shows no origin.
    let p = vm(&s, "s1");
    assert_eq!(p.row("fixtures").unwrap().origin, OriginVm::default());
    assert!(p.row("fixtures.local").is_none());
    // Editing a field of the inherited list writes the whole list to the
    // child, with the change, and leaves the parent alone.
    let s = apply_to(&mut j, "s2", "fixtures.0.length", "33");
    let own = fixtures_of(&s, "s2").unwrap();
    assert_eq!(own.len(), 2);
    let Fixture::Chuck { length, .. } = &own[0] else {
        panic!()
    };
    assert_eq!(length.v, 33.0);
    let Fixture::Chuck { length, .. } = &fixtures_of(&s, "s1").unwrap()[0] else {
        panic!()
    };
    assert_ne!(length.v, 33.0);
    let head = vm(&s, "s2").row("fixtures").unwrap().origin.clone();
    assert_eq!(head.inherited, None);
    assert_eq!(head.reset, Some("fixtures"));
    // Reset to inherited drops the child's list.
    j.edit(JobEdit::Model(reset_edit("s2", "fixtures")));
    let s = j.wait_snapshot().unwrap();
    assert_eq!(fixtures_of(&s, "s2"), None);
    // "Remove all" on the child overrides the parent's list with none.
    let s = apply_to(&mut j, "s2", "fixtures.clear", "true");
    assert_eq!(fixtures_of(&s, "s2"), Some(Vec::new()));
    assert_eq!(vm(&s, "s2").row("fixtures").unwrap().value, "None");
    assert_eq!(fixtures_of(&s, "s1").unwrap().len(), 2);
    // A local copy is the same list, written to the child.
    j.edit(JobEdit::Model(reset_edit("s2", "fixtures")));
    j.wait_snapshot().unwrap();
    let s = apply_to(&mut j, "s2", "fixtures.local", "true");
    assert_eq!(fixtures_of(&s, "s2"), fixtures_of(&s, "s1"));
    // All of it undoes to the start.
    for _ in 0..7 {
        undo(&mut j);
    }
    assert_eq!(j.snapshot.clone().unwrap().project, base.project);
}

fn fixture_points(s: &Snapshot, id: &str) -> usize {
    let e = &s.scenes[id].edges;
    e.tag
        .iter()
        .filter(|t| **t == crate::scene::FIXTURE_TAG)
        .count()
        / 2
}

#[test]
fn fixtures_are_drawn_in_the_scene_and_follow_the_index_angle() {
    let mut j = opened("flipside");
    let s = j.snapshot.clone().unwrap();
    assert_eq!(fixture_points(&s, "s1"), 0);
    let s = apply_to(&mut j, "s1", "fixtures.add", "clamp");
    // A clamp is a box of twelve lines; the child inherits and shows it.
    assert_eq!(fixture_points(&s, "s1"), 12);
    assert_eq!(fixture_points(&s, "s2"), 12);
    let bounds = |s: &Snapshot, id: &str| {
        let e = &s.scenes[id].edges;
        let pts: Vec<[f32; 3]> = e
            .points
            .iter()
            .zip(&e.tag)
            .filter(|(_, t)| **t == crate::scene::FIXTURE_TAG)
            .map(|(p, _)| *p)
            .collect();
        crate::scene::Bounds::of(&pts)
    };
    let (b1, b2) = (bounds(&s, "s1"), bounds(&s, "s2"));
    // The box is at +Y of the stock at A = 0; at 180 degrees it is at -Y.
    assert!(b1.min[1] > 0.0, "{b1:?}");
    assert!(b2.max[1] < 0.0, "{b2:?}");
    let s = apply_to(&mut j, "s1", "index_angle", "90");
    let b = bounds(&s, "s1");
    // The +Y side goes up to +Z, the way the part's own +Y does.
    assert!(b.min[2] > 0.0 && b.max[1] < 20.0, "{b:?}");
    // A chuck is a drum behind the stock's start.
    let s = apply_to(&mut j, "s1", "fixtures.add", "chuck");
    let stock = s.scenes["s1"].stock.unwrap();
    assert!(bounds(&s, "s1").min[0] < stock.min[0]);
    // Undo takes them off again.
    undo(&mut j);
    undo(&mut j);
    undo(&mut j);
    assert_eq!(fixture_points(&j.snapshot.clone().unwrap(), "s1"), 0);
    // A three_axis Setup draws none even if its file lists some.
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let e = fluut_model::edit::Edit::set_setup_field(
        "s1",
        "fixtures",
        Some("[{ kind = \"chuck\", diameter = 30, length = 10 }]"),
    );
    j.edit(JobEdit::Model(e));
    let s2 = j.wait_snapshot().unwrap();
    assert_eq!(fixture_points(&s2, "s1"), 0);
    // The Setup says why, and offers no group for them.
    let v = vm(&s2, "s1");
    assert!(
        v.diagnostics
            .iter()
            .any(|d| d.code == "setup.fixtures.ignored"),
        "{:?}",
        v.diagnostics
    );
    assert!(v.groups.iter().all(|g| g.id != "setup-fixtures"));
    let _ = s;
}
