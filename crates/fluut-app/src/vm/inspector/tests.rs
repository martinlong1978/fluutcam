// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::jobs::Jobs;
use crate::session::tests::opened;
use crate::text::{Lang, set_lang};
use std::sync::Arc;

fn tp_vm(snap: &Snapshot, id: &str) -> ToolpathVm {
    match InspectorVm::new(Some(snap), &Selection::Toolpath(id.into())) {
        InspectorVm::Toolpath(t) => t,
        other => panic!("{other:?}"),
    }
}

fn field<'a>(vm: &'a ToolpathVm, path: &str) -> &'a FieldVm {
    vm.tabs
        .iter()
        .flat_map(|t| t.fields.iter())
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("no field {path}"))
}

fn apply(j: &mut Jobs, e: Edit) -> Arc<Snapshot> {
    j.edit(e);
    j.wait_snapshot().unwrap()
}

#[test]
fn empty_and_setup() {
    assert!(matches!(
        InspectorVm::new(None, &Selection::None),
        InspectorVm::Empty(_)
    ));
    let j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let InspectorVm::Setup(vm) = InspectorVm::new(Some(&s), &Selection::Setup("s1".into())) else {
        panic!()
    };
    assert_eq!(vm.title, "Top");
    assert_eq!(vm.row("material").unwrap().value, "MDF");
}

#[test]
fn tabs_follow_the_groups() {
    let j = opened("block");
    let vm = tp_vm(j.snapshot.as_ref().unwrap(), "t1");
    let labels: Vec<&str> = vm.tabs.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(labels, ["Tool", "Geometry", "Heights", "Passes", "Linking"]);
    assert_eq!(vm.title, "Pocket");
    assert_eq!(vm.subtitle, "pocket_2d · Top · 6 mm flat");
}

#[test]
fn visible_if_follows_the_pattern() {
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    // t1 uses the default pattern (offset); t2 is raster.
    let t1 = tp_vm(&s, "t1");
    assert!(field(&t1, "passes.offset_order").visible);
    assert!(!field(&t1, "passes.pass_angle_deg").visible);
    let t2 = tp_vm(&s, "t2");
    assert!(!field(&t2, "passes.offset_order").visible);
    assert!(field(&t2, "passes.pass_angle_deg").visible);
    assert_eq!(field(&t2, "passes.pass_angle_deg").value, "45");
    // Switching t1 to raster shows the angle.
    let s = apply(
        &mut j,
        value_edit("t1", "passes.pattern", Some(PValue::Enum("raster".into()))),
    );
    assert!(field(&tp_vm(&s, "t1"), "passes.pass_angle_deg").visible);
}

#[test]
fn values_defaults_and_units() {
    let j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let vm = tp_vm(&s, "t1");
    let so = field(&vm, "passes.stepover");
    assert!(so.set);
    assert_eq!(so.value, "2.4");
    assert_eq!(so.widget, Widget::Number { unit: Some("mm") });
    assert_eq!(so.default_text, "Default: 2.4 mm (tool preset stepover)");
    // Feed comes from the MDF preset.
    let feed = field(&vm, "tool_params.feed.cut");
    assert!(!feed.set);
    assert_eq!(feed.value, "2000");
    assert_eq!(
        feed.widget,
        Widget::Number {
            unit: Some("mm/min")
        }
    );
    let dir = field(&vm, "passes.direction");
    assert_eq!(dir.value, "climb");
    assert!(matches!(&dir.widget, Widget::Enum { options } if options.len() == 2));
    let t2 = tp_vm(&s, "t2");
    let top = field(&t2, "heights.top");
    assert_eq!(
        (top.value.as_str(), &top.widget),
        ("selection + 0.1 mm", &Widget::ReadOnly)
    );
    assert_eq!(field(&vm, "geometry.drive_faces").value, "1 selected");

    // In inches the same values show converted; entries are in inches.
    let mut inch = Snapshot::clone(&s);
    inch.project.units = DisplayUnit::Inch;
    let vm = tp_vm(&inch, "t1");
    assert_eq!(field(&vm, "passes.stepover").value, "0.0945");
    assert_eq!(
        field(&vm, "passes.stepover").widget,
        Widget::Number { unit: Some("in") }
    );
    let e = number_edit(&inch, "t1", "passes.stepover", "0.1").unwrap();
    assert_eq!(
        e,
        value_edit("t1", "passes.stepover", Some(PValue::Num(Num::float(2.54))))
    );
    // An explicit unit wins.
    let e = number_edit(&inch, "t1", "passes.stepover", "2mm").unwrap();
    assert_eq!(
        e,
        value_edit("t1", "passes.stepover", Some(PValue::Num(Num::float(2.0))))
    );
}

#[test]
fn entries() {
    let j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let e = number_edit(&s, "t1", "passes.stepover", "tool.diameter*0.5").unwrap();
    assert_eq!(
        e,
        value_edit("t1", "passes.stepover", Some(PValue::Num(Num::float(3.0))))
    );
    // Empty entry clears the local value.
    let e = number_edit(&s, "t1", "passes.stepover", " ").unwrap();
    assert_eq!(e, value_edit("t1", "passes.stepover", None));
    // Counts are integers.
    let e = number_edit(&s, "t3", "passes.tabs.count", "6").unwrap();
    assert_eq!(
        e,
        value_edit("t3", "passes.tabs.count", Some(PValue::Num(Num::int(6))))
    );
    let err = number_edit(&s, "t1", "passes.stepover", "abc").unwrap_err();
    assert!(err.starts_with("Not a value: abc"), "{err}");
}

#[test]
fn validation_shows_under_its_field() {
    let mut j = opened("block");
    let e = number_edit(
        j.snapshot.as_ref().unwrap(),
        "t1",
        "passes.stepover",
        "tool.diameter+1",
    )
    .unwrap();
    let s = apply(&mut j, e);
    let vm = tp_vm(&s, "t1");
    let so = field(&vm, "passes.stepover");
    assert_eq!(so.value, "7");
    assert_eq!(so.diagnostics.len(), 1, "{:?}", so.diagnostics);
    assert_eq!(so.diagnostics[0].code, "param.passes.stepover.max");
    assert!(so.diagnostics[0].error);
    let passes = vm.tabs.iter().find(|t| t.group == Group::Passes).unwrap();
    assert_eq!(passes.errors, 1);
    // Other fields carry nothing.
    assert!(field(&vm, "passes.step_down").diagnostics.is_empty());
}

#[test]
fn pseudo_locale_reaches_every_label() {
    let j = opened("block");
    let s = j.snapshot.clone().unwrap();
    set_lang(Lang::Pseudo);
    let vm = tp_vm(&s, "t3");
    set_lang(Lang::English);
    for t in &vm.tabs {
        assert!(t.label.starts_with('⟦'), "{}", t.label);
        for f in &t.fields {
            assert!(f.label.starts_with('⟦'), "{}", f.label);
            assert!(f.default_text.starts_with('⟦'), "{}", f.default_text);
        }
    }
}
