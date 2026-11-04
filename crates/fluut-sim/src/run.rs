// SPDX-License-Identifier: MIT OR Apache-2.0

//! A whole simulation (D11 §2–§5): the stock of a Setup cut by its
//! toolpaths in order, with checkpoints for scrubbing and the findings of
//! the verification.
//!
//! Every chord keeps the toolpath and the segment it came from (the index in
//! [`ToolpathResult::segments`] order) and the program time at its end, from
//! the time estimate (D11 §6). A checkpoint is the stock before each
//! toolpath; tiles are shared between checkpoints until a cut changes them.

use crate::dexel::TriDexel;
use crate::motion::Chorder;
use crate::sim::{self, CONTACT, Hit, Stats};
use crate::stock::Stock;
use crate::sweep::Sweep;
use crate::time::{Feeds, Kinematics, segment_times};
use crate::tool::{Pose, Tool};
use crate::verify::{self, Part, PartHeights, Rest};
use fluut_path::{Motion, ToolpathResult};
use glam::DVec3;

/// Bytes per ray of the stock, with room for spilled spans (S5: +20 %) and
/// for the tiles the checkpoints copy: about 3 to 5 copies of the stock on
/// the M7 measurements, so 4.
pub const BYTES_PER_RAY: f64 = 24.0 * 1.2 * 4.0;
/// Default memory budget (D11 §2).
pub const DEFAULT_BUDGET: usize = 512 << 20;

/// The ray spacing for a stock: twice the error target (S5: the largest
/// error on sharp detail is about half the spacing), or coarser when that
/// would not fit `budget` bytes.
pub fn resolution(stock: &Stock, error: f64, budget: usize) -> f64 {
    let b = stock.bbox();
    let s = b.max.0 - b.min.0;
    let area = s.x * s.y + s.y * s.z + s.z * s.x;
    let by_budget = (area * BYTES_PER_RAY / budget as f64).sqrt();
    (2.0 * error).max(by_budget)
}

/// A tool with its non-cutting parts (spec 04 §4.14). Heights are from the
/// tip along the axis.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Assembly {
    pub tool: Tool,
    /// Material above this height is cut by a part that does not cut.
    pub flute_length: Option<f64>,
    /// A shank wider than the cutter: its radius, from the flute length up.
    pub shank: Option<f64>,
    /// Holder: radius, bottom and top heights.
    pub holder: Option<(f64, f64, f64)>,
}

impl Assembly {
    pub fn bare(tool: Tool) -> Assembly {
        Assembly {
            tool,
            flute_length: None,
            shank: None,
            holder: None,
        }
    }
}

/// What a verification finding is about (D11 §5).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Check {
    /// The stock went below the part by more than the tolerance.
    Gouge,
    /// A rapid move cut material.
    RapidInStock,
    /// The holder met material.
    Holder,
    /// A shank wider than the cutter met material.
    Shank,
    /// The tool cut above its flute length.
    FluteLength,
}

impl Check {
    pub fn is_error(self) -> bool {
        self != Check::FluteLength
    }

    /// Stable id, the last part of the diagnostic code.
    pub fn id(self) -> &'static str {
        match self {
            Check::Gouge => "gouge",
            Check::RapidInStock => "rapid_in_stock",
            Check::Holder => "holder",
            Check::Shank => "shank",
            Check::FluteLength => "flute_length",
        }
    }

    fn probe(kind: u8) -> Check {
        match kind {
            CONTACT => Check::RapidInStock,
            HOLDER => Check::Holder,
            SHANK => Check::Shank,
            _ => Check::FluteLength,
        }
    }
}

const FLUTE: u8 = 1;
const SHANK: u8 = 2;
const HOLDER: u8 = 3;

/// One finding: the first place in a segment where a check failed.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Finding {
    pub check: Check,
    /// Index into [`Simulation::toolpaths`].
    pub toolpath: usize,
    /// Index of the segment in the toolpath's `segments()`.
    pub segment: usize,
    /// In the stock frame.
    pub position: DVec3,
    /// How far below the part, for gouges; 0 for the others.
    pub depth: f64,
}

/// Chords between checkpoints within a toolpath: a scrub re-runs at most
/// this many (D11 §4).
pub const CHECKPOINT: usize = 32768;

/// Most findings kept per check; the rest are counted.
pub const MAX_FINDINGS: usize = 1000;

/// One toolpath of the program.
#[derive(Clone, Debug)]
pub struct Program<'a> {
    pub id: String,
    pub result: &'a ToolpathResult,
    pub tool: Assembly,
    pub feeds: Feeds,
}

/// Settings of a run.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Settings {
    /// Ray spacing, mm (see [`resolution`]).
    pub res: f64,
    /// Rotary Setup: A angles turn the part.
    pub rotary: bool,
    pub kinematics: Kinematics,
    /// Gouge tolerance, mm (`tol.machining`).
    pub tol: f64,
}

/// Progress and cancel for a run.
pub trait Monitor: Sync {
    fn cancelled(&self) -> bool {
        false
    }

    /// `done` of `total` steps.
    fn progress(&self, _done: u64, _total: u64) {}
}

/// A monitor that never cancels.
#[derive(Copy, Clone, Debug, Default)]
pub struct Quiet;

impl Monitor for Quiet {}

/// The run was cancelled.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Cancelled;

/// A chord with where it came from.
#[derive(Copy, Clone, Debug)]
pub struct Chord {
    pub sweep: Sweep,
    pub toolpath: u32,
    pub segment: u32,
    pub rapid: bool,
    /// Program time at the end of the chord, s.
    pub end: f64,
    reach: (DVec3, DVec3),
}

impl sim::Chord for Chord {
    fn sweep(&self) -> &Sweep {
        &self.sweep
    }

    fn reach(&self) -> (DVec3, DVec3) {
        self.reach
    }

    fn reports_contact(&self) -> bool {
        self.rapid
    }
}

/// A toolpath in the simulation.
#[derive(Clone, PartialEq, Debug)]
pub struct Clip {
    pub id: String,
    /// Its chords in [`Simulation::chords`].
    pub chords: std::ops::Range<usize>,
    /// Program time at its start and end, s.
    pub start: f64,
    pub end: f64,
    pub tool: Assembly,
    /// The index in `segments()` of each pass's first segment.
    pub passes: Vec<usize>,
}

/// How far a playback step goes.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Step {
    /// One segment of a toolpath result.
    Move,
    Pass,
    Toolpath,
}

/// The finished simulation of a program.
#[derive(Clone, Debug)]
pub struct Simulation {
    pub stock: Stock,
    pub res: f64,
    pub toolpaths: Vec<Clip>,
    pub chords: Vec<Chord>,
    /// The stock after the first `n` chords, by `n`: before each toolpath
    /// and every [`CHECKPOINT`] chords within one, then the final stock.
    checkpoints: Vec<(usize, TriDexel)>,
    /// Sorted by toolpath, segment and check.
    pub findings: Vec<Finding>,
    /// Findings dropped past [`MAX_FINDINGS`], per check.
    pub dropped: usize,
    /// Rest material, when the run had a part.
    pub rest: Option<Rest>,
    /// The part's heights at the Z rays, when the run had a part.
    pub part: Option<PartHeights>,
    pub stats: Stats,
}

/// A part of the tool between heights `h0` and `h1` as a flat cylinder of
/// radius `r` over the chord.
fn band(s: &Sweep, r: f64, h0: f64, h1: f64) -> Option<Sweep> {
    if h1 <= h0 {
        return None;
    }
    let at = |p: &Pose| Pose {
        tip: p.tip + p.axis * h0,
        axis: p.axis,
        length: h1 - h0,
    };
    Some(Sweep::new(Tool::flat(r), at(&s.p0), at(&s.p1)))
}

/// The probes of a chord's tool (D11 §5 "holder/shank contact", "flute
/// length").
fn probes(a: &Assembly, s: &Sweep, out: &mut Vec<(u8, Sweep)>) {
    let top = s.p0.length;
    let below_holder = a.holder.map_or(top, |(_, b, _)| b.min(top));
    if let Some(f) = a.flute_length {
        let from = f.max(a.tool.profile_height());
        if let Some(r) = a.shank.filter(|&r| r > a.tool.radius) {
            out.extend(band(s, r, from, below_holder).map(|x| (SHANK, x)));
        }
        out.extend(band(s, a.tool.radius, from, below_holder).map(|x| (FLUTE, x)));
    }
    if let Some((r, b, t)) = a.holder {
        out.extend(band(s, r, b, t).map(|x| (HOLDER, x)));
    }
}

impl Simulation {
    /// Cuts `stock` with `programs` in order and verifies the result. With a
    /// `part`, gouges and rest material are checked too (D11 §5).
    pub fn run(
        stock: Stock,
        programs: &[Program],
        settings: &Settings,
        part: Option<&dyn Part>,
        monitor: &dyn Monitor,
    ) -> Result<Simulation, Cancelled> {
        Simulation::run_from(stock, None, programs, settings, part, monitor)
    }

    /// As [`Simulation::run`], from the tri-dexel stock `start` when given
    /// (a child Setup's material from its parent); `stock` then only bounds
    /// the tool's reach, and the ray spacing is `start`'s.
    pub fn run_from(
        stock: Stock,
        start: Option<TriDexel>,
        programs: &[Program],
        settings: &Settings,
        part: Option<&dyn Part>,
        monitor: &dyn Monitor,
    ) -> Result<Simulation, Cancelled> {
        let res = start.as_ref().map_or(settings.res, |d| d.res);
        let mut chords = Vec::new();
        let mut toolpaths = Vec::new();
        let mut t = 0.0;
        for (pi, p) in programs.iter().enumerate() {
            if pi > 0 {
                t += settings.kinematics.tool_change * settings.kinematics.factor;
            }
            let start = t;
            let first = chords.len();
            let mut chorder = Chorder::new(p.tool.tool, stock, settings.rotary, res);
            // Chords of arcs lie inside them; keep that well inside the
            // gouge tolerance.
            chorder.tol = chorder.tol.min(settings.tol / 4.0);
            let times = segment_times(p.result.segments(), &p.feeds, &settings.kinematics);
            let mut buf = Vec::new();
            let mut parts = Vec::new();
            for (si, seg) in p.result.segments().enumerate() {
                buf.clear();
                chorder.segment(seg, &mut buf);
                let rapid = seg.motion().is_some_and(|m| m.motion == Motion::Rapid);
                let n = buf.len();
                for (ci, s) in buf.iter().enumerate() {
                    parts.clear();
                    probes(&p.tool, s, &mut parts);
                    let reach = parts.iter().fold((s.min, s.max), |(lo, hi), (_, x)| {
                        (lo.min(x.min), hi.max(x.max))
                    });
                    chords.push(Chord {
                        sweep: *s,
                        toolpath: pi as u32,
                        segment: si as u32,
                        rapid,
                        end: t + times[si] * (ci + 1) as f64 / n as f64,
                        reach,
                    });
                }
                t += times[si];
            }
            let mut at = 0;
            let passes = p
                .result
                .passes()
                .iter()
                .map(|x| {
                    at += x.segments.len();
                    at - x.segments.len()
                })
                .collect();
            toolpaths.push(Clip {
                id: p.id.clone(),
                chords: first..chords.len(),
                start,
                end: t,
                tool: p.tool,
                passes,
            });
        }
        let total = chords.len() as u64;
        monitor.progress(0, total);
        let mut d = start.unwrap_or_else(|| TriDexel::new(&stock, res));
        let mut checkpoints = Vec::with_capacity(toolpaths.len() + 1);
        let mut stats = Stats::default();
        let mut hits = Vec::new();
        let assemblies: Vec<Assembly> = toolpaths.iter().map(|s| s.tool).collect();
        let probe = |c: &Chord, out: &mut Vec<(u8, Sweep)>| {
            probes(&assemblies[c.toolpath as usize], &c.sweep, out);
        };
        for span in &toolpaths {
            let mut from = span.chords.start;
            loop {
                checkpoints.push((from, d.clone()));
                let to = (from + CHECKPOINT).min(span.chords.end);
                let base = from as u64;
                let stop = |done: usize| {
                    monitor.progress(base + done as u64, total);
                    monitor.cancelled()
                };
                let (st, h) = sim::cut_chords(&mut d, &chords[from..to], Some(&probe), &stop);
                if monitor.cancelled() {
                    return Err(Cancelled);
                }
                stats.add(&st);
                hits.extend(h.into_iter().map(|x| Hit {
                    chord: x.chord + from as u32,
                    ..x
                }));
                from = to;
                if from >= span.chords.end {
                    break;
                }
            }
        }
        checkpoints.push((chords.len(), d));
        monitor.progress(total, total);
        let mut sim = Simulation {
            stock,
            res,
            toolpaths,
            chords,
            checkpoints,
            findings: Vec::new(),
            dropped: 0,
            rest: None,
            part: None,
            stats,
        };
        let mut findings: Vec<Finding> = hits
            .iter()
            .map(|h| {
                let c = &sim.chords[h.chord as usize];
                Finding {
                    check: Check::probe(h.kind),
                    toolpath: c.toolpath as usize,
                    segment: c.segment as usize,
                    position: h.point,
                    depth: 0.0,
                }
            })
            .collect();
        if let Some(part) = part {
            let (g, rest, heights) =
                verify::gouges_and_rest(&sim, part, settings.tol, &|| monitor.cancelled())
                    .ok_or(Cancelled)?;
            findings.extend(g);
            sim.rest = Some(rest);
            sim.part = Some(heights);
        }
        sim.set_findings(findings);
        Ok(sim)
    }

    /// Keeps the first finding per toolpath, segment and check, at most
    /// [`MAX_FINDINGS`] per check.
    fn set_findings(&mut self, mut f: Vec<Finding>) {
        f.sort_by_key(|x| (x.toolpath, x.segment, x.check));
        f.dedup_by_key(|x| (x.toolpath, x.segment, x.check));
        let mut count = std::collections::BTreeMap::new();
        let before = f.len();
        f.retain(|x| {
            let n = count.entry(x.check).or_insert(0usize);
            *n += 1;
            *n <= MAX_FINDINGS
        });
        self.dropped = before - f.len();
        self.findings = f;
    }

    /// The stock after the whole program.
    pub fn stock_final(&self) -> &TriDexel {
        &self.checkpoints.last().expect("a final stock").1
    }

    /// The stock before any cut.
    pub fn stock_initial(&self) -> &TriDexel {
        &self.checkpoints[0].1
    }

    /// The stock before toolpath `i` (`i = toolpaths.len()`: the final one).
    pub fn checkpoint(&self, i: usize) -> &TriDexel {
        let n = self
            .toolpaths
            .get(i)
            .map_or(self.chords.len(), |c| c.chords.start);
        let k = self.checkpoints.partition_point(|(m, _)| *m <= n);
        &self.checkpoints[k - 1].1
    }

    /// Bytes of the stock and every checkpoint, shared tiles once.
    pub fn memory(&self) -> usize {
        let mut seen = std::collections::BTreeSet::new();
        self.checkpoints
            .iter()
            .flat_map(|(_, d)| d.grids.iter())
            .map(|g| g.unique_bytes(&mut seen))
            .sum()
    }

    /// Program length in seconds.
    pub fn duration(&self) -> f64 {
        self.toolpaths.last().map_or(0.0, |s| s.end)
    }

    /// Number of chords that end by time `t`.
    pub fn chords_at(&self, t: f64) -> usize {
        self.chords.partition_point(|c| c.end <= t)
    }

    /// The stock after the first `n` chords: the latest checkpoint at or
    /// before them, cut by the chords after it (D11 §4).
    pub fn stock_after(&self, n: usize) -> TriDexel {
        let n = n.min(self.chords.len());
        let (from, cp) = &self.checkpoints[self.checkpoint_index(n)];
        let mut d = cp.clone();
        self.cut(&mut d, *from..n);
        d
    }

    /// The latest checkpoint at or before `n` chords: its chord count and
    /// stock.
    pub(crate) fn checkpoint_at(&self, n: usize) -> (usize, &TriDexel) {
        let (from, d) = &self.checkpoints[self.checkpoint_index(n)];
        (*from, d)
    }

    /// The index of the latest checkpoint at or before `n` chords.
    pub(crate) fn checkpoint_index(&self, n: usize) -> usize {
        self.checkpoints.partition_point(|(m, _)| *m <= n) - 1
    }

    /// Cuts `d` with chords `r`. Span subtraction is a set difference, so
    /// cutting `a..b` and then `b..c` gives the same stock as `a..c`.
    pub(crate) fn cut(&self, d: &mut TriDexel, r: std::ops::Range<usize>) {
        sim::cut_chords(d, &self.chords[r], None, &|_| false);
    }

    /// The chords of the move or pass that chord `i` belongs to.
    fn unit(&self, i: usize, unit: Step) -> std::ops::Range<usize> {
        let c = &self.chords[i];
        let clip = &self.toolpaths[c.toolpath as usize];
        let seg = c.segment as usize;
        let (s0, s1) = match unit {
            Step::Toolpath => return clip.chords.clone(),
            Step::Move => (seg, seg + 1),
            Step::Pass => {
                let p = clip.passes.partition_point(|&s| s <= seg).max(1);
                let next = clip.passes.get(p).copied().unwrap_or(usize::MAX);
                (clip.passes[p - 1], next)
            }
        };
        let r = clip.chords.clone();
        let cs = &self.chords[r.clone()];
        let at = |s: usize| r.start + cs.partition_point(|x| (x.segment as usize) < s);
        at(s0)..at(s1)
    }

    /// The program time one move, pass or toolpath on from `t`: the end of
    /// the one under way at `t`; going back, the end of the one before it
    /// (D11 §4). Moves and passes end with their last chord, so moves that
    /// cut nowhere near the stock are stepped over; toolpaths end at their
    /// clip's end.
    pub fn step(&self, t: f64, unit: Step, forward: bool) -> f64 {
        if unit == Step::Toolpath {
            let ends = self.toolpaths.iter().map(|c| c.end);
            return if forward {
                ends.into_iter().find(|&e| e > t).unwrap_or(self.duration())
            } else {
                ends.rev().find(|&e| e < t).unwrap_or(0.0)
            };
        }
        if forward {
            let n = self.chords_at(t);
            if n >= self.chords.len() {
                return self.duration();
            }
            return self.chords[self.unit(n, unit).end - 1].end;
        }
        let n = self.chords.partition_point(|c| c.end < t);
        if n == 0 {
            return 0.0;
        }
        let u = self.unit(n - 1, unit);
        let end = self.chords[u.end - 1].end;
        if end < t {
            end
        } else if u.start == 0 {
            0.0
        } else {
            self.chords[u.start - 1].end
        }
    }

    /// The stock at program time `t`, s.
    pub fn stock_at(&self, t: f64) -> TriDexel {
        self.stock_after(self.chords_at(t))
    }

    /// Toolpath index and tool pose at time `t`: the end of the last chord
    /// done by then.
    pub fn tool_at(&self, t: f64) -> Option<(usize, Pose)> {
        let n = self.chords_at(t);
        let c = self.chords.get(n.checked_sub(1)?)?;
        Some((c.toolpath as usize, c.sweep.p1))
    }
}

#[cfg(test)]
pub(crate) mod tests;
