// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every D15 parameter validation code for contour_2d, pocket_2d and the
//! common fields (and the parameter-only parallel rules): one passing and
//! one failing case each (D15 §6).

use fluut_model::project::{SetupKind, Toolpath, read};
use fluut_model::resolve::ResolvedHeights;
use fluut_model::tools::{Tool, ToolLibrary};
use fluut_model::validate::{CycleSet, MachineLimits, PostCaps, ValidateCx, validate};

fn tools() -> Vec<Tool> {
    let src = r#"
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
id = "bull-6"
shape = "bull"
diameter = 6
corner_radius = 1
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
  stepover = 0.5
  step_down = 3
"#;
    ToolLibrary::parse(src).unwrap().0.tools
}

fn tool(id: &str) -> Tool {
    tools().into_iter().find(|t| t.id == id).unwrap()
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
         id = \"t\"\ntype = \"{kind}\"\ntool = \"lib#flat-6\"\n{groups}"
    );
    let p = read(&src).unwrap_or_else(|e| panic!("{e}\n{src}")).project;
    p.setups[0].toolpaths[0].clone()
}

const DRIVE: &str = "[geometry]\ndrive_curves = [{ body = \"b\" }]\n";
const HEIGHTS: ResolvedHeights = ResolvedHeights {
    clearance: 30.0,
    retract: 25.0,
    feed: 22.0,
    top: 20.0,
    bottom: 0.0,
};

#[derive(Clone)]
struct Cx {
    tool: Option<Tool>,
    material: Option<&'static str>,
    kind: SetupKind,
    heights: Option<ResolvedHeights>,
    stock_bottom: Option<f64>,
    machine: Option<MachineLimits>,
    post: Option<PostCaps>,
}

impl Default for Cx {
    fn default() -> Self {
        Cx {
            tool: Some(tool("flat-6")),
            material: Some("MDF"),
            kind: SetupKind::ThreeAxis,
            heights: Some(HEIGHTS),
            stock_bottom: None,
            machine: None,
            post: None,
        }
    }
}

fn codes(t: &Toolpath, c: &Cx) -> Vec<String> {
    let cx = ValidateCx {
        tool: c.tool.as_ref(),
        material: c.material,
        setup_kind: c.kind,
        heights: c.heights.as_ref(),
        stock_bottom: c.stock_bottom,
        machine: c.machine.as_ref(),
        post: c.post.as_ref(),
    };
    validate(t, &cx)
        .into_iter()
        .inspect(|d| assert_eq!(d.toolpath.as_deref(), Some("t")))
        .map(|d| d.code)
        .collect()
}

/// `code` is absent for `pass` and present for `fail`.
fn case(code: &str, pass: (&Toolpath, &Cx), fail: (&Toolpath, &Cx)) {
    let p = codes(pass.0, pass.1);
    assert!(
        !p.contains(&code.to_owned()),
        "{code} raised by the passing case: {p:?}"
    );
    let f = codes(fail.0, fail.1);
    assert!(
        f.contains(&code.to_owned()),
        "{code} not raised by the failing case: {f:?}"
    );
}

/// Same context, two parameter sets.
fn params_case(code: &str, kind: &str, pass: &str, fail: &str) {
    let c = Cx::default();
    case(code, (&tp(kind, pass), &c), (&tp(kind, fail), &c));
}

/// Same parameters, two contexts.
fn cx_case(code: &str, t: &Toolpath, pass: Cx, fail: Cx) {
    case(code, (t, &pass), (t, &fail));
}

#[test]
fn baseline_has_no_diagnostics() {
    for kind in ["contour_2d", "pocket_2d"] {
        assert_eq!(
            codes(&tp(kind, DRIVE), &Cx::default()),
            Vec::<String>::new(),
            "{kind}"
        );
    }
    let par = tp("parallel", "");
    assert_eq!(codes(&par, &Cx::default()), Vec::<String>::new());
}

#[test]
fn item_and_tool_codes() {
    let t = tp("contour_2d", DRIVE);
    cx_case(
        "param.tool.missing",
        &t,
        Cx::default(),
        Cx {
            tool: None,
            ..Cx::default()
        },
    );
    let mut zero = tool("flat-6");
    zero.diameter = 0.0;
    cx_case(
        "param.tool.diameter",
        &t,
        Cx::default(),
        Cx {
            tool: Some(zero),
            ..Cx::default()
        },
    );
    cx_case(
        "param.type.setup_kind",
        &t,
        Cx {
            kind: SetupKind::Indexed,
            ..Cx::default()
        },
        Cx {
            kind: SetupKind::Rotary,
            ..Cx::default()
        },
    );
    cx_case(
        "param.required",
        &t,
        Cx::default(),
        Cx {
            material: None,
            ..Cx::default()
        },
    );
    let set = tp(
        "contour_2d",
        &format!(
            "{DRIVE}[tool_params]\nspindle_rpm = 1\nfeed = {{ cut = 1, plunge = 1 }}\n[passes]\nstep_down = 1\n"
        ),
    );
    assert!(
        !codes(
            &set,
            &Cx {
                material: None,
                ..Cx::default()
            }
        )
        .contains(&"param.required".into())
    );
}

#[test]
fn tool_group_codes() {
    let m = Some(MachineLimits {
        rpm: (6000.0, 24000.0),
        max_cut_feed: 10000.0,
    });
    let c = Cx {
        machine: m,
        ..Cx::default()
    };
    let ok = tp("pocket_2d", DRIVE);
    let fast = tp(
        "pocket_2d",
        &format!("{DRIVE}[tool_params]\nspindle_rpm = 30000\n"),
    );
    case(
        "param.tool_params.spindle_rpm.machine_range",
        (&ok, &c),
        (&fast, &c),
    );
    let feed = tp(
        "pocket_2d",
        &format!("{DRIVE}[tool_params]\nfeed = {{ cut = 12000 }}\n"),
    );
    case("param.tool_params.feed.machine_max", (&ok, &c), (&feed, &c));
    let c = Cx {
        post: Some(PostCaps {
            cutter_comp: false,
            coolant: false,
            helix: true,
            cycles: CycleSet::default(),
        }),
        ..Cx::default()
    };
    let flood = tp(
        "pocket_2d",
        &format!("{DRIVE}[tool_params]\ncoolant = \"flood\"\n"),
    );
    case(
        "param.tool_params.coolant.unsupported",
        (&ok, &c),
        (&flood, &c),
    );
    params_case(
        "param.tool_params.feed.ramp.range",
        "pocket_2d",
        DRIVE,
        &format!("{DRIVE}[tool_params]\nfeed = {{ ramp = 0 }}\n"),
    );
}

#[test]
fn geometry_codes() {
    params_case("param.geometry.drive.empty", "contour_2d", DRIVE, "");
    params_case(
        "param.geometry.overlap",
        "parallel",
        "[geometry]\ndrive_faces = [{ face = \"f1\" }]\nexcluded_faces = [{ face = \"f2\" }]\n",
        "[geometry]\ndrive_faces = [{ face = \"f1\" }]\nexcluded_faces = [{ face = \"f1\" }]\n",
    );
    params_case(
        "param.geometry.rest_from.unsupported",
        "pocket_2d",
        &format!("{DRIVE}rest_from = \"off\"\n"),
        &format!("{DRIVE}rest_from = \"previous\"\n"),
    );
}

#[test]
fn heights_codes() {
    let t = tp("contour_2d", DRIVE);
    let bad = ResolvedHeights {
        top: -1.0,
        ..HEIGHTS
    };
    cx_case(
        "param.heights.order",
        &t,
        Cx::default(),
        Cx {
            heights: Some(bad),
            ..Cx::default()
        },
    );
    let flat = ResolvedHeights {
        bottom: 20.0,
        ..HEIGHTS
    };
    cx_case(
        "param.heights.order",
        &t,
        Cx::default(),
        Cx {
            heights: Some(flat),
            ..Cx::default()
        },
    );
    // parallel accepts top = bottom.
    let par = tp("parallel", "");
    assert!(
        !codes(
            &par,
            &Cx {
                heights: Some(flat),
                ..Cx::default()
            }
        )
        .contains(&"param.heights.order".into())
    );
    params_case("param.heights.reference.empty", "contour_2d", DRIVE, "");
    cx_case(
        "param.heights.below_stock",
        &t,
        Cx {
            stock_bottom: Some(0.0),
            ..Cx::default()
        },
        Cx {
            stock_bottom: Some(0.5),
            ..Cx::default()
        },
    );
}

#[test]
fn passes_codes() {
    let p = |s: &str| format!("{DRIVE}[passes]\n{s}\n");
    params_case(
        "param.passes.machining_tolerance.min",
        "contour_2d",
        &p("machining_tolerance = 0.0001"),
        &p("machining_tolerance = 0.00005"),
    );
    params_case(
        "param.passes.machining_tolerance.coarse",
        "contour_2d",
        &p("machining_tolerance = 0.1"),
        &p("machining_tolerance = 0.2"),
    );
    params_case(
        "param.passes.max_segment_length.short",
        "contour_2d",
        &p("max_segment_length = 0.1"),
        &p("max_segment_length = 0.09"),
    );
    params_case(
        "param.passes.stock_to_leave.range",
        "contour_2d",
        &p("stock_to_leave = -2.9"),
        &p("stock_to_leave = -3"),
    );
    params_case(
        "param.passes.stock_to_leave.negative",
        "pocket_2d",
        &p("stock_to_leave = 0"),
        &p("stock_to_leave = -0.1"),
    );
    params_case(
        "param.passes.stock_to_leave_axial.range",
        "pocket_2d",
        &p("stock_to_leave_axial = 0"),
        &p("stock_to_leave_axial = -0.1"),
    );
    params_case(
        "param.passes.stepover.max",
        "pocket_2d",
        &p("stepover = 6"),
        &p("stepover = 6.5"),
    );
    params_case(
        "param.passes.step_down.flute",
        "pocket_2d",
        &p("step_down = 22"),
        &p("step_down = 23"),
    );
    // 3-D types: > −r, and ≥ 0 for flat tools.
    params_case(
        "param.passes.stock_to_leave.range",
        "parallel",
        "[passes]\nstock_to_leave = 0",
        "[passes]\nstock_to_leave = -0.01",
    );
    let bull = Cx {
        tool: Some(tool("bull-6")),
        ..Cx::default()
    };
    let t = |s: &str| tp("parallel", &format!("[passes]\n{s}\n"));
    case(
        "param.passes.stock_to_leave.range",
        (&t("stock_to_leave = -0.9"), &bull),
        (&t("stock_to_leave = -1"), &bull),
    );
    let t = |s: &str| tp("pocket_2d", &p(s));
    case(
        "param.passes.stepover.flat_width",
        (&t("stepover = 4"), &bull),
        (&t("stepover = 4.5"), &bull),
    );
}

#[test]
fn linking_codes() {
    let l = |s: &str| format!("{DRIVE}[linking]\n{s}\n");
    params_case(
        "param.linking.lead.radius",
        "contour_2d",
        &l("lead_in = { kind = \"arc\", radius = 0.5 }"),
        &l("lead_out = { kind = \"arc\", radius = 0 }"),
    );
    params_case(
        "param.linking.ramp.angle",
        "pocket_2d",
        &l("ramp = { kind = \"helix\", angle_deg = 90 }"),
        &l("ramp = { kind = \"helix\", angle_deg = 0 }"),
    );
    let mut side = tool("flat-6");
    side.centre_cutting = false;
    let c = Cx {
        tool: Some(side),
        ..Cx::default()
    };
    case(
        "param.linking.ramp.plunge_tool",
        (&tp("contour_2d", &l("ramp = { kind = \"profile\" }")), &c),
        (&tp("contour_2d", DRIVE), &c),
    );
}

#[test]
fn contour_2d_codes() {
    let p = |s: &str| format!("{DRIVE}[passes]\n{s}\n");
    params_case(
        "param.contour_2d.depths.both",
        "contour_2d",
        &p("depth_count = 2"),
        &p("depth_count = 2\nstep_down = 1"),
    );
    let t = tp("contour_2d", DRIVE);
    let deep = ResolvedHeights {
        bottom: -3.0,
        ..HEIGHTS
    };
    cx_case(
        "param.contour_2d.depth.flute",
        &t,
        Cx::default(),
        Cx {
            heights: Some(deep),
            ..Cx::default()
        },
    );
    params_case(
        "param.contour_2d.compensation.on",
        "contour_2d",
        &p("side = \"on\""),
        &p("side = \"on\"\ncompensation = \"control\""),
    );
    let ctl = tp("contour_2d", &p("compensation = \"wear\""));
    cx_case(
        "param.contour_2d.compensation.post",
        &ctl,
        Cx {
            post: Some(PostCaps {
                cutter_comp: true,
                coolant: true,
                helix: true,
                cycles: CycleSet::default(),
            }),
            ..Cx::default()
        },
        Cx {
            post: Some(PostCaps {
                cutter_comp: false,
                coolant: true,
                helix: true,
                cycles: CycleSet::default(),
            }),
            ..Cx::default()
        },
    );
    params_case(
        "param.contour_2d.compensation.lead",
        "contour_2d",
        &format!(
            "{}[linking]\nlead_in = {{ kind = \"arc\", length = 3 }}\n",
            p("compensation = \"control\"")
        ),
        &format!(
            "{}[linking]\nlead_in = {{ kind = \"arc\", length = 2.9 }}\n",
            p("compensation = \"control\"")
        ),
    );
    params_case(
        "param.contour_2d.finishing_stepover.max",
        "contour_2d",
        &p("finishing_pass_count = 1\nfinishing_stepover = 6"),
        &p("finishing_pass_count = 1\nfinishing_stepover = 6.1"),
    );
    params_case(
        "param.contour_2d.tabs.height",
        "contour_2d",
        &p("tabs = { mode = \"count\", height = 20 }"),
        &p("tabs = { mode = \"count\", height = 20.5 }"),
    );
    params_case(
        "param.contour_2d.tabs.points.empty",
        "contour_2d",
        &p("tabs = { mode = \"points\", points = [[0, 0, 0]] }"),
        &p("tabs = { mode = \"points\" }"),
    );
    params_case(
        "param.contour_2d.finishing_pass_count.range",
        "contour_2d",
        &p("finishing_pass_count = 10"),
        &p("finishing_pass_count = 11"),
    );
    params_case(
        "param.contour_2d.tabs.distance.range",
        "contour_2d",
        &p("tabs = { mode = \"distance\", distance = 7, width = 6 }"),
        &p("tabs = { mode = \"distance\", distance = 6, width = 6 }"),
    );
}

#[test]
fn pocket_2d_codes() {
    let t = tp("pocket_2d", DRIVE);
    cx_case(
        "param.pocket_2d.tool_shape",
        &t,
        Cx {
            tool: Some(tool("bull-6")),
            ..Cx::default()
        },
        Cx {
            tool: Some(tool("ball-6")),
            ..Cx::default()
        },
    );
}

#[test]
fn parallel_codes() {
    let p = |s: &str| format!("[passes]\n{s}\n");
    let ball = Cx {
        tool: Some(tool("ball-6")),
        ..Cx::default()
    };
    let t = |s: &str| tp("parallel", &p(s));
    case(
        "param.parallel.stepover_and_scallop",
        (&t("scallop_height = 0.01"), &ball),
        (&t("scallop_height = 0.01\nstepover = 1"), &ball),
    );
    cx_case(
        "param.parallel.scallop.radius",
        &t("scallop_height = 0.01"),
        ball.clone(),
        Cx::default(),
    );
    case(
        "param.parallel.scallop.range",
        (&t("scallop_height = 2.9"), &ball),
        (&t("scallop_height = 3"), &ball),
    );
    params_case(
        "param.parallel.slope.order",
        "parallel",
        &p("min_slope_deg = 10\nmax_slope_deg = 20"),
        &p("min_slope_deg = 20\nmax_slope_deg = 20"),
    );
    params_case(
        "param.parallel.slope.range",
        "parallel",
        &p("max_slope_deg = 90"),
        &p("max_slope_deg = 91"),
    );
}
