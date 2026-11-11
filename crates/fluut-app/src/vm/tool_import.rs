// SPDX-License-Identifier: MIT OR Apache-2.0

//! The tool library import (design D16 §10.2): which file, where to, the
//! import itself, and what the result window shows. No `egui` here.
//!
//! The steps are two native dialogs (the Fusion file, then the new library
//! file) and an import on a worker thread. An import always writes a new
//! file: it replaces an existing one only when the Save dialog has already
//! asked the user to confirm that, and it never replaces its own input.

use crate::text::{diagnostic, tr, tr_args};
use fluut_model::{Diagnostic, Severity};
use fluut_post_import::{ImportOptions, ImportReport, fusion, limits, slug, valid_library_id};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// The input kinds the import knows by extension.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Format {
    FusionJson,
}

/// The format of `source`, from its extension; else the
/// `import.format.unsupported` error (a Vectric `.vtdb` is answered with
/// its own text).
pub fn format_of(source: &Path) -> Result<Format, Diagnostic> {
    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match ext.as_deref() {
        Some("json") => Ok(Format::FusionJson),
        Some("vtdb") => Err(Diagnostic::error(
            "import.format.unsupported",
            "Vectric .vtdb tool databases cannot be imported yet (design D16 §7)",
        )),
        _ => Err(Diagnostic::error(
            "import.format.unsupported",
            format!(
                "{}: expected a Fusion tool library (.json)",
                source.display()
            ),
        )),
    }
}

/// The file name the Save dialog offers: the input's name with `.toml`.
pub fn default_name(source: &Path) -> String {
    let stem = source
        .file_stem()
        .map_or_else(|| "tools".to_owned(), |s| s.to_string_lossy().into_owned());
    format!("{stem}.toml")
}

/// Where the Save dialog starts: the project's `tools` folder if there is
/// one, else the project's folder.
pub fn start_dir(project: Option<&Path>) -> Option<PathBuf> {
    let dir = project?.parent()?;
    let tools = dir.join("tools");
    Some(if tools.is_dir() {
        tools
    } else {
        dir.to_path_buf()
    })
}

/// The target as written: `.toml` is added when the name has no extension.
pub fn with_extension(mut target: PathBuf) -> PathBuf {
    if target.extension().is_none() {
        target.set_extension("toml");
    }
    target
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Whether the library at `target` is loaded with the project at
/// `project`: a `.toml` file directly in the `tools` folder beside it.
pub fn loads_with_project(project: &Path, target: &Path) -> bool {
    let Some(dir) = project.parent() else {
        return false;
    };
    target.extension().is_some_and(|e| e == "toml")
        && target
            .parent()
            .is_some_and(|p| same_path(p, &dir.join("tools")))
}

/// What a finished import shows.
#[derive(Clone, PartialEq, Debug)]
pub struct Done {
    pub source: PathBuf,
    pub target: PathBuf,
    pub library_id: String,
    /// `Some` when the file was written.
    pub report: Option<ImportReport>,
    /// Warnings, or the errors that stopped the import.
    pub diagnostics: Vec<Diagnostic>,
    /// The project has been told to use the library ("Use in this
    /// project").
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
            library_id: String::new(),
            report: None,
            diagnostics: d,
            used: false,
        }
    }

    /// The headline: where the file went, or that nothing was imported.
    pub fn headline(&self) -> String {
        match &self.report {
            Some(r) => tr_args(
                "tools-import-done",
                &[
                    ("written", &r.tools_written.to_string()),
                    ("read", &r.tools_read.to_string()),
                    ("path", &self.target.display().to_string()),
                ],
            ),
            None => tr("tools-import-failed"),
        }
    }

    /// The counts, one line each (library, read, written, skipped, dropped
    /// keys).
    pub fn counts(&self) -> Vec<String> {
        let Some(r) = &self.report else {
            return Vec::new();
        };
        let n = |id: &str, v: usize| tr_args(id, &[("n", &v.to_string())]);
        let mut v = vec![
            tr_args("tools-import-library", &[("id", &self.library_id)]),
            n("tools-import-read", r.tools_read),
            n("tools-import-written", r.tools_written),
            n("tools-import-skipped", r.tools_skipped),
        ];
        if !r.dropped_keys.is_empty() {
            let total: usize = r.dropped_keys.values().sum();
            v.push(tr_args(
                "tools-import-dropped",
                &[
                    ("keys", &r.dropped_keys.len().to_string()),
                    ("total", &total.to_string()),
                ],
            ));
        }
        v
    }

    /// The source keys that were not used, with their counts.
    pub fn dropped(&self) -> Vec<(String, usize)> {
        self.report.as_ref().map_or_else(Vec::new, |r| {
            r.dropped_keys
                .iter()
                .map(|(k, n)| (k.clone(), *n))
                .collect()
        })
    }

    /// The warnings or errors, as `(is error, code, text)`.
    pub fn messages(&self) -> Vec<(bool, String, String)> {
        self.diagnostics
            .iter()
            .map(|d| (d.severity == Severity::Error, d.code.clone(), diagnostic(d)))
            .collect()
    }
}

/// Reads at most the size limit plus one byte, so a huge file is refused
/// without being read whole.
fn read_limited(p: &Path) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    std::fs::File::open(p)?
        .take(limits::MAX_FILE_BYTES + 1)
        .read_to_end(&mut buf)?;
    Ok(buf)
}

fn fail(code: &str, msg: String) -> Vec<Diagnostic> {
    vec![Diagnostic::error(code, msg)]
}

/// Reads `source`, converts it and writes the library to `target`.
/// `replace`: the Save dialog has already asked about an existing target.
/// Without it the file is created only if it is new.
pub fn run(source: &Path, target: &Path, replace: bool) -> Done {
    let (s, t) = (source.to_path_buf(), target.to_path_buf());
    if let Err(d) = format_of(source) {
        return Done::error(s, t, vec![d]);
    }
    if same_path(source, target) {
        let msg = format!(
            "{}: the library cannot replace its own source",
            target.display()
        );
        return Done::error(s, t, fail("import.output.is_input", msg));
    }
    let stem = source
        .file_stem()
        .map(|x| x.to_string_lossy().into_owned())
        .unwrap_or_default();
    let id = format!("fusion/{}", slug(&stem, 40, "tools"));
    if !valid_library_id(&id) {
        let msg = format!("`{id}` is not a library id");
        return Done::error(s, t, fail("import.library.bad_id", msg));
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
    let opts = ImportOptions {
        library_name: stem,
        library_id: id.clone(),
    };
    let r = match fusion::import(&bytes, &opts) {
        Ok(r) => r,
        Err(d) => return Done::error(s, t, d),
    };
    let written = if replace {
        std::fs::write(target, &r.library)
    } else {
        // Atomic: never replaces a file that appeared meanwhile.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .and_then(|mut f| std::io::Write::write_all(&mut f, r.library.as_bytes()))
    };
    if let Err(e) = written {
        let msg = format!("{}: {e}", target.display());
        return Done::error(s, t, fail("import.output.unwritable", msg));
    }
    Done {
        source: s,
        target: t,
        library_id: id,
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
    /// The Fusion file is chosen; the Save dialog is open.
    AwaitTarget { source: PathBuf },
    /// The worker is converting.
    Running { source: PathBuf, rx: Receiver<Done> },
    /// The result window is open.
    Done(Box<Done>),
}

#[derive(Debug, Default)]
pub struct ToolImport {
    pub phase: Phase,
}

impl ToolImport {
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

    /// The Fusion file was chosen. Returns the name for the Save dialog, or
    /// shows the error for a format that cannot be imported.
    pub fn source_picked(&mut self, source: PathBuf) -> Option<String> {
        match format_of(&source) {
            Ok(_) => {
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
            .name("fluut-tool-import".into())
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

    /// The project now uses the library of the open result.
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
                Some(tr_args("tools-import-working", &[("file", &f)]))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
