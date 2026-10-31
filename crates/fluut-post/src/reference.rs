// SPDX-License-Identifier: MIT OR Apache-2.0

//! The reference toolpath suite (spec 05 §5.11, D10 §8): synthetic toolpaths
//! built in code that exercise every event and the rounding edge cases. Used
//! for golden output of the built-in posts, rebasing checks and post-editor
//! previews.

use crate::job::{
    Coolant, Feeds, MachineInfo, PostJob, Provenance, SetupJob, SetupKind, ToolInfo, ToolpathJob,
};
use crate::template::CompiledPost;
use fluut_geom::{Point3, UnitVec3, Vec3};
use fluut_path::{
    AxisPos, CompSide, CycleKind, CycleParams, FeedRef, MarkerKind, Motion, MotionInfo, Pass,
    PassKind, Segment, ToolpathResult,
};
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

/// Which optional parts of the suite a post can take.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct SuiteOptions {
    /// Indexed and rotary Setups.
    pub rotary: bool,
    /// A pass with controller cutter compensation.
    pub comp: bool,
}

impl SuiteOptions {
    pub fn for_post(cp: &CompiledPost) -> SuiteOptions {
        let has = |b: &str| cp.blocks.get(b).is_some_and(|l| !l.is_empty());
        SuiteOptions {
            rotary: cp.post.rotary.enabled,
            comp: has("cutter_comp_left") && has("cutter_comp_off"),
        }
    }
}

/// Machining tolerance of every reference segment, mm.
const TOL: f32 = 0.01;

/// Builds one toolpath's passes from a moving current position.
struct PathBuilder {
    cur: AxisPos,
    segs: Vec<Segment>,
    passes: Vec<Pass>,
}

fn z_axis() -> UnitVec3 {
    UnitVec3::Z
}

impl PathBuilder {
    fn new(x: f64, y: f64, z: f64, a: Option<f64>) -> PathBuilder {
        PathBuilder {
            cur: AxisPos {
                xyz: Point3::new(x, y, z),
                a,
                b: None,
            },
            segs: Vec::new(),
            passes: Vec::new(),
        }
    }

    fn to(&self, x: f64, y: f64, z: f64) -> AxisPos {
        AxisPos {
            xyz: Point3::new(x, y, z),
            ..self.cur
        }
    }

    fn line(&mut self, motion: Motion, feed: FeedRef, b: AxisPos) -> &mut Self {
        self.segs.push(Segment::Line {
            a: self.cur,
            b,
            m: MotionInfo::new(motion, feed, TOL),
        });
        self.cur = b;
        self
    }

    fn rapid(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        let b = self.to(x, y, z);
        self.line(Motion::Rapid, FeedRef::Rapid, b)
    }

    fn retract(&mut self, z: f64) -> &mut Self {
        let p = self.cur.xyz;
        let b = self.to(p.x(), p.y(), z);
        self.line(Motion::Retract, FeedRef::Rapid, b)
    }

    fn plunge(&mut self, z: f64) -> &mut Self {
        let p = self.cur.xyz;
        let b = self.to(p.x(), p.y(), z);
        self.line(Motion::Plunge, FeedRef::Plunge, b)
    }

    fn cut(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        let b = self.to(x, y, z);
        self.line(Motion::Cut, FeedRef::Cut, b)
    }

    /// Arc about `cur + offset` with axis `normal`; a helix if `rise` ≠ 0.
    fn arc(&mut self, offset: [f64; 3], normal: UnitVec3, sweep: f64, rise: f64) -> &mut Self {
        let a = self.cur;
        let centre = a.xyz + Vec3::new(offset[0], offset[1], offset[2]);
        let n = normal.get();
        let b = if sweep.abs() == TAU && rise == 0.0 {
            a
        } else {
            let v = a.xyz - centre;
            let (s, c) = (libm::sin(sweep), libm::cos(sweep));
            let rot = v * c + n.cross(v) * s + n * (v.dot(n) * (1.0 - c));
            AxisPos {
                xyz: centre + rot + n * rise,
                ..a
            }
        };
        let m = MotionInfo::new(Motion::Cut, FeedRef::Cut, TOL);
        self.segs.push(if rise == 0.0 {
            Segment::Arc {
                a,
                b,
                centre,
                normal,
                sweep,
                m,
            }
        } else {
            Segment::Helix {
                a,
                b,
                centre,
                normal,
                sweep,
                m,
            }
        });
        self.cur = b;
        self
    }

    fn rotary(&mut self, a_to: f64, x_to: f64, radius: f64) -> &mut Self {
        let p = self.cur.xyz;
        let b = AxisPos {
            xyz: Point3::new(x_to, p.y(), p.z()),
            a: Some(a_to),
            b: None,
        };
        self.segs.push(Segment::Rotary {
            a: self.cur,
            b,
            radius,
            m: MotionInfo::new(Motion::Cut, FeedRef::Cut, TOL),
        });
        self.cur = b;
        self
    }

    /// A rapid to the hole above its R plane, then the cycle.
    fn hole(&mut self, x: f64, y: f64, cycle: CycleParams) -> &mut Self {
        self.rapid(x, y, cycle.retract_z);
        self.segs.push(Segment::Cycle {
            at: self.cur,
            cycle,
            m: MotionInfo::new(Motion::Plunge, FeedRef::Plunge, TOL),
        });
        self
    }

    fn marker(&mut self, kind: MarkerKind) -> &mut Self {
        self.segs.push(Segment::Marker { kind });
        self
    }

    fn pass(&mut self, kind: PassKind, comp: Option<CompSide>) -> &mut Self {
        let mut p = Pass::new(kind, std::mem::take(&mut self.segs));
        p.comp = comp;
        self.passes.push(p);
        self
    }

    fn finish(&mut self) -> Arc<ToolpathResult> {
        if !self.segs.is_empty() {
            self.pass(PassKind::Cut, None);
        }
        let passes = std::mem::take(&mut self.passes);
        Arc::new(ToolpathResult::new(passes).expect("reference toolpaths are valid"))
    }
}

fn flat6() -> ToolInfo {
    ToolInfo {
        number: 1,
        name: "6 mm flat".into(),
        diameter: 6.0,
        corner_radius: 0.0,
    }
}

fn drill5() -> ToolInfo {
    ToolInfo {
        number: 2,
        name: "5 mm drill".into(),
        diameter: 5.0,
        corner_radius: 0.0,
    }
}

fn feeds(cut: f64, plunge: f64) -> Feeds {
    Feeds {
        cut,
        plunge,
        ramp: plunge,
        lead_in: cut / 2.0,
        lead_out: cut / 2.0,
        transition: 2000.0,
    }
}

/// Lines in all axes, arcs in every plane and sweep, tiny and huge radii, a
/// helix and values that stress rounding; a compensated contour if asked.
fn profile(opts: SuiteOptions) -> Arc<ToolpathResult> {
    let mut p = PathBuilder::new(0.0, 0.0, 25.0, None);
    p.rapid(0.0, 0.0, 5.0).plunge(-1.0);
    p.cut(10.0, 0.0, -1.0)
        .cut(10.0, 10.0, -1.0)
        .cut(0.0, 10.0, -2.0);
    let deg = PI / 180.0;
    let mut sign = 1.0;
    for sweep in [1.0, 90.0, 179.0, 181.0, 359.0, 360.0] {
        p.arc([5.0, 0.0, 0.0], z_axis(), sign * sweep * deg, 0.0);
        sign = -sign;
    }
    p.arc([3.0, 0.0, 0.0], UnitVec3::Y, -90.0 * deg, 0.0);
    p.arc([0.0, 3.0, 0.0], UnitVec3::X, 90.0 * deg, 0.0);
    p.arc([0.01, 0.0, 0.0], z_axis(), 90.0 * deg, 0.0);
    p.arc([2000.0, 0.0, 0.0], z_axis(), -0.5 * deg, 0.0);
    p.arc([4.0, 0.0, 0.0], z_axis(), -2.0 * TAU, -2.0);
    // Rounding: a 5 at the last decimal, tiny negatives, binary near-halves.
    p.cut(12.0005, 3.0, -4.0)
        .cut(-0.00001, 3.0, -4.0)
        .cut(1.23449, 2.675, -4.0)
        .cut(1.2345, -0.0005, -4.0);
    p.retract(5.0);
    p.pass(PassKind::Cut, None);
    if opts.comp {
        p.rapid(30.0, 0.0, 5.0)
            .plunge(-1.0)
            .pass(PassKind::Lead, None);
        p.cut(40.0, 0.0, -1.0)
            .cut(40.0, 10.0, -1.0)
            .cut(30.0, 10.0, -1.0)
            .cut(30.0, 0.0, -1.0)
            .pass(PassKind::Cut, Some(CompSide::Left));
        p.retract(5.0).pass(PassKind::Retract, None);
    }
    p.finish()
}

/// One hole of each cycle kind except tapping (no built-in post taps), two
/// of the plain drill and peck kinds so `cycle_point` is used.
fn holes() -> Arc<ToolpathResult> {
    let c = |kind, peck, dwell_s| CycleParams {
        kind,
        top_z: 0.0,
        depth_z: -6.0,
        retract_z: 2.0,
        peck,
        dwell_s,
    };
    let mut p = PathBuilder::new(50.0, 0.0, 25.0, None);
    p.hole(50.0, 0.0, c(CycleKind::Drill, None, None))
        .hole(60.0, 0.0, c(CycleKind::Drill, None, None))
        .hole(70.0, 0.0, c(CycleKind::Peck, Some(2.5), None))
        .hole(80.0, 0.0, c(CycleKind::Peck, Some(2.5), None))
        .hole(90.0, 0.0, c(CycleKind::ChipBreak, Some(2.0), None))
        .hole(100.0, 0.0, c(CycleKind::Spot, None, Some(0.5)))
        .hole(110.0, 0.0, c(CycleKind::Ream, None, None))
        .hole(120.0, 0.0, c(CycleKind::Bore, None, None));
    p.retract(25.0);
    p.finish()
}

/// Manual NC items around a short finishing cut.
fn finish_with_markers() -> Arc<ToolpathResult> {
    let mut p = PathBuilder::new(0.0, 0.0, 25.0, None);
    p.marker(MarkerKind::Comment("Check clamps".into()))
        .marker(MarkerKind::OptionalStop)
        .rapid(0.0, 20.0, 5.0)
        .plunge(-0.5)
        .cut(20.0, 20.0, -0.5)
        .marker(MarkerKind::Dwell(2.0))
        .retract(25.0)
        .marker(MarkerKind::Stop);
    p.finish()
}

fn side_pocket() -> Arc<ToolpathResult> {
    let mut p = PathBuilder::new(0.0, 0.0, 25.0, None);
    p.rapid(5.0, 5.0, 2.0)
        .plunge(-1.0)
        .cut(15.0, 5.0, -1.0)
        .cut(15.0, 10.0, -1.0)
        .retract(25.0);
    p.finish()
}

/// Rotary moves without and with X motion.
fn wrap() -> Arc<ToolpathResult> {
    let mut p = PathBuilder::new(0.0, 0.0, 30.0, Some(0.0));
    p.rapid(0.0, 0.0, 15.0)
        .plunge(10.0)
        .rotary(360.0, 0.0, 10.0)
        .rotary(540.0, 20.0, 10.0)
        .retract(30.0);
    p.finish()
}

fn toolpath(
    name: &str,
    kind: &str,
    tool: ToolInfo,
    rpm: f64,
    coolant: Coolant,
    feeds: Feeds,
    result: Arc<ToolpathResult>,
) -> ToolpathJob {
    ToolpathJob {
        name: name.into(),
        kind: kind.into(),
        tool,
        spindle_rpm: rpm,
        coolant,
        feeds,
        safe_z: 25.0,
        result,
    }
}

/// The reference program. `provenance` comes from the post being tested.
pub fn reference_job(opts: SuiteOptions, provenance: Provenance) -> PostJob {
    let mut setups = vec![SetupJob {
        name: "Top".into(),
        kind: SetupKind::ThreeAxis,
        work_offset: "G54".into(),
        index_a: None,
        index_b: None,
        toolpaths: vec![
            toolpath(
                "Profile",
                "contour_2d",
                flat6(),
                18000.0,
                Coolant::Flood,
                feeds(1200.0, 300.0),
                profile(opts),
            ),
            toolpath(
                "Holes",
                "drill",
                drill5(),
                2500.0,
                Coolant::Flood,
                feeds(300.0, 150.0),
                holes(),
            ),
            toolpath(
                "Finish",
                "contour_2d",
                flat6(),
                20000.0,
                Coolant::Mist,
                feeds(1500.0, 400.0),
                finish_with_markers(),
            ),
        ],
    }];
    if opts.rotary {
        setups.push(SetupJob {
            name: "Side".into(),
            kind: SetupKind::Indexed,
            work_offset: "G55".into(),
            index_a: Some(90.0),
            index_b: None,
            toolpaths: vec![toolpath(
                "Side pocket",
                "pocket_2d",
                flat6(),
                20000.0,
                Coolant::Mist,
                feeds(1500.0, 400.0),
                side_pocket(),
            )],
        });
        setups.push(SetupJob {
            name: "Wrap".into(),
            kind: SetupKind::Rotary,
            work_offset: "G56".into(),
            index_a: None,
            index_b: None,
            toolpaths: vec![toolpath(
                "Wrap",
                "rotary_around_a",
                flat6(),
                20000.0,
                Coolant::Off,
                feeds(1000.0, 300.0),
                wrap(),
            )],
        });
    }
    PostJob {
        project_name: "Reference suite".into(),
        notes: String::new(),
        date: "2026-10-31".into(),
        time: "12:00:00".into(),
        provenance,
        machine: MachineInfo {
            name: "Reference machine".into(),
            tool_changer: "atc".into(),
            has_rotary: opts.rotary,
        },
        setups,
    }
}
