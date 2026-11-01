// SPDX-License-Identifier: MIT OR Apache-2.0

//! The 3-D finishing types of D15b §5 (`contour`, `horizontal`, `spiral`,
//! `scallop`, `steep_and_shallow`): every example round-trips, defaults
//! resolve, and every parameter-only validation code has a passing and a
//! failing case (D15b §8). Calculation codes (`cam.…`) are tested in
//! `fluut-cam`.

use fluut_model::params::ToolpathTypeId;
use fluut_model::project::{SetupKind, Toolpath, read, write};
use fluut_model::resolve::{
    CutDirection, OffsetOrder, ParallelDirection, Pattern, RegionOrder, ResolvedHeights,
    ShallowPattern, resolve,
};
use fluut_model::tools::{Tool, ToolLibrary};
use fluut_model::validate::{ValidateCx, validate};

const HEADER: &str = "format = \"fluut.project/1\"
name   = \"D15b examples\"
units  = \"mm\"

[[setup]]
id   = \"s1\"
name = \"Top\"
kind = \"three_axis\"
";

/// The ```toml blocks of D15b §5, by the heading they follow.
fn d15b_finishing_examples() -> Vec<(String, String)> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/design/D15b-toolpath-parameters-2.md"
    );
    let doc = std::fs::read_to_string(path).unwrap();
    let mut out = Vec::new();
    let mut section = String::new();
    let mut heading = String::new();
    let mut block: Option<String> = None;
    for line in doc.lines() {
        if let Some(b) = &mut block {
            if line == "```" {
                if section.starts_with("## 5.") {
                    out.push((heading.clone(), block.take().unwrap()));
                }
                block = None;
            } else {
                b.push_str(line);
                b.push('\n');
            }
        } else if line.starts_with("## ") {
            section = line.to_owned();
        } else if line.starts_with("### ") {
            heading = line.to_owned();
        } else if line == "```toml" {
            block = Some(String::new());
        }
    }
    out
}

#[test]
fn every_d15b_finishing_example_round_trips() {
    let examples = d15b_finishing_examples();
    let headings: Vec<&str> = examples.iter().map(|(h, _)| h.as_str()).collect();
    assert_eq!(
        headings,
        [
            "### 5.1 `contour`",
            "### 5.2 `horizontal`",
            "### 5.3 `spiral`",
            "### 5.4 `scallop`",
            "### 5.5 `steep_and_shallow`"
        ]
    );
    for (h, block) in examples {
        let src = format!("{HEADER}\n{block}");
        let loaded = read(&src).unwrap_or_else(|e| panic!("{h}: {e}"));
        assert!(loaded.warnings.is_empty(), "{h}: {:?}", loaded.warnings);
        assert_eq!(write(&loaded.project), src, "{h}");
    }
}

fn tools() -> Vec<Tool> {
    let preset = "  [[tool.preset]]\n  material = \"MDF\"\n  rpm = 18000\n  feed = 2500\n  \
                  plunge = 800\n  stepover = 1.5\n  step_down = 3\n";
    let src = format!(
        "format = \"fluut.tools/1\"\nid = \"lib\"\n\
         [[tool]]\nid = \"flat-6\"\nshape = \"flat\"\ndiameter = 6\nflute_length = 22\n{preset}\
         [[tool]]\nid = \"bull-6\"\nshape = \"bull\"\ndiameter = 6\ncorner_radius = 1\n{preset}\
         [[tool]]\nid = \"ball-6\"\nshape = \"ball\"\ndiameter = 6\nflute_length = 20\n{preset}"
    );
    ToolLibrary::parse(&src).unwrap().0.tools
}

fn tool(id: &str) -> Tool {
    tools().into_iter().find(|t| t.id == id).unwrap()
}

/// A toolpath of `kind` with the given `[passes]` keys.
fn tp(kind: &str, passes: &str) -> Toolpath {
    let src = format!(
        "format = \"fluut.project/1\"\n[[setup]]\nid = \"s\"\n[[setup.toolpath]]\n\
         id = \"t\"\ntype = \"{kind}\"\ntool = \"lib#ball-6\"\n\
         [setup.toolpath.passes]\n{passes}\n"
    );
    let p = read(&src).unwrap_or_else(|e| panic!("{e}\n{src}")).project;
    p.setups[0].toolpaths[0].clone()
}

const HEIGHTS: ResolvedHeights = ResolvedHeights {
    clearance: 30.0,
    retract: 25.0,
    feed: 22.0,
    top: 20.0,
    bottom: 20.0,
};

fn codes(t: &Toolpath, tool_id: &str) -> Vec<String> {
    let tl = tool(tool_id);
    let cx = ValidateCx {
        tool: Some(&tl),
        material: Some("MDF"),
        setup_kind: SetupKind::ThreeAxis,
        heights: Some(&HEIGHTS),
        stock_bottom: None,
        machine: None,
        post: None,
    };
    validate(t, &cx).into_iter().map(|d| d.code).collect()
}

/// `code` is absent for `pass` and present for `fail`.
fn case(code: &str, kind: &str, pass: (&str, &str), fail: (&str, &str)) {
    let p = codes(&tp(kind, pass.0), pass.1);
    assert!(
        !p.contains(&code.to_owned()),
        "{code} raised by the passing case: {p:?}"
    );
    let f = codes(&tp(kind, fail.0), fail.1);
    assert!(
        f.contains(&code.to_owned()),
        "{code} not raised by the failing case: {f:?}"
    );
}

const KINDS: [&str; 5] = [
    "contour",
    "horizontal",
    "spiral",
    "scallop",
    "steep_and_shallow",
];

#[test]
fn types_are_registered() {
    for k in KINDS {
        let id = ToolpathTypeId::parse(k).unwrap();
        assert_eq!(id.as_str(), k);
        assert!(ToolpathTypeId::ALL.contains(&id));
    }
}

#[test]
fn baseline_has_no_diagnostics() {
    // Top = bottom is allowed, as for parallel.
    for k in KINDS {
        let tl = if k == "horizontal" {
            "flat-6"
        } else {
            "ball-6"
        };
        assert_eq!(codes(&tp(k, ""), tl), Vec::<String>::new(), "{k}");
    }
}

#[test]
fn defaults_resolve() {
    let b = tool("ball-6");
    let r = |k: &str, s: &str| resolve(&tp(k, s).params, &b, Some("MDF"));
    let c = r("contour", "");
    let c = c.contour3d().unwrap();
    assert!((c.step_down - 0.6).abs() < 1e-12);
    assert_eq!(c.scallop_height, None);
    assert_eq!((c.min_slope_deg, c.max_slope_deg), (0.0, 90.0));
    assert_eq!(c.direction, CutDirection::Climb);
    let h = r("horizontal", "");
    let h = h.horizontal().unwrap();
    assert_eq!(h.pattern, Pattern::Offset);
    assert_eq!(h.stepover, Some(1.5));
    let s = r("spiral", "");
    let s = s.spiral().unwrap();
    assert_eq!(s.centre, None);
    assert_eq!(s.direction, OffsetOrder::InsideOut);
    assert!(s.ccw);
    assert!(!s.machine_corners);
    let sc = r("scallop", "");
    let sc = sc.scallop().unwrap();
    assert_eq!(sc.direction, OffsetOrder::OutsideIn);
    assert_eq!(sc.cut_direction, CutDirection::Climb);
    assert!(!sc.continuous);
    let ss = r("steep_and_shallow", "");
    let ss = ss.steep_and_shallow().unwrap();
    assert_eq!(ss.threshold_deg, 45.0);
    assert!((ss.overlap - 1.2).abs() < 1e-12);
    assert_eq!(ss.region_order, RegionOrder::SteepFirst);
    assert!((ss.steep.step_down - 0.6).abs() < 1e-12);
    assert_eq!(ss.shallow.pattern, ShallowPattern::Parallel);
    assert_eq!(ss.shallow.stepover, Some(1.5));
    assert_eq!(ss.shallow.direction, ParallelDirection::Zigzag);
    let ss = r("steep_and_shallow", "shallow = { pattern = \"scallop\" }");
    let ss = ss.steep_and_shallow().unwrap();
    assert_eq!(ss.shallow.direction, ParallelDirection::Climb);
}

#[test]
fn contour_codes() {
    let k = "contour";
    case(
        "param.contour.scallop.radius",
        k,
        ("scallop_height = 0.01", "ball-6"),
        ("scallop_height = 0.01", "flat-6"),
    );
    case(
        "param.contour.scallop.range",
        k,
        ("scallop_height = 0.9", "bull-6"),
        ("scallop_height = 1", "bull-6"),
    );
    case(
        "param.contour.slope.order",
        k,
        ("min_slope_deg = 10\nmax_slope_deg = 20", "ball-6"),
        ("min_slope_deg = 20\nmax_slope_deg = 20", "ball-6"),
    );
    case(
        "param.contour.slope.range",
        k,
        ("min_slope_deg = 0", "ball-6"),
        ("min_slope_deg = -1", "ball-6"),
    );
    case(
        "param.passes.step_down.flute",
        k,
        ("step_down = 20", "ball-6"),
        ("step_down = 21", "ball-6"),
    );
    case(
        "param.passes.step_down.range",
        k,
        ("step_down = 0.1", "ball-6"),
        ("step_down = 0", "ball-6"),
    );
}

#[test]
fn horizontal_codes() {
    let k = "horizontal";
    case(
        "param.horizontal.tool_shape",
        k,
        ("", "bull-6"),
        ("", "ball-6"),
    );
    case(
        "param.passes.stepover.max",
        k,
        ("stepover = 6", "ball-6"),
        ("stepover = 6.5", "ball-6"),
    );
    case(
        "param.passes.stepover.flat_width",
        k,
        ("stepover = 4", "bull-6"),
        ("stepover = 4.5", "bull-6"),
    );
    case(
        "param.passes.stock_to_leave_axial.range",
        k,
        ("stock_to_leave_axial = 0", "flat-6"),
        ("stock_to_leave_axial = -0.1", "flat-6"),
    );
}

#[test]
fn spiral_codes() {
    let k = "spiral";
    case(
        "param.spiral.stepover_and_scallop",
        k,
        ("scallop_height = 0.01", "ball-6"),
        ("scallop_height = 0.01\nstepover = 1", "ball-6"),
    );
    case(
        "param.spiral.scallop.radius",
        k,
        ("scallop_height = 0.01", "bull-6"),
        ("scallop_height = 0.01", "flat-6"),
    );
    case(
        "param.spiral.scallop.range",
        k,
        ("scallop_height = 2.9", "ball-6"),
        ("scallop_height = 3", "ball-6"),
    );
    case(
        "param.passes.stepover.max",
        k,
        ("stepover = 6", "ball-6"),
        ("stepover = 7", "ball-6"),
    );
}

#[test]
fn scallop_codes() {
    let k = "scallop";
    case(
        "param.scallop.stepover_and_scallop",
        k,
        ("stepover = 0.3", "ball-6"),
        ("stepover = 0.3\nscallop_height = 0.01", "ball-6"),
    );
    case(
        "param.scallop.scallop.radius",
        k,
        ("scallop_height = 0.01", "ball-6"),
        ("scallop_height = 0.01", "flat-6"),
    );
    case(
        "param.scallop.scallop.range",
        k,
        ("scallop_height = 0.5", "bull-6"),
        ("scallop_height = 1.5", "bull-6"),
    );
    case(
        "param.scallop.slope.order",
        k,
        ("max_slope_deg = 60", "ball-6"),
        ("min_slope_deg = 70\nmax_slope_deg = 60", "ball-6"),
    );
    case(
        "param.scallop.slope.range",
        k,
        ("max_slope_deg = 90", "ball-6"),
        ("max_slope_deg = 90.5", "ball-6"),
    );
    case(
        "param.passes.stepover.max",
        k,
        ("stepover = 3", "ball-6"),
        ("stepover = 6.01", "ball-6"),
    );
}

#[test]
fn steep_and_shallow_codes() {
    let k = "steep_and_shallow";
    case(
        "param.steep_and_shallow.threshold.range",
        k,
        ("threshold_deg = 89", "ball-6"),
        ("threshold_deg = 90", "ball-6"),
    );
    case(
        "param.steep_and_shallow.threshold.range",
        k,
        ("threshold_deg = 1", "ball-6"),
        ("threshold_deg = 0", "ball-6"),
    );
    case(
        "param.steep_and_shallow.shallow.stepover_and_scallop",
        k,
        ("shallow = { scallop_height = 0.01 }", "ball-6"),
        (
            "shallow = { stepover = 1, scallop_height = 0.01 }",
            "ball-6",
        ),
    );
    case(
        "param.steep_and_shallow.scallop.radius",
        k,
        ("steep = { scallop_height = 0.01 }", "ball-6"),
        ("steep = { scallop_height = 0.01 }", "flat-6"),
    );
    case(
        "param.steep_and_shallow.scallop.radius",
        k,
        ("shallow = { scallop_height = 0.01 }", "bull-6"),
        ("shallow = { scallop_height = 0.01 }", "flat-6"),
    );
    case(
        "param.steep_and_shallow.scallop.range",
        k,
        ("shallow = { scallop_height = 0.9 }", "bull-6"),
        ("shallow = { scallop_height = 1 }", "bull-6"),
    );
    case(
        "param.steep_and_shallow.scallop.range",
        k,
        ("steep = { scallop_height = 0.9 }", "bull-6"),
        ("steep = { scallop_height = 1.1 }", "bull-6"),
    );
    case(
        "param.steep_and_shallow.shallow.direction",
        k,
        (
            "shallow = { pattern = \"scallop\", direction = \"climb\" }",
            "ball-6",
        ),
        (
            "shallow = { pattern = \"scallop\", direction = \"zigzag\" }",
            "ball-6",
        ),
    );
    case(
        "param.passes.stepover.max",
        k,
        ("shallow = { stepover = 6 }", "ball-6"),
        ("shallow = { stepover = 6.5 }", "ball-6"),
    );
    case(
        "param.steep_and_shallow.overlap.range",
        k,
        ("overlap = 0", "ball-6"),
        ("overlap = -1", "ball-6"),
    );
    case(
        "param.steep_and_shallow.steep.step_down.range",
        k,
        ("steep = { step_down = 0.1 }", "ball-6"),
        ("steep = { step_down = 0 }", "ball-6"),
    );
}

#[test]
fn stock_to_leave_follows_the_3d_rule() {
    // > −r (corner radius), ≥ 0 for flat tools (D15 §2.5).
    for k in KINDS {
        let tl = if k == "horizontal" {
            "flat-6"
        } else {
            "ball-6"
        };
        let (pass, fail) = if tl == "flat-6" {
            ("0", "-0.01")
        } else {
            ("-2.9", "-3")
        };
        case(
            "param.passes.stock_to_leave.range",
            k,
            (&format!("stock_to_leave = {pass}"), tl),
            (&format!("stock_to_leave = {fail}"), tl),
        );
    }
}

#[test]
fn ramp_is_not_a_field() {
    for k in KINDS {
        let src = format!(
            "format = \"fluut.project/1\"\n[[setup]]\nid = \"s\"\n[[setup.toolpath]]\n\
             id = \"t\"\ntype = \"{k}\"\ntool = \"lib#ball-6\"\n\
             [setup.toolpath.linking]\nramp = {{ kind = \"helix\" }}\n"
        );
        assert!(
            read(&src).map_or(true, |l| !l.warnings.is_empty()),
            "{k}: ramp accepted"
        );
    }
}
