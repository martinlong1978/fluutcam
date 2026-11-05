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
    assert_eq!(top.value, "selection + 0.1 mm");
    assert!(matches!(
        &top.widget,
        Widget::Height { reference, offset, unit: Some("mm"), .. }
            if reference == "selection" && offset == "0.1"
    ));
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

#[test]
fn the_tool_dropdown_lists_the_libraries_and_writes_the_reference() {
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let vm = tp_vm(&s, "t1");
    let starter = "fluut/starter#ball-6";
    let (_, label) = vm
        .tool
        .options
        .iter()
        .find(|(r, _)| r == starter)
        .unwrap_or_else(|| panic!("{:?}", vm.tool.options));
    assert!(label.contains("Ball 6 mm"), "{label}");
    assert!(vm.tool.options.iter().any(|(r, _)| *r == vm.tool.value));
    assert!(vm.tool.diagnostics.is_empty());
    let e = crate::jobs::Edit::Model(tool_edit("t1", starter));
    let s = apply(&mut j, e);
    assert_eq!(s.project.toolpath("t1").unwrap().1.tool, starter);
    assert_eq!(tp_vm(&s, "t1").tool.value, starter);
    j.undo();
    j.wait();
    let s = j.snapshot.clone().unwrap();
    assert_ne!(s.project.toolpath("t1").unwrap().1.tool, starter);
}

#[test]
fn a_missing_tool_is_shown_and_reported_at_the_dropdown() {
    let mut j = opened("block");
    let e = crate::jobs::Edit::Model(tool_edit("t1", "nope#gone"));
    let s = apply(&mut j, e);
    let vm = tp_vm(&s, "t1");
    assert_eq!(vm.tool.value, "nope#gone");
    assert_eq!(vm.tool.options[0].0, "nope#gone");
    assert_eq!(vm.tool.diagnostics[0].code, "param.tool.missing");
    assert!(vm.general.iter().all(|d| d.code != "param.tool.missing"));
}

fn height_of(vm: &ToolpathVm, path: &str) -> (String, String) {
    match &field(vm, path).widget {
        Widget::Height {
            reference, offset, ..
        } => (reference.clone(), offset.clone()),
        w => panic!("{w:?}"),
    }
}

#[test]
fn a_height_edits_its_reference_and_offset_apart() {
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    // An unset height starts from its default.
    let vm = tp_vm(&s, "t1");
    assert_eq!(
        height_of(&vm, "heights.retract"),
        ("stock_top".to_owned(), "5".to_owned())
    );
    let e = height_edit(&s, "t1", "heights.retract", None, Some("7")).unwrap();
    let s = apply(&mut j, e);
    assert_eq!(
        height_of(&tp_vm(&s, "t1"), "heights.retract"),
        ("stock_top".to_owned(), "7".to_owned())
    );
    let e = height_edit(&s, "t1", "heights.retract", Some("model_top"), None).unwrap();
    let s = apply(&mut j, e);
    assert_eq!(
        height_of(&tp_vm(&s, "t1"), "heights.retract"),
        ("model_top".to_owned(), "7".to_owned())
    );
    assert!(field(&tp_vm(&s, "t1"), "heights.retract").set);
    assert!(height_edit(&s, "t1", "heights.retract", None, Some("high")).is_err());
    // Undo steps back through both.
    j.undo();
    j.wait();
    j.undo();
    j.wait();
    let s = j.snapshot.clone().unwrap();
    assert!(!field(&tp_vm(&s, "t1"), "heights.retract").set);
}

#[test]
fn only_feed_offers_top_and_changing_its_offset_is_stored() {
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let vm = tp_vm(&s, "t1");
    let refs = |path: &str| match &field(&vm, path).widget {
        Widget::Height { refs, .. } => refs.iter().map(|r| r.0.clone()).collect::<Vec<_>>(),
        w => panic!("{w:?}"),
    };
    assert!(refs("heights.feed").contains(&"top".to_owned()));
    assert!(!refs("heights.retract").contains(&"top".to_owned()));
    let e = height_edit(&s, "t1", "heights.feed", None, Some("3")).unwrap();
    let s = apply(&mut j, e);
    assert!(s.edit_error.is_none(), "{:?}", s.edit_error);
    assert_eq!(
        height_of(&tp_vm(&s, "t1"), "heights.feed"),
        ("top".to_owned(), "3".to_owned())
    );
}

#[test]
fn heights_show_and_take_the_project_unit() {
    let j = opened("block");
    let mut inch = Snapshot::clone(j.snapshot.as_ref().unwrap());
    inch.project.units = DisplayUnit::Inch;
    let e = height_edit(&inch, "t1", "heights.retract", None, Some("1")).unwrap();
    assert_eq!(
        e,
        value_edit(
            "t1",
            "heights.retract",
            Some(PValue::Height(HeightSpec::new("stock_top", 25.4)))
        )
    );
}

#[test]
fn every_field_kind_of_every_type_has_an_editor() {
    use fluut_model::ToolpathTypeId;
    for t in ToolpathTypeId::ALL {
        for (path, m) in Params::leaf_fields(t) {
            let tp = Toolpath::new("x", "x", t, "");
            let cx = Cx {
                tp: &tp,
                tool: None,
                material: None,
                unit: DisplayUnit::Mm,
            };
            let w = cx.field(path.clone(), m, &[]).widget;
            let selection = m.kind == FieldKind::Selection;
            assert!(selection || w != Widget::ReadOnly, "{t:?} {path}");
        }
    }
}

#[test]
fn points_and_text_are_entered_as_text() {
    let start = "passes.start_point";
    let mut j = opened("plate2d");
    j.add_toolpath("s1", fluut_model::ToolpathTypeId::Contour2d);
    j.wait();
    let s2 = j.snapshot.clone().unwrap();
    let id = s2.project.setups[0].toolpaths.last().unwrap().id.clone();
    assert!(points_edit(&s2, &id, "passes.tabs.points", "1 2").is_err());
    assert!(points_edit(&s2, &id, "passes.tabs.points", "a b c").is_err());
    let e = points_edit(&s2, &id, "passes.tabs.points", "1 2 3; 4 5 6").unwrap();
    assert_eq!(
        e,
        value_edit(
            &id,
            "passes.tabs.points",
            Some(PValue::Points(vec![
                [Num::float(1.0), Num::float(2.0), Num::float(3.0)],
                [Num::float(4.0), Num::float(5.0), Num::float(6.0)]
            ]))
        )
    );
    assert_eq!(
        points_edit(&s2, &id, start, "").unwrap(),
        value_edit(&id, start, None)
    );
    assert!(points_edit(&s2, &id, start, "1 2 3; 4 5 6").is_err());
}
