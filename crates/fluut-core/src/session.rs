// SPDX-License-Identifier: MIT OR Apache-2.0

//! The session (D01 §5): one project, its definitions and geometry, the
//! result cache, and calculation and post jobs.

use crate::cache::DiskCache;
use crate::frame::SetupFrame;
use crate::graph::{Graph, NodeId};
use crate::reimport::{ReimportReport, Unmatched, belongs, match_bodies, references};
use crate::sim::{DropPart, SimResult, SimSettings, assembly, diagnostic, feeds, kinematics};
use crate::store::ModelStore;
use fluut_cam::geometry::SetupBody;
use fluut_cam::stock::{LayeredStock, StockFrame, Sweep};
use fluut_cam::{RotaryAxisInfo, RotaryInfo, StrategyInput};
use fluut_model::document::{Document, Outcome};
use fluut_model::edit::{Edit, EditError};
use fluut_model::journal::{Journal, file_hash, journal_path, recover};
use fluut_model::library::{DefKind, LayerKind, Library};
use fluut_model::machine::{BUILTIN_MACHINES, Machine};
use fluut_model::project::{BodyEntry, Project, SetupKind, read};
use fluut_model::resolve::{Coolant, DrillCycle, resolve};
use fluut_model::setup::{ResolvedSetup, resolve_all};
use fluut_model::tools::ToolStore;
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

/// Where definitions come from besides the built-in layer.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OpenOptions {
    /// Extra tool library files, loaded after the project's `tools/`.
    pub tool_files: Vec<PathBuf>,
    /// The user definition folder (`posts/`, `machines/`, `templates/`,
    /// `tools/`); none when `None`.
    pub user_dir: Option<PathBuf>,
    /// Read and write results in `<project>.fluut-cache/` (D06 §7.5).
    pub cache: bool,
    /// Record edits in `<project>.fluut-autosave` (D06 §7.3).
    pub journal: bool,
    /// Replay a journal left for this project file by a crash.
    pub recover: bool,
}

/// The built-in layer: posts and machines embedded in the binary.
pub fn builtin_library() -> Library {
    let mut l = Library::new();
    for (id, text) in fluut_post::builtin::BUILTIN_POSTS {
        l.add(DefKind::Post, LayerKind::BuiltIn, id, text)
            .unwrap_or_else(|e| panic!("built-in post {id}: {e}"));
    }
    for (id, text) in BUILTIN_MACHINES {
        l.add(DefKind::Machine, LayerKind::BuiltIn, id, text)
            .unwrap_or_else(|e| panic!("built-in machine {id}: {e}"));
    }
    l
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
    /// Posts, machines, templates and tool libraries (D06 §4.1).
    pub library: Library,
    /// The library's tools, all layers merged.
    pub tools: ToolStore,
    /// Load findings: project warnings, tool libraries, geometry.
    pub warnings: Vec<Diagnostic>,
    /// The project file, when the session has one.
    pub path: Option<PathBuf>,
    cache: BTreeMap<String, Cached>,
    disk: Option<DiskCache>,
    /// Undo history and journal; kept in step with `project` by `apply`.
    doc: Document,
    /// Results calculated (not taken from a cache) in this session.
    pub calculated: usize,
    next_job: u64,
    /// The last simulation of each Setup, with the key it was made for.
    sims: BTreeMap<String, (String, Arc<SimResult>)>,
}

impl Session {
    pub fn new(project: Project, dir: PathBuf, store: ModelStore, library: Library) -> Session {
        Session {
            doc: Document::new(project.clone()),
            project,
            dir,
            store,
            tools: library.tools(),
            library,
            path: None,
            disk: None,
            calculated: 0,
            warnings: Vec::new(),
            cache: BTreeMap::new(),
            next_job: 1,
            sims: BTreeMap::new(),
        }
    }

    /// Opens a project file. Tool libraries come from `tools/*.toml` beside
    /// the project, then from `tool_files` (later ids replace earlier ones).
    pub fn open(path: &Path, tool_files: &[PathBuf]) -> Result<Session, Vec<Diagnostic>> {
        Session::open_with(
            path,
            &OpenOptions {
                tool_files: tool_files.to_vec(),
                ..OpenOptions::default()
            },
        )
    }

    /// Opens a project file with the definition layers of `opts`: built-in,
    /// then the user folder, then the folders beside the project.
    pub fn open_with(path: &Path, opts: &OpenOptions) -> Result<Session, Vec<Diagnostic>> {
        let bytes = std::fs::read(path).map_err(|e| {
            vec![Diagnostic::error(
                "model.project.load",
                format!("cannot read {}: {e}", path.display()),
            )]
        })?;
        let loaded = read(&String::from_utf8_lossy(&bytes)).map_err(|e| {
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
        let mut library = builtin_library();
        if let Some(u) = &opts.user_dir {
            warnings.extend(library.load_dir(u, LayerKind::User));
        }
        warnings.extend(library.load_dir(&dir, LayerKind::Project));
        for f in &opts.tool_files {
            warnings.extend(library.load_tools_file(f, LayerKind::Project));
        }
        let mut store = ModelStore::new();
        for g in &loaded.project.geometry {
            warnings.extend(store.load(g, &dir));
        }
        let mut s = Session::new(loaded.project, dir, store, library);
        s.path = Some(path.to_owned());
        if opts.cache {
            s.disk = Some(DiskCache::for_project(path));
        }
        let hash = file_hash(&bytes);
        // Read the old journal before a new one replaces it.
        let records = if opts.recover {
            recover(&journal_path(path), &hash).ok().flatten()
        } else {
            None
        };
        if opts.journal {
            let j = Journal::create(&journal_path(path), &hash).map_err(|e| {
                vec![Diagnostic::error(
                    "model.journal.create",
                    format!("{}: {e}", journal_path(path).display()),
                )]
            })?;
            let doc = std::mem::replace(&mut s.doc, Document::new(Project::new("")));
            s.doc = doc.with_journal(j);
        }
        if let Some(records) = records {
            // Replayed through the new journal, so a second crash keeps them.
            let n = s.doc.replay(&records);
            s.project = s.doc.project().clone();
            warnings.push(Diagnostic::warning(
                "model.journal.recovered",
                format!("recovered {n} of {} unsaved changes", records.len()),
            ));
        }
        for (m, e) in s.machines() {
            if let Err(e) = e {
                warnings.push(Diagnostic::warning(
                    "setup.machine.missing",
                    format!("machine `{m}`: {e}"),
                ));
            }
        }
        s.warnings = warnings;
        Ok(s)
    }

    /// How many unsaved changes a crash left for the project file at
    /// `path`, if any (D06 §7.3: offered on open).
    pub fn pending_recovery(path: &Path) -> Option<usize> {
        let bytes = std::fs::read(path).ok()?;
        recover(&journal_path(path), &file_hash(&bytes))
            .ok()
            .flatten()
            .map(|r| r.len())
    }

    /// Applies an edit: one undo step, journalled when the journal is on
    /// (D01 §5: every mutation goes through here). Results whose inputs
    /// changed become stale through their hashes.
    pub fn apply(&mut self, edit: Edit) -> Result<Outcome, EditError> {
        self.sync_doc();
        let out = self.doc.apply(edit)?;
        self.project = self.doc.project().clone();
        Ok(out)
    }

    /// Opens an edit group: the edits until [`Session::end_group`] are one
    /// undo step (a script run, D12 §3.4).
    pub fn begin_group(&mut self, label: &str) {
        self.sync_doc();
        self.doc.begin_group(label);
    }

    /// Closes the group; its undo step, if it changed anything.
    pub fn end_group(&mut self) -> Option<Outcome> {
        self.sync_doc();
        self.doc.end_group()
    }

    pub fn undo(&mut self) -> Option<Outcome> {
        self.sync_doc();
        let out = self.doc.undo()?;
        self.project = self.doc.project().clone();
        Some(out)
    }

    pub fn redo(&mut self) -> Option<Outcome> {
        self.sync_doc();
        let out = self.doc.redo()?;
        self.project = self.doc.project().clone();
        Some(out)
    }

    /// A project changed directly (not through `apply`) starts a new
    /// history from its current state.
    fn sync_doc(&mut self) {
        if *self.doc.project() != self.project {
            let mut doc = Document::new(self.project.clone());
            if let Some(j) = self.doc_journal_take() {
                doc = doc.with_journal(j);
            }
            self.doc = doc;
        }
    }

    fn doc_journal_take(&mut self) -> Option<Journal> {
        let doc = std::mem::replace(&mut self.doc, Document::new(Project::new("")));
        doc.into_journal()
    }

    /// Saves the project to its file (atomic, with `.bak`; D06 §7.2),
    /// starts the journal afresh and removes cache files no longer needed
    /// (D06 §7.5).
    pub fn save(&mut self) -> Result<(), Diagnostic> {
        let path = self.path.clone().ok_or_else(|| {
            Diagnostic::error("model.project.save", "the session has no project file")
        })?;
        let saved = fluut_model::persist::save(&path, &self.project).map_err(|e| {
            Diagnostic::error("model.project.save", format!("{}: {e}", path.display()))
        })?;
        if let Some(j) = self.doc.journal_mut() {
            j.reset(&file_hash(&saved.bytes)).map_err(|e| {
                Diagnostic::error(
                    "model.journal.reset",
                    format!("{}: {e}", j.path().display()),
                )
            })?;
        }
        if let Some(d) = &self.disk {
            let g = self.graph();
            let referenced = g
                .nodes
                .iter()
                .filter(|(k, _)| matches!(k, NodeId::Result(_)))
                .map(|(_, n)| n.hash.clone())
                .collect();
            d.gc(&referenced, std::time::SystemTime::now())
                .map_err(|e| Diagnostic::warning("model.cache.gc", e.to_string()))?;
        }
        Ok(())
    }

    /// Re-reads geometry source `id` from its file and matches its
    /// entities to the ones loaded before (spec 03 §3.5). Matched
    /// selections are rewritten and the source's hash and body list
    /// updated, as one undo step. Unmatched selections are kept and
    /// reported.
    pub fn reimport(&mut self, id: &str) -> Result<ReimportReport, Vec<Diagnostic>> {
        let g = self.project.source(id).cloned().ok_or_else(|| {
            vec![Diagnostic::error(
                "model.geometry.missing",
                format!("geometry `{id}` is not in the project"),
            )]
        })?;
        let old = self
            .store
            .source(id)
            .map(|s| s.bodies.clone())
            .unwrap_or_default();
        let mut fresh = ModelStore::new();
        let mut diagnostics: Vec<Diagnostic> = fresh
            .load(&g, &self.dir)
            .into_iter()
            .filter(|d| {
                d.code != "model.geometry.changed" && d.code != "model.geometry.body_missing"
            })
            .collect();
        let Some(new) = fresh.source(id).cloned() else {
            return Err(diagnostics);
        };
        let matching = match_bodies(&old, &new.bodies);
        let unmatched: Vec<Unmatched> = references(&self.project)
            .into_iter()
            .filter(|r| belongs(&r.id, &old) && matching.new_id(&r.id).is_none())
            .collect();
        for u in &unmatched {
            let d = Diagnostic::warning(
                "model.reimport.unmatched",
                format!(
                    "`{}` ({} of `{}`) is not in the new {}; the selection is kept",
                    u.id, u.field, u.owner, g.path
                ),
            )
            .at(&u.field);
            let is_tp = self.project.toolpath(&u.owner).is_some();
            diagnostics.push(if is_tp { d.on(&u.owner) } else { d });
        }
        let mut ng = g.clone();
        ng.hash = Some(new.hash.clone());
        ng.bodies = new
            .bodies
            .iter()
            .map(|b| {
                let name = match &b.geom {
                    fluut_cam::geometry::BodyGeom::Brep(x) => x.name.clone(),
                    fluut_cam::geometry::BodyGeom::Wire(w) => w.name.clone(),
                };
                let old_name = g
                    .bodies
                    .iter()
                    .find(|e| matching.new_id(&e.id) == Some(&b.id))
                    .and_then(|e| e.name.clone());
                BodyEntry {
                    id: b.id.clone(),
                    name: if name.is_empty() {
                        old_name
                    } else {
                        Some(name)
                    },
                }
            })
            .collect();
        let edit = Edit::Group {
            label: format!("Re-import {}", g.path),
            edits: vec![
                Edit::set_geometry(&ng),
                Edit::RemapIds {
                    map: matching.renames(),
                },
            ],
        };
        self.apply(edit).map_err(|e| {
            vec![Diagnostic::error(
                "model.reimport.apply",
                format!("geometry `{id}`: {e}"),
            )]
        })?;
        self.store.insert(id, new);
        Ok(ReimportReport {
            source: id.to_owned(),
            matching,
            unmatched,
            diagnostics,
        })
    }

    /// Every machine the Setups name, resolved, by reference.
    pub fn machines(&self) -> BTreeMap<String, Result<Machine, String>> {
        resolve_all(&self.project)
            .into_iter()
            .filter_map(|r| r.ok()?.machine)
            .map(|m| {
                let r = DefRef::parse(&m)
                    .map_err(|e| e.to_string())
                    .and_then(|r| self.library.machine(&r).map_err(|e| e.to_string()));
                (m, r)
            })
            .collect()
    }

    fn first_machine(&self) -> Option<Machine> {
        let r = resolve_all(&self.project)
            .into_iter()
            .find_map(|r| r.ok()?.machine)?;
        self.machine(Some(&r))
    }

    fn machine(&self, r: Option<&str>) -> Option<Machine> {
        let r = DefRef::parse(r?).ok()?;
        self.library.machine(&r).ok()
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

    /// Estimated machining time of a toolpath's current result, s (D11 §6),
    /// with its feeds and its Setup's machine.
    pub fn time_estimate(&self, toolpath: &str) -> Option<f64> {
        let result = self.result(toolpath)?;
        let (s, t) = self.project.toolpath(toolpath)?;
        let rs = fluut_model::setup::resolve_setup(&self.project, &s.id).ok()?;
        let tool = self.tools.get(t.tool_ref())?;
        let f = feeds(&t.params, tool, rs.material.as_deref());
        let k = kinematics(self.machine(rs.machine.as_deref()).as_ref());
        Some(fluut_sim::time::estimate(result.segments(), &f, &k))
    }

    fn sim_key(&self, setup: &str, settings: &SimSettings) -> Option<String> {
        let g = self.graph();
        let h = g.hash(&NodeId::Simulation(setup.to_owned()))?;
        let rs = fluut_model::setup::resolve_setup(&self.project, setup).ok()?;
        let machine = self.machine(rs.machine.as_deref());
        Some(format!(
            "{h}\n{}machine = {:?}\n",
            settings.text(),
            machine.map(|m| m.fields)
        ))
    }

    /// The Setup's simulation, if its inputs have not changed since.
    pub fn simulation(&self, setup: &str, settings: &SimSettings) -> Option<Arc<SimResult>> {
        let (key, r) = self.sims.get(setup)?;
        (self.sim_key(setup, settings).as_ref() == Some(key)).then(|| r.clone())
    }

    /// Simulates a Setup's enabled toolpaths in order (D11). Every one
    /// needs a current result: calculate first. Findings come back as
    /// diagnostics on their toolpaths; the result is kept until an input
    /// changes.
    pub fn simulate(
        &mut self,
        setup: &str,
        settings: &SimSettings,
        monitor: &dyn fluut_sim::Monitor,
    ) -> Result<Arc<SimResult>, Vec<Diagnostic>> {
        if let Some(r) = self.simulation(setup, settings) {
            return Ok(r);
        }
        let err = |code: &str, m: String| vec![Diagnostic::error(code, m)];
        let s = self
            .project
            .setup(setup)
            .ok_or_else(|| {
                err(
                    "sim.setup.missing",
                    format!("Setup `{setup}` is not in the project"),
                )
            })?
            .clone();
        let rs = fluut_model::setup::resolve_setup(&self.project, setup).map_err(|d| vec![d])?;
        if rs.kind == SetupKind::Rotary {
            return Err(err(
                "sim.setup.rotary",
                format!("Setup `{setup}` is rotary; rotary Setups are not simulated yet"),
            ));
        }
        let fr = self
            .setup_bodies(&rs)
            .and_then(|b| SetupFrame::new(&rs, b))
            .map_err(|d| vec![d])?;
        let mut missing = Vec::new();
        let mut inputs = Vec::new();
        // `manual_nc` has no tool and no motion (D15b §7).
        for t in s
            .toolpaths
            .iter()
            .filter(|t| t.is_enabled() && t.kind() != fluut_model::params::ToolpathTypeId::ManualNc)
        {
            let (Some(r), Some(tool)) = (self.result(&t.id), self.tools.get(t.tool_ref())) else {
                missing.push(t.id.clone());
                continue;
            };
            let Some(a) = assembly(tool) else {
                return Err(err(
                    "sim.tool.unsupported",
                    format!("toolpath `{}`: the tool has no usable size", t.id),
                ));
            };
            inputs.push((
                t.id.clone(),
                r,
                a,
                feeds(&t.params, tool, rs.material.as_deref()),
            ));
        }
        if !missing.is_empty() {
            return Err(err(
                "sim.result.missing",
                format!("calculate first: {}", missing.join(", ")),
            ));
        }
        let stock = fluut_sim::Stock::Box {
            min: fr.stock.min,
            max: fr.stock.max,
        };
        // The gouge tolerance is the toolpaths' machining tolerance.
        let tol = inputs
            .iter()
            .flat_map(|(_, r, ..)| r.segments().filter_map(|x| x.motion()))
            .map(|m| f64::from(m.tol))
            .fold(0.0, f64::max);
        let tol = if tol > 0.0 { tol } else { 0.01 };
        let programs: Vec<fluut_sim::Program> = inputs
            .iter()
            .map(|(id, r, a, f)| fluut_sim::Program {
                id: id.clone(),
                result: r,
                tool: *a,
                feeds: *f,
            })
            .collect();
        let run = fluut_sim::Settings {
            res: fluut_sim::resolution(&stock, settings.error, settings.budget),
            rotary: false,
            kinematics: kinematics(self.machine(rs.machine.as_deref()).as_ref()),
            tol,
        };
        let part = DropPart::new(&fr.geometry);
        let sim = fluut_sim::Simulation::run(stock, &programs, &run, Some(&part), monitor)
            .map_err(|_| err("sim.cancelled", "the simulation was cancelled".into()))?;
        let mut diagnostics: Vec<Diagnostic> =
            sim.findings.iter().map(|f| diagnostic(&sim, f)).collect();
        if sim.dropped > 0 {
            diagnostics.push(Diagnostic::warning(
                "sim.findings.dropped",
                format!("{} more findings not listed", sim.dropped),
            ));
        }
        let r = Arc::new(SimResult {
            setup: setup.to_owned(),
            sim,
            diagnostics,
            tol,
        });
        if let Some(key) = self.sim_key(setup, settings) {
            self.sims.insert(setup.to_owned(), (key, r.clone()));
        }
        Ok(r)
    }

    fn setup_bodies(&self, s: &ResolvedSetup) -> Result<Vec<SetupBody>, Diagnostic> {
        match &s.geometry.bodies {
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
        let resolved = resolve_all(&project);
        let plan = stock_plan(&project, &resolved, targets);
        // The final in-process stock of each Setup a later one carries.
        let mut finals: BTreeMap<String, LayeredStock> = BTreeMap::new();
        for (si, (s, rs)) in project.setups.iter().zip(&resolved).enumerate() {
            let SetupPlan {
                wanted,
                feed_until,
                source,
            } = &plan[si];
            if !wanted.iter().any(|w| *w) {
                continue;
            }
            let mut frame: Option<Result<SetupFrame, Diagnostic>> = None;
            let mut stock: Option<LayeredStock> = None;
            if feed_until.is_some()
                && let Ok(r) = rs
            {
                let fr = frame.get_or_insert_with(|| {
                    self.setup_bodies(r).and_then(|b| SetupFrame::new(r, b))
                });
                if let Ok(f) = fr {
                    stock = Some(carried_stock(r, &resolved, &project, &finals, f));
                }
            }
            for (ti, t) in s.toolpaths.iter().enumerate() {
                if !wanted[ti] {
                    continue;
                }
                // Update the stock from this toolpath's result while a
                // later toolpath, or a child Setup, still reads it.
                let feeds_stock = feed_until.is_some_and(|l| ti < l) && t.kind().updates_stock();
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
                if !self.cache.contains_key(&hash)
                    && let Some((result, diagnostics)) =
                        self.disk.as_ref().and_then(|d| d.get(&hash))
                {
                    self.cache.insert(
                        hash.clone(),
                        Cached {
                            result: Arc::new(result),
                            diagnostics,
                        },
                    );
                }
                if let Some(c) = self.cache.get(&hash) {
                    if feeds_stock
                        && let (Some(st), Some(tool), Ok(rs)) =
                            (stock.as_mut(), self.tools.get(t.tool_ref()), &rs)
                    {
                        update_stock(st, &c.result, t, tool, rs);
                    }
                    let st = *c.result.stats();
                    reports.push(report(
                        JobState::Done { cached: true },
                        c.diagnostics.clone(),
                        Some(st),
                    ));
                    continue;
                }
                let rs = match &rs {
                    Ok(r) => r,
                    Err(d) => {
                        reports.push(report(JobState::Failed, vec![d.clone().on(&t.id)], None));
                        continue;
                    }
                };
                // `manual_nc` has no tool and no geometry (D15b §7).
                if t.kind() == fluut_model::params::ToolpathTypeId::ManualNc {
                    match fluut_cam::strategy::manual_nc(t, rs.kind) {
                        Ok(g) => {
                            self.calculated += 1;
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
                    continue;
                }
                let Some(tool) = self.tools.get(t.tool_ref()) else {
                    let d = validate(t, &ValidateCx::new(None));
                    reports.push(report(JobState::Failed, d, None));
                    continue;
                };
                let fr = frame.get_or_insert_with(|| {
                    self.setup_bodies(rs).and_then(|b| SetupFrame::new(rs, b))
                });
                let fr = match fr {
                    Ok(f) => f,
                    Err(d) => {
                        reports.push(report(JobState::Failed, vec![d.clone().on(&t.id)], None));
                        continue;
                    }
                };
                let in_process = if t.params.reads_stock() {
                    stock.as_ref()
                } else {
                    None
                };
                let input = StrategyInput {
                    toolpath: t,
                    geometry: &fr.geometry,
                    tool,
                    material: rs.material.as_deref(),
                    setup_kind: rs.kind,
                    env: fr.height_env(),
                    stock: Some(fr.stock),
                    to_work: fr.to_work,
                    machine: self.machine(rs.machine.as_deref()).and_then(|m| m.limits()),
                    post,
                    in_process,
                    rotary: fr.rotary_info().map(|i| RotaryInfo {
                        machine_axis: rs
                            .machine
                            .as_deref()
                            .map(|m| self.machine(Some(m)).and_then(|m| rotary_axis(&m))),
                        ..i
                    }),
                };
                match fluut_cam::calculate(&input) {
                    Ok(g) => {
                        self.calculated += 1;
                        if feeds_stock && let Some(st) = stock.as_mut() {
                            update_stock(st, &g.result, t, tool, rs);
                        }
                        if let Some(d) = &self.disk {
                            // A cache that cannot be written only costs time.
                            let _ = d.put(&hash, &g.result, &g.diagnostics);
                        }
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
            if *source && let Some(st) = stock {
                finals.insert(s.id.clone(), st);
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
        self.post_setups(post_id, posts, opts, None)
    }

    /// As [`Session::post`], for the Setups with the ids in `setups` only
    /// (`None`: every Setup).
    pub fn post_setups(
        &mut self,
        post_id: &str,
        posts: &dyn DefStore,
        opts: &PostOptions,
        setups: Option<&[String]>,
    ) -> Result<(PostOutput, Vec<JobReport>), Vec<Diagnostic>> {
        if let Some(ids) = setups
            && let Some(id) = ids.iter().find(|id| self.project.setup(id).is_none())
        {
            return Err(vec![Diagnostic::error(
                "post.setup.missing",
                format!("Setup `{id}` is not in the project"),
            )]);
        }
        let chosen = |id: &str| setups.is_none_or(|ids| ids.iter().any(|x| x == id));
        let err = |code: &str, m: String| vec![Diagnostic::error(code, m)];
        let r = DefRef::parse(post_id).map_err(|e| err("post.load", format!("{post_id}: {e}")))?;
        // The machine of the first Setup that names one: its post overrides
        // apply (spec 05 §5.2.3) and its name goes into the output.
        let machine = self.first_machine();
        let overrides = machine
            .as_ref()
            .and_then(|m| Some((&m.fields.post_overrides, m.layer()?)))
            .filter(|(t, _)| !t.is_empty());
        let resolved = resolve_post(posts, &r, overrides)
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
        let targets = match setups {
            None => Targets::All,
            Some(_) => Targets::Toolpaths(
                self.project
                    .setups
                    .iter()
                    .filter(|s| chosen(&s.id))
                    .flat_map(|s| s.toolpaths.iter().map(|t| t.id.clone()))
                    .collect(),
            ),
        };
        let mut reports = self.calculate_with(&targets, Some(caps));
        // `manual_nc` needs the post's `manual_nc` block (D15b §7).
        if !has("manual_nc") {
            let chosen_tps = self.project.setups.iter().filter(|s| chosen(&s.id));
            for t in chosen_tps.flat_map(|s| s.toolpaths.iter()) {
                if t.is_enabled()
                    && t.kind() == fluut_model::params::ToolpathTypeId::ManualNc
                    && let Some(rep) = reports.iter_mut().find(|r| r.toolpath == t.id)
                {
                    rep.diagnostics.push(
                        Diagnostic::error(
                            "param.manual_nc.post",
                            format!("post `{post_id}` has no manual_nc block"),
                        )
                        .on(&t.id),
                    );
                }
            }
        }
        // Post-dependent rules also for toolpaths taken from the cache.
        let project = self.project.clone();
        for (s, rs) in project.setups.iter().zip(resolve_all(&project)) {
            if !chosen(&s.id) {
                continue;
            }
            let material = rs.ok().and_then(|r| r.material);
            for t in s.toolpaths.iter().filter(|t| t.is_enabled()) {
                let Some(rep) = reports.iter_mut().find(|r| r.toolpath == t.id) else {
                    continue;
                };
                let cx = ValidateCx {
                    material: material.as_deref(),
                    post: Some(&caps),
                    ..ValidateCx::new(self.tools.get(t.tool_ref()))
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
        let mut job = self.post_job(&resolved, post_id, opts, machine.as_ref());
        if setups.is_some() {
            let names: Vec<&str> = project
                .setups
                .iter()
                .zip(resolve_all(&project))
                .filter(|(_, r)| r.is_ok())
                .map(|(s, _)| s.id.as_str())
                .collect();
            let mut i = 0;
            job.setups.retain(|_| {
                let keep = names.get(i).is_some_and(|id| chosen(id));
                i += 1;
                keep
            });
        }
        let mut out = fluut_post::post(&job, &cp).map_err(|e| err("post.emit", e.to_string()))?;
        out.warnings.splice(0..0, post_warnings);
        Ok((out, reports))
    }

    fn post_job(
        &self,
        resolved: &fluut_model::Resolved,
        post_id: &str,
        opts: &PostOptions,
        machine: Option<&Machine>,
    ) -> PostJob {
        let mut setups = Vec::new();
        let setups_r = resolve_all(&self.project);
        for (s, rs) in self.project.setups.iter().zip(&setups_r) {
            let Ok(rs) = rs else { continue };
            let mut toolpaths = Vec::new();
            for t in s.toolpaths.iter().filter(|t| t.is_enabled()) {
                if t.kind() == fluut_model::params::ToolpathTypeId::ManualNc {
                    // No tool: the post keeps the active one (D15b §9,
                    // decision 16) and emits only the markers.
                    let Some(result) = self.result(&t.id) else {
                        continue;
                    };
                    let prev = toolpaths.last().cloned();
                    toolpaths.push(ToolpathJob {
                        name: t.name.clone(),
                        kind: t.kind().as_str().to_owned(),
                        result,
                        ..prev.unwrap_or_else(|| ToolpathJob {
                            name: String::new(),
                            kind: String::new(),
                            tool: ToolInfo::default(),
                            spindle_rpm: 0.0,
                            coolant: fluut_post::Coolant::Off,
                            feeds: Feeds::default(),
                            safe_z: 0.0,
                            result: Arc::new(ToolpathResult::new(Vec::new()).expect("empty")),
                        })
                    });
                    continue;
                }
                let (Some(result), Some(tool)) = (self.result(&t.id), self.tools.get(t.tool_ref()))
                else {
                    continue;
                };
                let p = resolve(&t.params, tool, rs.material.as_deref());
                let f = p.tool.feed;
                let rpm = p.tool.spindle_rpm.unwrap_or(0.0);
                // A tap feeds at rpm × pitch, set at post time (D15b §3.1).
                let plunge = match p.drill() {
                    Some(d) if d.cycle == DrillCycle::Tap => rpm * d.pitch.unwrap_or(0.0),
                    _ => f.plunge.unwrap_or(0.0),
                };
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
                        plunge,
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
                kind: match rs.kind {
                    SetupKind::ThreeAxis => fluut_post::SetupKind::ThreeAxis,
                    SetupKind::Rotary => fluut_post::SetupKind::Rotary,
                    SetupKind::Indexed => fluut_post::SetupKind::Indexed,
                },
                work_offset: rs.work_offset.clone(),
                index_a: (rs.kind == SetupKind::Indexed).then_some(rs.index_angle),
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
            machine: match machine {
                Some(m) => MachineInfo {
                    name: if m.fields.name.is_empty() {
                        m.id.clone()
                    } else {
                        m.fields.name.clone()
                    },
                    tool_changer: m.fields.tool_changer.kind.as_str().into(),
                    has_rotary: m.has_rotary(),
                },
                None => MachineInfo {
                    name: setups_r
                        .iter()
                        .find_map(|s| s.as_ref().ok().and_then(|s| s.machine.clone()))
                        .unwrap_or_default(),
                    tool_changer: "atc".into(),
                    has_rotary: false,
                },
            },
            setups,
        }
    }
}

/// The machine's A axis (parallel to X), or `None` when it has none.
fn rotary_axis(m: &Machine) -> Option<RotaryAxisInfo> {
    let a = m
        .fields
        .axis
        .rotary
        .iter()
        .find(|a| a.parallel_to.eq_ignore_ascii_case("x"))?;
    Some(RotaryAxisInfo {
        continuous: a.continuous,
        limits_deg: match a.limits_deg.as_slice() {
            [lo, hi] => Some((*lo, *hi)),
            _ => None,
        },
    })
}

/// What a calculation does in one Setup.
struct SetupPlan {
    /// Toolpaths to calculate (or take from the cache).
    wanted: Vec<bool>,
    /// The in-process stock is kept, and updated by the toolpaths before
    /// this index; `None` when nothing reads it.
    feed_until: Option<usize>,
    /// A later `from_previous` Setup starts from this one's final stock.
    source: bool,
}

/// Which toolpaths to calculate for `targets`, and where the in-process
/// stock is needed (D08 §7). A toolpath that reads the stock needs every
/// earlier toolpath of its Setup and the whole of the Setup its stock
/// comes from (`from_previous`, spec 03 §3.9.6). From the last Setup back,
/// so chains of children reach their first ancestor.
fn stock_plan(
    p: &Project,
    resolved: &[Result<ResolvedSetup, Diagnostic>],
    targets: &Targets,
) -> Vec<SetupPlan> {
    let mut plan: Vec<SetupPlan> = p
        .setups
        .iter()
        .map(|s| SetupPlan {
            wanted: s
                .toolpaths
                .iter()
                .map(|t| match targets {
                    Targets::All => true,
                    Targets::Toolpaths(ids) => ids.contains(&t.id),
                })
                .collect(),
            feed_until: None,
            source: false,
        })
        .collect();
    for si in (0..p.setups.len()).rev() {
        let s = &p.setups[si];
        let sp = &mut plan[si];
        if sp.source {
            sp.wanted.iter_mut().for_each(|w| *w = true);
            sp.feed_until = Some(s.toolpaths.len());
        } else {
            sp.feed_until = s
                .toolpaths
                .iter()
                .zip(&sp.wanted)
                .rposition(|(t, w)| *w && t.is_enabled() && t.params.reads_stock());
            if let Some(i) = sp.feed_until {
                sp.wanted[..i].iter_mut().for_each(|w| *w = true);
            }
        }
        if sp.feed_until.is_some()
            && let Ok(r) = &resolved[si]
            && let Some(parent) = carried_from(r)
            && let Some(pi) = p.setup_index(parent)
        {
            plan[pi].source = true;
        }
    }
    plan
}

/// The Setup whose stock `r` starts from, for `stock = from_previous`.
fn carried_from(r: &ResolvedSetup) -> Option<&str> {
    match r.stock {
        Some(fluut_model::project::StockSpec::FromPrevious) => r.based_on.as_deref(),
        _ => None,
    }
}

/// The resolved rotary position of a Setup in degrees: its index angle,
/// 0 for `three_axis` and `rotary` Setups.
fn setup_angle(r: &ResolvedSetup) -> f64 {
    if r.kind == SetupKind::Indexed {
        r.index_angle
    } else {
        0.0
    }
}

/// The in-process stock a Setup starts from: its parent's final stock
/// turned into its frame for `from_previous` (D08 §7, CH-15), else its
/// own stock. Parent and child share the rotary axis and the zero (spec
/// 03 §3.9.4), so the child's frame is the parent's turned about X by the
/// difference of their angles.
fn carried_stock(
    r: &ResolvedSetup,
    resolved: &[Result<ResolvedSetup, Diagnostic>],
    p: &Project,
    finals: &BTreeMap<String, LayeredStock>,
    f: &SetupFrame,
) -> LayeredStock {
    let own = f.initial_stock();
    let Some(pid) = carried_from(r) else {
        return own;
    };
    let (Some(parent), Some(Ok(pr))) = (
        finals.get(pid),
        p.setup_index(pid).and_then(|i| resolved.get(i)),
    ) else {
        return own;
    };
    match (parent.frame, own.frame) {
        (StockFrame::Cylindrical, StockFrame::Cylindrical) => parent.clone(),
        // A rotary child of a planar parent: its own cylinder holds all
        // the material.
        (StockFrame::Planar, StockFrame::Cylindrical) => own,
        _ => parent.reframed(setup_angle(r) - setup_angle(pr)),
    }
}

/// Takes what `result` cut out of `stock` (D08 §7).
fn update_stock(
    stock: &mut LayeredStock,
    result: &ToolpathResult,
    t: &fluut_model::project::Toolpath,
    tool: &fluut_model::tools::Tool,
    rs: &ResolvedSetup,
) {
    let p = resolve(&t.params, tool, rs.material.as_deref());
    let tol = p.passes.machining_tolerance.max(1e-4);
    stock.apply(result, Sweep::of(tool), tol);
}

/// Validation codes that depend on the post. They are checked again at
/// post time for toolpaths taken from the cache.
const POST_RULES: &[&str] = &[
    "param.contour_2d.compensation.post",
    "param.tool_params.coolant.unsupported",
    "param.bore.helix.post",
    "param.drill.tap.post",
    "param.drill.cycle.expanded",
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
