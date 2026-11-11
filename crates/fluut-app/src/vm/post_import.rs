// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Vectric post import (design D10 §6.1): which file, where to, the
//! import itself, and what the result window shows. No `egui` here.
//!
//! The steps are two native dialogs (the `.pp` file, then the new post
//! file) and an import on a worker thread. An import always writes a new
//! file: it replaces an existing one only when the Save dialog has already
//! asked the user to confirm that, and it never replaces its own input. A
//! `POST_BASE` file is read from beside the input, as the command line
//! does.

use crate::text::{diagnostic, tr, tr_args};
use fluut_model::{Diagnostic, Severity};
use fluut_post_import::vectric::{self, Options, Report};
use fluut_post_import::{limits, valid_library_id};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// The error for a file that is not a Vectric post: a `.cps` gets the
/// answer `fluut posts import` gives.
pub fn check_format(source: &Path) -> Result<(), Diagnostic> {
    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match ext.as_deref() {
        Some("pp") => Ok(()),
        Some("cps") => Err(Diagnostic::error(
            "import.format.unsupported",
            "Autodesk .cps posts are programs; FluutCAM does not import them yet (spec 05 §5.10.3)",
        )),
        _ => Err(Diagnostic::error(
            "import.format.unsupported",
            format!(
                "{}: expected a Vectric post processor (.pp)",
                source.display()
            ),
        )),
    }
}

/// The file name the Save dialog offers: the input's name with
/// `.post.toml`, the name FluutCAM looks for in a `posts` folder.
pub fn default_name(source: &Path) -> String {
    let stem = source
        .file_stem()
        .map_or_else(|| "post".to_owned(), |s| s.to_string_lossy().into_owned());
    format!("{stem}.post.toml")
}

/// Where the Save dialog starts: the project's `posts` folder if there is
/// one, else the project's folder.
pub fn start_dir(project: Option<&Path>) -> Option<PathBuf> {
    let dir = project?.parent()?;
    let posts = dir.join("posts");
    Some(if posts.is_dir() {
        posts
    } else {
        dir.to_path_buf()
    })
}

/// The target as written: `.post.toml` is added when the name has no
/// extension.
pub fn with_extension(target: PathBuf) -> PathBuf {
    if target.extension().is_some() {
        return target;
    }
    let mut name = target.into_os_string();
    name.push(".post.toml");
    PathBuf::from(name)
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Whether the post at `target` is found with the project at `project`: a
/// `*.post.toml` file directly in the `posts` folder beside it.
pub fn loads_with_project(project: &Path, target: &Path) -> bool {
    let Some(dir) = project.parent() else {
        return false;
    };
    target
        .file_name()
        .is_some_and(|n| n.to_string_lossy().ends_with(".post.toml"))
        && target
            .parent()
            .is_some_and(|p| same_path(p, &dir.join("posts")))
}

/// What a finished import shows.
#[derive(Clone, PartialEq, Debug)]
pub struct Done {
    pub source: PathBuf,
    pub target: PathBuf,
    pub post_id: String,
    /// The post's display name.
    pub name: String,
    /// `Some` when the file was written.
    pub report: Option<Report>,
    /// Warnings, or the errors that stopped the import.
    pub diagnostics: Vec<Diagnostic>,
    /// Post has been chosen for the project ("Use this post").
    pub used: bool,
}

impl Done {
    pub fn ok(&self) -> bool {
        self.report.is_some()
    }

    fn error(source: PathBuf, target: PathBuf, d: Vec<Diagnostic>) -> Done {
        Done {
            source,
            target,
            post_id: String::new(),
            name: String::new(),
            report: None,
            diagnostics: d,
            used: false,
        }
    }

    /// The headline: where the file went, or that nothing was imported.
    pub fn headline(&self) -> String {
        match &self.report {
            Some(_) => tr_args(
                "posts-import-done",
                &[
                    ("id", &self.post_id),
                    ("path", &self.target.display().to_string()),
                ],
            ),
            None => tr("posts-import-failed"),
        }
    }

    /// The counts, one line each.
    pub fn counts(&self) -> Vec<String> {
        let Some(r) = &self.report else {
            return Vec::new();
        };
        let of = |id: &str, a: usize, b: usize| {
            tr_args(id, &[("n", &a.to_string()), ("of", &b.to_string())])
        };
        let n = |id: &str, v: usize| tr_args(id, &[("n", &v.to_string())]);
        let warnings = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count();
        let mut v = vec![
            tr_args("posts-import-id", &[("id", &self.post_id)]),
            tr_args("posts-import-name", &[("name", &self.name)]),
            of("posts-import-settings", r.settings_mapped, r.settings_read),
            of("posts-import-blocks", r.blocks_mapped, r.blocks_read),
            n("posts-import-words", r.words_written),
        ];
        if r.base_files > 0 {
            v.push(n("posts-import-base", r.base_files));
        }
        v.push(n("posts-import-warning-count", warnings));
        v
    }

    /// The warnings or errors, as `(is error, code, text)`.
    pub fn messages(&self) -> Vec<(bool, String, String)> {
        self.diagnostics
            .iter()
            .map(|d| (d.severity == Severity::Error, d.code.clone(), diagnostic(d)))
            .collect()
    }
}

/// Reads at most the post size limit plus one byte, so a huge file is
/// refused without being read whole.
fn read_limited(p: &Path) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    std::fs::File::open(p)?
        .take(limits::MAX_POST_BYTES + 1)
        .read_to_end(&mut buf)?;
    Ok(buf)
}

fn fail(code: &str, msg: String) -> Vec<Diagnostic> {
    vec![Diagnostic::error(code, msg)]
}

/// Reads `source`, converts it and writes the post to `target`.
/// `replace`: the Save dialog has already asked about an existing target.
/// Without it the file is created only if it is new.
pub fn run(source: &Path, target: &Path, replace: bool) -> Done {
    let (s, t) = (source.to_path_buf(), target.to_path_buf());
    if let Err(d) = check_format(source) {
        return Done::error(s, t, vec![d]);
    }
    if same_path(source, target) {
        let msg = format!(
            "{}: the post cannot replace its own source",
            target.display()
        );
        return Done::error(s, t, fail("import.output.is_input", msg));
    }
    let stem = source
        .file_stem()
        .map(|x| x.to_string_lossy().into_owned())
        .unwrap_or_default();
    let opts = Options::for_file_stem(&stem);
    if !valid_library_id(&opts.post_id) {
        let msg = format!("`{}` is not a post id", opts.post_id);
        return Done::error(s, t, fail("import.post.bad_id", msg));
    }
    if target.exists() && !replace {
        let msg = format!("{} exists; choose a new file name", target.display());
        return Done::error(s, t, fail("import.output.exists", msg));
    }
    let bytes = match read_limited(source) {
        Ok(b) => b,
        Err(e) => {
            let msg = format!("{}: {e}", source.display());
            return Done::error(s, t, fail("import.file.unreadable", msg));
        }
    };
    let folder = source.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut base = |name: &str| read_limited(&folder.join(name)).ok();
    let r = match vectric::import_with_base(&bytes, &opts, &mut base) {
        Ok(r) => r,
        Err(d) => return Done::error(s, t, d),
    };
    let written = if replace {
        std::fs::write(target, &r.post)
    } else {
        // Atomic: never replaces a file that appeared meanwhile.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .and_then(|mut f| std::io::Write::write_all(&mut f, r.post.as_bytes()))
    };
    if let Err(e) = written {
        let msg = format!("{}: {e}", target.display());
        return Done::error(s, t, fail("import.output.unwritable", msg));
    }
    Done {
        source: s,
        target: t,
        post_id: opts.post_id,
        name: r.name,
        report: Some(r.report),
        diagnostics: r.diagnostics,
        used: false,
    }
}

/// Where the import is.
#[derive(Debug, Default)]
pub enum Phase {
    #[default]
    Idle,
    /// The `.pp` file is chosen; the Save dialog is open.
    AwaitTarget { source: PathBuf },
    /// The worker is converting.
    Running { source: PathBuf, rx: Receiver<Done> },
    /// The result window is open.
    Done(Box<Done>),
}

#[derive(Debug, Default)]
pub struct PostImport {
    pub phase: Phase,
}

impl PostImport {
    /// Whether a new import may start.
    pub fn is_free(&self) -> bool {
        matches!(self.phase, Phase::Idle | Phase::Done(_))
    }

    /// Whether the Save dialog is what the import waits for.
    pub fn awaiting_target(&self) -> bool {
        matches!(self.phase, Phase::AwaitTarget { .. })
    }

    pub fn is_running(&self) -> bool {
        matches!(self.phase, Phase::Running { .. })
    }

    /// The `.pp` file was chosen. Returns the name for the Save dialog, or
    /// shows the error for a file that cannot be imported.
    pub fn source_picked(&mut self, source: PathBuf) -> Option<String> {
        match check_format(&source) {
            Ok(()) => {
                let name = default_name(&source);
                self.phase = Phase::AwaitTarget { source };
                Some(name)
            }
            Err(d) => {
                self.phase = Phase::Done(Box::new(Done::error(source, PathBuf::new(), vec![d])));
                None
            }
        }
    }

    /// The Save dialog gave a path: the worker starts. `replace`: the
    /// dialog confirmed replacing an existing file. `wake` is called when
    /// it finishes.
    pub fn target_picked(
        &mut self,
        target: PathBuf,
        replace: bool,
        wake: impl FnOnce() + Send + 'static,
    ) {
        let Phase::AwaitTarget { source } = std::mem::take(&mut self.phase) else {
            return;
        };
        let target = with_extension(target);
        let (tx, rx) = channel();
        let src = source.clone();
        let spawned = std::thread::Builder::new()
            .name("fluut-post-import".into())
            .spawn(move || {
                let _ = tx.send(run(&src, &target, replace));
                wake();
            });
        match spawned {
            Ok(_) => self.phase = Phase::Running { source, rx },
            Err(e) => {
                let d = fail("import.worker", format!("cannot start the import: {e}"));
                self.phase = Phase::Done(Box::new(Done::error(source, PathBuf::new(), d)));
            }
        }
    }

    /// A dialog was cancelled: back to nothing.
    pub fn cancel(&mut self) {
        if self.awaiting_target() {
            self.phase = Phase::Idle;
        }
    }

    /// Takes the worker's result when it is ready.
    pub fn poll(&mut self) {
        let Phase::Running { rx, source } = &self.phase else {
            return;
        };
        match rx.try_recv() {
            Ok(d) => self.phase = Phase::Done(Box::new(d)),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                let d = fail("import.worker", "the import stopped unexpectedly".into());
                let s = source.clone();
                self.phase = Phase::Done(Box::new(Done::error(s, PathBuf::new(), d)));
            }
        }
    }

    /// The result, while its window is open.
    pub fn done(&self) -> Option<&Done> {
        match &self.phase {
            Phase::Done(d) => Some(d),
            _ => None,
        }
    }

    /// The project now uses the post of the open result.
    pub fn mark_used(&mut self) {
        if let Phase::Done(d) = &mut self.phase {
            d.used = true;
        }
    }

    pub fn close(&mut self) {
        if matches!(self.phase, Phase::Done(_)) {
            self.phase = Phase::Idle;
        }
    }

    /// The text shown while the worker runs.
    pub fn working_text(&self) -> Option<String> {
        match &self.phase {
            Phase::Running { source, .. } => {
                let f = source.file_name().map_or_else(
                    || source.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                Some(tr_args("posts-import-working", &[("file", &f)]))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
