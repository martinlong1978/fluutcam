// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::jobs::{Edit as JobEdit, Jobs};
use crate::session::tests::opened;
use crate::vm::Selection;
use crate::vm::inspector::InspectorVm;
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
            "setup-stock",
            "setup-zero",
            "setup-clearance"
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
