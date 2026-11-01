// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;

fn load(kind: ToolpathTypeId, src: &str) -> Result<(Params, Vec<String>), String> {
    let t: toml::Table = toml::from_str(src).map_err(|e| e.to_string())?;
    let mut w = Vec::new();
    Params::from_toml(kind, &t, &mut w).map(|p| (p, w))
}

fn contour(src: &str) -> Result<(Params, Vec<String>), String> {
    load(ToolpathTypeId::Contour2d, src)
}

#[test]
fn every_kind_parses() {
    let (p, w) = contour(
        r#"
        [tool_params]
        spindle_rpm = 18000
        feed = { cut = 2000.5, plunge = 600 }
        coolant = "mist"
        [geometry]
        drive_curves = [{ body = "b-1" }, { edge = "e-2" }, { face = "f-3" }]
        [heights]
        bottom = { ref = "stock_bottom", offset = -0.2 }
        [passes]
        depth_count = 3
        finish_every_level = true
        start_point = [1, 2.5, -3]
        tabs = { mode = "points", points = [[0, 0, 0], [1.5, 2, 3]], ramp_angle_deg = 30 }
        "#,
    )
    .unwrap();
    assert!(w.is_empty());
    assert_eq!(
        p.get("tool_params.spindle_rpm"),
        Some(&PValue::Num(Num::int(18000)))
    );
    assert_eq!(
        p.get("tool_params.feed.cut"),
        Some(&PValue::Num(Num::float(2000.5)))
    );
    assert_eq!(
        p.get("geometry.drive_curves"),
        Some(&PValue::Selection(vec![
            Sel::Body("b-1".into()),
            Sel::Edge("e-2".into()),
            Sel::Face("f-3".into())
        ]))
    );
    assert_eq!(
        p.get("heights.bottom"),
        Some(&PValue::Height(HeightSpec::new("stock_bottom", -0.2)))
    );
    assert_eq!(
        p.get("passes.finish_every_level"),
        Some(&PValue::Bool(true))
    );
    assert!(matches!(p.get("passes.tabs.points"), Some(PValue::Points(v)) if v.len() == 2));
    assert_eq!(p.get("passes.side"), None);
}

#[test]
fn wrong_kinds_are_load_errors() {
    for (src, want) in [
        ("[passes]\nstep_down = \"4\"", "expected a number"),
        ("[passes]\ndepth_count = 2.0", "expected an integer"),
        ("[passes]\nfinish_every_level = 1", "expected true or false"),
        ("[passes]\nside = \"middle\"", "is not one of"),
        (
            "[heights]\ntop = { ref = \"top\" }",
            "is not a height reference",
        ),
        (
            "[heights]\ntop = { ref = \"stock_top\", z = 1 }",
            "unknown key `z`",
        ),
        (
            "[geometry]\ndrive_curves = [{ solid = \"x\" }]",
            "is not body, face or edge",
        ),
        (
            "[geometry]\ndrive_curves = [{ body = 1 }]",
            "ids are strings",
        ),
        ("[passes]\nstart_point = [1, 2]", "expected a point"),
        ("[passes]\ntabs = 3", "expected an inline table"),
        ("[passes]\nstep_down = nan", "finite"),
        ("passes = 3", "must be a table"),
    ] {
        let e = contour(src).unwrap_err();
        assert!(e.contains(want), "{src}: {e}");
    }
}

#[test]
fn unknown_keys() {
    // Passes: error.
    let e = contour("[passes]\nstepp_down = 1").unwrap_err();
    assert!(e.contains("unknown key `passes.stepp_down`"), "{e}");
    // In an inline table of passes: error.
    let e = contour("[passes]\ntabs = { mode = \"off\", size = 2 }").unwrap_err();
    assert!(e.contains("unknown key `passes.tabs.size`"), "{e}");
    // Other groups: warning, value dropped.
    let (p, w) = contour("[linking]\nretrakt = \"full\"").unwrap();
    assert_eq!(w, ["unknown key `linking.retrakt` ignored"]);
    assert!(p.group(Group::Linking).is_empty());
    // A common field the type does not declare: error (spec 03 §3.10.3).
    let e = contour("[geometry]\nboundary = { side = \"inside\" }").unwrap_err();
    assert!(e.contains("not used by this toolpath type"), "{e}");
    let e = load(
        ToolpathTypeId::Parallel,
        "[linking]\nramp = { kind = \"helix\" }",
    )
    .unwrap_err();
    assert!(e.contains("not used"), "{e}");
    // Ramp kinds are per type.
    assert!(contour("[linking]\nramp = { kind = \"helix\" }").is_err());
    assert!(
        load(
            ToolpathTypeId::Pocket2d,
            "[linking]\nramp = { kind = \"helix\" }"
        )
        .is_ok()
    );
}

#[test]
fn numbers_keep_their_form() {
    let (p, _) =
        contour("[passes]\nstep_down = 4.0\nstock_to_leave = 0\nmachining_tolerance = 1e-5")
            .unwrap();
    let mut out = String::new();
    p.write(&mut out, "");
    assert_eq!(
        out,
        "\n[setup.toolpath.passes]\nmachining_tolerance = 1e-5\nstep_down           = 4.0\nstock_to_leave      = 0\n"
    );
}

#[test]
fn set_keeps_canonical_order_and_checks_kind() {
    let mut p = Params::new(ToolpathTypeId::Pocket2d);
    p.set("passes.stock_to_leave", Some(PValue::Num(Num::float(0.2))))
        .unwrap();
    p.set("passes.pattern", Some(PValue::Enum("raster".into())))
        .unwrap();
    p.set("linking.ramp.angle_deg", Some(PValue::Num(Num::int(5))))
        .unwrap();
    let keys: Vec<_> = p.group(Group::Passes).iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["pattern", "stock_to_leave"]);
    assert!(
        p.set("passes.pattern", Some(PValue::Enum("spiral".into())))
            .is_err()
    );
    assert!(
        p.set("passes.side", Some(PValue::Enum("inside".into())))
            .is_err()
    );
    assert!(p.set("passes.stepover", Some(PValue::Bool(true))).is_err());
    p.set("linking.ramp.angle_deg", None).unwrap();
    assert!(p.group(Group::Linking).is_empty());
}

#[test]
fn class_text_lists_every_leaf() {
    let p = Params::new(ToolpathTypeId::Contour2d);
    let r = p.class_text(Class::R);
    assert!(r.contains("passes.tabs.width = -"));
    assert!(!r.contains("tool_params"));
    let pp = p.class_text(Class::P);
    assert!(pp.contains("tool_params.feed.transition = -"));
    assert!(!pp.contains("passes"));
}

#[test]
fn metadata_lookup() {
    let m = Params::meta(ToolpathTypeId::Contour2d, "linking.lead_in.radius").unwrap();
    assert_eq!(m.class, Class::R);
    assert_eq!(m.label_id(), "param-radius-label");
    assert!(Params::meta(ToolpathTypeId::Contour2d, "geometry.boundary").is_none());
    assert_eq!(
        Params::meta(ToolpathTypeId::Parallel, "heights.top")
            .unwrap()
            .default,
        DefaultRule::Const(ConstVal::Height("model_top", 0.0))
    );
}
