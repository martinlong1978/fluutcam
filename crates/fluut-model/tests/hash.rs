// SPDX-License-Identifier: MIT OR Apache-2.0

//! The result hash covers R fields only (D15 §1.8): changing a P field
//! leaves it unchanged, changing an R field changes it.

use fluut_model::Class;
use fluut_model::hash::{toolpath_p_text, toolpath_r_text};
use fluut_model::params::{FieldKind, HeightSpec, Num, PValue, Params, Sel, ToolpathTypeId};
use fluut_model::project::Toolpath;
use fluut_model::tools::{Tool, ToolLibrary};

fn tool() -> Tool {
    let src = "format = \"fluut.tools/1\"\nid = \"lib\"\n[[tool]]\nid = \"f\"\nshape = \"flat\"\n\
               diameter = 6\nnumber = 3\n[[tool.preset]]\nmaterial = \"MDF\"\nrpm = 18000\n\
               feed = 2000\nplunge = 500\nstepover = 2\nstep_down = 3\n";
    ToolLibrary::parse(src).unwrap().0.tools.remove(0)
}

/// A value of the right kind for every field.
fn sample(kind: FieldKind) -> PValue {
    match kind {
        FieldKind::Count => PValue::Num(Num::int(7)),
        FieldKind::Length
        | FieldKind::Feed
        | FieldKind::Speed
        | FieldKind::Angle
        | FieldKind::Duration => PValue::Num(Num::float(7.25)),
        FieldKind::Bool => PValue::Bool(false),
        FieldKind::Enum(e) => PValue::Enum(e[e.len() - 1].to_owned()),
        FieldKind::Height => PValue::Height(HeightSpec::new("work_zero", 1.5)),
        FieldKind::Selection => PValue::Selection(vec![Sel::Face("f-9".into())]),
        FieldKind::Point => PValue::Point([Num::int(1), Num::int(2), Num::int(3)]),
        FieldKind::Points => PValue::Points(vec![[Num::int(1), Num::int(2), Num::int(3)]]),
        FieldKind::Table(_) => unreachable!("leaf fields only"),
    }
}

#[test]
fn p_fields_keep_the_result_hash_and_r_fields_change_it() {
    let t = tool();
    let m = Some("MDF");
    for kind in ToolpathTypeId::ALL {
        let base = Toolpath::new("t", "Name", kind, "lib#f");
        let r0 = toolpath_r_text(&base, Some(&t), m);
        let p0 = toolpath_p_text(&base, Some(&t), m);
        let fields = Params::leaf_fields(kind);
        assert!(fields.iter().any(|(_, f)| f.class == Class::P));
        for (path, f) in fields {
            let mut tp = base.clone();
            tp.params.set(&path, Some(sample(f.kind))).unwrap();
            let r = toolpath_r_text(&tp, Some(&t), m);
            let p = toolpath_p_text(&tp, Some(&t), m);
            match f.class {
                Class::P => {
                    assert_eq!(r, r0, "{kind} {path} (P) changed the result hash");
                    assert_ne!(p, p0, "{kind} {path} (P) did not change the post hash");
                }
                Class::R => assert_ne!(r, r0, "{kind} {path} (R) did not change the result hash"),
                Class::N => assert_eq!(r, r0, "{kind} {path} (N)"),
            }
        }
    }
}

#[test]
fn item_keys_and_tool_data() {
    let t = tool();
    let m = Some("MDF");
    let base = Toolpath::new("t", "Name", ToolpathTypeId::Pocket2d, "lib#f");
    let r0 = toolpath_r_text(&base, Some(&t), m);
    let p0 = toolpath_p_text(&base, Some(&t), m);
    // P: name, enabled, tool number, preset feeds and speed.
    let mut tp = base.clone();
    tp.name = "Other".into();
    tp.enabled = Some(false);
    assert_eq!(toolpath_r_text(&tp, Some(&t), m), r0);
    assert_ne!(toolpath_p_text(&tp, Some(&t), m), p0);
    let mut t2 = t.clone();
    t2.number = 9;
    t2.presets[0].feed = Some(1.0);
    t2.presets[0].rpm = Some(1.0);
    assert_eq!(toolpath_r_text(&base, Some(&t2), m), r0);
    assert_ne!(toolpath_p_text(&base, Some(&t2), m), p0);
    // R: tool reference, geometry, R presets, material.
    let mut tp = base.clone();
    tp.tool = "lib#g".into();
    assert_ne!(toolpath_r_text(&tp, Some(&t), m), r0);
    for change in [
        |t: &mut Tool| t.diameter = 8.0,
        |t: &mut Tool| t.presets[0].step_down = Some(1.0),
        |t: &mut Tool| t.presets[0].stepover = Some(1.0),
        |t: &mut Tool| t.flute_length = Some(1.0),
        |t: &mut Tool| t.half_angle_deg = Some(30.0),
        |t: &mut Tool| t.tip_diameter = Some(0.2),
        |t: &mut Tool| t.point_angle_deg = Some(135.0),
        |t: &mut Tool| t.pitch = Some(0.8),
        |t: &mut Tool| t.presets[0].optimal_load = Some(0.4),
    ] {
        let mut t2 = t.clone();
        change(&mut t2);
        assert_ne!(toolpath_r_text(&base, Some(&t2), m), r0);
    }
    assert_ne!(toolpath_r_text(&base, Some(&t), None), r0);
}
