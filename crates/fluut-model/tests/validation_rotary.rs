// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every D15b §1.3 and §6 parameter validation code of the rotary types
//! that needs no machine or geometry: one passing and one failing case
//! each (D15 §6). Rules on the machine's rotary axis and the X range run at
//! calculation start and are tested in `fluut-cam`.

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
id = "ball-6"
shape = "ball"
diameter = 6
flute_length = 20
  [[tool.preset]]
  material = "MDF"
  rpm = 18000
  feed = 2500
  plunge = 800
  stepover = 1
  step_down = 2
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

/// Radii (D15b §1.3).
const HEIGHTS: ResolvedHeights = ResolvedHeights {
    clearance: 40.0,
    retract: 35.0,
    feed: 32.0,
    top: 20.0,
    bottom: 5.0,
};

fn codes(t: &Toolpath, tool_id: &str) -> Vec<String> {
    let tool = tool(tool_id);
    let cx = ValidateCx {
        tool: Some(&tool),
        material: Some("MDF"),
        setup_kind: SetupKind::Rotary,
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
    for kind in [
        "rotary_rough",
        "rotary_along_x",
        "rotary_around_a",
        "rotary_spiral",
    ] {
        assert_eq!(
            codes(&tp(kind, ""), "ball-6"),
            Vec::<String>::new(),
            "{kind}"
        );
    }
}

#[test]
fn setup_kind_and_axis_heights() {
    let tool = tool("ball-6");
    for kind in ["rotary_rough", "rotary_spiral"] {
        let t = tp(kind, "");
        let in_3x = ValidateCx {
            setup_kind: SetupKind::ThreeAxis,
            ..ValidateCx::new(Some(&tool))
        };
        let c: Vec<String> = validate(&t, &in_3x).into_iter().map(|d| d.code).collect();
        assert!(c.contains(&"param.type.setup_kind".to_owned()), "{c:?}");
        assert!(!codes(&t, "ball-6").contains(&"param.type.setup_kind".to_owned()));
    }
    // 3-D types are not allowed in rotary Setups.
    assert!(codes(&tp("pocket_3d", ""), "flat-10").contains(&"param.type.setup_kind".to_owned()));
    let t = tp("rotary_around_a", "");
    let at = |bottom: f64| -> Vec<String> {
        let h = ResolvedHeights { bottom, ..HEIGHTS };
        let cx = ValidateCx {
            setup_kind: SetupKind::Rotary,
            heights: Some(&h),
            stock_bottom: Some(0.0),
            ..ValidateCx::new(Some(&tool))
        };
        validate(&t, &cx).into_iter().map(|d| d.code).collect()
    };
    assert!(!at(0.0).contains(&"param.heights.below_axis".to_owned()));
    assert!(at(-0.1).contains(&"param.heights.below_axis".to_owned()));
    // Not `below_stock`: the stock bottom is the axis.
    assert!(!at(-0.1).contains(&"param.heights.below_stock".to_owned()));
}

#[test]
fn rotary_rough_codes() {
    let k = "rotary_rough";
    case(
        "param.passes.stepover.max",
        k,
        "flat-10",
        "[passes]\nstepover_x = 10\n",
        "[passes]\nstepover_x = 10.5\n",
    );
    case(
        "param.passes.step_down.flute",
        k,
        "flat-10",
        "[passes]\nradial_step_down = 22\n",
        "[passes]\nradial_step_down = 22.5\n",
    );
}

#[test]
fn scallop_radius_needs_a_corner() {
    for k in ["rotary_along_x", "rotary_around_a", "rotary_spiral"] {
        let pass = codes(&tp(k, "[passes]\nscallop_height = 0.01\n"), "ball-6");
        let fail = codes(&tp(k, "[passes]\nscallop_height = 0.01\n"), "flat-10");
        let code = format!("param.{k}.scallop.radius");
        assert!(!pass.contains(&code), "{pass:?}");
        assert!(fail.contains(&code), "{fail:?}");
    }
}

#[test]
fn rotary_along_x_codes() {
    let k = "rotary_along_x";
    case(
        &format!("param.{k}.stepover_and_scallop"),
        k,
        "ball-6",
        "[passes]\nscallop_height = 0.01\n",
        "[passes]\nstepover_deg = 1\nscallop_height = 0.01\n",
    );
    case(
        &format!("param.{k}.scallop.range"),
        k,
        "ball-6",
        "[passes]\nscallop_height = 2.9\n",
        "[passes]\nscallop_height = 3\n",
    );
    case(
        "param.rotary_along_x.a_range",
        k,
        "ball-6",
        "[passes]\na_start_deg = 10\na_end_deg = 370\n",
        "[passes]\na_start_deg = 10\na_end_deg = 371\n",
    );
    case(
        "param.rotary_along_x.a_range",
        k,
        "ball-6",
        "[passes]\na_start_deg = 90\na_end_deg = 91\n",
        "[passes]\na_start_deg = 90\na_end_deg = 90\n",
    );
    // 6 mm at the top radius 20 is 17.19°.
    case(
        "param.passes.stepover.max",
        k,
        "ball-6",
        "[passes]\nstepover_deg = 17\n",
        "[passes]\nstepover_deg = 17.5\n",
    );
}

#[test]
fn rotary_around_a_and_spiral_codes() {
    for (k, step) in [
        ("rotary_around_a", "stepover_x"),
        ("rotary_spiral", "advance"),
    ] {
        case(
            &format!("param.{k}.stepover_and_scallop"),
            k,
            "ball-6",
            "[passes]\nscallop_height = 0.01\n",
            &format!("[passes]\n{step} = 1\nscallop_height = 0.01\n"),
        );
        case(
            &format!("param.{k}.scallop.range"),
            k,
            "ball-6",
            "[passes]\nscallop_height = 2.9\n",
            "[passes]\nscallop_height = 3\n",
        );
        case(
            "param.passes.stepover.max",
            k,
            "ball-6",
            &format!("[passes]\n{step} = 6\n"),
            &format!("[passes]\n{step} = 6.1\n"),
        );
    }
}

#[test]
fn x_range_references() {
    use fluut_model::resolve::{XEnv, resolve};
    let t = tp(
        "rotary_around_a",
        "[geometry]\nx_start = { ref = \"stock_left\", offset = 2 }\nx_end = { ref = \"work_zero\", offset = 50 }\n",
    );
    let r = resolve(&t.params, &tool("ball-6"), None);
    let (a, b) = r.geometry.x_range.clone().unwrap();
    let env = XEnv {
        stock: (-1.0, 101.0),
        model: (0.0, 100.0),
    };
    assert_eq!((env.resolve(&a), env.resolve(&b)), (1.0, 50.0));
    let d = resolve(&tp("rotary_spiral", "").params, &tool("ball-6"), None);
    let (a, b) = d.geometry.x_range.unwrap();
    assert_eq!((env.resolve(&a), env.resolve(&b)), (0.0, 100.0));
    // An unknown X reference is a load error.
    let src = "format = \"fluut.project/1\"\n[[setup]]\nid = \"s\"\n[[setup.toolpath]]\nid = \"t\"\ntype = \"rotary_spiral\"\ntool = \"lib#x\"\n[setup.toolpath.geometry]\nx_start = { ref = \"stock_top\" }\n";
    assert!(read(src).is_err());
}
