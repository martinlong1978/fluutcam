// SPDX-License-Identifier: MIT OR Apache-2.0

//! Helpers for the strategy tests: a tool library, STEP loading, toolpaths
//! from group tables, and calculation with a stock box.

#![allow(dead_code)]

use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry};
use fluut_cam::stock::{LayeredStock, Sweep};
use fluut_cam::{Generated, StrategyInput, calculate};
use fluut_geom::{Aabb3, Affine, Point2, Point3};
use fluut_model::project::{SetupKind, Toolpath, read};
use fluut_model::resolve::HeightEnv;
use fluut_model::tools::{Tool, ToolLibrary};
use fluut_model::validate::PostCaps;
use fluut_path::{Motion, Segment};

pub const LIB: &str = r#"
format = "fluut.tools/1"
id = "lib"
[[tool]]
id = "f6"
shape = "flat"
diameter = 6
flute_length = 25
  [[tool.preset]]
  material = "MDF"
  rpm = 18000
  feed = 2000
  plunge = 500
  stepover = 2.4
  step_down = 4
[[tool]]
id = "f20"
shape = "flat"
diameter = 20
flute_length = 25
  [[tool.preset]]
  material = "MDF"
  rpm = 12000
  feed = 3000
  plunge = 800
  stepover = 14
  step_down = 2
[[tool]]
id = "v90"
shape = "v"
diameter = 12
half_angle_deg = 45
tip_diameter = 0.2
flute_length = 10
  [[tool.preset]]
  material = "MDF"
  rpm = 18000
  feed = 1500
  plunge = 500
  step_down = 2
[[tool]]
id = "d5"
shape = "drill"
diameter = 5
flute_length = 30
  [[tool.preset]]
  material = "MDF"
  rpm = 3000
  feed = 300
  plunge = 250
"#;

pub fn tool(id: &str) -> Tool {
    ToolLibrary::parse(LIB)
        .unwrap()
        .0
        .tools
        .into_iter()
        .find(|t| t.id == id)
        .unwrap()
}

pub fn load(text: &str) -> SetupGeometry {
    let m = fluut_io_step::parse(text.as_bytes()).unwrap();
    let bodies = fluut_io_step::map_bodies(&m)
        .into_iter()
        .map(|b| {
            let b = b.unwrap();
            assert!(b.skipped.is_empty(), "{:?}", b.skipped);
            SetupBody {
                id: format!("b-{}", b.id),
                geom: BodyGeom::Brep(b.body),
            }
        })
        .collect::<Vec<_>>();
    SetupGeometry { bodies }
}

/// A toolpath of `kind` with tool `lib#<tool>` and the group tables
/// `groups` (headers without the `setup.toolpath.` prefix).
pub fn tp(kind: &str, tool: &str, groups: &str) -> Toolpath {
    let groups = groups.replace("\n[", "\n[setup.toolpath.");
    let groups = match groups.strip_prefix('[') {
        Some(g) => format!("[setup.toolpath.{g}"),
        None => groups,
    };
    let src = format!(
        "format = \"fluut.project/1\"\n[[setup]]\nid = \"s\"\n[[setup.toolpath]]\n\
         id = \"t\"\ntype = \"{kind}\"\ntool = \"lib#{tool}\"\n{groups}"
    );
    read(&src)
        .unwrap_or_else(|e| panic!("{e}\n{src}"))
        .project
        .setups[0]
        .toolpaths[0]
        .clone()
}

/// The stock box and the heights it gives.
#[derive(Clone, Copy, Debug)]
pub struct Stock {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub model_top: f64,
    pub model_bottom: f64,
}

impl Stock {
    pub fn new(min: [f64; 3], max: [f64; 3]) -> Stock {
        Stock {
            min,
            max,
            model_top: max[2],
            model_bottom: min[2],
        }
    }

    pub fn model(self, bottom: f64, top: f64) -> Stock {
        Stock {
            model_top: top,
            model_bottom: bottom,
            ..self
        }
    }

    pub fn aabb(&self) -> Aabb3 {
        Aabb3 {
            min: Point3::new(self.min[0], self.min[1], self.min[2]),
            max: Point3::new(self.max[0], self.max[1], self.max[2]),
        }
    }

    pub fn env(&self) -> HeightEnv {
        HeightEnv {
            setup_clearance: self.max[2] + 10.0,
            stock_top: self.max[2],
            stock_bottom: self.min[2],
            model_top: self.model_top,
            model_bottom: self.model_bottom,
            selection: None,
        }
    }
}

pub fn try_run_with(
    g: &SetupGeometry,
    t: &Toolpath,
    s: Stock,
    post: Option<PostCaps>,
) -> Result<Generated, Vec<String>> {
    try_run_full(g, t, s, post, None)
}

/// As [`run`], starting from the in-process stock `stock`.
pub fn run_rest(g: &SetupGeometry, t: &Toolpath, s: Stock, stock: &LayeredStock) -> Generated {
    try_run_full(g, t, s, None, Some(stock)).unwrap_or_else(|e| panic!("{e:?}"))
}

/// `stock` with the cuts of `r` (made with tool `tool`) taken out.
pub fn after(stock: &LayeredStock, r: &Generated, tool_id: &str) -> LayeredStock {
    let mut st = stock.clone();
    st.apply(&r.result, Sweep::of(&tool(tool_id)), 0.01);
    st
}

pub fn try_run_full(
    g: &SetupGeometry,
    t: &Toolpath,
    s: Stock,
    post: Option<PostCaps>,
    in_process: Option<&LayeredStock>,
) -> Result<Generated, Vec<String>> {
    let tool = tool(t.tool.split('#').nth(1).unwrap());
    let input = StrategyInput {
        toolpath: t,
        geometry: g,
        tool: &tool,
        material: Some("MDF"),
        setup_kind: SetupKind::ThreeAxis,
        env: s.env(),
        stock: Some(s.aabb()),
        to_work: Affine::identity(),
        machine: None,
        post,
        in_process,
        rotary: None,
    };
    calculate(&input).map_err(|e| e.diagnostics().into_iter().map(|d| d.code).collect())
}

pub fn run(g: &SetupGeometry, t: &Toolpath, s: Stock) -> Generated {
    try_run_with(g, t, s, None).unwrap_or_else(|e| panic!("{e:?}"))
}

pub fn run_err(g: &SetupGeometry, t: &Toolpath, s: Stock) -> Vec<String> {
    match try_run_with(g, t, s, None) {
        Ok(r) => panic!("expected an error, got warnings {:?}", codes(&r)),
        Err(e) => e,
    }
}

pub fn codes(r: &Generated) -> Vec<String> {
    r.diagnostics.iter().map(|d| d.code.clone()).collect()
}

/// Segments with motion `m`, in order.
pub fn moves(r: &Generated, m: Motion) -> Vec<&Segment> {
    r.result
        .segments()
        .filter(|s| s.motion().is_some_and(|i| i.motion == m))
        .collect()
}

pub fn cuts(r: &Generated) -> Vec<&Segment> {
    moves(r, Motion::Cut)
}

pub fn xy(p: Point3) -> Point2 {
    Point2::new(p.x(), p.y())
}

pub fn start(s: &Segment) -> Point3 {
    s.start().unwrap().xyz
}

pub fn end(s: &Segment) -> Point3 {
    s.end().unwrap().xyz
}

/// Distinct Z values of the cut moves, top first.
pub fn cut_levels(r: &Generated) -> Vec<f64> {
    let mut zs: Vec<f64> = cuts(r).iter().map(|s| end(s).z()).collect();
    zs.sort_by(|a, b| b.total_cmp(a));
    zs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    zs
}

/// A wire body `b-w` of straight lines through `pts`.
pub fn wire(pts: &[[f64; 3]]) -> SetupGeometry {
    use fluut_geom::{Curve3, Interval, UnitVec3};
    let curves = pts
        .windows(2)
        .map(|w| {
            let a = Point3::new(w[0][0], w[0][1], w[0][2]);
            let b = Point3::new(w[1][0], w[1][1], w[1][2]);
            fluut_topo::WireCurve {
                curve: Curve3::line(a, UnitVec3::new(b - a).unwrap()),
                range: Interval {
                    lo: 0.0,
                    hi: a.distance(b),
                },
            }
        })
        .collect();
    SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-w".into(),
            geom: BodyGeom::Wire(fluut_topo::WireBody {
                name: "w".into(),
                tol_model: 1e-6,
                curves,
            }),
        }],
    }
}

/// A wire body `b-w` with one full circle of radius `r` about `(x, y)` at
/// `z`.
pub fn wire_circle(x: f64, y: f64, z: f64, r: f64) -> SetupGeometry {
    use fluut_geom::{Curve3, Frame, Interval, Vec3};
    let f = Frame::new(
        Point3::new(x, y, z),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, 0.0),
    )
    .unwrap();
    SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-w".into(),
            geom: BodyGeom::Wire(fluut_topo::WireBody {
                name: "w".into(),
                tol_model: 1e-6,
                curves: vec![fluut_topo::WireCurve {
                    curve: Curve3::circle(f, r).unwrap(),
                    range: Interval {
                        lo: 0.0,
                        hi: std::f64::consts::TAU,
                    },
                }],
            }),
        }],
    }
}
