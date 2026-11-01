// SPDX-License-Identifier: MIT OR Apache-2.0

//! Parameter validation (D15 §1.7, §2–§5). One function per rule group;
//! every code has a passing and a failing test. Rules that need geometry
//! (chains, closed regions, face kinds) run at calculation start in
//! `fluut-cam` and keep the `cam.` prefix, except where D15 gives them a
//! `param.` code.

use crate::diag::Diagnostic;
use crate::params::{Params, ToolpathTypeId};
use crate::project::{SetupKind, Toolpath};
use crate::resolve::{
    Compensation, LeadKind, RampKind, ResolvedHeights, ResolvedParams, Side, TabMode, resolve,
};
use crate::tools::{Tool, ToolShape};

/// Machine limits used by the machine rules (spec 02 §2.3).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct MachineLimits {
    pub rpm: (f64, f64),
    pub max_cut_feed: f64,
}

/// What the resolved post can do (D10).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct PostCaps {
    pub cutter_comp: bool,
    pub coolant: bool,
}

/// Inputs beyond the parameters. Rules whose input is `None` are skipped.
#[derive(Copy, Clone, Debug)]
pub struct ValidateCx<'a> {
    pub tool: Option<&'a Tool>,
    pub material: Option<&'a str>,
    pub setup_kind: SetupKind,
    pub heights: Option<&'a ResolvedHeights>,
    pub stock_bottom: Option<f64>,
    pub machine: Option<&'a MachineLimits>,
    pub post: Option<&'a PostCaps>,
}

impl<'a> ValidateCx<'a> {
    pub fn new(tool: Option<&'a Tool>) -> ValidateCx<'a> {
        ValidateCx {
            tool,
            material: None,
            setup_kind: SetupKind::ThreeAxis,
            heights: None,
            stock_bottom: None,
            machine: None,
            post: None,
        }
    }
}

/// Validates one toolpath. The diagnostics carry the toolpath id.
pub fn validate(tp: &Toolpath, cx: &ValidateCx) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let Some(tool) = cx.tool else {
        out.push(
            Diagnostic::error(
                "param.tool.missing",
                format!("tool `{}` is not in any loaded tool library", tp.tool),
            )
            .at("tool"),
        );
        return tag(out, tp);
    };
    if tool.diameter.is_nan() || tool.diameter <= 0.0 {
        out.push(
            Diagnostic::error("param.tool.diameter", "the tool diameter must be above 0")
                .at("tool"),
        );
        return tag(out, tp);
    }
    let r = resolve(&tp.params, tool, cx.material);
    let mut v = V {
        out: &mut out,
        p: &tp.params,
        r: &r,
        tool,
        cx,
    };
    v.item();
    v.tool_group();
    v.geometry();
    v.heights();
    v.passes();
    v.linking();
    match tp.kind() {
        ToolpathTypeId::Contour2d => v.contour(),
        ToolpathTypeId::Pocket2d => v.pocket(),
        ToolpathTypeId::Parallel => v.parallel(),
    }
    tag(out, tp)
}

fn tag(out: Vec<Diagnostic>, tp: &Toolpath) -> Vec<Diagnostic> {
    out.into_iter().map(|d| d.on(&tp.id)).collect()
}

struct V<'a, 'b> {
    out: &'b mut Vec<Diagnostic>,
    p: &'a Params,
    r: &'a ResolvedParams,
    tool: &'a Tool,
    cx: &'a ValidateCx<'a>,
}

impl V<'_, '_> {
    fn e(&mut self, code: &str, field: &str, msg: String) {
        self.out.push(Diagnostic::error(code, msg).at(field));
    }

    fn w(&mut self, code: &str, field: &str, msg: String) {
        self.out.push(Diagnostic::warning(code, msg).at(field));
    }

    fn ty(&self) -> &'static str {
        self.r.kind.as_str()
    }

    /// Generic range rule `param.<scope>.<key>.range` for fields D15 gives a
    /// range but no code of its own.
    fn range(&mut self, scope: &str, field: &str, ok: bool, what: &str) {
        if !ok {
            let sub = field.split_once('.').map_or(field, |(_, k)| k);
            self.e(
                &format!("param.{scope}.{sub}.range"),
                field,
                format!("`{field}` must be {what}"),
            );
        }
    }

    fn required(&mut self, field: &str, v: Option<f64>) {
        if v.is_none() {
            self.e(
                "param.required",
                field,
                format!(
                    "`{field}` has no value and the tool has no preset for the Setup's material"
                ),
            );
        }
    }

    fn item(&mut self) {
        if !matches!(
            self.cx.setup_kind,
            SetupKind::ThreeAxis | SetupKind::Indexed
        ) {
            self.e(
                "param.type.setup_kind",
                "type",
                format!("{} runs in three_axis and indexed Setups only", self.ty()),
            );
        }
    }

    fn tool_group(&mut self) {
        let t = &self.r.tool;
        self.required("tool_params.spindle_rpm", t.spindle_rpm);
        self.required("tool_params.feed.cut", t.feed.cut);
        self.required("tool_params.feed.plunge", t.feed.plunge);
        let feeds = [
            ("cut", t.feed.cut),
            ("plunge", t.feed.plunge),
            ("ramp", t.feed.ramp),
            ("lead_in", t.feed.lead_in),
            ("lead_out", t.feed.lead_out),
            ("transition", t.feed.transition),
        ];
        if let Some(rpm) = t.spindle_rpm {
            self.range(
                "tool_params",
                "tool_params.spindle_rpm",
                rpm > 0.0,
                "above 0",
            );
        }
        for (k, f) in feeds {
            if let Some(f) = f {
                self.range(
                    "tool_params",
                    &format!("tool_params.feed.{k}"),
                    f > 0.0,
                    "above 0",
                );
            }
        }
        if let Some(m) = self.cx.machine {
            if let Some(rpm) = t.spindle_rpm
                && (rpm < m.rpm.0 || rpm > m.rpm.1)
            {
                self.w(
                    "param.tool_params.spindle_rpm.machine_range",
                    "tool_params.spindle_rpm",
                    format!(
                        "spindle speed {rpm} is outside the machine's range {} to {}",
                        m.rpm.0, m.rpm.1
                    ),
                );
            }
            for (k, f) in feeds {
                if let Some(f) = f
                    && f > m.max_cut_feed
                {
                    self.w(
                        "param.tool_params.feed.machine_max",
                        &format!("tool_params.feed.{k}"),
                        format!("feed {f} is above the machine's maximum {}", m.max_cut_feed),
                    );
                }
            }
        }
        if let Some(post) = self.cx.post
            && t.coolant != crate::resolve::Coolant::Off
            && !post.coolant
        {
            self.w(
                "param.tool_params.coolant.unsupported",
                "tool_params.coolant",
                "the post has no coolant_on block, so coolant is not switched on".into(),
            );
        }
    }

    fn geometry(&mut self) {
        let g = &self.r.geometry;
        let two_d = matches!(
            self.r.kind,
            ToolpathTypeId::Contour2d | ToolpathTypeId::Pocket2d
        );
        if two_d && g.drive_curves.is_empty() && g.drive_faces.is_empty() {
            self.e(
                "param.geometry.drive.empty",
                "geometry.drive_curves",
                "select drive curves or drive faces".into(),
            );
        }
        let sets: [(&str, &[crate::params::Sel]); 3] = [
            ("drive_faces", &g.drive_faces),
            ("check_faces", g.check_faces.as_deref().unwrap_or(&[])),
            ("excluded_faces", &g.excluded_faces),
        ];
        'outer: for i in 0..3 {
            for j in i + 1..3 {
                if let Some(s) = sets[i].1.iter().find(|s| sets[j].1.contains(s)) {
                    self.e(
                        "param.geometry.overlap",
                        &format!("geometry.{}", sets[j].0),
                        format!("`{}` is in both {} and {}", s.id(), sets[i].0, sets[j].0),
                    );
                    break 'outer;
                }
            }
        }
        if g.rest_previous {
            self.e(
                "param.geometry.rest_from.unsupported",
                "geometry.rest_from",
                "rest machining from previous toolpaths is not available yet".into(),
            );
        }
    }

    fn heights(&mut self) {
        let h = &self.r.heights;
        let g = &self.r.geometry;
        if h.uses_selection() && g.drive_curves.is_empty() && g.drive_faces.is_empty() {
            self.e(
                "param.heights.reference.empty",
                "heights",
                "a height refers to the selection, but the drive selection is empty".into(),
            );
        }
        let Some(z) = self.cx.heights.copied() else {
            return;
        };
        let top_ok = if self.r.kind == ToolpathTypeId::Parallel {
            z.top >= z.bottom
        } else {
            z.top > z.bottom
        };
        if !(z.clearance >= z.retract && z.retract >= z.feed && z.feed >= z.top && top_ok) {
            self.e(
                "param.heights.order",
                "heights",
                format!(
                    "heights must satisfy clearance ≥ retract ≥ feed ≥ top {} bottom \
                     (now {} / {} / {} / {} / {})",
                    if self.r.kind == ToolpathTypeId::Parallel {
                        "≥"
                    } else {
                        ">"
                    },
                    z.clearance,
                    z.retract,
                    z.feed,
                    z.top,
                    z.bottom
                ),
            );
        }
        if let Some(sb) = self.cx.stock_bottom
            && z.bottom < sb
        {
            self.w(
                "param.heights.below_stock",
                "heights.bottom",
                format!(
                    "bottom {} is below the stock bottom {sb}: the tool cuts the fixture",
                    z.bottom
                ),
            );
        }
    }

    fn passes(&mut self) {
        let p = &self.r.passes;
        let tol = p.machining_tolerance;
        if tol < 1e-4 {
            self.e(
                "param.passes.machining_tolerance.min",
                "passes.machining_tolerance",
                format!("machining tolerance {tol} is below 0.0001 mm"),
            );
        } else if tol > 0.1 {
            self.w(
                "param.passes.machining_tolerance.coarse",
                "passes.machining_tolerance",
                format!("machining tolerance {tol} mm is coarse"),
            );
        }
        if let Some(m) = p.max_segment_length {
            if m <= 0.0 {
                self.range("passes", "passes.max_segment_length", false, "above 0");
            } else if m < 10.0 * tol {
                self.w(
                    "param.passes.max_segment_length.short",
                    "passes.max_segment_length",
                    format!("segments capped at {m} mm make very long output"),
                );
            }
        }
        let stl = p.stock_to_leave;
        let two_d = self.r.kind != ToolpathTypeId::Parallel;
        let (bad, bound) = if two_d {
            (stl <= -self.tool.radius(), "−R")
        } else {
            let r = self.tool.corner_radius();
            (
                if r > 0.0 { stl <= -r } else { stl < 0.0 },
                "−r (corner radius)",
            )
        };
        if bad {
            self.e(
                "param.passes.stock_to_leave.range",
                "passes.stock_to_leave",
                format!("stock to leave {stl} is at or below {bound} for this tool"),
            );
        } else if stl < 0.0 {
            self.w(
                "param.passes.stock_to_leave.negative",
                "passes.stock_to_leave",
                format!("stock to leave {stl} is negative: the cut goes past the model"),
            );
        }
        if self.p.get("passes.stock_to_leave_axial").is_some() || two_d {
            self.range(
                "passes",
                "passes.stock_to_leave_axial",
                p.stock_to_leave_axial >= 0.0,
                "0 or more",
            );
        }
        if let Some(s) = self.r.stepover() {
            let d = self.tool.diameter;
            if s <= 0.0 {
                self.range("passes", "passes.stepover", false, "above 0");
            } else if s > d {
                self.e(
                    "param.passes.stepover.max",
                    "passes.stepover",
                    format!("stepover {s} is more than the tool diameter {d}"),
                );
            } else if matches!(self.tool.shape, ToolShape::Flat | ToolShape::Bull)
                && s > d - 2.0 * self.tool.corner_radius()
            {
                self.w(
                    "param.passes.stepover.flat_width",
                    "passes.stepover",
                    format!(
                        "stepover {s} is wider than the flat of the tool: the floor keeps cusps"
                    ),
                );
            }
        }
        if let Some(sd) = self.r.step_down() {
            if sd <= 0.0 {
                self.range("passes", "passes.step_down", false, "above 0");
            } else if let Some(fl) = self.tool.flute_length
                && sd > fl
            {
                self.w(
                    "param.passes.step_down.flute",
                    "passes.step_down",
                    format!("step-down {sd} is more than the flute length {fl}"),
                );
            }
        }
    }

    fn linking(&mut self) {
        let l = self.r.linking.clone();
        self.range(
            "linking",
            "linking.safety_distance",
            l.safety_distance >= 0.0,
            "0 or more",
        );
        for (k, lead) in [("lead_in", l.lead_in), ("lead_out", l.lead_out)] {
            if lead.kind == LeadKind::Arc && lead.radius <= 0.0 {
                self.e(
                    "param.linking.lead.radius",
                    &format!("linking.{k}.radius"),
                    format!("the arc {k} needs a radius above 0"),
                );
            }
            if lead.kind != LeadKind::None {
                self.range(
                    "linking",
                    &format!("linking.{k}.length"),
                    lead.length >= 0.0,
                    "0 or more",
                );
            }
        }
        let uses_ramp = self.r.kind != ToolpathTypeId::Parallel;
        if uses_ramp {
            let a = l.ramp.angle_deg;
            if l.ramp.kind != RampKind::Plunge && !(a > 0.0 && a <= 90.0) {
                self.e(
                    "param.linking.ramp.angle",
                    "linking.ramp.angle_deg",
                    format!("ramp angle {a}° is outside (0, 90]"),
                );
            }
            if l.ramp.kind == RampKind::Helix {
                self.range(
                    "linking",
                    "linking.ramp.radius",
                    l.ramp.radius > 0.0,
                    "above 0",
                );
            }
            if l.ramp.kind == RampKind::Plunge && !self.tool.centre_cutting {
                self.w(
                    "param.linking.ramp.plunge_tool",
                    "linking.ramp.kind",
                    "the tool is not centre cutting and should not plunge".into(),
                );
            }
        }
    }

    fn contour(&mut self) {
        let Some(c) = self.r.contour().cloned() else {
            return;
        };
        let d = self.tool.diameter;
        if self.p.is_set("passes.step_down") && self.p.is_set("passes.depth_count") {
            self.e(
                "param.contour_2d.depths.both",
                "passes.depth_count",
                "set either step_down or depth_count, not both".into(),
            );
        }
        if c.depth_count.is_none() {
            self.required("passes.step_down", c.step_down);
        }
        if let Some(n) = c.depth_count {
            self.range("contour_2d", "passes.depth_count", n >= 1, "1 or more");
        }
        if let (Some(z), Some(fl)) = (self.cx.heights, self.tool.flute_length)
            && z.top - z.bottom > fl
        {
            self.w(
                "param.contour_2d.depth.flute",
                "heights.bottom",
                format!(
                    "the cut depth {} is more than the flute length {fl}",
                    z.top - z.bottom
                ),
            );
        }
        if c.compensation != Compensation::Computer {
            if c.side == Some(Side::On) {
                self.e(
                    "param.contour_2d.compensation.on",
                    "passes.compensation",
                    "controller compensation needs a side other than `on`".into(),
                );
            }
            if let Some(post) = self.cx.post
                && !post.cutter_comp
            {
                self.e(
                    "param.contour_2d.compensation.post",
                    "passes.compensation",
                    "the post has no cutter_comp blocks".into(),
                );
            }
            let li = self.r.linking.lead_in;
            let straight = if li.kind == LeadKind::None {
                0.0
            } else {
                li.length
            };
            if straight < self.tool.radius() {
                self.w(
                    "param.contour_2d.compensation.lead",
                    "linking.lead_in.length",
                    format!(
                        "the straight part of the lead-in ({straight}) is shorter than the tool \
                         radius; most controls need a linear move to start compensation"
                    ),
                );
            }
        }
        self.range(
            "contour_2d",
            "passes.finishing_pass_count",
            (0..=10).contains(&c.finishing_pass_count),
            "0 to 10",
        );
        self.range(
            "contour_2d",
            "passes.spring_pass_count",
            (0..=5).contains(&c.spring_pass_count),
            "0 to 5",
        );
        if c.finishing_pass_count > 0 {
            if c.finishing_stepover > d {
                self.e(
                    "param.contour_2d.finishing_stepover.max",
                    "passes.finishing_stepover",
                    format!(
                        "finishing stepover {} is more than the tool diameter {d}",
                        c.finishing_stepover
                    ),
                );
            }
            self.range(
                "contour_2d",
                "passes.finishing_stepover",
                c.finishing_stepover > 0.0,
                "above 0",
            );
        }
        let t = &c.tabs;
        if t.mode != TabMode::Off {
            if let Some(z) = self.cx.heights
                && t.height > z.top - z.bottom
            {
                self.e(
                    "param.contour_2d.tabs.height",
                    "passes.tabs.height",
                    format!(
                        "tab height {} is more than the cut depth {}",
                        t.height,
                        z.top - z.bottom
                    ),
                );
            }
            self.range(
                "contour_2d",
                "passes.tabs.height",
                t.height > 0.0,
                "above 0",
            );
            self.range("contour_2d", "passes.tabs.width", t.width > 0.0, "above 0");
            self.range(
                "contour_2d",
                "passes.tabs.ramp_angle_deg",
                t.ramp_angle_deg > 0.0 && t.ramp_angle_deg <= 90.0,
                "in (0, 90]",
            );
            match t.mode {
                TabMode::Count => {
                    self.range("contour_2d", "passes.tabs.count", t.count >= 1, "1 or more");
                }
                TabMode::Distance => self.range(
                    "contour_2d",
                    "passes.tabs.distance",
                    t.distance > t.width,
                    "more than the tab width",
                ),
                TabMode::Points if t.points.is_empty() => self.e(
                    "param.contour_2d.tabs.points.empty",
                    "passes.tabs.points",
                    "tab mode `points` needs at least one point".into(),
                ),
                _ => {}
            }
        }
    }

    fn pocket(&mut self) {
        let Some(p) = self.r.pocket().cloned() else {
            return;
        };
        self.required("passes.stepover", p.stepover);
        self.required("passes.step_down", p.step_down);
        if !matches!(self.tool.shape, ToolShape::Flat | ToolShape::Bull) {
            self.w(
                "param.pocket_2d.tool_shape",
                "tool",
                "the tool is not flat or bull nosed: the floor will be scalloped".into(),
            );
        }
    }

    fn parallel(&mut self) {
        let Some(p) = self.r.parallel().cloned() else {
            return;
        };
        let r = self.tool.corner_radius();
        if self.p.is_set("passes.stepover") && self.p.is_set("passes.scallop_height") {
            self.e(
                "param.parallel.stepover_and_scallop",
                "passes.scallop_height",
                "set either stepover or scallop_height, not both".into(),
            );
        }
        match p.scallop_height {
            None => self.required("passes.stepover", p.stepover),
            Some(h) => {
                if r <= 0.0 {
                    self.e(
                        "param.parallel.scallop.radius",
                        "passes.scallop_height",
                        "scallop height needs a tool with a corner radius".into(),
                    );
                } else if !(h > 0.0 && h < r) {
                    self.e(
                        "param.parallel.scallop.range",
                        "passes.scallop_height",
                        format!(
                            "scallop height {h} must be above 0 and below the corner radius {r}"
                        ),
                    );
                }
            }
        }
        let (lo, hi) = (p.min_slope_deg, p.max_slope_deg);
        if !(0.0..=90.0).contains(&lo) || !(0.0..=90.0).contains(&hi) {
            self.e(
                "param.parallel.slope.range",
                "passes.min_slope_deg",
                "slope limits must lie in [0, 90]".into(),
            );
        } else if lo >= hi {
            self.e(
                "param.parallel.slope.order",
                "passes.min_slope_deg",
                format!("min slope {lo}° must be below max slope {hi}°"),
            );
        }
    }
}
