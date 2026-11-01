// SPDX-License-Identifier: MIT OR Apache-2.0

//! The session (D01 §5): one project, its definitions and geometry, the
//! result cache, and calculation and post jobs.

use crate::cache::DiskCache;
use crate::frame::SetupFrame;
use crate::graph::{Graph, NodeId};
use crate::reimport::{ReimportReport, Unmatched, belongs, match_bodies, references};
use crate::store::ModelStore;
use fluut_cam::StrategyInput;
use fluut_cam::geometry::SetupBody;
use fluut_cam::stock::{LayeredStock, Sweep};
use fluut_model::document::{Document, Outcome};
use fluut_model::edit::{Edit, EditError};
use fluut_model::journal::{Journal, file_hash, journal_path, recover};
use fluut_model::library::{DefKind, LayerKind, Library};
use fluut_model::machine::{BUILTIN_MACHINES, Machine};
use fluut_model::project::{BodyEntry, Project, read};
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
        for (s, rs) in project.setups.iter().zip(resolve_all(&project)) {
            let mut wanted: Vec<bool> = s
                .toolpaths
                .iter()
                .map(|t| match targets {
                    Targets::All => true,
                    Targets::Toolpaths(ids) => ids.contains(&t.id),
                })
                .collect();
            // A toolpath that reads the in-process stock needs every
            // earlier toolpath of its Setup (D08 §7).
            let last_reader = s
                .toolpaths
                .iter()
                .zip(&wanted)
                .rposition(|(t, w)| *w && t.is_enabled() && t.params.reads_in_process());
            if let Some(i) = last_reader {
                wanted[..i].iter_mut().for_each(|w| *w = true);
            }
            if !wanted.iter().any(|w| *w) {
                continue;
            }
            let mut frame: Option<Result<SetupFrame, Diagnostic>> = None;
            let mut stock: Option<LayeredStock> = None;
            if last_reader.is_some()
                && let Ok(r) = &rs
            {
                let fr = frame.get_or_insert_with(|| {
                    self.setup_bodies(r).and_then(|b| SetupFrame::new(r, b))
                });
                if let Ok(f) = fr {
                    stock = Some(f.initial_stock());
                }
            }
            for (ti, t) in s.toolpaths.iter().enumerate() {
                if !wanted[ti] {
                    continue;
                }
                // Update the stock from this toolpath's result while a
                // later toolpath still reads it.
                let feeds_stock = last_reader.is_some_and(|l| ti < l) && t.kind().updates_stock();
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
                let in_process = if t.params.reads_in_process() {
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
                    rotary: fr.rotary_info(),
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
        let mut reports = self.calculate_with(&Targets::All, Some(caps));
        // Post-dependent rules also for toolpaths taken from the cache.
        let project = self.project.clone();
        for (s, rs) in project.setups.iter().zip(resolve_all(&project)) {
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
        let job = self.post_job(&resolved, post_id, opts, machine.as_ref());
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
                kind: fluut_post::SetupKind::ThreeAxis,
                work_offset: rs.work_offset.clone(),
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
