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
use fluut_model::project::{BodyEntry, Project, SetupKind, StockSpec, read};
use fluut_model::resolve::{Coolant, DrillCycle, resolve};
use fluut_model::setup::{ResolvedSetup, resolve_all};
use fluut_model::tools::{BUILTIN_TOOLS, ToolLibrary, ToolStore};
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
    for (id, text) in BUILTIN_TOOLS {
        let (lib, _) =
            ToolLibrary::parse(text).unwrap_or_else(|e| panic!("built-in tools {id}: {e}"));
        l.add_tools(LayerKind::BuiltIn, lib)
            .unwrap_or_else(|e| panic!("built-in tools {id}: {e}"));
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
    /// The project as last saved or opened, for [`Session::is_dirty`].
    pub(crate) saved: Project,
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
            saved: project.clone(),
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

    /// Whether there is an edit to undo.
    pub fn can_undo(&self) -> bool {
        *self.doc.project() == self.project && self.doc.can_undo()
    }

    /// Whether there is an undone edit to redo.
    pub fn can_redo(&self) -> bool {
        *self.doc.project() == self.project && self.doc.can_redo()
    }

    /// The label of the edit that [`Session::undo`] would undo.
    pub fn undo_label(&self) -> Option<String> {
        (*self.doc.project() == self.project)
            .then(|| self.doc.undo_labels().next().map(str::to_owned))
            .flatten()
    }

    pub fn undo(&mut self) -> Option<Outcome> {
        self.sync_doc();
        let out = self.doc.undo()?;
        self.project = self.doc.project().clone();
        self.sync_store();
        Some(out)
    }

    pub fn redo(&mut self) -> Option<Outcome> {
        self.sync_doc();
        let out = self.doc.redo()?;
        self.project = self.doc.project().clone();
        self.sync_store();
        Some(out)
    }

    /// Makes the model store hold the project's sources again after an undo
    /// or redo of an import: sources the project no longer lists are
    /// dropped, new ones are read from their files (problems go to
    /// `warnings`).
    fn sync_store(&mut self) {
        let gone: Vec<String> = self
            .store
            .sources()
            .map(|(id, _)| id.to_owned())
            .filter(|id| self.project.source(id).is_none())
            .collect();
        for id in gone {
            self.store.remove(&id);
        }
        for g in &self.project.geometry {
            if self.store.source(&g.id).is_none() {
                let d = self.store.load(g, &self.dir);
                self.warnings.extend(d);
            }
        }
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
        self.saved = self.project.clone();
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
        let (start, mut notes) = self.carried_sim_stock(&rs, settings, monitor)?;
        let mut missing = Vec::new();
        let mut inputs = Vec::new();
        let mut warned: Vec<String> = Vec::new();
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
            // The holder starts at the overall length: without it the
            // holder is not simulated. Once per tool.
            if tool.holder.is_some()
                && tool.gauge_length().is_none()
                && !warned.iter().any(|w| w == t.tool_ref())
            {
                warned.push(t.tool_ref().to_owned());
                notes.push(length_warning(tool, &[], true).on(&t.id));
            }
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
        let stock = match fr.index.and_then(|i| i.cylinder) {
            Some((radius, (x0, x1))) => fluut_sim::Stock::Cylinder { x0, x1, radius },
            None => fluut_sim::Stock::Box {
                min: fr.stock.min,
                max: fr.stock.max,
            },
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
        if rs.kind == SetupKind::Indexed
            && let Some(d) = self.check_turn(setup, start.as_ref(), &stock, run.res)
        {
            notes.push(match inputs.first() {
                Some((id, ..)) => d.on(id),
                None => d,
            });
        }
        for (id, r, a, _) in &inputs {
            if let Some(d) = fixture_finding(&fr, r, a) {
                notes.push(d.on(id));
            }
        }
        let part = DropPart::new(&fr.geometry);
        let sim =
            fluut_sim::Simulation::run_from(stock, start, &programs, &run, Some(&part), monitor)
                .map_err(|_| err("sim.cancelled", "the simulation was cancelled".into()))?;
        let mut diagnostics: Vec<Diagnostic> =
            sim.findings.iter().map(|f| diagnostic(&sim, f)).collect();
        diagnostics.append(&mut notes);
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

    /// The index turn at the start of `setup` (D11 §5): the tool where the
    /// post leaves it, at the end of the previous Setup's last toolpath
    /// raised to the turn height, against the Setup's start stock (`start`,
    /// else the full `stock` at `res`) turned about X. The error
    /// `sim.index_turn` when material reaches the tool; `None` when the
    /// turn is clear, or when nothing has moved the tool on the axis yet.
    fn check_turn(
        &self,
        setup: &str,
        start: Option<&fluut_sim::TriDexel>,
        stock: &fluut_sim::Stock,
        res: f64,
    ) -> Option<Diagnostic> {
        let si = self.project.setups.iter().position(|s| s.id == setup)?;
        let h = self.turn_heights().0.get(si).copied().flatten()?;
        let resolved = resolve_all(&self.project);
        // The last toolpath of the nearest earlier Setup that has one.
        let (r, t) = self.project.setups[..si]
            .iter()
            .zip(&resolved)
            .rev()
            .find_map(|(s, r)| {
                s.toolpaths
                    .iter()
                    .rev()
                    .find(|t| {
                        t.is_enabled() && t.kind() != fluut_model::params::ToolpathTypeId::ManualNc
                    })
                    .map(|t| (r, t))
            })?;
        // A three_axis Setup's frame does not lie on the axis.
        if r.as_ref().ok()?.kind == SetupKind::ThreeAxis {
            return None;
        }
        let end = *self
            .result(&t.id)?
            .segments()
            .filter_map(|s| s.end())
            .last()?;
        let a = assembly(self.tools.get(t.tool_ref())?)?;
        let p = end.xyz;
        let tip = glam::DVec3::new(p.x(), p.y(), p.z().max(h));
        let full;
        let d = match start {
            Some(d) => d,
            None => {
                full = fluut_sim::TriDexel::new(stock, res);
                &full
            }
        };
        let depth = fluut_sim::index_turn(d, &a, tip)?;
        Some(Diagnostic::error(
            "sim.index_turn",
            format!(
                "the stock turns {depth:.3} mm into the tool, at ({:.3}, {:.3}, {:.3}) \
                 from `{}`, before Setup `{setup}`",
                tip.x, tip.y, tip.z, t.id
            ),
        ))
    }

    /// The simulated stock a `from_previous` Setup starts from: its
    /// parent's final stock, simulated first, turned by the difference of
    /// their index angles when that is a multiple of 90°. Otherwise `None`
    /// (the Setup's full stock) with the warning `sim.stock.full`.
    fn carried_sim_stock(
        &mut self,
        rs: &ResolvedSetup,
        settings: &SimSettings,
        monitor: &dyn fluut_sim::Monitor,
    ) -> Result<(Option<fluut_sim::TriDexel>, Vec<Diagnostic>), Vec<Diagnostic>> {
        let Some(pid) = carried_from(rs).map(str::to_owned) else {
            return Ok((None, Vec::new()));
        };
        let full = |why: &str| {
            Ok((
                None,
                vec![Diagnostic::warning(
                    "sim.stock.full",
                    format!(
                        "Setup `{}` starts from its full stock, not from `{pid}`'s: {why}",
                        rs.id
                    ),
                )],
            ))
        };
        let pr = fluut_model::setup::resolve_setup(&self.project, &pid).map_err(|d| vec![d])?;
        if pr.kind == SetupKind::Rotary {
            return full("rotary Setups are not simulated yet");
        }
        let turn = setup_angle(rs) - setup_angle(&pr);
        let quarters = turn / 90.0;
        if quarters != quarters.round() {
            return full("its angle is not a multiple of 90° from the parent's");
        }
        let parent = self.simulate(&pid, settings, monitor)?;
        let mut d = parent.sim.stock_final().clone();
        for _ in 0..(quarters.round() as i64).rem_euclid(4) {
            d = d.turned_quarter();
        }
        Ok((Some(d), Vec::new()))
    }

    fn setup_bodies(&self, s: &ResolvedSetup) -> Result<Vec<SetupBody>, Diagnostic> {
        // A stock body comes along, for the frame to take out again.
        let stock = match &s.stock {
            Some(StockSpec::Body { body }) => Some(body),
            _ => None,
        };
        match &s.geometry.bodies {
            None => Ok(self.store.bodies().cloned().collect()),
            Some(ids) => ids
                .iter()
                .chain(stock.filter(|b| !ids.contains(b)))
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
        self.add_length_warnings(&mut reports);
        reports
    }

    /// The warning `cam.tool.length_missing`, once per tool, on the first report
    /// of a toolpath that uses a tool with no overall length whose length a
    /// check needs: the Z travel check before an A turn (the tool in the
    /// spindle), and the holder, which starts at the overall length.
    fn add_length_warnings(&self, reports: &mut [JobReport]) {
        let used = || {
            self.project
                .setups
                .iter()
                .flat_map(|s| s.toolpaths.iter())
                .filter(|t| {
                    t.is_enabled() && t.kind() != fluut_model::params::ToolpathTypeId::ManualNc
                })
        };
        let lacks = |r: &str| {
            self.tools
                .get(r)
                .is_some_and(|t| t.gauge_length().is_none())
        };
        if !used().any(|t| lacks(t.tool_ref())) {
            return;
        }
        // Tool reference -> the Setups whose turn clearance needs it, and
        // whether its holder needs it.
        let mut need: BTreeMap<String, (Vec<String>, bool)> = BTreeMap::new();
        for t in used() {
            let r = t.tool_ref();
            if lacks(r) && self.tools.get(r).is_some_and(|t| t.holder.is_some()) {
                need.entry(r.to_owned()).or_default().1 = true;
            }
        }
        for (setup, tool) in self.turn_heights().2 {
            need.entry(tool).or_default().0.push(setup);
        }
        for (tref, (setups, holder)) in need {
            let Some(tool) = self.tools.get(&tref) else {
                continue;
            };
            let Some(t) =
                used().find(|t| t.tool_ref() == tref && reports.iter().any(|r| r.toolpath == t.id))
            else {
                continue;
            };
            let d = length_warning(tool, &setups, holder).on(&t.id);
            if let Some(rep) = reports.iter_mut().find(|r| r.toolpath == t.id) {
                rep.diagnostics.push(d);
            }
        }
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
        let (turns, travel, _) = self.turn_heights();
        let errors: Vec<Diagnostic> = errors
            .into_iter()
            .chain(
                travel
                    .into_iter()
                    .filter(|(id, _)| chosen(id))
                    .map(|(_, d)| d),
            )
            .collect();
        if !errors.is_empty() {
            return Err(errors);
        }
        let mut job = self.post_job(&resolved, post_id, opts, machine.as_ref(), &turns);
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

    /// The Z the tool rises to before A turns at each Setup's start (spec
    /// 05 §5.6), by Setup in project order. An indexed Setup turns, and so
    /// does a rotary one after another Setup; the height is the larger turn
    /// clearance of the Setup and of the Setup before it, since the work on
    /// the table is one of the two. Also `(setup, error)` for each height
    /// whose spindle gauge line is above the machine's Z travel
    /// (`setup.clearance.index_travel`), and `(setup, tool)` for each height
    /// that was checked against that travel for a tool with no overall
    /// length (`cam.tool.length_missing`). A Setup's own height counts the
    /// fixtures within the X range its tools reach (`Session::tool_reach`).
    #[allow(clippy::type_complexity)]
    fn turn_heights(
        &self,
    ) -> (
        Vec<Option<f64>>,
        Vec<(String, Diagnostic)>,
        Vec<(String, String)>,
    ) {
        let mut heights = Vec::new();
        let mut errors = Vec::new();
        let mut unmeasured = Vec::new();
        let mut prev: Option<Option<f64>> = None;
        for (si, r) in resolve_all(&self.project).into_iter().enumerate() {
            let Ok(r) = r else {
                heights.push(None);
                continue;
            };
            let own = self
                .setup_bodies(&r)
                .and_then(|b| SetupFrame::new(&r, b))
                .ok()
                .and_then(|f| f.turn_clearance_reaching(self.tool_reach(si)));
            let turns = match r.kind {
                SetupKind::Indexed => true,
                SetupKind::Rotary => prev.is_some(),
                SetupKind::ThreeAxis => false,
            };
            let h = own
                .into_iter()
                .chain(prev.flatten())
                .reduce(f64::max)
                .filter(|_| turns);
            prev = Some(own);
            heights.push(h);
            // W's zero lies on the axis: Z in W is machine Z − the axis's.
            let Some(h) = h else { continue };
            let Some(m) = self.machine(r.machine.as_deref()) else {
                continue;
            };
            if let (Some([_, top]), Some(axis)) = (m.fields.travel.z, m.fields.axis.rotary.first())
            {
                let tool = self.spindle_tool(si);
                if let (Some(t), Some(tref)) = (tool, self.spindle_tool_ref(si))
                    && t.gauge_length().is_none()
                {
                    unmeasured.push((r.id.clone(), tref.to_owned()));
                }
                if let Some(d) = travel_error(&r.id, h, tool, top - axis.centre[2]) {
                    errors.push((r.id.clone(), d));
                }
            }
        }
        (heights, errors, unmeasured)
    }

    /// How far beyond the ends of the stock in X the tools of Setup `si`
    /// reach: the largest cutter radius among its enabled toolpaths. A
    /// fixture within that range of the stock can be met by the tool.
    fn tool_reach(&self, si: usize) -> f64 {
        self.project.setups[si]
            .toolpaths
            .iter()
            .filter(|t| t.is_enabled() && t.kind() != fluut_model::params::ToolpathTypeId::ManualNc)
            .filter_map(|t| self.tools.get(t.tool_ref()))
            .map(|t| t.radius())
            .fold(0.0, f64::max)
    }

    /// The tool in the spindle when A turns at the start of Setup `si`: the
    /// tool of the last enabled toolpath of the nearest earlier Setup that
    /// has one. The turn comes before the Setup's first tool change.
    fn spindle_tool(&self, si: usize) -> Option<&fluut_model::tools::Tool> {
        self.spindle_tool_ref(si).and_then(|r| self.tools.get(r))
    }

    /// The tool reference of [`Session::spindle_tool`].
    fn spindle_tool_ref(&self, si: usize) -> Option<&str> {
        self.project.setups[..si].iter().rev().find_map(|s| {
            s.toolpaths
                .iter()
                .rev()
                .find(|t| {
                    t.is_enabled() && t.kind() != fluut_model::params::ToolpathTypeId::ManualNc
                })
                .map(|t| t.tool_ref())
        })
    }

    fn post_job(
        &self,
        resolved: &fluut_model::Resolved,
        post_id: &str,
        opts: &PostOptions,
        machine: Option<&Machine>,
        turns: &[Option<f64>],
    ) -> PostJob {
        let mut setups = Vec::new();
        let setups_r = resolve_all(&self.project);
        for (si, (s, rs)) in self.project.setups.iter().zip(&setups_r).enumerate() {
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
                turn_z: turns.get(si).copied().flatten(),
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

/// The error `setup.clearance.index_travel` when A turns at Setup `setup`
/// with the tool tip at Z `tip` in W and the machine's Z travel ends at Z
/// `top` in W. Travel limits are machine coordinates of the spindle gauge
/// line (spec 02 §2.3), so the gauge line, at the tip + the gauge length of
/// `tool`, is compared; with no known length, the tip.
fn travel_error(
    setup: &str,
    tip: f64,
    tool: Option<&fluut_model::tools::Tool>,
    top: f64,
) -> Option<Diagnostic> {
    let gauge = tool.and_then(|t| t.gauge_length().map(|l| (t, l)));
    let line = tip + gauge.map_or(0.0, |(_, l)| l);
    if line <= top {
        return None;
    }
    let what = match (gauge, tool) {
        (Some((t, l)), _) => format!(
            "the spindle gauge line at Z {line:.3} (tool `{}`, gauge length {l:.3})",
            t.id
        ),
        (None, Some(t)) => format!(
            "the spindle gauge line at Z {line:.3} (tool `{}` has no overall length:              gauge length taken as 0)",
            t.id
        ),
        (None, None) => format!("the spindle gauge line at Z {line:.3} (no tool known)"),
    };
    Some(
        Diagnostic::error(
            "setup.clearance.index_travel",
            format!(
                "Setup `{setup}`: A turns with the tool tip at Z {tip:.3}, clear of the stock                  and the part, and {what}; the machine's Z travel ends at Z {top:.3}"
            ),
        )
        .at("clearance"),
    )
}

/// The warning `cam.tool.length_missing` for `tool`, which has no overall
/// length, when the turn clearance before A turns at `setups` is checked
/// against the Z travel and/or when its holder (`holder`) is to be checked.
fn length_warning(tool: &fluut_model::tools::Tool, setups: &[String], holder: bool) -> Diagnostic {
    let mut needs = Vec::new();
    if !setups.is_empty() {
        let names: Vec<String> = setups.iter().map(|s| format!("`{s}`")).collect();
        needs.push(format!(
            "the retract before A turns at Setup {} is checked against the machine's Z travel at the tool tip only, so a long tool can still pass it and reach the top of the travel",
            names.join(", ")
        ));
    }
    if holder {
        needs.push(
            "its holder starts at the overall length, so the simulation cannot place it and does not check it"
                .to_owned(),
        );
    }
    Diagnostic::warning(
        "cam.tool.length_missing",
        format!(
            "tool `{}` has no overall length: {}",
            tool.id,
            needs.join("; ")
        ),
    )
}

/// The error `sim.fixture` for the first place where the tool of `a`,
/// moving as `result` says, meets a fixture of `fr` that turns with A.
/// Checked at the ends of every segment, and along lines every millimetre;
/// the tool is its cutter (and a wider shank) from the tip up, and the
/// holder from the overall length up, each a vertical cylinder, so the check
/// errs towards a hit. Arcs and helices are checked at their ends only.
fn fixture_finding(
    fr: &SetupFrame,
    result: &ToolpathResult,
    a: &fluut_sim::Assembly,
) -> Option<Diagnostic> {
    if fr.fixtures.is_empty() {
        return None;
    }
    let cutter = a.tool.radius.max(a.shank.unwrap_or(0.0));
    let at = |p: fluut_geom::Point3| {
        let hit =
            |r: f64, up: f64| fr.fixture_hit(fluut_geom::Point3::new(p.x(), p.y(), p.z() + up), r);
        hit(cutter, 0.0).or_else(|| a.holder.and_then(|(r, from, _)| hit(r, from)))
    };
    let report = |n: usize, p: fluut_geom::Point3, f: &crate::fixture::FixtureShape| {
        Diagnostic::error(
            "sim.fixture",
            format!(
                "the tool meets the {} at ({:.3}, {:.3}, {:.3}), segment {n}",
                f.kind(),
                p.x(),
                p.y(),
                p.z()
            ),
        )
    };
    for (n, seg) in result.segments().enumerate() {
        let (Some(from), Some(to)) = (seg.start(), seg.end()) else {
            continue;
        };
        let (p, q) = (from.xyz, to.xyz);
        let straight = matches!(seg, fluut_path::Segment::Line { .. });
        let steps = if straight {
            (p.distance(q) / 1.0).ceil() as usize
        } else {
            1
        };
        for i in 0..=steps.max(1) {
            let t = i as f64 / steps.max(1) as f64;
            let x = p + (q - p) * t;
            if let Some(f) = at(x) {
                return Some(report(n, x, f));
            }
        }
    }
    None
}

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

#[cfg(test)]
mod travel_tests {
    use super::travel_error;
    use fluut_model::tools::ToolLibrary;

    fn tool(extra: &str) -> fluut_model::tools::Tool {
        let src = format!(
            "format = \"fluut.tools/1\"
id = \"t\"
[[tool]]
id = \"flat-6\"
             shape = \"flat\"
diameter = 6.0
{extra}
"
        );
        ToolLibrary::parse(&src).unwrap().0.tools.remove(0)
    }

    #[test]
    fn the_gauge_line_is_checked_not_the_tip() {
        let t = tool(
            "overall_length = 60
holder = { diameter = 20, length = 40 }",
        );
        // The tip at 41.321 clears a travel top of 80; the gauge line does not.
        let d = travel_error("s2", 41.321, Some(&t), 80.0).unwrap();
        assert_eq!(d.code, "setup.clearance.index_travel");
        assert!(d.message.contains("tool tip at Z 41.321"), "{}", d.message);
        assert!(
            d.message.contains("gauge line at Z 141.321"),
            "{}",
            d.message
        );
        assert!(d.message.contains("gauge length 100.000"), "{}", d.message);
        assert!(d.message.contains("ends at Z 80.000"), "{}", d.message);
        assert!(travel_error("s2", 41.321, Some(&t), 141.321).is_none());
        assert!(travel_error("s2", 41.321, Some(&t), 150.0).is_none());
    }

    #[test]
    fn without_a_length_the_tip_is_checked() {
        let t = tool("");
        assert!(travel_error("s2", 41.0, Some(&t), 50.0).is_none());
        let d = travel_error("s2", 41.0, Some(&t), 30.0).unwrap();
        assert!(d.message.contains("no overall length"), "{}", d.message);
        let d = travel_error("s2", 41.0, None, 30.0).unwrap();
        assert!(d.message.contains("no tool known"), "{}", d.message);
    }
}
