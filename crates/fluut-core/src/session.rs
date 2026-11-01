// SPDX-License-Identifier: MIT OR Apache-2.0

//! The session (D01 §5): one project, its definitions and geometry, the
//! result cache, and calculation and post jobs.

use crate::frame::SetupFrame;
use crate::graph::{Graph, NodeId};
use crate::store::ModelStore;
use fluut_cam::StrategyInput;
use fluut_cam::geometry::SetupBody;
use fluut_model::project::{Project, Setup, read};
use fluut_model::resolve::{Coolant, resolve};
use fluut_model::tools::{ToolLibrary, ToolStore};
use fluut_model::validate::{CycleSet, PostCaps, ValidateCx, validate};
use fluut_model::{DefRef, DefStore, Diagnostic, Severity};
use fluut_path::{PathStats, ToolpathResult};
use fluut_post::{
    CompiledPost, Feeds, MachineInfo, Post, PostJob, PostOutput, Provenance, SetupJob, ToolInfo,
    ToolpathJob, resolve_post,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What to calculate.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Targets {
    All,
    Toolpaths(Vec<String>),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum JobState {
    /// Calculated now, or taken from the cache.
    Done {
        cached: bool,
    },
    Failed,
    /// Disabled toolpath.
    Skipped,
}

/// The outcome of one calculation job.
#[derive(Clone, PartialEq, Debug)]
pub struct JobReport {
    pub job: u64,
    pub toolpath: String,
    pub state: JobState,
    pub diagnostics: Vec<Diagnostic>,
    pub stats: Option<PathStats>,
}

/// Values for the post output that do not come from the project.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PostOptions {
    pub date: String,
    pub time: String,
    pub notes: String,
}

#[derive(Clone, Debug)]
struct Cached {
    result: Arc<ToolpathResult>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub struct Session {
    pub project: Project,
    /// Directory of the project file; geometry paths are relative to it.
    pub dir: PathBuf,
    pub store: ModelStore,
    pub tools: ToolStore,
    /// Load findings: project warnings, tool libraries, geometry.
    pub warnings: Vec<Diagnostic>,
    cache: BTreeMap<String, Cached>,
    next_job: u64,
}

/// Tool library files beside the project: `tools/*.toml`, by name.
fn project_tool_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir.join("tools"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    v.sort();
    v
}

impl Session {
    pub fn new(project: Project, dir: PathBuf, store: ModelStore, tools: ToolStore) -> Session {
        Session {
            project,
            dir,
            store,
            tools,
            warnings: Vec::new(),
            cache: BTreeMap::new(),
            next_job: 1,
        }
    }

    /// Opens a project file. Tool libraries come from `tools/*.toml` beside
    /// the project, then from `tool_files` (later ids replace earlier ones).
    pub fn open(path: &Path, tool_files: &[PathBuf]) -> Result<Session, Vec<Diagnostic>> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            vec![Diagnostic::error(
                "model.project.load",
                format!("cannot read {}: {e}", path.display()),
            )]
        })?;
        let loaded = read(&text).map_err(|e| {
            vec![Diagnostic::error(
                "model.project.load",
                format!("{}: {e}", path.display()),
            )]
        })?;
        let dir = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let mut warnings: Vec<Diagnostic> = loaded
            .warnings
            .into_iter()
            .map(|w| Diagnostic::warning("model.project.warning", w))
            .collect();
        let mut tools = ToolStore::new();
        for f in project_tool_files(&dir).iter().chain(tool_files) {
            match std::fs::read_to_string(f)
                .map_err(|e| e.to_string())
                .and_then(|t| ToolLibrary::parse(&t))
            {
                Ok((lib, w)) => {
                    warnings.extend(w.into_iter().map(|w| {
                        Diagnostic::warning("model.tools.warning", format!("{}: {w}", f.display()))
                    }));
                    tools.insert(lib);
                }
                Err(e) => warnings.push(Diagnostic::error(
                    "model.tools.load",
                    format!("{}: {e}", f.display()),
                )),
            }
        }
        let mut store = ModelStore::new();
        for g in &loaded.project.geometry {
            warnings.extend(store.load(g, &dir));
        }
        let mut s = Session::new(loaded.project, dir, store, tools);
        s.warnings = warnings;
        Ok(s)
    }

    pub fn graph(&self) -> Graph {
        Graph::build(&self.project, &self.store, &self.tools)
    }

    /// The cached result of a toolpath, if its inputs have not changed.
    pub fn result(&self, toolpath: &str) -> Option<Arc<ToolpathResult>> {
        let h = self.graph();
        let hash = h.hash(&NodeId::Result(toolpath.to_owned()))?;
        self.cache.get(hash).map(|c| c.result.clone())
    }

    pub fn is_stale(&self, toolpath: &str) -> bool {
        self.result(toolpath).is_none()
    }

    fn setup_bodies(&self, s: &Setup) -> Result<Vec<SetupBody>, Diagnostic> {
        match &s.bodies {
            None => Ok(self.store.bodies().cloned().collect()),
            Some(ids) => ids
                .iter()
                .map(|id| {
                    self.store
                        .bodies()
                        .find(|b| b.id == *id)
                        .cloned()
                        .ok_or_else(|| {
                            Diagnostic::error(
                                "setup.geometry.missing",
                                format!("Setup `{}`: body `{id}` is not in the model", s.id),
                            )
                        })
                })
                .collect(),
        }
    }

    /// Calculates the targets, one job per toolpath, in project order. Up
    /// to date results come from the cache.
    pub fn calculate(&mut self, targets: &Targets) -> Vec<JobReport> {
        self.calculate_with(targets, None)
    }

    fn calculate_with(&mut self, targets: &Targets, post: Option<PostCaps>) -> Vec<JobReport> {
        let graph = self.graph();
        let mut reports = Vec::new();
        let project = self.project.clone();
        for s in &project.setups {
            let wanted: Vec<_> = s
                .toolpaths
                .iter()
                .filter(|t| match targets {
                    Targets::All => true,
                    Targets::Toolpaths(ids) => ids.contains(&t.id),
                })
                .collect();
            if wanted.is_empty() {
                continue;
            }
            let mut frame: Option<Result<SetupFrame, Diagnostic>> = None;
            for t in wanted {
                let job = self.next_job;
                self.next_job += 1;
                let report = |state, diagnostics, stats| JobReport {
                    job,
                    toolpath: t.id.clone(),
                    state,
                    diagnostics,
                    stats,
                };
                if !t.is_enabled() {
                    reports.push(report(JobState::Skipped, Vec::new(), None));
                    continue;
                }
                let hash = graph
                    .hash(&NodeId::Result(t.id.clone()))
                    .unwrap_or_default()
                    .to_owned();
                if let Some(c) = self.cache.get(&hash) {
                    let st = *c.result.stats();
                    reports.push(report(
                        JobState::Done { cached: true },
                        c.diagnostics.clone(),
                        Some(st),
                    ));
                    continue;
                }
                let Some(tool) = self.tools.get(&t.tool) else {
                    let d = validate(t, &ValidateCx::new(None));
                    reports.push(report(JobState::Failed, d, None));
                    continue;
                };
                let fr = frame.get_or_insert_with(|| {
                    self.setup_bodies(s).and_then(|b| SetupFrame::new(s, b))
                });
                let fr = match fr {
                    Ok(f) => f,
                    Err(d) => {
                        reports.push(report(JobState::Failed, vec![d.clone().on(&t.id)], None));
                        continue;
                    }
                };
                let input = StrategyInput {
                    toolpath: t,
                    geometry: &fr.geometry,
                    tool,
                    material: s.material.as_deref(),
                    setup_kind: s.kind,
                    env: fr.height_env(),
                    stock: Some(fr.stock),
                    to_work: fr.to_work,
                    machine: None,
                    post,
                };
                match fluut_cam::calculate(&input) {
                    Ok(g) => {
                        let st = *g.result.stats();
                        self.cache.insert(
                            hash,
                            Cached {
                                result: Arc::new(g.result),
                                diagnostics: g.diagnostics.clone(),
                            },
                        );
                        reports.push(report(
                            JobState::Done { cached: false },
                            g.diagnostics,
                            Some(st),
                        ));
                    }
                    Err(e) => reports.push(report(JobState::Failed, e.diagnostics(), None)),
                }
            }
        }
        reports
    }

    /// Calculates what is stale, then posts every enabled toolpath with the
    /// post `post_id` from `posts`. Fails with the error diagnostics when a
    /// toolpath cannot be calculated or posted.
    pub fn post(
        &mut self,
        post_id: &str,
        posts: &dyn DefStore,
        opts: &PostOptions,
    ) -> Result<(PostOutput, Vec<JobReport>), Vec<Diagnostic>> {
        let err = |code: &str, m: String| vec![Diagnostic::error(code, m)];
        let r = DefRef::parse(post_id).map_err(|e| err("post.load", format!("{post_id}: {e}")))?;
        let resolved = resolve_post(posts, &r, None)
            .map_err(|e| err("post.load", format!("{post_id}: {e}")))?;
        let loaded =
            Post::load(&resolved).map_err(|e| err("post.load", format!("{post_id}: {e}")))?;
        let post_warnings = loaded.warnings.clone();
        let cp = CompiledPost::compile(loaded).map_err(|es| {
            es.into_iter()
                .map(|e| Diagnostic::error("post.compile", format!("{post_id}: {e}")))
                .collect::<Vec<_>>()
        })?;
        let has = |b: &str| cp.blocks.get(b).is_some_and(|l| !l.is_empty());
        let caps = PostCaps {
            cutter_comp: has("cutter_comp_left")
                && has("cutter_comp_right")
                && has("cutter_comp_off"),
            coolant: has("coolant_on"),
            helix: cp.post.helix.enabled,
            cycles: if cp.post.cycles.enabled && has("cycle_start") {
                CycleSet::of(
                    &cp.post
                        .cycles
                        .supported
                        .iter()
                        .map(|c| cycle_name(*c))
                        .collect::<Vec<_>>(),
                )
            } else {
                CycleSet::default()
            },
        };
        let mut reports = self.calculate_with(&Targets::All, Some(caps));
        // Post-dependent rules also for toolpaths taken from the cache.
        let project = self.project.clone();
        for s in &project.setups {
            for t in s.toolpaths.iter().filter(|t| t.is_enabled()) {
                let Some(rep) = reports.iter_mut().find(|r| r.toolpath == t.id) else {
                    continue;
                };
                let cx = ValidateCx {
                    material: s.material.as_deref(),
                    post: Some(&caps),
                    ..ValidateCx::new(self.tools.get(&t.tool))
                };
                for d in validate(t, &cx) {
                    let post_rule = POST_RULES.contains(&d.code.as_str());
                    if post_rule && !rep.diagnostics.contains(&d) {
                        rep.diagnostics.push(d);
                    }
                }
            }
        }
        let errors: Vec<Diagnostic> = reports
            .iter()
            .flat_map(|r| r.diagnostics.iter())
            .filter(|d| d.severity == Severity::Error)
            .cloned()
            .collect();
        if !errors.is_empty() {
            return Err(errors);
        }
        let job = self.post_job(&resolved, post_id, opts);
        let mut out = fluut_post::post(&job, &cp).map_err(|e| err("post.emit", e.to_string()))?;
        out.warnings.splice(0..0, post_warnings);
        Ok((out, reports))
    }

    fn post_job(
        &self,
        resolved: &fluut_model::Resolved,
        post_id: &str,
        opts: &PostOptions,
    ) -> PostJob {
        let mut setups = Vec::new();
        for s in &self.project.setups {
            let mut toolpaths = Vec::new();
            for t in s.toolpaths.iter().filter(|t| t.is_enabled()) {
                let (Some(result), Some(tool)) = (self.result(&t.id), self.tools.get(&t.tool))
                else {
                    continue;
                };
                let p = resolve(&t.params, tool, s.material.as_deref());
                let f = p.tool.feed;
                let safe_z = result
                    .segments()
                    .find_map(|x| x.start())
                    .map_or(0.0, |a| a.xyz.z());
                toolpaths.push(ToolpathJob {
                    name: t.name.clone(),
                    kind: t.kind().as_str().to_owned(),
                    tool: ToolInfo {
                        number: tool.number,
                        name: if tool.name.is_empty() {
                            tool.id.clone()
                        } else {
                            tool.name.clone()
                        },
                        diameter: tool.diameter,
                        corner_radius: tool.corner_radius(),
                    },
                    spindle_rpm: p.tool.spindle_rpm.unwrap_or(0.0),
                    coolant: match p.tool.coolant {
                        Coolant::Off => fluut_post::Coolant::Off,
                        Coolant::Flood => fluut_post::Coolant::Flood,
                        Coolant::Mist => fluut_post::Coolant::Mist,
                    },
                    feeds: Feeds {
                        cut: f.cut.unwrap_or(0.0),
                        plunge: f.plunge.unwrap_or(0.0),
                        ramp: f.ramp.unwrap_or(0.0),
                        lead_in: f.lead_in.unwrap_or(0.0),
                        lead_out: f.lead_out.unwrap_or(0.0),
                        transition: f.transition.unwrap_or(0.0),
                    },
                    safe_z,
                    result,
                });
            }
            setups.push(SetupJob {
                name: s.name.clone(),
                kind: fluut_post::SetupKind::ThreeAxis,
                work_offset: s.work_offset().to_owned(),
                index_a: None,
                index_b: None,
                toolpaths,
            });
        }
        PostJob {
            project_name: self.project.name.clone(),
            notes: opts.notes.clone(),
            date: opts.date.clone(),
            time: opts.time.clone(),
            provenance: Provenance::of(resolved, post_id),
            machine: MachineInfo {
                name: self
                    .project
                    .setups
                    .iter()
                    .find_map(|s| s.machine.clone())
                    .unwrap_or_default(),
                tool_changer: "atc".into(),
                has_rotary: false,
            },
            setups,
        }
    }
}

/// Validation codes that depend on the post. They are checked again at
/// post time for toolpaths taken from the cache.
const POST_RULES: &[&str] = &[
    "param.contour_2d.compensation.post",
    "param.tool_params.coolant.unsupported",
    "param.bore.helix.post",
];

/// The spec 05 §5.5 name of a post cycle.
fn cycle_name(c: fluut_post::post::CycleName) -> &'static str {
    use fluut_post::post::CycleName as C;
    match c {
        C::Drill => "drill",
        C::Spot => "spot",
        C::Peck => "peck",
        C::ChipBreak => "chip_break",
        C::Tap => "tap",
        C::Ream => "ream",
        C::Bore => "bore",
    }
}
