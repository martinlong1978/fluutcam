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
}

/// A path the user chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picked {
    pub kind: DialogKind,
    pub path: PathBuf,
}

/// Model file extensions the import dialog offers.
pub const MODEL_EXTENSIONS: [&str; 5] = ["step", "stp", "stl", "dxf", "svg"];

pub struct Dialogs {
    tx: Sender<Option<Picked>>,
    rx: Receiver<Option<Picked>>,
    wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    open: bool,
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
        }
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
        };
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let spawned = std::thread::Builder::new()
            .name("fluut-dialog".into())
            .spawn(move || {
                let handle = pollster::block_on(async {
                    match kind {
                        DialogKind::Open | DialogKind::Import => d.pick_file().await,
                        DialogKind::New(_) | DialogKind::SaveAs => d.save_file().await,
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
    fn model_filter_covers_the_importers() {
        for e in ["step", "stp", "stl", "dxf", "svg"] {
            assert!(MODEL_EXTENSIONS.contains(&e));
        }
    }
}
