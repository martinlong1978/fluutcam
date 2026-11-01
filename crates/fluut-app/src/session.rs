// SPDX-License-Identifier: MIT OR Apache-2.0

//! The session adapter: the only module that calls `fluut-core`.
//!
//! A worker thread owns the `fluut_core::Session` and runs every request:
//! open, edit, calculate and post. After each step it sends the UI a fresh
//! [`Snapshot`] (D01 §3.1: snapshots, not locks).
//!
//! `fluut-core` (M3) has no job ids, progress, cancel or `Session::apply`
//! yet, so this module adds them around today's API: a calculation runs one
//! toolpath at a time and checks the cancel token between toolpaths, and
//! edits change `Session::project` directly. When M4 lands `apply`, undo and
//! the journal, this is the file that changes.

use crate::jobs::{CalcTargets, Edit, Event, JobCx, Outcome, PostRequest, Request};
use crate::scene::{SetupScene, ToolpathLines};
use fluut_cam::geometry::SetupBody;
use fluut_core::{JobReport, JobState, NodeId, PostOptions, Session, SetupFrame, Targets};
use fluut_model::project::{Project, Setup};
use fluut_model::tools::ToolStore;
use fluut_model::validate::{ValidateCx, validate};
use fluut_model::{Diagnostic, Severity};
use fluut_path::{PathStats, ToolpathResult};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

/// Toolpath state shown in the browser (S 08 §8.6). "Calculating" is not
/// here: the UI takes it from the running job.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ToolpathStatus {
    NotCalculated,
    Done,
    /// Done, with warnings.
    Warning,
    /// Calculated earlier; its inputs have changed since.
    Stale,
    /// Cannot be calculated: a validation or calculation error.
    Error,
    Disabled,
}

/// The document and its results as of one moment. Immutable; the worker
/// sends a new one after every change.
#[derive(Debug)]
pub struct Snapshot {
    /// Increases with every snapshot of a session.
    pub revision: u64,
    pub path: PathBuf,
    pub project: Project,
    pub tools: ToolStore,
    /// Load findings and Setup frame errors.
    pub warnings: Vec<Diagnostic>,
    pub status: BTreeMap<String, ToolpathStatus>,
    /// Per toolpath: validation, plus the calculation's diagnostics when its
    /// result is current.
    pub diagnostics: BTreeMap<String, Vec<Diagnostic>>,
    pub stats: BTreeMap<String, PathStats>,
    /// Per Setup id, in its work frame.
    pub scenes: BTreeMap<String, Arc<SetupScene>>,
    /// Per toolpath id, for current results only.
    pub paths: BTreeMap<String, Arc<ToolpathLines>>,
    /// Why the last edit was refused, if it was.
    pub edit_error: Option<String>,
}

impl Snapshot {
    pub fn file_name(&self) -> String {
        file_name(&self.path)
    }

    pub fn status(&self, toolpath: &str) -> ToolpathStatus {
        self.status
            .get(toolpath)
            .copied()
            .unwrap_or(ToolpathStatus::NotCalculated)
    }

    pub fn diagnostics(&self, toolpath: &str) -> &[Diagnostic] {
        self.diagnostics.get(toolpath).map_or(&[], Vec::as_slice)
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Starts the worker thread.
pub(crate) fn spawn(wake: Box<dyn Fn() + Send>) -> (Sender<Request>, Receiver<Event>) {
    let (req_tx, req_rx) = channel();
    let (ev_tx, ev_rx) = channel();
    std::thread::Builder::new()
        .name("fluut-session".into())
        .spawn(move || {
            let mut w = Worker {
                tx: ev_tx,
                wake,
                state: None,
                revision: 0,
            };
            while let Ok(r) = req_rx.recv() {
                w.run(r);
            }
        })
        .expect("cannot start the session thread");
    (req_tx, ev_rx)
}

/// The last calculation report of a toolpath and the result hash it was for.
#[derive(Debug)]
struct Report {
    hash: String,
    report: JobReport,
}

#[derive(Debug)]
struct State {
    session: Session,
    path: PathBuf,
    /// Toolpaths calculated at least once, so a missing result is stale.
    calculated: BTreeSet<String>,
    reports: BTreeMap<String, Report>,
    lines: BTreeMap<String, (Arc<ToolpathResult>, Arc<ToolpathLines>)>,
    /// Scenes by Setup id, with the Setup node hash they were built for.
    scenes: BTreeMap<String, (String, Arc<SetupScene>)>,
    frame_errors: BTreeMap<String, Diagnostic>,
    edit_error: Option<String>,
}

struct Worker {
    tx: Sender<Event>,
    wake: Box<dyn Fn() + Send>,
    state: Option<State>,
    revision: u64,
}

impl Worker {
    fn send(&self, e: Event) {
        // The UI may have gone; nothing to do then.
        let _ = self.tx.send(e);
        (self.wake)();
    }

    fn finish(&self, cx: &JobCx, outcome: Outcome) {
        self.send(Event::Finished {
            job: cx.id,
            outcome,
        });
    }

    fn run(&mut self, r: Request) {
        match r {
            Request::Open(cx, path) => self.open(&cx, path),
            Request::Calculate(cx, targets) => {
                let outcome = self.calculate(&cx, &targets);
                self.finish(&cx, outcome);
            }
            Request::Post(cx, req) => {
                let outcome = self.post(&cx, &req);
                self.finish(&cx, outcome);
            }
            Request::Edit(e) => self.edit(e),
        }
    }

    fn open(&mut self, cx: &JobCx, path: PathBuf) {
        cx.progress.set_total(1);
        match Session::open(&path, &[]) {
            Ok(session) => {
                self.state = Some(State {
                    session,
                    path: path.clone(),
                    calculated: BTreeSet::new(),
                    reports: BTreeMap::new(),
                    lines: BTreeMap::new(),
                    scenes: BTreeMap::new(),
                    frame_errors: BTreeMap::new(),
                    edit_error: None,
                });
                cx.progress.advance();
                self.snapshot();
                self.finish(
                    cx,
                    Outcome::Opened {
                        file: file_name(&path),
                    },
                );
            }
            Err(d) => self.finish(cx, Outcome::Failed(d)),
        }
    }

    fn edit(&mut self, e: Edit) {
        let Some(st) = self.state.as_mut() else {
            return;
        };
        st.edit_error = match e {
            Edit::SetParam {
                toolpath,
                path,
                value,
            } => match st.session.project.toolpath_mut(&toolpath) {
                Some(t) => t.params.set(&path, value).err(),
                None => Some(format!("no toolpath `{toolpath}`")),
            },
        };
        self.snapshot();
    }

    /// Calculates the targets one toolpath at a time, in project order.
    fn calculate(&mut self, cx: &JobCx, targets: &CalcTargets) -> Outcome {
        let Some(st) = self.state.as_ref() else {
            return Outcome::Failed(vec![no_project()]);
        };
        let ids: Vec<String> = st
            .session
            .project
            .setups
            .iter()
            .flat_map(|s| s.toolpaths.iter())
            .filter(|t| match targets {
                CalcTargets::All => true,
                CalcTargets::Toolpaths(ids) => ids.contains(&t.id),
            })
            .map(|t| t.id.clone())
            .collect();
        cx.progress.set_total(ids.len() as u64);
        let mut count = 0;
        for id in ids {
            if cx.cancel.is_cancelled() {
                return Outcome::Cancelled;
            }
            self.send(Event::Step {
                job: cx.id,
                toolpath: id.clone(),
            });
            let Some(st) = self.state.as_mut() else {
                break;
            };
            let graph = st.session.graph();
            let hash = graph
                .hash(&NodeId::Result(id.clone()))
                .unwrap_or_default()
                .to_owned();
            for report in st.session.calculate(&Targets::Toolpaths(vec![id.clone()])) {
                if let JobState::Done { .. } = report.state {
                    st.calculated.insert(report.toolpath.clone());
                    count += 1;
                }
                st.reports.insert(
                    report.toolpath.clone(),
                    Report {
                        hash: hash.clone(),
                        report,
                    },
                );
            }
            cx.progress.advance();
            self.snapshot();
        }
        Outcome::Calculated { count }
    }

    /// Calculates what is stale (with progress and cancel), then posts.
    fn post(&mut self, cx: &JobCx, req: &PostRequest) -> Outcome {
        match self.calculate(cx, &CalcTargets::All) {
            Outcome::Calculated { .. } => {}
            other => return other,
        }
        let Some(st) = self.state.as_mut() else {
            return Outcome::Failed(vec![no_project()]);
        };
        let opts = PostOptions {
            date: req.date.clone(),
            time: req.time.clone(),
            notes: String::new(),
        };
        let store = fluut_post::builtin::builtin_store();
        let output = match st.session.post(&req.post, &store, &opts) {
            Ok((output, _)) => output,
            Err(d) => return Outcome::Failed(d),
        };
        let mut files = Vec::new();
        for (i, text) in output.files.iter().enumerate() {
            let path = if i == 0 {
                req.output.clone()
            } else {
                numbered(&req.output, i + 1)
            };
            if let Err(e) = std::fs::write(&path, text) {
                return Outcome::Failed(vec![Diagnostic::error(
                    "app.post.write",
                    format!("cannot write {}: {e}", path.display()),
                )]);
            }
            files.push(path);
        }
        Outcome::Posted { files }
    }

    /// Builds and sends a snapshot of the current state.
    fn snapshot(&mut self) {
        let Some(st) = self.state.as_mut() else {
            return;
        };
        self.revision += 1;
        let snap = st.snapshot(self.revision);
        self.send(Event::Snapshot(Arc::new(snap)));
    }
}

fn no_project() -> Diagnostic {
    Diagnostic::error("app.project.none", "no project is open")
}

/// `out.ngc` → `out.2.ngc` for the files after the first, as `fluut post`
/// names them.
pub fn numbered(p: &Path, n: usize) -> PathBuf {
    let stem = p
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let name = match p.extension() {
        Some(e) => format!("{stem}.{n}.{}", e.to_string_lossy()),
        None => format!("{stem}.{n}"),
    };
    p.with_file_name(name)
}

fn push_unique(v: &mut Vec<Diagnostic>, d: Diagnostic) {
    if !v.contains(&d) {
        v.push(d);
    }
}

impl State {
    fn setup_bodies(&self, s: &Setup) -> Vec<SetupBody> {
        let all = self.session.store.bodies();
        match &s.bodies {
            None => all.cloned().collect(),
            Some(ids) => all.filter(|b| ids.contains(&b.id)).cloned().collect(),
        }
    }

    fn snapshot(&mut self, revision: u64) -> Snapshot {
        let graph = self.session.graph();
        let project = self.session.project.clone();
        // Setup scenes, rebuilt only when the Setup's node hash changes.
        let mut scenes = BTreeMap::new();
        for s in &project.setups {
            let hash = graph
                .hash(&NodeId::Setup(s.id.clone()))
                .unwrap_or_default()
                .to_owned();
            if self.scenes.get(&s.id).is_none_or(|(h, _)| *h != hash) {
                let scene = match SetupFrame::new(s, self.setup_bodies(s)) {
                    Ok(fr) => {
                        self.frame_errors.remove(&s.id);
                        SetupScene::new(&fr.geometry, Some(&fr.stock))
                    }
                    Err(d) => {
                        self.frame_errors.insert(s.id.clone(), d);
                        SetupScene::default()
                    }
                };
                self.scenes.insert(s.id.clone(), (hash, Arc::new(scene)));
            }
            scenes.insert(s.id.clone(), self.scenes[&s.id].1.clone());
        }
        let mut status = BTreeMap::new();
        let mut diagnostics = BTreeMap::new();
        let mut stats = BTreeMap::new();
        let mut paths = BTreeMap::new();
        for s in &project.setups {
            for t in &s.toolpaths {
                let tool = self.session.tools.get(&t.tool);
                let cx = ValidateCx {
                    material: s.material.as_deref(),
                    ..ValidateCx::new(tool)
                };
                let mut diags = validate(t, &cx);
                let hash = graph
                    .hash(&NodeId::Result(t.id.clone()))
                    .unwrap_or_default();
                let report = self
                    .reports
                    .get(&t.id)
                    .filter(|r| r.hash == hash)
                    .map(|r| &r.report);
                if let Some(r) = report {
                    for d in &r.diagnostics {
                        push_unique(&mut diags, d.clone());
                    }
                }
                let result = self.session.result(&t.id);
                let has_error = diags.iter().any(|d| d.severity == Severity::Error)
                    || report.is_some_and(|r| r.state == JobState::Failed);
                let st = if !t.is_enabled() {
                    ToolpathStatus::Disabled
                } else if let Some(res) = &result {
                    stats.insert(t.id.clone(), *res.stats());
                    let lines = match self.lines.get(&t.id) {
                        Some((r, l)) if Arc::ptr_eq(r, res) => l.clone(),
                        _ => {
                            let l = Arc::new(ToolpathLines::new(res));
                            self.lines.insert(t.id.clone(), (res.clone(), l.clone()));
                            l
                        }
                    };
                    paths.insert(t.id.clone(), lines);
                    if diags.iter().any(|d| d.severity == Severity::Warning) {
                        ToolpathStatus::Warning
                    } else {
                        ToolpathStatus::Done
                    }
                } else if has_error {
                    ToolpathStatus::Error
                } else if self.calculated.contains(&t.id) {
                    ToolpathStatus::Stale
                } else {
                    ToolpathStatus::NotCalculated
                };
                status.insert(t.id.clone(), st);
                diagnostics.insert(t.id.clone(), diags);
            }
        }
        let mut warnings = self.session.warnings.clone();
        warnings.extend(self.frame_errors.values().cloned());
        Snapshot {
            revision,
            path: self.path.clone(),
            project,
            tools: self.session.tools.clone(),
            warnings,
            status,
            diagnostics,
            stats,
            scenes,
            paths,
            edit_error: self.edit_error.clone(),
        }
    }
}

/// The current UTC date and time (`yyyy-mm-dd`, `hh:mm:ss`) for the post
/// header, as `fluut post` uses when none is given.
pub fn now_utc() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        format!("{y:04}-{m:02}-{d:02}"),
        format!("{:02}:{:02}:{:02}", rem / 3600, rem / 60 % 60, rem % 60),
    )
}

#[cfg(test)]
mod tests;
