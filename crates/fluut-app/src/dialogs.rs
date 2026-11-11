// SPDX-License-Identifier: MIT OR Apache-2.0

//! Native file dialogs (plan 2026-11-04-app-project-ops, decision 9).
//!
//! `rfd` shows the system dialog: Win32 on Windows, Cocoa on macOS, the
//! xdg-desktop-portal on Linux. A dialog runs on its own thread, so the frame
//! loop never waits for it; the chosen path comes back through a channel
//! that the frame polls, and the window is woken when the dialog closes.
//! One dialog is open at a time.

use crate::text::tr;
use fluut_model::project::DisplayUnit;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

/// Which dialog to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    Open,
    /// A new project in the unit.
    New(DisplayUnit),
    SaveAs,
    Import,
    /// Where Post writes its first file.
    PostOutput,
    /// The tool library file to import (Fusion JSON).
    ImportToolsSource,
    /// Where the imported tool library is written.
    ImportToolsTarget,
    /// A tool library file (TOML) for the project to refer to.
    AddToolLibrary,
    /// The Vectric post processor (`.pp`) to import.
    ImportPostSource,
    /// Where the imported post is written.
    ImportPostTarget,
}

/// A path the user chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picked {
    pub kind: DialogKind,
    pub path: PathBuf,
}

/// Model file extensions the import dialog offers.
pub const MODEL_EXTENSIONS: [&str; 5] = ["step", "stp", "stl", "dxf", "svg"];

/// A dialog that was asked for: what tests look at when dialogs are
/// scripted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    pub kind: DialogKind,
    /// The folder it would start in.
    pub dir: Option<PathBuf>,
    /// The file name it would offer.
    pub name: String,
}

/// Dialogs that answer at once from a list, so tests open no window.
#[derive(Debug, Default)]
struct Script {
    /// Whether a save dialog counts as having confirmed an overwrite.
    confirms_overwrite: bool,
    answers: Vec<(DialogKind, PathBuf)>,
    asked: Vec<Asked>,
}

pub struct Dialogs {
    tx: Sender<Option<Picked>>,
    rx: Receiver<Option<Picked>>,
    wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    open: bool,
    script: Option<Script>,
}

impl std::fmt::Debug for Dialogs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dialogs")
            .field("open", &self.open)
            .finish_non_exhaustive()
    }
}

impl Dialogs {
    /// `wake` is called when a dialog closes, so the window repaints.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Dialogs {
        let (tx, rx) = channel();
        Dialogs {
            tx,
            rx,
            wake: std::sync::Arc::new(wake),
            open: false,
            script: None,
        }
    }

    /// Dialogs for tests: none opens; a request is recorded
    /// ([`Dialogs::asked`]) and answered with the path given to
    /// [`Dialogs::answer`] for its kind, else cancelled.
    pub fn scripted() -> Dialogs {
        let mut d = Dialogs::new(|| {});
        d.script = Some(Script::default());
        d
    }

    /// Whether the dialogs are scripted.
    pub fn is_scripted(&self) -> bool {
        self.script.is_some()
    }

    /// Whether a save dialog has already asked the user to confirm
    /// replacing an existing file. The system dialogs do; scripted ones
    /// say so only when told ([`Dialogs::script_confirms_overwrite`]).
    pub fn confirms_overwrite(&self) -> bool {
        self.script.as_ref().is_none_or(|s| s.confirms_overwrite)
    }

    /// Scripted dialogs: whether a chosen existing file counts as
    /// confirmed for replacing.
    pub fn script_confirms_overwrite(&mut self, yes: bool) {
        if let Some(s) = self.script.as_mut() {
            s.confirms_overwrite = yes;
        }
    }

    /// Scripted dialogs: the next request of `kind` is answered with `path`.
    pub fn answer(&mut self, kind: DialogKind, path: impl Into<PathBuf>) {
        if let Some(s) = self.script.as_mut() {
            s.answers.push((kind, path.into()));
        }
    }

    /// Scripted dialogs: the requests so far, oldest first.
    pub fn asked(&self) -> &[Asked] {
        self.script.as_ref().map_or(&[], |s| &s.asked)
    }

    /// Whether a dialog is showing.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Shows the dialog for `kind`, starting in `dir` with the file name
    /// `name` (save dialogs). Ignored while a dialog is open.
    pub fn request(&mut self, kind: DialogKind, dir: Option<PathBuf>, name: &str) {
        if self.open {
            return;
        }
        self.open = true;
        if let Some(script) = self.script.as_mut() {
            script.asked.push(Asked {
                kind,
                dir,
                name: name.to_owned(),
            });
            let at = script.answers.iter().position(|(k, _)| *k == kind);
            let picked = at.map(|i| Picked {
                kind,
                path: script.answers.remove(i).1,
            });
            let _ = self.tx.send(picked);
            return;
        }
        let mut d = rfd::AsyncFileDialog::new().set_file_name(name);
        if let Some(dir) = dir.filter(|d| d.is_dir()) {
            d = d.set_directory(dir);
        }
        // Text comes from the catalog on the UI thread (the language is
        // per thread).
        d = match kind {
            DialogKind::Open => d
                .set_title(tr("dialog-open-title"))
                .add_filter(tr("dialog-filter-project"), &["fluut"]),
            DialogKind::New(_) => d
                .set_title(tr("dialog-new-title"))
                .add_filter(tr("dialog-filter-project"), &["fluut"]),
            DialogKind::SaveAs => d
                .set_title(tr("dialog-save-as-title"))
                .add_filter(tr("dialog-filter-project"), &["fluut"]),
            DialogKind::Import => d
                .set_title(tr("dialog-import-title"))
                .add_filter(tr("dialog-filter-model"), &MODEL_EXTENSIONS),
            DialogKind::PostOutput => d.set_title(tr("dialog-post-title")),
            DialogKind::ImportToolsSource => d
                .set_title(tr("dialog-tools-source-title"))
                .add_filter(tr("dialog-filter-tools-fusion"), &["json"])
                .add_filter(tr("dialog-filter-tools-vectric"), &["vtdb"]),
            DialogKind::ImportToolsTarget => d
                .set_title(tr("dialog-tools-target-title"))
                .add_filter(tr("dialog-filter-tools-library"), &["toml"]),
            DialogKind::AddToolLibrary => d
                .set_title(tr("dialog-tools-add-title"))
                .add_filter(tr("dialog-filter-tools-library"), &["toml"]),
            DialogKind::ImportPostSource => d
                .set_title(tr("dialog-post-source-title"))
                .add_filter(tr("dialog-filter-post-vectric"), &["pp"]),
            DialogKind::ImportPostTarget => d
                .set_title(tr("dialog-post-target-title"))
                .add_filter(tr("dialog-filter-post-library"), &["toml"]),
        };
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let spawned = std::thread::Builder::new()
            .name("fluut-dialog".into())
            .spawn(move || {
                let handle = pollster::block_on(async {
                    match kind {
                        DialogKind::Open
                        | DialogKind::Import
                        | DialogKind::ImportToolsSource
                        | DialogKind::AddToolLibrary
                        | DialogKind::ImportPostSource => d.pick_file().await,
                        DialogKind::New(_)
                        | DialogKind::SaveAs
                        | DialogKind::PostOutput
                        | DialogKind::ImportToolsTarget
                        | DialogKind::ImportPostTarget => d.save_file().await,
                    }
                });
                let picked = handle.map(|h| Picked {
                    kind,
                    path: h.path().to_path_buf(),
                });
                let _ = tx.send(picked);
                wake();
            });
        if spawned.is_err() {
            self.open = false;
        }
    }

    /// The path chosen since the last call, if a dialog closed with one.
    pub fn poll(&mut self) -> Option<Picked> {
        match self.rx.try_recv() {
            Ok(r) => {
                self.open = false;
                r
            }
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_picked_until_a_dialog_closes() {
        let mut d = Dialogs::new(|| {});
        assert!(!d.is_open() && d.poll().is_none());
        // A closed dialog frees the slot.
        d.open = true;
        d.tx.send(None).unwrap();
        assert!(d.poll().is_none() && !d.is_open());
        d.tx.send(Some(Picked {
            kind: DialogKind::Open,
            path: "a.fluut".into(),
        }))
        .unwrap();
        assert_eq!(d.poll().unwrap().path, PathBuf::from("a.fluut"));
    }

    #[test]
    fn scripted_dialogs_answer_at_once_and_remember_what_was_asked() {
        let mut d = Dialogs::scripted();
        d.answer(DialogKind::Import, "a.step");
        d.request(DialogKind::Import, Some("dir".into()), "x");
        assert!(d.is_open());
        let p = d.poll().unwrap();
        assert_eq!((p.kind, p.path), (DialogKind::Import, "a.step".into()));
        assert!(!d.is_open());
        // No answer: the user cancelled.
        d.request(DialogKind::PostOutput, None, "out.ngc");
        assert!(d.poll().is_none() && !d.is_open());
        assert_eq!(d.asked().len(), 2);
        assert_eq!(d.asked()[1].name, "out.ngc");
    }

    #[test]
    fn model_filter_covers_the_importers() {
        for e in ["step", "stp", "stl", "dxf", "svg"] {
            assert!(MODEL_EXTENSIONS.contains(&e));
        }
    }
}
