// SPDX-License-Identifier: MIT OR Apache-2.0

//! Resolution: typed parameters with the D15 defaults applied (D06 §3.1,
//! D15 §1.5). Runs at calculation time, so preset edits reach every
//! toolpath that uses the default.

use crate::diag::Diagnostic;
use crate::params::{HeightSpec, PValue, Params, Sel, ToolpathTypeId};
use crate::tools::Tool;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Coolant {
    Off,
    Flood,
    Mist,
}

/// Feeds in mm/min; `None` when a `FromTool` default has no preset.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Feeds {
    pub cut: Option<f64>,
    pub plunge: Option<f64>,
    pub ramp: Option<f64>,
    pub lead_in: Option<f64>,
    pub lead_out: Option<f64>,
    pub transition: Option<f64>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ToolGroup {
    pub spindle_rpm: Option<f64>,
    pub feed: Feeds,
    pub coolant: Coolant,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum BoundarySide {
    Inside,
    Outside,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Containment {
    Inside,
    Centre,
    Outside,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Boundary {
    pub curves: Vec<Sel>,
    pub side: BoundarySide,
    pub containment: Containment,
}

#[derive(Clone, PartialEq, Debug)]
pub struct GeometryGroup {
    pub drive_faces: Vec<Sel>,
    pub drive_curves: Vec<Sel>,
    /// `None`: all faces not in `drive_faces` or `excluded_faces`.
    pub check_faces: Option<Vec<Sel>>,
    pub excluded_faces: Vec<Sel>,
    pub boundary: Option<Boundary>,
    pub rest_previous: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct HeightsGroup {
    pub clearance: HeightSpec,
    pub retract: HeightSpec,
    /// May reference `top` (its default only).
    pub feed: HeightSpec,
    pub top: HeightSpec,
    pub bottom: HeightSpec,
}

/// Z values of the heights in the work frame W.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct ResolvedHeights {
    pub clearance: f64,
    pub retract: f64,
    pub feed: f64,
    pub top: f64,
    pub bottom: f64,
}

/// What height references resolve to, in W (D15 §2.4).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct HeightEnv {
    pub setup_clearance: f64,
    pub stock_top: f64,
    pub stock_bottom: f64,
    pub model_top: f64,
    pub model_bottom: f64,
    /// Lowest and highest Z of the drive selection; `None` when empty.
    pub selection: Option<(f64, f64)>,
}

impl HeightsGroup {
    /// Resolves every height. Fails with `param.heights.reference.empty`
    /// when `selection` is used and the drive selection is empty.
    pub fn resolve(&self, env: &HeightEnv) -> Result<ResolvedHeights, Diagnostic> {
        let z = |h: &HeightSpec, low: bool, top: f64| -> Result<f64, Diagnostic> {
            let base = match h.reference.as_str() {
                "setup_clearance" => env.setup_clearance,
                "stock_top" => env.stock_top,
                "stock_bottom" => env.stock_bottom,
                "model_top" => env.model_top,
                "model_bottom" => env.model_bottom,
                "work_zero" => 0.0,
                "top" => top,
                _ => match env.selection {
                    Some((lo, hi)) => {
                        if low {
                            lo
                        } else {
                            hi
                        }
                    }
                    None => {
                        return Err(Diagnostic::error(
                            "param.heights.reference.empty",
                            "a height refers to the selection, but the drive selection is empty",
                        ));
                    }
                },
            };
            Ok(base + h.offset())
        };
        let top = z(&self.top, false, 0.0)?;
        Ok(ResolvedHeights {
            top,
            bottom: z(&self.bottom, true, top)?,
            feed: z(&self.feed, false, top)?,
            retract: z(&self.retract, false, top)?,
            clearance: z(&self.clearance, false, top)?,
        })
    }

    pub fn uses_selection(&self) -> bool {
        [
            &self.clearance,
            &self.retract,
            &self.feed,
            &self.top,
            &self.bottom,
        ]
        .iter()
        .any(|h| h.reference == "selection")
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PassLayout {
    Even,
    FixedFromEdge,
}

#[derive(Clone, PartialEq, Debug)]
pub struct CommonPasses {
    pub machining_tolerance: f64,
    pub max_segment_length: Option<f64>,
    pub output_arcs: bool,
    pub pass_layout: PassLayout,
    pub stock_to_leave: f64,
    pub stock_to_leave_axial: f64,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum RetractPolicy {
    Full,
    Minimum,
    StayDown,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Transitions {
    Direct,
    Smooth,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum LeadKind {
    None,
    Arc,
    Linear,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Lead {
    pub kind: LeadKind,
    pub radius: f64,
    pub length: f64,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum RampKind {
    Plunge,
    Zigzag,
    Helix,
    Profile,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Ramp {
    pub kind: RampKind,
    pub angle_deg: f64,
    pub radius: f64,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Order {
    ByLevel,
    ByArea,
}

#[derive(Clone, PartialEq, Debug)]
pub struct LinkingGroup {
    pub retract: RetractPolicy,
    pub safety_distance: f64,
    pub transitions: Transitions,
    pub lead_in: Lead,
    pub lead_out: Lead,
    /// `plunge` for types without ramps.
    pub ramp: Ramp,
    pub order: Order,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Side {
    Inside,
    Outside,
    Left,
    Right,
    On,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CutDirection {
    Climb,
    Conventional,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Compensation {
    Computer,
    Control,
    Wear,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TabMode {
    Off,
    Count,
    Distance,
    Points,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Tabs {
    pub mode: TabMode,
    pub count: i64,
    pub distance: f64,
    /// Model frame M.
    pub points: Vec<[f64; 3]>,
    pub width: f64,
    pub height: f64,
    pub ramp_angle_deg: f64,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Contour2d {
    /// `None`: `outside` for closed chains, `right` for open ones.
    pub side: Option<Side>,
    pub direction: CutDirection,
    pub compensation: Compensation,
    pub step_down: Option<f64>,
    pub depth_count: Option<i64>,
    pub finishing_pass_count: i64,
    pub finishing_stepover: f64,
    pub finish_every_level: bool,
    pub spring_pass_count: i64,
    /// Model frame M.
    pub start_point: Option<[f64; 3]>,
    pub tabs: Tabs,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Pattern {
    Offset,
    Raster,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum OffsetOrder {
    InsideOut,
    OutsideIn,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Pocket2d {
    pub pattern: Pattern,
    pub stepover: Option<f64>,
    pub step_down: Option<f64>,
    pub direction: CutDirection,
    pub offset_order: OffsetOrder,
    pub pass_angle_deg: f64,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ParallelDirection {
    Zigzag,
    Climb,
    Conventional,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Parallel {
    pub stepover: Option<f64>,
    pub scallop_height: Option<f64>,
    pub pass_angle_deg: f64,
    pub direction: ParallelDirection,
    pub min_slope_deg: f64,
    pub max_slope_deg: f64,
}

/// `face` (D15b §2.1).
#[derive(Clone, PartialEq, Debug)]
pub struct Face {
    pub stepover: Option<f64>,
    pub direction: ParallelDirection,
    pub pass_angle_deg: f64,
    pub overlap: f64,
    pub step_down: Option<f64>,
    pub final_step_down: Option<f64>,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum DepthMode {
    Heights,
    Curve,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TraceDirection {
    Any,
    Selection,
    Reversed,
}

/// `trace` (D15b §2.3).
#[derive(Clone, PartialEq, Debug)]
pub struct Trace {
    pub depth_mode: DepthMode,
    pub curve_depth: f64,
    pub side_offset: f64,
    pub step_down: Option<f64>,
    pub depth_count: Option<i64>,
    pub direction: TraceDirection,
}

#[derive(Clone, PartialEq, Debug)]
pub enum TypeParams {
    Contour2d(Contour2d),
    Pocket2d(Pocket2d),
    Parallel(Parallel),
    Face(Face),
    Trace(Trace),
}

/// Every parameter of a toolpath with defaults applied.
#[derive(Clone, PartialEq, Debug)]
pub struct ResolvedParams {
    pub kind: ToolpathTypeId,
    pub tool: ToolGroup,
    pub geometry: GeometryGroup,
    pub heights: HeightsGroup,
    pub passes: CommonPasses,
    pub linking: LinkingGroup,
    pub ty: TypeParams,
}

impl ResolvedParams {
    pub fn contour(&self) -> Option<&Contour2d> {
        match &self.ty {
            TypeParams::Contour2d(c) => Some(c),
            _ => None,
        }
    }

    pub fn pocket(&self) -> Option<&Pocket2d> {
        match &self.ty {
            TypeParams::Pocket2d(c) => Some(c),
            _ => None,
        }
    }

    pub fn parallel(&self) -> Option<&Parallel> {
        match &self.ty {
            TypeParams::Parallel(c) => Some(c),
            _ => None,
        }
    }

    pub fn face(&self) -> Option<&Face> {
        match &self.ty {
            TypeParams::Face(c) => Some(c),
            _ => None,
        }
    }

    pub fn trace(&self) -> Option<&Trace> {
        match &self.ty {
            TypeParams::Trace(c) => Some(c),
            _ => None,
        }
    }

    /// Step-down of the type, if it has one.
    pub fn step_down(&self) -> Option<f64> {
        match &self.ty {
            TypeParams::Contour2d(c) => c.step_down,
            TypeParams::Pocket2d(p) => p.step_down,
            TypeParams::Face(p) => p.step_down,
            TypeParams::Trace(p) => p.step_down,
            TypeParams::Parallel(_) => None,
        }
    }

    /// Stepover of the type, if it has one.
    pub fn stepover(&self) -> Option<f64> {
        match &self.ty {
            TypeParams::Pocket2d(p) => p.stepover,
            TypeParams::Parallel(p) => p.stepover,
            TypeParams::Face(p) => p.stepover,
            TypeParams::Contour2d(_) | TypeParams::Trace(_) => None,
        }
    }
}

/// Reads local values with fallbacks.
struct Get<'a> {
    p: &'a Params,
    tool: &'a Tool,
    material: Option<&'a str>,
}

impl Get<'_> {
    fn num(&self, path: &str) -> Option<f64> {
        self.p.get(path).and_then(PValue::num)
    }

    fn num_or(&self, path: &str, d: f64) -> f64 {
        self.num(path).unwrap_or(d)
    }

    /// Local value, else the preset value `key`.
    fn or_preset(&self, path: &str, key: &str) -> Option<f64> {
        self.num(path)
            .or_else(|| self.tool.preset(self.material)?.get(key))
    }

    fn int(&self, path: &str) -> Option<i64> {
        self.num(path).map(|v| v as i64)
    }

    fn flag(&self, path: &str, d: bool) -> bool {
        match self.p.get(path) {
            Some(PValue::Bool(b)) => *b,
            _ => d,
        }
    }

    /// The enum value as stored, or the type's constant default.
    fn text(&self, path: &str) -> &str {
        match self.p.get(path) {
            Some(PValue::Enum(s)) => s,
            _ => match Params::meta(self.p.kind(), path).map(|m| m.default) {
                Some(crate::params::DefaultRule::Const(crate::params::ConstVal::Str(s))) => s,
                _ => "",
            },
        }
    }

    fn sel(&self, path: &str) -> Option<Vec<Sel>> {
        match self.p.get(path) {
            Some(PValue::Selection(s)) => Some(s.clone()),
            _ => None,
        }
    }

    fn height(&self, path: &str) -> HeightSpec {
        if let Some(PValue::Height(h)) = self.p.get(path) {
            return h.clone();
        }
        match Params::meta(self.p.kind(), path).map(|m| m.default) {
            Some(crate::params::DefaultRule::Const(crate::params::ConstVal::Height(r, o))) => {
                HeightSpec::new(r, o)
            }
            _ => HeightSpec::new("work_zero", 0.0),
        }
    }

    fn point(&self, path: &str) -> Option<[f64; 3]> {
        match self.p.get(path) {
            Some(PValue::Point(p)) => Some([p[0].v, p[1].v, p[2].v]),
            _ => None,
        }
    }

    fn points(&self, path: &str) -> Vec<[f64; 3]> {
        match self.p.get(path) {
            Some(PValue::Points(ps)) => ps.iter().map(|p| [p[0].v, p[1].v, p[2].v]).collect(),
            _ => Vec::new(),
        }
    }

    fn lead(&self, key: &str, base: Option<&Lead>) -> Lead {
        let kind = match self.p.get(&format!("linking.{key}.kind")) {
            Some(PValue::Enum(s)) => lead_kind(s),
            _ => base.map_or_else(|| lead_kind(self.text("linking.lead_in.kind")), |b| b.kind),
        };
        let d = self.tool.diameter;
        let radius = self
            .num(&format!("linking.{key}.radius"))
            .unwrap_or_else(|| base.map_or(0.5 * d, |b| b.radius));
        let length = self
            .num(&format!("linking.{key}.length"))
            .unwrap_or_else(|| match base {
                Some(b) if b.kind == kind => b.length,
                _ => match kind {
                    LeadKind::Linear => 0.5 * d,
                    _ => 0.0,
                },
            });
        Lead {
            kind,
            radius,
            length,
        }
    }
}

fn lead_kind(s: &str) -> LeadKind {
    match s {
        "arc" => LeadKind::Arc,
        "linear" => LeadKind::Linear,
        _ => LeadKind::None,
    }
}

fn parallel_direction(s: &str) -> ParallelDirection {
    match s {
        "climb" => ParallelDirection::Climb,
        "conventional" => ParallelDirection::Conventional,
        _ => ParallelDirection::Zigzag,
    }
}

fn direction(s: &str) -> CutDirection {
    match s {
        "conventional" => CutDirection::Conventional,
        _ => CutDirection::Climb,
    }
}

/// Applies the D15 defaults. `material` is the Setup's material, which
/// selects the tool preset.
pub fn resolve(p: &Params, tool: &Tool, material: Option<&str>) -> ResolvedParams {
    let g = Get { p, tool, material };
    let d = tool.diameter;
    let cut = g.or_preset("tool_params.feed.cut", "feed");
    let lead_in_feed = g.num("tool_params.feed.lead_in").or(cut);
    let feed = Feeds {
        cut,
        plunge: g.or_preset("tool_params.feed.plunge", "plunge"),
        ramp: g.num("tool_params.feed.ramp").or(cut),
        lead_in: lead_in_feed,
        lead_out: g.num("tool_params.feed.lead_out").or(lead_in_feed),
        transition: g.num("tool_params.feed.transition").or(cut),
    };
    let coolant = match g.text("tool_params.coolant") {
        "flood" => Coolant::Flood,
        "mist" => Coolant::Mist,
        _ => Coolant::Off,
    };
    let boundary = g.p.get("geometry.boundary").map(|_| Boundary {
        curves: g.sel("geometry.boundary.curves").unwrap_or_default(),
        side: match g.text("geometry.boundary.side") {
            "outside" => BoundarySide::Outside,
            _ => BoundarySide::Inside,
        },
        containment: match g.text("geometry.boundary.tool_containment") {
            "centre" => Containment::Centre,
            "outside" => Containment::Outside,
            _ => Containment::Inside,
        },
    });
    let geometry = GeometryGroup {
        drive_faces: g.sel("geometry.drive_faces").unwrap_or_default(),
        drive_curves: g.sel("geometry.drive_curves").unwrap_or_default(),
        check_faces: g.sel("geometry.check_faces"),
        excluded_faces: g.sel("geometry.excluded_faces").unwrap_or_default(),
        boundary,
        rest_previous: g.text("geometry.rest_from") == "previous",
    };
    let heights = HeightsGroup {
        clearance: g.height("heights.clearance"),
        retract: g.height("heights.retract"),
        feed: g.height("heights.feed"),
        top: g.height("heights.top"),
        bottom: g.height("heights.bottom"),
    };
    let stl = g.num_or("passes.stock_to_leave", 0.0);
    let passes = CommonPasses {
        machining_tolerance: g.num_or("passes.machining_tolerance", 0.01),
        max_segment_length: g.num("passes.max_segment_length"),
        output_arcs: g.flag("passes.output_arcs", true),
        pass_layout: match g.text("passes.pass_layout") {
            "fixed_from_edge" => PassLayout::FixedFromEdge,
            _ => PassLayout::Even,
        },
        stock_to_leave: stl,
        stock_to_leave_axial: g.num_or("passes.stock_to_leave_axial", stl),
    };
    let lead_in = g.lead("lead_in", None);
    let lead_out = if p.is_set("linking.lead_out") {
        g.lead("lead_out", Some(&lead_in))
    } else {
        lead_in
    };
    let linking = LinkingGroup {
        retract: match g.text("linking.retract") {
            "full" => RetractPolicy::Full,
            "stay_down" => RetractPolicy::StayDown,
            _ => RetractPolicy::Minimum,
        },
        safety_distance: g.num_or("linking.safety_distance", 1.0),
        transitions: match g.text("linking.transitions") {
            "smooth" => Transitions::Smooth,
            _ => Transitions::Direct,
        },
        lead_in,
        lead_out,
        ramp: Ramp {
            kind: match g.text("linking.ramp.kind") {
                "zigzag" => RampKind::Zigzag,
                "helix" => RampKind::Helix,
                "profile" => RampKind::Profile,
                _ => RampKind::Plunge,
            },
            angle_deg: g.num_or("linking.ramp.angle_deg", 3.0),
            radius: g.num_or("linking.ramp.radius", 0.5 * d),
        },
        order: match g.text("linking.order") {
            "by_area" => Order::ByArea,
            _ => Order::ByLevel,
        },
    };
    let ty = match p.kind() {
        ToolpathTypeId::Contour2d => TypeParams::Contour2d(Contour2d {
            side: match g.p.get("passes.side") {
                Some(PValue::Enum(s)) => Some(match s.as_str() {
                    "inside" => Side::Inside,
                    "left" => Side::Left,
                    "right" => Side::Right,
                    "on" => Side::On,
                    _ => Side::Outside,
                }),
                _ => None,
            },
            direction: direction(g.text("passes.direction")),
            compensation: match g.text("passes.compensation") {
                "control" => Compensation::Control,
                "wear" => Compensation::Wear,
                _ => Compensation::Computer,
            },
            step_down: g.or_preset("passes.step_down", "step_down"),
            depth_count: g.int("passes.depth_count"),
            finishing_pass_count: g.int("passes.finishing_pass_count").unwrap_or(0),
            finishing_stepover: g.num_or("passes.finishing_stepover", 0.1 * d),
            finish_every_level: g.flag("passes.finish_every_level", false),
            spring_pass_count: g.int("passes.spring_pass_count").unwrap_or(0),
            start_point: g.point("passes.start_point"),
            tabs: Tabs {
                mode: match g.text("passes.tabs.mode") {
                    "count" => TabMode::Count,
                    "distance" => TabMode::Distance,
                    "points" => TabMode::Points,
                    _ => TabMode::Off,
                },
                count: g.int("passes.tabs.count").unwrap_or(4),
                distance: g.num_or("passes.tabs.distance", 100.0),
                points: g.points("passes.tabs.points"),
                width: g.num_or("passes.tabs.width", d),
                height: g.num_or("passes.tabs.height", 1.0),
                ramp_angle_deg: g.num_or("passes.tabs.ramp_angle_deg", 45.0),
            },
        }),
        ToolpathTypeId::Pocket2d => TypeParams::Pocket2d(Pocket2d {
            pattern: match g.text("passes.pattern") {
                "raster" => Pattern::Raster,
                _ => Pattern::Offset,
            },
            stepover: g.or_preset("passes.stepover", "stepover"),
            step_down: g.or_preset("passes.step_down", "step_down"),
            direction: direction(g.text("passes.direction")),
            offset_order: match g.text("passes.offset_order") {
                "outside_in" => OffsetOrder::OutsideIn,
                _ => OffsetOrder::InsideOut,
            },
            pass_angle_deg: g.num_or("passes.pass_angle_deg", 0.0),
        }),
        ToolpathTypeId::Parallel => TypeParams::Parallel(Parallel {
            stepover: g.or_preset("passes.stepover", "stepover"),
            scallop_height: g.num("passes.scallop_height"),
            pass_angle_deg: g.num_or("passes.pass_angle_deg", 0.0),
            direction: parallel_direction(g.text("passes.direction")),
            min_slope_deg: g.num_or("passes.min_slope_deg", 0.0),
            max_slope_deg: g.num_or("passes.max_slope_deg", 90.0),
        }),
        ToolpathTypeId::Face => TypeParams::Face(Face {
            stepover: g.or_preset("passes.stepover", "stepover"),
            direction: parallel_direction(g.text("passes.direction")),
            pass_angle_deg: g.num_or("passes.pass_angle_deg", 0.0),
            overlap: g.num_or("passes.overlap", 0.6 * d),
            step_down: g.or_preset("passes.step_down", "step_down"),
            final_step_down: g.num("passes.final_step_down"),
        }),
        ToolpathTypeId::Trace => TypeParams::Trace(Trace {
            depth_mode: match g.text("passes.depth_mode") {
                "curve" => DepthMode::Curve,
                _ => DepthMode::Heights,
            },
            curve_depth: g.num_or("passes.curve_depth", 0.0),
            side_offset: g.num_or("passes.side_offset", 0.0),
            step_down: g.or_preset("passes.step_down", "step_down"),
            depth_count: g.int("passes.depth_count"),
            direction: match g.text("passes.direction") {
                "selection" => TraceDirection::Selection,
                "reversed" => TraceDirection::Reversed,
                _ => TraceDirection::Any,
            },
        }),
    };
    ResolvedParams {
        kind: p.kind(),
        tool: ToolGroup {
            spindle_rpm: g.or_preset("tool_params.spindle_rpm", "rpm"),
            feed,
            coolant,
        },
        geometry,
        heights,
        passes,
        linking,
        ty,
    }
}
