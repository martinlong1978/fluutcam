// SPDX-License-Identifier: MIT OR Apache-2.0

//! The G-code emitter (spec 05 §5.6, D10 §4).
//!
//! Walks Setups, Toolpaths, passes and segments in order, fires the block for
//! each event, and renders block lines: words are formatted per their
//! `[words.*]` entry and suppressed when `when = "changed"` and unchanged;
//! built-in placeholders use the default format.
//!
//! Safety rule: after any non-motion block writes a line (tool change, setup
//! start, …), the emitter forgets the last emitted axis words and motion code.
//! Such blocks may move the machine (e.g. `G53 G0 Z0`), so the next move
//! writes every axis in full.

use crate::arcs::{self, Plane, corrected_centre, principal_plane, radius_error, split};
use crate::expr::{Context, Value};
use crate::format::{format_number, round_dec};
use crate::job::{Coolant, PostJob, SetupJob, SetupKind, ToolpathJob};
use crate::post::{ArcPlane, CentreMode, CycleName, RotaryMethod, Units, When, WordFmt};
use crate::template::{CompiledPost, Tok};
use fluut_geom::{Aabb3, Point3};
use fluut_path::{
    AxisPos, CompSide, CycleKind, CycleParams, MarkerKind, Motion, MotionInfo, Segment, linearize,
};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

/// Retract between pecks when expanding peck and chip-break cycles, mm
/// (capped at half the peck depth).
const PECK_GAP: f64 = 0.5;

/// Words forgotten after a non-motion block (see module docs).
const POSITION_WORDS: [&str; 8] = ["X", "Y", "Z", "A", "B", "I", "J", "K"];

/// One output line that came from a segment (D10 §4.7), for the Post-tab
/// preview.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct LineMapEntry {
    pub file: usize,
    /// 0-based line index in the file.
    pub line: usize,
    pub setup: usize,
    pub toolpath: usize,
    pub pass: usize,
    pub segment: usize,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct PostOutput {
    pub files: Vec<String>,
    pub line_map: Vec<LineMapEntry>,
    pub warnings: Vec<String>,
}

#[derive(Clone, PartialEq, Debug, thiserror::Error)]
pub enum EmitError {
    #[error("setup {setup}, toolpath {toolpath}: {what}")]
    Unsupported {
        setup: usize,
        toolpath: usize,
        what: &'static str,
    },
}

#[derive(Clone, PartialEq, Debug)]
enum Val {
    Num(f64),
    Text(String),
}

/// Posts `job` with `post`.
pub fn post(job: &PostJob, post: &CompiledPost) -> Result<PostOutput, EmitError> {
    let mut e = Emitter::new(job, post);
    e.run()?;
    Ok(e.finish())
}

struct Emitter<'a> {
    cp: &'a CompiledPost,
    job: &'a PostJob,
    eol: &'static str,
    /// Factor from mm to output units.
    unit: f64,
    lines: Vec<String>,
    line_map: Vec<LineMapEntry>,
    warnings: Vec<String>,
    line_no: u64,
    /// Last emitted text per word key, for `when = "changed"`.
    last_word: BTreeMap<String, String>,
    /// Motion code (`G0`…`G3`) last written.
    motion_code: Option<String>,
    prev_event: Option<String>,
    tool: Option<u32>,
    spindle: Option<f64>,
    coolant: Coolant,
    feed: Option<f64>,
    /// Values valid for the current Setup and Toolpath.
    base: BTreeMap<&'static str, Val>,
    /// Values for the current event only.
    event: BTreeMap<&'static str, Val>,
    cx: BTreeMap<String, Value>,
    /// (setup, toolpath, pass, segment) of the segment being written.
    at: Option<(usize, usize, usize, usize)>,
    /// Rounded value last written per position word, in output units: the
    /// controller's idea of where the tool is (spec 05 §5.8).
    emitted: BTreeMap<String, f64>,
    /// Decimals raised for this event's words (arc centre correction).
    dec_override: BTreeMap<&'static str, u32>,
    rotary_clamped: bool,
    /// G93 in effect (spec 05 §5.7).
    inverse_time: bool,
    /// Parameters of the canned cycle in effect.
    cycle_open: Option<CycleParams>,
    /// Rapid links between holes held back while a cycle is in effect: the
    /// next `cycle_point` makes them unnecessary; anything else writes them.
    pending_links: Vec<AxisPos>,
    comp: Option<CompSide>,
    /// Finished files, when output is split.
    files: Vec<String>,
    /// Bytes in the current file.
    bytes: u64,
}

impl<'a> Emitter<'a> {
    fn new(job: &'a PostJob, cp: &'a CompiledPost) -> Emitter<'a> {
        let out = &cp.post.output;
        let mut e = Emitter {
            cp,
            job,
            eol: out.line_ending.as_str(),
            unit: if out.units == Units::Inch {
                1.0 / 25.4
            } else {
                1.0
            },
            lines: Vec::new(),
            line_map: Vec::new(),
            warnings: cp.warnings.clone(),
            line_no: cp.post.line_numbering.start,
            last_word: BTreeMap::new(),
            motion_code: None,
            prev_event: None,
            tool: None,
            spindle: None,
            coolant: Coolant::Off,
            feed: None,
            base: BTreeMap::new(),
            event: BTreeMap::new(),
            cx: BTreeMap::new(),
            at: None,
            emitted: BTreeMap::new(),
            dec_override: BTreeMap::new(),
            rotary_clamped: false,
            inverse_time: false,
            cycle_open: None,
            pending_links: Vec::new(),
            comp: None,
            files: Vec::new(),
            bytes: 0,
        };
        e.init_program_values();
        e
    }

    fn init_program_values(&mut self) {
        let j = self.job;
        let text = |s: &str| Val::Text(s.to_owned());
        self.base.insert("DATE", text(&j.date));
        self.base.insert("TIME", text(&j.time));
        self.base.insert("FILE_NOTES", text(&j.notes));
        self.base.insert("MACHINE_NAME", text(&j.machine.name));
        self.base.insert("POST_ID", text(&j.provenance.post_id));
        self.base
            .insert("FLUUT_VERSION", text(env!("CARGO_PKG_VERSION")));
        let bounds = j
            .setups
            .iter()
            .flat_map(|s| &s.toolpaths)
            .fold(Aabb3::empty(), |b, t| b.union(&t.result.stats().bounds));
        if !bounds.is_empty() {
            for (k, v) in [
                ("XMIN", bounds.min.x()),
                ("YMIN", bounds.min.y()),
                ("ZMIN", bounds.min.z()),
                ("XMAX", bounds.max.x()),
                ("YMAX", bounds.max.y()),
                ("ZMAX", bounds.max.z()),
            ] {
                self.base.insert(k, Val::Num(v * self.unit));
            }
        }
        for (k, v) in [("XH", 0.0), ("YH", 0.0), ("ZH", 0.0)] {
            self.base.insert(k, Val::Num(v));
        }
        let cx = &mut self.cx;
        cx.insert("machine.name".into(), Value::Str(j.machine.name.clone()));
        cx.insert(
            "machine.tool_changer".into(),
            Value::Str(j.machine.tool_changer.clone()),
        );
        cx.insert(
            "machine.has_rotary".into(),
            Value::Bool(j.machine.has_rotary),
        );
        self.update_state_cx();
    }

    fn update_state_cx(&mut self) {
        let cx = &mut self.cx;
        cx.insert(
            "state.coolant".into(),
            Value::Str(self.coolant.as_str().into()),
        );
        cx.insert(
            "state.spindle_on".into(),
            Value::Bool(self.spindle.is_some()),
        );
        cx.insert(
            "state.spindle_rpm".into(),
            Value::Num(self.spindle.unwrap_or(0.0)),
        );
        cx.insert("state.feed".into(), Value::Num(self.feed.unwrap_or(0.0)));
        cx.insert(
            "state.rotary_clamped".into(),
            Value::Bool(self.rotary_clamped),
        );
    }

    fn run(&mut self) -> Result<(), EmitError> {
        self.provenance();
        self.block("header");
        for (si, setup) in self.job.setups.iter().enumerate() {
            self.setup_values(si, setup);
            self.block("setup_start");
            self.setup_rotary(setup.kind);
            for (ti, tp) in setup.toolpaths.iter().enumerate() {
                self.toolpath(si, ti, tp)?;
            }
            self.block("setup_end");
        }
        self.stop_spindle_and_coolant();
        self.block("footer");
        Ok(())
    }

    fn finish(mut self) -> PostOutput {
        self.close_file();
        PostOutput {
            files: self.files,
            line_map: self.line_map,
            warnings: self.warnings,
        }
    }

    fn close_file(&mut self) {
        let mut text = std::mem::take(&mut self.lines).join(self.eol);
        if !text.is_empty() {
            text.push_str(self.eol);
        }
        self.files.push(text);
        self.bytes = 0;
    }

    /// Ends the current file and starts the next (spec 05 §5.5
    /// `max_file_size`). Each file is a whole program: provenance, header,
    /// spindle and coolant restored, …, footer. The tool stays loaded.
    fn split_file(&mut self) {
        self.block("footer");
        self.close_file();
        self.provenance();
        self.block("header");
        if self.spindle.is_some() {
            self.block("spindle_on");
        }
        if self.coolant != Coolant::Off {
            self.block("coolant_on");
        }
    }

    /// Rotary state at a Setup start (spec 05 §5.6).
    fn setup_rotary(&mut self, kind: SetupKind) {
        match kind {
            SetupKind::Indexed => {
                if self.rotary_clamped {
                    self.block("rotary_unclamp");
                }
                self.block("rotary_index");
                self.rotary_clamped = self.has("rotary_clamp");
                self.block("rotary_clamp");
            }
            SetupKind::Rotary if self.rotary_clamped => {
                self.block("rotary_unclamp");
                self.rotary_clamped = false;
            }
            _ => {}
        }
        self.update_state_cx();
    }

    fn provenance(&mut self) {
        let o = &self.cp.post.output;
        let p = &self.job.provenance;
        let mut body = vec![format!("FluutCAM {}", env!("CARGO_PKG_VERSION"))];
        if !self.job.project_name.is_empty() {
            body.push(format!("Project: {}", self.job.project_name));
        }
        body.push(format!("Post: {}", p.post_id));
        if !p.layers.is_empty() {
            body.push(format!("Layers: {}", p.layers.join(", ")));
        }
        if !p.hash.is_empty() {
            body.push(format!("Post hash: {}", p.hash));
        }
        let body: Vec<String> = body
            .into_iter()
            .map(|b| format!("{}{}{}", o.comment_start, b, o.comment_end))
            .collect();
        for line in body {
            self.push_line(line, false, false);
        }
    }

    fn setup_values(&mut self, si: usize, s: &SetupJob) {
        let text = |v: &str| Val::Text(v.to_owned());
        self.base.insert("SETUP_NAME", text(&s.name));
        self.base.insert("WORK_OFFSET", text(&s.work_offset));
        for (k, v) in [("INDEX_A", s.index_a), ("INDEX_B", s.index_b)] {
            match v {
                Some(a) => self.base.insert(k, Val::Num(a)),
                None => self.base.remove(k),
            };
        }
        let cx = &mut self.cx;
        cx.insert("setup.name".into(), Value::Str(s.name.clone()));
        cx.insert("setup.kind".into(), Value::Str(s.kind.as_str().into()));
        cx.insert("setup.index".into(), Value::Num(si as f64 + 1.0));
        cx.insert("setup.index_a".into(), Value::Num(s.index_a.unwrap_or(0.0)));
        cx.insert("setup.index_b".into(), Value::Num(s.index_b.unwrap_or(0.0)));
        cx.insert(
            "setup.work_offset".into(),
            Value::Str(s.work_offset.clone()),
        );
    }

    fn toolpath_values(&mut self, ti: usize, tp: &ToolpathJob) {
        let u = self.unit;
        let t = &tp.tool;
        self.base.insert("T", Val::Num(t.number as f64));
        self.base.insert("TOOLNAME", Val::Text(t.name.clone()));
        self.base.insert("TDIA", Val::Num(t.diameter * u));
        self.base.insert("S", Val::Num(tp.spindle_rpm));
        self.base.insert("SAFEZ", Val::Num(tp.safe_z * u));
        self.base
            .insert("TOOLPATH_NAME", Val::Text(tp.name.clone()));
        self.base.insert("FP", Val::Num(tp.feeds.plunge * u));
        self.base.insert("FC", Val::Num(tp.feeds.cut * u));
        let cx = &mut self.cx;
        cx.insert("tool.number".into(), Value::Num(t.number as f64));
        cx.insert("tool.name".into(), Value::Str(t.name.clone()));
        cx.insert("tool.diameter".into(), Value::Num(t.diameter));
        cx.insert("tool.corner_radius".into(), Value::Num(t.corner_radius));
        cx.insert("toolpath.name".into(), Value::Str(tp.name.clone()));
        cx.insert("toolpath.type".into(), Value::Str(tp.kind.clone()));
        cx.insert("toolpath.index".into(), Value::Num(ti as f64 + 1.0));
        cx.insert("state.first_tool".into(), Value::Bool(self.tool.is_none()));
    }

    fn stop_spindle_and_coolant(&mut self) {
        if self.coolant != Coolant::Off {
            self.block("coolant_off");
            self.coolant = Coolant::Off;
        }
        if self.spindle.is_some() {
            self.block("spindle_off");
            self.spindle = None;
        }
        self.update_state_cx();
    }

    fn toolpath(&mut self, si: usize, ti: usize, tp: &ToolpathJob) -> Result<(), EmitError> {
        // A toolpath of markers only (`manual_nc`) emits its blocks and
        // nothing else: no tool change, spindle or toolpath blocks.
        if tp.result.segments().all(|s| s.start().is_none()) {
            for seg in tp.result.segments() {
                if let Segment::Marker { kind } = seg {
                    self.marker(kind);
                }
            }
            return Ok(());
        }
        self.toolpath_values(ti, tp);
        if self.tool != Some(tp.tool.number) {
            self.stop_spindle_and_coolant();
            self.block("tool_change");
            self.tool = Some(tp.tool.number);
        }
        if self.spindle != Some(tp.spindle_rpm) {
            self.spindle = Some(tp.spindle_rpm);
            self.update_state_cx();
            self.block("spindle_on");
        }
        if self.coolant != tp.coolant {
            let off = tp.coolant == Coolant::Off;
            if off {
                self.block("coolant_off");
            }
            self.coolant = tp.coolant;
            self.update_state_cx();
            if !off {
                self.block("coolant_on");
            }
        }
        self.block("toolpath_start");
        // The result starts where the tool must be (D10 §4.7): rapid there
        // first, so the first move never runs from wherever the tool change
        // left the tool.
        if let Some(start) = tp.result.segments().find_map(Segment::start).copied() {
            self.motion("rapid", &start, None, Vec::new());
        }
        let err = |what| EmitError::Unsupported {
            setup: si,
            toolpath: ti,
            what,
        };
        for (pi, pass) in tp.result.passes().iter().enumerate() {
            let max = self.cp.post.output.max_file_size;
            if max > 0
                && self.bytes > max
                && self.cycle_open.is_none()
                && self.comp.is_none()
                && matches!(self.prev_event.as_deref(), Some("rapid" | "retract"))
            {
                self.split_file();
            }
            if pass.comp != self.comp {
                let need = match pass.comp {
                    Some(CompSide::Left) => "cutter_comp_left",
                    Some(CompSide::Right) => "cutter_comp_right",
                    None => "cutter_comp_off",
                };
                if !self.has(need) {
                    return Err(err(
                        "controller compensation needs the post's cutter_comp blocks",
                    ));
                }
                self.set_comp(pass.comp);
            }
            for (gi, seg) in pass.segments.iter().enumerate() {
                self.at = Some((si, ti, pi, gi));
                self.segment(tp, seg).map_err(err)?;
            }
            if pass.comp.is_some() {
                self.set_comp(None);
            }
        }
        self.close_cycle();
        self.leave_inverse_time();
        self.at = None;
        self.block("toolpath_end");
        Ok(())
    }

    fn segment(&mut self, tp: &ToolpathJob, seg: &Segment) -> Result<(), &'static str> {
        if let (Some(c), Segment::Line { a, b, m }) = (&self.cycle_open, seg)
            && m.motion == Motion::Rapid
            && a.xyz.z().min(b.xyz.z()) >= c.retract_z - 1e-9
        {
            self.pending_links.push(*b);
            return Ok(());
        }
        if !matches!(seg, Segment::Cycle { .. }) {
            self.close_cycle();
        }
        if !matches!(seg, Segment::Rotary { .. } | Segment::Marker { .. }) {
            self.leave_inverse_time();
        }
        match seg {
            Segment::Line { a, b, m } => {
                let event = self.classify_line(a, b, m.motion, m.feed);
                let feed = tp.feeds.get(m.feed);
                let max = self.cp.post.output.max_segment_length;
                let len = b.xyz.distance(a.xyz);
                let n = if max > 0.0 && len > max {
                    (len / max).ceil() as usize
                } else {
                    1
                };
                for k in 1..=n {
                    let p = if k == n {
                        *b
                    } else {
                        lerp(a, b, k as f64 / n as f64)
                    };
                    self.motion(event, &p, feed, Vec::new());
                }
                Ok(())
            }
            Segment::Arc { .. } | Segment::Helix { .. } => self.arc(tp, seg),
            Segment::Spline { .. } => self.linear(tp, seg),
            Segment::Rotary { a, b, radius, m } => self.rotary(tp, seg, a, b, *radius, m),
            Segment::Cycle { at, cycle, m } => self.cycle(tp, at, cycle, m),
            Segment::Marker { kind } => {
                self.marker(kind);
                Ok(())
            }
        }
    }

    fn set_comp(&mut self, side: Option<CompSide>) {
        if self.comp.is_some() {
            self.block("cutter_comp_off");
        }
        match side {
            Some(CompSide::Left) => self.block("cutter_comp_left"),
            Some(CompSide::Right) => self.block("cutter_comp_right"),
            None => {}
        }
        self.comp = side;
    }

    /// A rotary move (spec 05 §5.7): split above `max_angle_per_line`, feed
    /// converted per `feed.rotary_method`.
    fn rotary(
        &mut self,
        tp: &ToolpathJob,
        seg: &Segment,
        a: &AxisPos,
        b: &AxisPos,
        radius: f64,
        m: &MotionInfo,
    ) -> Result<(), &'static str> {
        let post = &self.cp.post;
        if !post.rotary.enabled {
            return Err("rotary moves need a post with rotary.enabled");
        }
        let method = post.feed.rotary_method;
        let turn = |x: Option<f64>, y: Option<f64>| (y.unwrap_or(0.0) - x.unwrap_or(0.0)).abs();
        let angle = turn(a.a, b.a).max(turn(a.b, b.b));
        let n = ((angle / post.rotary.max_angle_per_line) - 1e-9)
            .ceil()
            .max(1.0) as usize;
        let piece_len = seg.length() / n as f64;
        let feed = tp.feeds.get(m.feed);
        if feed.is_some() && method == RotaryMethod::InverseTime && !self.inverse_time {
            self.push_line("G93".into(), false, true);
            self.inverse_time = true;
            self.last_word.remove("F");
        }
        for k in 1..=n {
            let p = if k == n {
                *b
            } else {
                lerp(a, b, k as f64 / n as f64)
            };
            let mut extra = Vec::new();
            if let Some(f) = feed {
                let fv = match method {
                    RotaryMethod::Degrees if radius > 0.0 => {
                        f / radius * 180.0 / std::f64::consts::PI
                    }
                    RotaryMethod::InverseTime if piece_len > 0.0 => f / piece_len,
                    _ => f * self.unit,
                };
                extra.push(("F", Val::Num(fv)));
                if method == RotaryMethod::InverseTime {
                    extra.push(("FI", Val::Num(fv)));
                }
            }
            // Inverse time needs F on every block (RS-274/NGC 3.5.19).
            if self.inverse_time {
                self.last_word.remove("F");
            }
            let event = if feed.is_none() { "rapid" } else { "rotary" };
            self.motion(event, &p, None, extra);
        }
        // The next linear move restates its feed through feed_change.
        self.feed = None;
        Ok(())
    }

    fn leave_inverse_time(&mut self) {
        if self.inverse_time {
            self.push_line("G94".into(), false, true);
            self.inverse_time = false;
            self.feed = None;
            self.last_word.remove("F");
        }
    }

    /// A drilling cycle: native when the post supports the kind, else
    /// expanded into moves (D10 §5).
    fn cycle(
        &mut self,
        tp: &ToolpathJob,
        at: &AxisPos,
        c: &CycleParams,
        m: &MotionInfo,
    ) -> Result<(), &'static str> {
        let post = &self.cp.post;
        let feed = tp.feeds.get(m.feed).or(Some(tp.feeds.plunge));
        let native = post.cycles.enabled && post.cycles.supported.contains(&cycle_name(c.kind));
        if !native {
            self.close_cycle();
            return self.expand_cycle(at, c, feed);
        }
        let u = self.unit;
        let mut extra = vec![
            ("CYCLE_Z", Val::Num(c.depth_z * u)),
            ("CYCLE_R", Val::Num(c.retract_z * u)),
        ];
        if let Some(q) = c.peck {
            extra.push(("CYCLE_Q", Val::Num(q * u)));
        }
        if let Some(d) = c.dwell_s {
            extra.push(("CYCLE_P", Val::Num(d)));
        }
        if let Some(f) = feed {
            extra.push(("CYCLE_F", Val::Num(f * u)));
        }
        let event = if self.cycle_open == Some(*c) {
            self.pending_links.clear();
            "cycle_point"
        } else {
            self.close_cycle();
            self.cycle_open = Some(*c);
            "cycle_start"
        };
        self.cx
            .insert("cycle.kind".into(), Value::Str(cycle_str(c.kind).into()));
        self.motion(event, &at_z(at, c.depth_z), feed, extra);
        Ok(())
    }

    fn close_cycle(&mut self) {
        if self.cycle_open.take().is_some() {
            self.block("cycle_end");
            for p in std::mem::take(&mut self.pending_links) {
                self.motion("rapid", &p, None, Vec::new());
            }
        }
    }

    fn expand_cycle(
        &mut self,
        at: &AxisPos,
        c: &CycleParams,
        feed: Option<f64>,
    ) -> Result<(), &'static str> {
        let rapid = |e: &mut Self, z: f64| e.motion("rapid", &at_z(at, z), None, Vec::new());
        let cut = |e: &mut Self, z: f64| e.motion("feed", &at_z(at, z), feed, Vec::new());
        rapid(self, c.retract_z);
        match c.kind {
            CycleKind::Tap => return Err("tapping needs a post with a native tap cycle"),
            CycleKind::Peck | CycleKind::ChipBreak if c.peck.is_some_and(|q| q > 0.0) => {
                let q = c.peck.unwrap_or(0.0);
                let gap = PECK_GAP.min(q / 2.0);
                let mut reached = c.top_z;
                loop {
                    if reached < c.top_z {
                        rapid(self, reached + gap);
                    }
                    let next = (reached - q).max(c.depth_z);
                    cut(self, next);
                    reached = next;
                    if next <= c.depth_z {
                        break;
                    }
                    if c.kind == CycleKind::Peck {
                        rapid(self, c.retract_z);
                    } else {
                        rapid(self, next + gap);
                    }
                }
            }
            _ => cut(self, c.depth_z),
        }
        if let Some(d) = c.dwell_s {
            self.dwell(d);
        }
        if matches!(c.kind, CycleKind::Ream | CycleKind::Bore) {
            cut(self, c.retract_z);
        } else {
            rapid(self, c.retract_z);
        }
        Ok(())
    }

    fn dwell(&mut self, seconds: f64) {
        if !self.has("dwell") {
            self.warnings
                .push("dwell skipped: the post has no `dwell` block".into());
            return;
        }
        self.event.clear();
        self.event.insert("DWELL", Val::Num(seconds));
        self.render("dwell", false);
        self.event.clear();
    }

    /// Manual NC items (spec 05 §5.4.1 `manual_nc`): the block's lines can
    /// test `marker.kind`.
    fn marker(&mut self, kind: &MarkerKind) {
        if !self.has("manual_nc") {
            self.warnings
                .push("manual NC item skipped: the post has no `manual_nc` block".into());
            return;
        }
        let (name, comment, dwell) = match kind {
            MarkerKind::Stop => ("stop", None, None),
            MarkerKind::OptionalStop => ("optional_stop", None, None),
            MarkerKind::Comment(t) => ("comment", Some(t.clone()), None),
            MarkerKind::Dwell(d) => ("dwell", None, Some(*d)),
            MarkerKind::PassThrough(t) => ("pass_through", Some(t.clone()), None),
        };
        self.cx
            .insert("marker.kind".into(), Value::Str(name.into()));
        self.event.clear();
        if let Some(c) = comment {
            self.event.insert("COMMENT", Val::Text(c));
        }
        if let Some(d) = dwell {
            self.event.insert("DWELL", Val::Num(d));
        }
        self.block("manual_nc");
        self.event.clear();
        self.cx.remove("marker.kind");
    }

    /// Writes `seg` as line moves within its own tolerance.
    fn linear(&mut self, tp: &ToolpathJob, seg: &Segment) -> Result<(), &'static str> {
        let tol = seg.motion().map_or(0.01, |m| f64::from(m.tol));
        for piece in linearize(seg, tol) {
            self.segment(tp, &piece)?;
        }
        Ok(())
    }

    /// An arc or helix: accepted, split and written as arc events, or
    /// linearized (spec 05 §5.5, D10 §4.2).
    fn arc(&mut self, tp: &ToolpathJob, seg: &Segment) -> Result<(), &'static str> {
        let (a, b, centre, normal, sweep, m, helix) = match seg {
            Segment::Arc {
                a,
                b,
                centre,
                normal,
                sweep,
                m,
            } => (a, b, *centre, *normal, *sweep, m, false),
            Segment::Helix {
                a,
                b,
                centre,
                normal,
                sweep,
                m,
            } => (a, b, *centre, *normal, *sweep, m, true),
            _ => unreachable!("arc() takes arcs and helices"),
        };
        let post = &self.cp.post;
        let cfg = &post.arcs;
        let feed = tp.feeds.get(m.feed);
        let Some((plane, axis_positive)) = principal_plane(normal) else {
            return self.linear(tp, seg);
        };
        let allowed = |planes: &[ArcPlane]| {
            planes.iter().any(|p| {
                *p == ArcPlane::Any
                    || matches!(
                        (p, plane),
                        (ArcPlane::XY, Plane::XY)
                            | (ArcPlane::XZ, Plane::XZ)
                            | (ArcPlane::YZ, Plane::YZ)
                    )
            })
        };
        let kind_ok = if helix {
            post.helix.enabled && allowed(&post.helix.planes)
        } else {
            cfg.enabled && allowed(&cfg.planes)
        };
        let r = arcs::radius(a, centre, normal);
        if !kind_ok || feed.is_none() || r < cfg.min_radius || r > cfg.max_radius {
            return self.linear(tp, seg);
        }
        let max = cfg.max_sweep_deg * std::f64::consts::PI / 180.0;
        let n = ((sweep.abs() / max) - 1e-9).ceil().max(1.0) as usize;
        let ccw = (sweep > 0.0) == axis_positive;
        for (pa, pb, sw) in split(a, b, centre, normal, sweep, n) {
            if !self.arc_piece(plane, ccw, helix, &pa, &pb, centre, sw, r, feed) {
                let piece = if helix {
                    Segment::Helix {
                        a: pa,
                        b: pb,
                        centre,
                        normal,
                        sweep: sw,
                        m: *m,
                    }
                } else {
                    Segment::Arc {
                        a: pa,
                        b: pb,
                        centre,
                        normal,
                        sweep: sw,
                        m: *m,
                    }
                };
                self.linear(tp, &piece)?;
            }
        }
        Ok(())
    }

    fn decimals(&self, key: &str) -> u32 {
        self.cp.post.words.get(key).map_or(3, |w| w.decimals)
    }

    /// Writes one arc piece with centre correction (D10 §4.4). Returns false
    /// if the rounded arc fails the radius check even with 6 decimals, or
    /// its rounded chord vanishes; the caller then linearizes it.
    #[allow(clippy::too_many_arguments)]
    fn arc_piece(
        &mut self,
        plane: Plane,
        ccw: bool,
        helix: bool,
        a: &AxisPos,
        b: &AxisPos,
        centre: fluut_geom::Point3,
        sweep: f64,
        radius: f64,
        feed: Option<f64>,
    ) -> bool {
        const AXES: [&str; 3] = ["X", "Y", "Z"];
        const INC: [&str; 3] = ["I", "J", "K"];
        const ABS: [&str; 3] = ["IA", "JA", "KA"];
        let u = self.unit;
        let (iu, iv, _) = plane.axes();
        let out = |p: fluut_geom::Point3| arcs::coords(p).map(|v| v * u);
        let (s, e, c) = (out(a.xyz), out(b.xyz), out(centre));
        let d0 = self.decimals(AXES[iu]).max(self.decimals(AXES[iv]));
        let start = |k: usize| {
            self.emitted
                .get(AXES[k])
                .copied()
                .unwrap_or_else(|| round_dec(s[k], d0))
        };
        let s2 = [start(iu), start(iv)];
        let full = sweep.abs() >= 2.0 * std::f64::consts::PI - 1e-9;
        let tol = self.cp.post.arcs.radius_tolerance * u;
        let mut chosen = None;
        for d in d0..=d0.max(6) {
            let e2 = [round_dec(e[iu], d), round_dec(e[iv], d)];
            if !full && e2 == s2 {
                return false;
            }
            let c2 = corrected_centre(s2, e2, [c[iu], c[iv]], d);
            if radius_error(s2, e2, c2) <= tol {
                chosen = Some((d, e2, c2));
                break;
            }
        }
        let Some((d, e2, c2)) = chosen else {
            return false;
        };
        let mut extra = Vec::new();
        for (k, idx) in [(0, iu), (1, iv)] {
            extra.push((INC[idx], Val::Num(c2[k] - s2[k])));
            extra.push((ABS[idx], Val::Num(c2[k])));
        }
        let c1 = corrected_centre(s2, e2, [c[iu], c[iv]], 9);
        let r_out = libm::hypot(c1[0] - s2[0], c1[1] - s2[1]);
        let sign = if sweep.abs() > std::f64::consts::PI {
            -1.0
        } else {
            1.0
        };
        if self.cp.post.arcs.centre_mode == CentreMode::R {
            extra.push(("R", Val::Num(sign * r_out)));
        }
        extra.push(("Radius", Val::Num(radius * u)));
        extra.push(("PLANE", Val::Text(plane.code().into())));
        if d > d0 {
            for k in [iu, iv] {
                self.dec_override.insert(AXES[k], d);
                self.dec_override.insert(INC[k], d);
                self.dec_override.insert(ABS[k], d);
            }
        }
        // RS-274 and Grbl reject an arc without an in-plane axis word, even
        // a full circle, so those words are always written.
        self.last_word.remove(AXES[iu]);
        self.last_word.remove(AXES[iv]);
        let event = match (helix, ccw) {
            (false, true) => "arc_ccw",
            (false, false) => "arc_cw",
            (true, true) => "helix_ccw",
            (true, false) => "helix_cw",
        };
        self.motion(event, b, feed, extra);
        self.dec_override.clear();
        true
    }

    /// Whether the post has block `block` with at least one line. A block
    /// emptied by `mode = "remove"` counts as absent, so its event falls back
    /// (e.g. `plunge` to `feed`) instead of writing nothing.
    fn has(&self, block: &str) -> bool {
        self.cp.blocks.get(block).is_some_and(|l| !l.is_empty())
    }

    /// Event for a line move (D10 §4.2).
    fn classify_line(
        &self,
        a: &AxisPos,
        b: &AxisPos,
        motion: Motion,
        feed: fluut_path::FeedRef,
    ) -> &'static str {
        if motion == Motion::Rapid || feed == fluut_path::FeedRef::Rapid {
            return "rapid";
        }
        let down = b.xyz.z() < a.xyz.z();
        match motion {
            Motion::Retract if self.has("retract") => "retract",
            Motion::Plunge | Motion::Ramp if down && self.has("plunge") => "plunge",
            _ => "feed",
        }
    }

    fn motion(
        &mut self,
        event: &'static str,
        b: &AxisPos,
        feed: Option<f64>,
        extra: Vec<(&'static str, Val)>,
    ) {
        let u = self.unit;
        self.event.clear();
        self.event.extend(extra);
        self.event.insert("X", Val::Num(b.xyz.x() * u));
        self.event.insert("Y", Val::Num(b.xyz.y() * u));
        self.event.insert("Z", Val::Num(b.xyz.z() * u));
        if let Some(a) = b.a {
            self.event.insert("A", Val::Num(a));
        }
        if let Some(bb) = b.b {
            self.event.insert("B", Val::Num(bb));
        }
        if let Some(f) = feed {
            self.event.insert("F", Val::Num(f * u));
            if self.feed != Some(f) {
                self.feed = Some(f);
                self.update_state_cx();
                if self.has("feed_change") {
                    self.render("feed_change", true);
                }
            }
        }
        let first = format!("first_{event}");
        let name = if self.prev_event.as_deref() != Some(event) && self.has(&first) {
            first
        } else {
            event.to_owned()
        };
        self.render(&name, true);
        self.prev_event = Some(event.to_owned());
        self.event.clear();
    }

    /// Fires a non-motion block, then applies the safety rule.
    fn block(&mut self, name: &str) {
        if self.render(name, false) > 0 {
            for w in POSITION_WORDS {
                self.last_word.remove(w);
                self.emitted.remove(w);
            }
            self.motion_code = None;
            self.prev_event = None;
        }
    }

    fn value(&self, key: &str) -> Option<&Val> {
        self.event.get(key).or_else(|| self.base.get(key))
    }

    /// Renders block `name`; returns the number of lines written.
    fn render(&mut self, name: &str, is_motion: bool) -> usize {
        let Some(lines) = self.cp.blocks.get(name) else {
            return 0;
        };
        let post = &self.cp.post;
        let dc = post.output.decimal_char.as_str();
        let default_fmt = WordFmt::default();
        let mut written = 0;
        for l in lines {
            if let Some(w) = &l.when
                && w.eval(&self.cx as &dyn Context) != Value::Bool(true)
            {
                continue;
            }
            let mut text = String::new();
            let mut lit = String::new();
            let mut words = 0;
            let mut shown = 0;
            let mut changed: Vec<(String, String, Option<f64>)> = Vec::new();
            for t in &l.toks {
                match t {
                    Tok::Lit(s) => {
                        text.push_str(s);
                        lit.push_str(s);
                    }
                    Tok::Word(key) => {
                        words += 1;
                        let over = self.dec_override.get(key.as_str()).map(|&d| WordFmt {
                            decimals: d,
                            ..post.words[key].clone()
                        });
                        let f = over.as_ref().unwrap_or(&post.words[key]);
                        let Some(v) = self.value(key) else {
                            strip_attached(&mut text);
                            strip_attached(&mut lit);
                            continue;
                        };
                        let s = match v {
                            Val::Num(n) => format!("{}{}", f.prefix, format_number(*n, f, dc)),
                            Val::Text(s) => format!("{}{}", f.prefix, s),
                        };
                        if f.when == When::Changed && self.last_word.get(key) == Some(&s) {
                            strip_attached(&mut text);
                            strip_attached(&mut lit);
                            continue;
                        }
                        shown += 1;
                        let rounded = match v {
                            Val::Num(n) => Some(round_dec(n * f.scale, f.decimals) / f.scale),
                            Val::Text(_) => None,
                        };
                        changed.push((key.clone(), s.clone(), rounded));
                        text.push_str(&s);
                    }
                    Tok::Placeholder(p) => match self.value(p) {
                        Some(Val::Num(n)) => {
                            shown += 1;
                            text.push_str(&format_number(*n, &default_fmt, dc));
                        }
                        Some(Val::Text(s)) => {
                            shown += 1;
                            text.push_str(s);
                        }
                        None => {}
                    },
                }
            }
            // D10 §3.1: drop a line whose words are all suppressed and whose
            // literal text is at most the motion code already in effect.
            let lit_code = motion_code(&lit);
            let lit_trim = lit.trim();
            if words > 0
                && shown == 0
                && (lit_trim.is_empty()
                    || (lit_code.is_some()
                        && lit_code == self.motion_code
                        && lit_trim.split_whitespace().count() == 1))
            {
                continue;
            }
            let mut text = text.trim().to_owned();
            if text.is_empty() {
                continue;
            }
            for (k, s, rounded) in changed {
                if let Some(r) = rounded
                    && POSITION_WORDS.contains(&k.as_str())
                {
                    self.emitted.insert(k.clone(), r);
                }
                self.last_word.insert(k, s);
            }
            if lit_code.is_some() {
                self.motion_code = lit_code;
            }
            for (from, to) in &post.output.substitute {
                text = text.replace(from.as_str(), to);
            }
            self.push_line(text, is_motion, true);
            written += 1;
        }
        written
    }

    /// Appends a line: line number (if enabled and `numbered`), line map
    /// entry (if `is_motion`), byte count.
    fn push_line(&mut self, mut text: String, is_motion: bool, numbered: bool) {
        let ln = &self.cp.post.line_numbering;
        if numbered && ln.enabled && !text.starts_with('%') {
            text = format!("{}{} {}", ln.prefix, self.line_no, text);
            self.line_no += ln.increment;
            if self.line_no > ln.maximum {
                self.line_no = ln.start;
            }
        }
        if is_motion && let Some((setup, toolpath, pass, segment)) = self.at {
            self.line_map.push(LineMapEntry {
                file: self.files.len(),
                line: self.lines.len(),
                setup,
                toolpath,
                pass,
                segment,
            });
        }
        self.bytes += (text.len() + self.eol.len()) as u64;
        self.lines.push(text);
    }
}

fn lerp(a: &AxisPos, b: &AxisPos, f: f64) -> AxisPos {
    let mix = |x: Option<f64>, y: Option<f64>| match (x, y) {
        (Some(x), Some(y)) => Some(x + (y - x) * f),
        _ => y,
    };
    AxisPos {
        xyz: a.xyz.lerp(b.xyz, f),
        a: mix(a.a, b.a),
        b: mix(a.b, b.b),
    }
}

fn at_z(p: &AxisPos, z: f64) -> AxisPos {
    AxisPos {
        xyz: Point3::new(p.xyz.x(), p.xyz.y(), z),
        ..*p
    }
}

fn cycle_str(k: CycleKind) -> &'static str {
    match k {
        CycleKind::Drill => "drill",
        CycleKind::Spot => "spot",
        CycleKind::Peck => "peck",
        CycleKind::ChipBreak => "chip_break",
        CycleKind::Tap => "tap",
        CycleKind::Ream => "ream",
        CycleKind::Bore => "bore",
    }
}

fn cycle_name(k: CycleKind) -> CycleName {
    match k {
        CycleKind::Drill => CycleName::Drill,
        CycleKind::Spot => CycleName::Spot,
        CycleKind::Peck => CycleName::Peck,
        CycleKind::ChipBreak => CycleName::ChipBreak,
        CycleKind::Tap => CycleName::Tap,
        CycleKind::Ream => CycleName::Ream,
        CycleKind::Bore => CycleName::Bore,
    }
}

/// Removes the letters written directly before a suppressed word (`F` in
/// `F[F]`) and one space before them, so `G1 X[X] Y[Y]` without `X` reads
/// `G1 Y…`.
fn strip_attached(s: &mut String) {
    while s.ends_with(|c: char| c.is_ascii_alphabetic()) {
        s.pop();
    }
    if s.ends_with(' ') {
        s.pop();
    }
}

/// The motion code a line's literal text starts with, normalised (`G00` →
/// `G0`), if any.
fn motion_code(lit: &str) -> Option<String> {
    let first = lit.split_whitespace().next()?;
    let n = first.strip_prefix(['G', 'g'])?;
    match n.trim_start_matches('0') {
        "" => Some("G0".into()),
        d @ ("1" | "2" | "3") => Some(format!("G{d}")),
        _ => None,
    }
}
