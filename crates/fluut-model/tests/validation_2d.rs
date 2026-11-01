// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every D15b §2–§3 parameter validation code of the 2-D and hole types:
//! one passing and one failing case each (D15 §6). Calculation codes
//! (`cam.*`) are tested in `fluut-cam`.

use fluut_model::project::{SetupKind, Toolpath, read};
use fluut_model::resolve::ResolvedHeights;
use fluut_model::tools::{Tool, ToolLibrary};
use fluut_model::validate::{PostCaps, ValidateCx, validate};

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
