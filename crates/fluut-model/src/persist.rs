// SPDX-License-Identifier: MIT OR Apache-2.0

//! Saving and loading `.fluut` files (D06 §7.1–§7.2).
//!
//! A save writes `name.fluut.tmp` beside the project, syncs it, moves the
//! current file to `name.fluut.bak`, then moves the temporary file into
//! place. A crash leaves either the old file or the new one, never half of
//! one.

use crate::project::{Loaded, Project, read, write};
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// `name.fluut.bak`.
pub fn backup_path(path: &Path) -> PathBuf {
    with_suffix(path, ".bak")
}

/// The result of a save: the file bytes (their hash heads the journal).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Saved {
    pub bytes: Vec<u8>,
}

/// Writes `p` to `path` atomically, keeping the previous file as `.bak`.
pub fn save(path: &Path, p: &Project) -> io::Result<Saved> {
    let text = write(p);
    let tmp = with_suffix(path, ".tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    if path.exists() {
        let bak = backup_path(path);
        // Windows does not rename over an existing file.
        if bak.exists() {
            std::fs::remove_file(&bak)?;
        }
        std::fs::rename(path, &bak)?;
    }
    std::fs::rename(&tmp, path)?;
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        // Make the renames durable; not possible on every file system.
        let _ = File::open(dir).and_then(|d| d.sync_all());
    }
    Ok(Saved {
        bytes: text.into_bytes(),
    })
}

/// A file that could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("cannot read {0}: {1}")]
    Io(PathBuf, io::Error),
    #[error("{0}: {1}")]
    Load(PathBuf, crate::project::LoadError),
}

/// Reads a project file; also returns its bytes for the journal hash.
pub fn load(path: &Path) -> Result<(Loaded, Vec<u8>), OpenError> {
    let bytes = std::fs::read(path).map_err(|e| OpenError::Io(path.to_owned(), e))?;
    let text = String::from_utf8_lossy(&bytes);
    let l = read(&text).map_err(|e| OpenError::Load(path.to_owned(), e))?;
    Ok((l, bytes))
}
