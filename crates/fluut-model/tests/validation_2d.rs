// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every D15b §2–§3 parameter validation code of the 2-D and hole types:
//! one passing and one failing case each (D15 §6). Calculation codes
//! (`cam.*`) are tested in `fluut-cam`.

use fluut_model::project::{SetupKind, Toolpath, read};
use fluut_model::resolve::ResolvedHeights;
use fluut_model::tools::{Tool, ToolLibrary};
use fluut_model::validate::{CycleSet, PostCaps, ValidateCx, validate};

const LIB: &str = r#"
format = "fluut.tools/1"
id = "lib"
[[tool]]
id = "flat-6"
shape = "flat"
diameter = 6
flute_length = 22
  [[tool.preset]]
  material = "MDF"
  rpm = 18000
  feed = 2500
  plunge = 800
  stepover = 2.4
  step_down = 3
[[tool]]
id = "ball-6"
shape = "ball"
diameter = 6
  [[tool.preset]]
  material = "MDF"
  rpm = 18000
  feed = 2500
  plunge = 800
  stepover = 2.4
  step_down = 3
[[tool]]
id = "v-90"
shape = "v"
diameter = 12
half_angle_deg = 45
tip_diameter = 0.2
flute_length = 8
  [[tool.preset]]
  material = "MDF"
  rpm = 18000
  feed = 2500
  plunge = 800
  step_down = 2
[[tool]]
id = "v-60-short"
shape = "v"
diameter = 12
half_angle_deg = 30
flute_length = 4
[[tool]]
id = "drill-5"
shape = "drill"
diameter = 5
flute_length = 30
  [[tool.preset]]
  material = "MDF"
  rpm = 3000
  feed = 300
  plunge = 300
[[tool]]
id = "tap-6"
shape = "drill"
diameter = 6
pitch = 1.0
  [[tool.preset]]
  material = "MDF"
  rpm = 300
  feed = 300
  plunge = 300
"#;

fn tool(id: &str) -> Tool {
    ToolLibrary::parse(LIB)
        .unwrap()
        .0
        .tools
        .into_iter()
        .find(|t| t.id == id)
        .unwrap()
}

/// A toolpath of `kind` with the given group tables.
fn tp(kind: &str, groups: &str) -> Toolpath {
    let groups = groups.replace("\n[", "\n[setup.toolpath.");
    let groups = match groups.strip_prefix('[') {
        Some(g) => format!("[setup.toolpath.{g}"),
        None => groups,
    };
    let src = format!(
        "format = \"fluut.project/1\"\n[[setup]]\nid = \"s\"\n[[setup.toolpath]]\n\
         id = \"t\"\ntype = \"{kind}\"\ntool = \"lib#x\"\n{groups}"
    );
    let p = read(&src).unwrap_or_else(|e| panic!("{e}\n{src}")).project;
    p.setups[0].toolpaths[0].clone()
}

const HEIGHTS: ResolvedHeights = ResolvedHeights {
    clearance: 30.0,
    retract: 25.0,
    feed: 22.0,
    top: 20.0,
    bottom: 0.0,
};

/// Post capabilities: helices, and the canned cycles named.
fn post(helix: bool, cycles: &[&str]) -> PostCaps {
    PostCaps {
        cutter_comp: true,
        coolant: true,
        helix,
        cycles: CycleSet::of(cycles),
    }
}

fn codes(t: &Toolpath, tool_id: &str, post: Option<PostCaps>) -> Vec<String> {
    let tool = tool(tool_id);
    let cx = ValidateCx {
        tool: Some(&tool),
        material: Some("MDF"),
        setup_kind: SetupKind::ThreeAxis,
        heights: Some(&HEIGHTS),
        stock_bottom: None,
        machine: None,
        post: post.as_ref(),
    };
    validate(t, &cx).into_iter().map(|d| d.code).collect()
}

/// `code` is absent for `pass` and present for `fail`, both on `kind`
/// with tool `tool_id`.
fn case(code: &str, kind: &str, tool_id: &str, pass: &str, fail: &str) {
    case_with(
        code,
        (kind, tool_id, pass, None),
        (kind, tool_id, fail, None),
    );
}

type Side<'a> = (&'a str, &'a str, &'a str, Option<PostCaps>);

fn case_with(code: &str, pass: Side, fail: Side) {
    let p = codes(&tp(pass.0, pass.2), pass.1, pass.3);
    assert!(
        !p.contains(&code.to_owned()),
        "{code} raised by the passing case: {p:?}"
    );
    let f = codes(&tp(fail.0, fail.2), fail.1, fail.3);
    assert!(
        f.contains(&code.to_owned()),
        "{code} not raised by the failing case: {f:?}"
    );
}

fn clean(kind: &str, tool_id: &str, groups: &str) {
    assert_eq!(
        codes(&tp(kind, groups), tool_id, None),
        Vec::<String>::new(),
        "{kind}"
    );
}

#[test]
fn face_codes() {
    clean("face", "flat-6", "");
    case(
        "param.face.overlap.short",
        "face",
        "flat-6",
        "[passes]\noverlap = 3\n",
        "[passes]\noverlap = 2.9\n",
    );
    case(
        "param.face.final_step_down.range",
        "face",
        "flat-6",
        "[passes]\nfinal_step_down = 3\n",
        "[passes]\nfinal_step_down = 3.5\n",
    );
    case_with(
        "param.face.tool_shape",
        ("face", "flat-6", "", None),
        ("face", "ball-6", "", None),
    );
    case(
        "param.passes.stepover.max",
        "face",
        "flat-6",
        "[passes]\nstepover = 6\n",
        "[passes]\nstepover = 6.5\n",
    );
}

const CURVES: &str = "[geometry]\ndrive_curves = [{ body = \"b\" }]\n";

#[test]
fn trace_codes() {
    clean("trace", "v-90", CURVES);
    case(
        "param.trace.depths.both",
        "trace",
        "v-90",
        &format!("{CURVES}[passes]\ndepth_count = 2\n"),
        &format!("{CURVES}[passes]\ndepth_count = 2\nstep_down = 1\n"),
    );
    case(
        "param.trace.curve_depth.unused",
        "trace",
        "v-90",
        &format!("{CURVES}[passes]\ndepth_mode = \"curve\"\ncurve_depth = 1\n"),
        &format!("{CURVES}[passes]\ncurve_depth = 1\n"),
    );
    case("param.geometry.drive.empty", "trace", "v-90", CURVES, "");
}

#[test]
fn chamfer_2d_codes() {
    clean("chamfer_2d", "v-90", CURVES);
    case_with(
        "param.chamfer_2d.tool_shape",
        ("chamfer_2d", "v-90", CURVES, None),
        ("chamfer_2d", "flat-6", CURVES, None),
    );
    // Rt = 0.1: o + w = 0.1 + t + w must stay within R = 6.
    case(
        "param.chamfer_2d.width.tool",
        "chamfer_2d",
        "v-90",
        &format!("{CURVES}[passes]\nchamfer_width = 4\ntip_offset = 1.9\n"),
        &format!("{CURVES}[passes]\nchamfer_width = 4\ntip_offset = 2\n"),
    );
    // Flute length 8: d = w + t.
    case(
        "param.chamfer_2d.depth.flute",
        "chamfer_2d",
        "v-90",
        &format!("{CURVES}[passes]\nchamfer_width = 4\ntip_offset = 4\n"),
        &format!("{CURVES}[passes]\nchamfer_width = 4\ntip_offset = 4.1\n"),
    );
    case(
        "param.geometry.drive.empty",
        "chamfer_2d",
        "v-90",
        CURVES,
        "",
    );
}

const HOLES: &str = "[geometry]\nhole_diameter = { min = 21.9, max = 22.1 }\n";

#[test]
fn bore_codes() {
    clean("bore", "flat-6", HOLES);
    clean("bore", "flat-6", CURVES);
    case(
        "param.bore.finishing_stepover.max",
        "bore",
        "flat-6",
        &format!("{HOLES}[passes]\nfinishing_pass = true\nfinishing_stepover = 6\n"),
        &format!("{HOLES}[passes]\nfinishing_pass = true\nfinishing_stepover = 6.5\n"),
    );
    case_with(
        "param.bore.helix.post",
        ("bore", "flat-6", HOLES, Some(post(true, &[]))),
        ("bore", "flat-6", HOLES, Some(post(false, &[]))),
    );
    case("param.geometry.drive.empty", "bore", "flat-6", HOLES, "");
    case(
        "param.geometry.hole_diameter.range",
        "bore",
        "flat-6",
        "[geometry]\nhole_diameter = { min = 5 }\n",
        "[geometry]\nhole_diameter = { min = 5, max = 4 }\n",
    );
}

const DRILLS: &str = "[geometry]\nhole_diameter = { min = 4.9, max = 5.1 }\n";

fn drill(passes: &str) -> String {
    format!("{DRILLS}[passes]\n{passes}")
}

#[test]
fn drill_codes() {
    clean("drill", "drill-5", DRILLS);
    case_with(
        "param.drill.tool_shape",
        ("drill", "drill-5", DRILLS, None),
        ("drill", "flat-6", DRILLS, None),
    );
    case_with(
        "param.drill.pitch.required",
        ("drill", "tap-6", &drill("cycle = \"tap\"\n"), None),
        ("drill", "drill-5", &drill("cycle = \"tap\"\n"), None),
    );
    case_with(
        "param.drill.pitch.override",
        (
            "drill",
            "tap-6",
            &drill("cycle = \"tap\"\npitch = 1.0\n"),
            None,
        ),
        (
            "drill",
            "tap-6",
            &drill("cycle = \"tap\"\npitch = 1.25\n"),
            None,
        ),
    );
    let tap = drill("cycle = \"tap\"\n");
    case_with(
        "param.drill.tap.post",
        ("drill", "tap-6", &tap, Some(post(true, &["tap"]))),
        ("drill", "tap-6", &tap, Some(post(true, &["drill", "peck"]))),
    );
    // `dwell` is posted as a spot cycle.
    let dwell = drill("cycle = \"dwell\"\n");
    case_with(
        "param.drill.cycle.expanded",
        ("drill", "drill-5", &dwell, Some(post(true, &["spot"]))),
        ("drill", "drill-5", &dwell, Some(post(true, &["drill"]))),
    );
    case(
        "param.drill.peck_depth.deep",
        "drill",
        "drill-5",
        &drill("cycle = \"peck\"\npeck_depth = 30\n"),
        &drill("cycle = \"peck\"\npeck_depth = 31\n"),
    );
    case("param.geometry.drive.empty", "drill", "drill-5", DRILLS, "");
}

#[test]
fn optimised_load_2d_codes() {
    clean("optimised_load_2d", "flat-6", CURVES);
    case(
        "param.optimised_load_2d.optimal_load.range",
        "optimised_load_2d",
        "flat-6",
        &format!("{CURVES}[passes]\noptimal_load = 3\n"),
        &format!("{CURVES}[passes]\noptimal_load = 3.1\n"),
    );
    case(
        "param.optimised_load_2d.optimal_load.high",
        "optimised_load_2d",
        "flat-6",
        &format!("{CURVES}[passes]\noptimal_load = 1.7\n"),
        &format!("{CURVES}[passes]\noptimal_load = 1.9\n"),
    );
    case(
        "param.optimised_load_2d.min_cutting_radius.large",
        "optimised_load_2d",
        "flat-6",
        &format!("{CURVES}[passes]\nmin_cutting_radius = 3\n"),
        &format!("{CURVES}[passes]\nmin_cutting_radius = 3.5\n"),
    );
    case(
        "param.passes.step_down.flute",
        "optimised_load_2d",
        "flat-6",
        &format!("{CURVES}[passes]\nstep_down = 22\n"),
        &format!("{CURVES}[passes]\nstep_down = 23\n"),
    );
    case(
        "param.geometry.drive.empty",
        "optimised_load_2d",
        "flat-6",
        CURVES,
        "",
    );
}

#[test]
fn engrave_codes() {
    clean("engrave", "v-90", CURVES);
    case_with(
        "param.engrave.tool_shape",
        ("engrave", "v-90", CURVES, None),
        ("engrave", "flat-6", CURVES, None),
    );
    // Cone height (6 − 0.1) / tan 45° = 5.9; flute length 8.
    case(
        "param.engrave.max_depth.cone",
        "engrave",
        "v-90",
        &format!("{CURVES}[passes]\nmax_depth = 5.9\n"),
        &format!("{CURVES}[passes]\nmax_depth = 6\n"),
    );
    // Cone height 6 / tan 30° = 10.4, flute length 4.
    case(
        "param.engrave.max_depth.flute",
        "engrave",
        "v-60-short",
        &format!("{CURVES}[passes]\nmax_depth = 4\n"),
        &format!("{CURVES}[passes]\nmax_depth = 4.5\n"),
    );
    case("param.geometry.drive.empty", "engrave", "v-90", CURVES, "");
}
