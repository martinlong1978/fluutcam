// SPDX-License-Identifier: MIT OR Apache-2.0

//! The job layer (D01 §3, D13 §8): requests to the session worker, events
//! back, and per-job cancel and progress.
//!
//! The UI thread sends [`Request`]s and drains [`Event`]s without blocking.
//! Progress is two atomics the UI samples each frame; cancellation is a
//! shared flag the worker checks between steps.

use crate::text::{tr, tr_args};
use fluut_model::project::{DisplayUnit, SetupKind};
use fluut_model::{Diagnostic, PValue, ToolpathTypeId};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

pub use crate::session::{Frame, Snapshot};

/// Job number, unique per [`Jobs`].
pub type JobId = u64;

/// Cooperative cancellation (D01 §3).
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Lock-free progress: `done` of `total` steps.
#[derive(Debug, Default)]
pub struct Progress {
    done: AtomicU64,
    total: AtomicU64,
}

impl Progress {
    pub fn set_total(&self, n: u64) {
        self.total.store(n, Ordering::Relaxed);
    }

    pub fn advance(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    /// Sets both counts at once (a job that reports its own steps).
    pub fn set(&self, done: u64, total: u64) {
        self.total.store(total, Ordering::Relaxed);
        self.done.store(done, Ordering::Relaxed);
    }

    /// `(done, total)`.
    pub fn get(&self) -> (u64, u64) {
        (
            self.done.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        )
    }

    /// Fraction done, 0 when the total is not known yet.
    pub fn fraction(&self) -> f32 {
        let (d, t) = self.get();
        if t == 0 { 0.0 } else { d as f32 / t as f32 }
    }
}

/// What a job needs from the job layer.
#[derive(Clone, Debug)]
pub struct JobCx {
    pub id: JobId,
    pub cancel: CancelToken,
    pub progress: Arc<Progress>,
}

/// Toolpaths to calculate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalcTargets {
    All,
    Toolpaths(Vec<String>),
}

/// A document edit (D01 §5 `Edit`). M4 replaces the application of edits
/// with `Session::apply`, which adds undo.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Sets (`Some`) or clears (`None`) a toolpath parameter at `path`
    /// (`group.key` or `group.key.sub`).
    SetParam {
        toolpath: String,
        path: String,
        value: Option<PValue>,
    },
    /// Any other change to the project, one undo step.
    Model(fluut_model::edit::Edit),
    /// Reads the project's tool library files again (a library file was
    /// written or changed); not a change to the project, so no undo step.
    ReloadToolLibraries,
    /// Reads the project's posts, machines and templates again (a post
    /// file was written beside the project); not a change to the project,
    /// so no undo step.
    ReloadDefinitions,
}

/// What to post and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostRequest {
    /// Post id, e.g. `fluut/linuxcnc`.
    pub post: String,
    /// First output file; more files get `.2`, `.3`, … before the extension.
    pub output: PathBuf,
    /// Header date and time; the UI passes the current UTC time.
    pub date: String,
    pub time: String,
}

#[derive(Clone, Debug)]
pub enum Request {
    Open(JobCx, PathBuf),
    New(JobCx, PathBuf, DisplayUnit),
    /// Saves to the project's file, or as the path.
    Save(JobCx, Option<PathBuf>),
    /// Imports a model file; the unit applies to STL and DXF files.
    Import(JobCx, PathBuf, Option<String>),
    /// Re-reads every geometry source from its file.
    Reimport(JobCx),
    AddSetup {
        cx: JobCx,
        kind: SetupKind,
        based_on: Option<String>,
        name: String,
    },
    AddToolpath {
        cx: JobCx,
        setup: String,
        kind: ToolpathTypeId,
        name: String,
    },
    /// Copies a toolpath, under the name, just after itself.
    Duplicate {
        cx: JobCx,
        toolpath: String,
        name: String,
    },
    Undo(JobCx),
    Redo(JobCx),
    Calculate(JobCx, CalcTargets),
    Post(JobCx, PostRequest),
    /// Calculates what is stale in the Setup, then simulates it.
    Simulate(JobCx, String),
    /// The stock of a simulated Setup at a program time, s. Only the
    /// latest of several waiting requests is served.
    Scrub {
        setup: String,
        t: f64,
    },
    Edit(Edit),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum JobKind {
    Open,
    New,
    Save,
    Import,
    Reimport,
    AddSetup,
    AddToolpath,
    Duplicate,
    Undo,
    Redo,
    Calculate,
    Post,
    Simulate,
}

/// How a job ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Opened {
        file: String,
    },
    Created {
        file: String,
    },
    Saved {
        file: String,
    },
    Imported {
        file: String,
        bodies: usize,
    },
    Reimported {
        sources: usize,
        /// Selections that no entity of the new file matches.
        unmatched: usize,
    },
    SetupAdded {
        id: String,
        name: String,
    },
    ToolpathAdded {
        id: String,
        name: String,
    },
    Undone {
        label: String,
    },
    Redone {
        label: String,
    },
    Calculated {
        count: usize,
    },
    Posted {
        files: Vec<PathBuf>,
    },
    Simulated {
        findings: usize,
    },
    Cancelled,
    /// The diagnostics that stopped it.
    Failed(Vec<Diagnostic>),
}

#[derive(Clone, Debug)]
pub enum Event {
    /// A step started: the toolpath being calculated, by id.
    Step {
        job: JobId,
        toolpath: String,
    },
    /// The document or its results changed.
    Snapshot(Arc<Snapshot>),
    /// A stock mesh for scrubbing.
    Frame(Arc<Frame>),
    Finished {
        job: JobId,
        outcome: Outcome,
    },
}

/// The running job as the UI sees it.
#[derive(Clone, Debug)]
pub struct Running {
    pub cx: JobCx,
    pub kind: JobKind,
    /// Toolpath of the current step, if any.
    pub toolpath: Option<String>,
    /// Post id or file name, for the status line.
    pub subject: String,
}

/// The UI's handle on the worker.
#[derive(Debug)]
pub struct Jobs {
    tx: Sender<Request>,
    rx: Receiver<Event>,
    next: JobId,
    pub running: Option<Running>,
    pub snapshot: Option<Arc<Snapshot>>,
    /// Outcome of the last finished job.
    pub last: Option<(JobKind, Outcome)>,
    /// The latest stock frame.
    pub frame: Option<Arc<Frame>>,
    /// Scrub requests sent and not yet answered.
    pub scrubbing: usize,
    /// What the last finished add operation made, until the UI takes it to
    /// select it.
    pub created: Option<Created>,
}

/// An entity an operation added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Created {
    Setup(String),
    Toolpath(String),
}

impl Jobs {
    /// Starts the worker. `wake` is called after each event is sent, so the
    /// UI can repaint (it must not block).
    pub fn start(wake: impl Fn() + Send + 'static) -> Jobs {
        let (tx, rx) = crate::session::spawn(Box::new(wake));
        Jobs {
            tx,
            rx,
            next: 1,
            running: None,
            snapshot: None,
            last: None,
            frame: None,
            scrubbing: 0,
            created: None,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.running.is_some()
    }

    fn cx(&mut self) -> JobCx {
        let id = self.next;
        self.next += 1;
        JobCx {
            id,
            cancel: CancelToken::default(),
            progress: Arc::new(Progress::default()),
        }
    }

    fn start_job(&mut self, kind: JobKind, subject: String, req: impl FnOnce(JobCx) -> Request) {
        let cx = self.cx();
        self.running = Some(Running {
            cx: cx.clone(),
            kind,
            toolpath: None,
            subject,
        });
        // A closed channel means the worker is gone; the job then never
        // finishes, which `poll` reports.
        let _ = self.tx.send(req(cx));
    }

    /// Starts opening `path`. Ignored while a job runs.
    pub fn open(&mut self, path: PathBuf) {
        if self.is_busy() {
            return;
        }
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        self.start_job(JobKind::Open, name, |cx| Request::Open(cx, path));
    }

    /// Starts a new empty project at `path` (`.fluut` is added when there
    /// is no extension), written at once. Ignored while a job runs. The UI
    /// asks about unsaved changes before it calls this.
    pub fn new_project(&mut self, path: PathBuf, units: DisplayUnit) {
        if self.is_busy() {
            return;
        }
        let name = file_label(&path);
        self.start_job(JobKind::New, name, |cx| Request::New(cx, path, units));
    }

    /// Saves the project to its file. Ignored while a job runs or with no
    /// project.
    pub fn save(&mut self) {
        if self.is_busy() || self.snapshot.is_none() {
            return;
        }
        let name = self
            .snapshot
            .as_ref()
            .map_or_else(String::new, |s| s.file_name());
        self.start_job(JobKind::Save, name, |cx| Request::Save(cx, None));
    }

    /// Saves the project as `path`, which becomes its file. Ignored while
    /// a job runs or with no project.
    pub fn save_as(&mut self, path: PathBuf) {
        if self.is_busy() || self.snapshot.is_none() {
            return;
        }
        let name = file_label(&path);
        self.start_job(JobKind::Save, name, |cx| Request::Save(cx, Some(path)));
    }

    /// Imports a model file (STEP, and STL, DXF or SVG by extension) as a
    /// new geometry source. The unit of an STL or DXF file is the
    /// project's. Ignored while a job runs or with no project.
    pub fn import_model(&mut self, path: PathBuf) {
        self.import_model_unit(path, None);
    }

    /// As [`Jobs::import_model`], with the unit of the file (`mm`, `in`).
    pub fn import_model_unit(&mut self, path: PathBuf, unit: Option<String>) {
        if self.is_busy() || self.snapshot.is_none() {
            return;
        }
        let name = file_label(&path);
        self.start_job(JobKind::Import, name, |cx| Request::Import(cx, path, unit));
    }

    /// Re-reads the project's geometry files and matches their entities to
    /// the earlier ones. Ignored while a job runs or with no project.
    pub fn reimport(&mut self) {
        if self.is_busy() || self.snapshot.is_none() {
            return;
        }
        self.start_job(JobKind::Reimport, String::new(), Request::Reimport);
    }

    /// Adds a Setup of `kind`, optionally based on Setup `based_on`, named
    /// "Setup N". Ignored while a job runs or with no project.
    pub fn add_setup(&mut self, kind: SetupKind, based_on: Option<&str>) {
        if self.is_busy() {
            return;
        }
        let Some(s) = self.snapshot.as_ref() else {
            return;
        };
        let name = unused_name(
            &tr("setup-name"),
            s.project.setups.len() + 1,
            s.project.setups.iter().map(|x| x.name.as_str()),
        );
        let based_on = based_on.map(str::to_owned);
        self.start_job(JobKind::AddSetup, name.clone(), |cx| Request::AddSetup {
            cx,
            kind,
            based_on,
            name,
        });
    }

    /// Adds a toolpath of type `kind` to Setup `setup`, named "Type N".
    /// Ignored while a job runs or with no project.
    pub fn add_toolpath(&mut self, setup: &str, kind: ToolpathTypeId) {
        if self.is_busy() {
            return;
        }
        let Some(s) = self.snapshot.as_ref() else {
            return;
        };
        let label = tr(&format!("toolpath-type-{}", kind.as_str()));
        let all = || s.project.setups.iter().flat_map(|x| x.toolpaths.iter());
        let same = all().filter(|t| t.kind() == kind).count();
        let name = unused_name(&label, same + 1, all().map(|t| t.name.as_str()));
        let setup = setup.to_owned();
        self.start_job(JobKind::AddToolpath, name.clone(), |cx| {
            Request::AddToolpath {
                cx,
                setup,
                kind,
                name,
            }
        });
    }

    /// Copies toolpath `id` to just after itself, as "name copy". Ignored
    /// while a job runs or with no project.
    pub fn duplicate_toolpath(&mut self, id: &str) {
        if self.is_busy() {
            return;
        }
        let Some((_, t)) = self.snapshot.as_ref().and_then(|s| s.project.toolpath(id)) else {
            return;
        };
        let name = tr_args("name-copy", &[("name", &t.name)]);
        let toolpath = id.to_owned();
        self.start_job(JobKind::Duplicate, name.clone(), |cx| Request::Duplicate {
            cx,
            toolpath,
            name,
        });
    }

    /// Undoes the last edit. Ignored while a job runs or when there is
    /// nothing to undo.
    pub fn undo(&mut self) {
        if self.is_busy() || !self.snapshot.as_ref().is_some_and(|s| s.can_undo) {
            return;
        }
        self.start_job(JobKind::Undo, String::new(), Request::Undo);
    }

    /// Redoes the last undone edit. Ignored while a job runs or when there
    /// is nothing to redo.
    pub fn redo(&mut self) {
        if self.is_busy() || !self.snapshot.as_ref().is_some_and(|s| s.can_redo) {
            return;
        }
        self.start_job(JobKind::Redo, String::new(), Request::Redo);
    }

    /// Starts a calculation. Ignored while a job runs or with no project.
    pub fn calculate(&mut self, targets: CalcTargets) {
        if self.is_busy() || self.snapshot.is_none() {
            return;
        }
        self.start_job(JobKind::Calculate, String::new(), |cx| {
            Request::Calculate(cx, targets)
        });
    }

    /// Starts posting. Ignored while a job runs or with no project.
    pub fn post(&mut self, req: PostRequest) {
        if self.is_busy() || self.snapshot.is_none() {
            return;
        }
        let subject = req.post.clone();
        self.start_job(JobKind::Post, subject, |cx| Request::Post(cx, req));
    }

    /// Starts simulating a Setup. Ignored while a job runs or with no
    /// project.
    pub fn simulate(&mut self, setup: &str) {
        if self.is_busy() || self.snapshot.is_none() {
            return;
        }
        let setup = setup.to_owned();
        self.start_job(JobKind::Simulate, setup.clone(), |cx| {
            Request::Simulate(cx, setup)
        });
    }

    /// Asks for the stock of `setup` at time `t`.
    pub fn scrub(&mut self, setup: &str, t: f64) {
        self.scrubbing += 1;
        let _ = self.tx.send(Request::Scrub {
            setup: setup.to_owned(),
            t,
        });
    }

    pub fn edit(&mut self, e: Edit) {
        let _ = self.tx.send(Request::Edit(e));
    }

    /// Asks the running job to stop at its next check.
    pub fn cancel(&self) {
        if let Some(r) = &self.running {
            r.cx.cancel.cancel();
        }
    }

    /// Applies every waiting event. Returns `true` if anything changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        loop {
            match self.rx.try_recv() {
                Ok(e) => {
                    changed = true;
                    self.apply(e);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if let Some(r) = self.running.take() {
                        let msg = Diagnostic::error("app.worker.stopped", "the worker stopped");
                        self.last = Some((r.kind, Outcome::Failed(vec![msg])));
                        changed = true;
                    }
                    break;
                }
            }
        }
        changed
    }

    fn apply(&mut self, e: Event) {
        match e {
            Event::Step { job, toolpath } => {
                if let Some(r) = self.running.as_mut().filter(|r| r.cx.id == job) {
                    r.toolpath = Some(toolpath);
                }
            }
            Event::Snapshot(s) => self.snapshot = Some(s),
            Event::Frame(f) => {
                self.scrubbing = self.scrubbing.saturating_sub(f.requests);
                self.frame = Some(f);
            }
            Event::Finished { job, outcome } => {
                match &outcome {
                    Outcome::SetupAdded { id, .. } => {
                        self.created = Some(Created::Setup(id.clone()));
                    }
                    Outcome::ToolpathAdded { id, .. } => {
                        self.created = Some(Created::Toolpath(id.clone()));
                    }
                    _ => {}
                }
                if let Some(r) = self.running.take_if(|r| r.cx.id == job) {
                    self.last = Some((r.kind, outcome));
                }
            }
        }
    }

    /// Waits (blocking) for one event and applies it. `false` when the
    /// worker has stopped.
    pub fn wait_event(&mut self) -> bool {
        match self.rx.recv() {
            Ok(e) => {
                self.apply(e);
                true
            }
            Err(_) => {
                self.running = None;
                false
            }
        }
    }

    /// Waits (blocking) until the running job finishes, for tests and
    /// scripts. Returns its outcome.
    pub fn wait(&mut self) -> Option<Outcome> {
        while self.running.is_some() {
            if !self.wait_event() {
                return None;
            }
        }
        self.last.as_ref().map(|(_, o)| o.clone())
    }

    /// Waits for the next snapshot (after an edit, in tests).
    pub fn wait_snapshot(&mut self) -> Option<Arc<Snapshot>> {
        let rev = self.snapshot.as_ref().map(|s| s.revision);
        while self.snapshot.as_ref().map(|s| s.revision) == rev {
            if !self.wait_event() {
                return None;
            }
        }
        self.snapshot.clone()
    }
}

fn file_label(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// "`prefix` n", with n raised until no name in `taken` is the same.
fn unused_name<'a>(
    prefix: &str,
    first: usize,
    taken: impl Iterator<Item = &'a str> + Clone,
) -> String {
    (first..)
        .map(|n| tr_args("name-numbered", &[("name", prefix), ("n", &n.to_string())]))
        .find(|c| !taken.clone().any(|t| t == c))
        .unwrap_or_default()
}
