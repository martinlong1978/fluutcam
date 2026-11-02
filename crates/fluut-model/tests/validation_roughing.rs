// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every D15b §4 parameter validation code of the 3-D roughing types: one
//! passing and one failing case each (D15 §6). Calculation codes (`cam.*`)
//! are tested in `fluut-cam`.

use fluut_model::project::{SetupKind, Toolpath, read};
use fluut_model::resolve::ResolvedHeights;
use fluut_model::tools::{Tool, ToolLibrary};
use fluut_model::validate::{ValidateCx, validate};

const LIB: &str = r#"
format = "fluut.tools/1"
id = "lib"
[[tool]]
id = "flat-10"
shape = "flat"
diameter = 10
flute_length = 22
  [[tool.preset]]
  material = "MDF"
  rpm = 18000
  feed = 2500
  plunge = 800
  stepover = 4
  step_down = 15
[[tool]]
id = "bull-10"
shape = "bull"
diameter = 10
corner_radius = 2
flute_length = 30
  [[tool.preset]]
  material = "MDF"
  rpm = 18000
  feed = 2500
  plunge = 800
  stepover = 4
  step_down = 6
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
    read(&src)
        .unwrap_or_else(|e| panic!("{e}\n{src}"))
        .project
        .setups[0]
        .toolpaths[0]
        .clone()
}

const HEIGHTS: ResolvedHeights = ResolvedHeights {
    clearance: 40.0,
    retract: 35.0,
    feed: 32.0,
    top: 30.0,
    bottom: 0.0,
};

fn codes(t: &Toolpath, tool_id: &str) -> Vec<String> {
    let tool = tool(tool_id);
    let cx = ValidateCx {
        tool: Some(&tool),
        material: Some("MDF"),
        setup_kind: SetupKind::ThreeAxis,
        heights: Some(&HEIGHTS),
        stock_bottom: None,
        machine: None,
        post: None,
    };
    validate(t, &cx).into_iter().map(|d| d.code).collect()
}

fn case(code: &str, kind: &str, tool_id: &str, pass: &str, fail: &str) {
    let p = codes(&tp(kind, pass), tool_id);
    assert!(
        !p.contains(&code.to_owned()),
        "{code} raised by the passing case: {p:?}"
    );
    let f = codes(&tp(kind, fail), tool_id);
    assert!(
        f.contains(&code.to_owned()),
        "{code} not raised by the failing case: {f:?}"
    );
}

#[test]
fn defaults_are_clean() {
    for kind in ["optimised_load_3d", "pocket_3d"] {
        assert_eq!(
            codes(&tp(kind, ""), "flat-10"),
            Vec::<String>::new(),
            "{kind}"
        );
    }
}

#[test]
fn optimised_load_3d_codes() {
    let k = "optimised_load_3d";
    case(
        "param.optimised_load_3d.optimal_load.range",
        k,
        "flat-10",
        "[passes]\noptimal_load = 5\n",
        "[passes]\noptimal_load = 5.1\n",
    );
    case(
        "param.optimised_load_3d.optimal_load.high",
        k,
        "flat-10",
        "[passes]\noptimal_load = 3\n",
        "[passes]\noptimal_load = 3.1\n",
    );
    case(
        "param.optimised_load_3d.fine_step_down.range",
        k,
        "flat-10",
        "[passes]\nstep_down = 6\nfine_step_down = 5.9\n",
        "[passes]\nstep_down = 6\nfine_step_down = 6\n",
    );
    case(
        "param.optimised_load_3d.min_cutting_radius.large",
        k,
        "flat-10",
        "[passes]\nmin_cutting_radius = 5\n",
        "[passes]\nmin_cutting_radius = 5.1\n",
    );
    case(
        "param.passes.step_down.flute",
        k,
        "flat-10",
        "[passes]\nstep_down = 22\n",
        "[passes]\nstep_down = 22.5\n",
    );
}

#[test]
fn pocket_3d_codes() {
    let k = "pocket_3d";
    case(
        "param.passes.stepover.max",
        k,
        "flat-10",
        "[passes]\nstepover = 10\n",
        "[passes]\nstepover = 10.5\n",
    );
    case(
        "param.passes.stepover.flat_width",
        k,
        "bull-10",
        "[passes]\nstepover = 6\n",
        "[passes]\nstepover = 6.5\n",
    );
    case(
        "param.passes.step_down.flute",
        k,
        "bull-10",
        "[passes]\nstep_down = 30\n",
        "[passes]\nstep_down = 31\n",
    );
}

#[test]
fn rest_and_kinds() {
    let t = tp("pocket_3d", "[geometry]\nrest_from = \"previous\"\n");
    assert!(t.params.reads_in_process());
    assert!(t.kind().updates_stock());
    let tool = tool("flat-10");
    let cx = ValidateCx {
        setup_kind: SetupKind::Rotary,
        ..ValidateCx::new(Some(&tool))
    };
    let d: Vec<String> = validate(&tp("optimised_load_3d", ""), &cx)
        .into_iter()
        .map(|d| d.code)
        .collect();
    assert!(d.contains(&"param.type.setup_kind".to_owned()), "{d:?}");
}
