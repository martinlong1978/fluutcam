// SPDX-License-Identifier: MIT OR Apache-2.0

//! The job layer (D01 §3, D13 §8): requests to the session worker, events
//! back, and per-job cancel and progress.
//!
//! The UI thread sends [`Request`]s and drains [`Event`]s without blocking.
//! Progress is two atomics the UI samples each frame; cancellation is a
//! shared flag the worker checks between steps.

use fluut_model::{Diagnostic, PValue};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

pub use crate::session::Snapshot;

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
    Calculate(JobCx, CalcTargets),
    Post(JobCx, PostRequest),
    Edit(Edit),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum JobKind {
    Open,
    Calculate,
    Post,
}

/// How a job ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Opened {
        file: String,
    },
    Calculated {
        count: usize,
    },
    Posted {
        files: Vec<PathBuf>,
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
            Event::Finished { job, outcome } => {
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
