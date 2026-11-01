// SPDX-License-Identifier: MIT OR Apache-2.0

//! The edit journal, `name.fluut-autosave` (D06 §7.3).
//!
//! ```text
//! header:  "FLUJ" | u16 version (LE) | SHA-256 of the project file (32 bytes)
//! records: u32 length (LE) | postcard(Record) | u32 crc32 of the payload (LE)
//! ```
//!
//! A record is appended after every apply, undo and redo. Recovery replays
//! the records up to the first that is cut off or fails its check, onto
//! the project file the header names.

use crate::edit::Edit;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8; 4] = b"FLUJ";
pub const VERSION: u16 = 1;
const HEADER_LEN: usize = 4 + 2 + 32;
/// Larger records are treated as damage.
const MAX_RECORD: usize = 64 << 20;
/// At most one `fsync` per second (D06 §7.3).
pub const SYNC_INTERVAL_MS: u64 = 1000;

/// One journal entry.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Record {
    Edit(Edit),
    Undo,
    Redo,
}

/// SHA-256 of a project file's bytes, as the journal header holds it.
pub fn file_hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// The journal path of a project file: `name.fluut-autosave`.
pub fn journal_path(project: &Path) -> PathBuf {
    let mut s = project.as_os_str().to_owned();
    s.push("-autosave");
    PathBuf::from(s)
}

fn header(hash: &[u8; 32]) -> Vec<u8> {
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(MAGIC);
    h.extend_from_slice(&VERSION.to_le_bytes());
    h.extend_from_slice(hash);
    h
}

/// Encodes one record with its length and check.
pub fn encode(r: &Record) -> Vec<u8> {
    let payload = postcard::to_allocvec(r).expect("records always serialize");
    let mut out = Vec::with_capacity(payload.len() + 8);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    out.extend_from_slice(&crc32fast::hash(&payload).to_le_bytes());
    out
}

/// The records of a journal for the project file with `hash`: `None` when
/// the header is damaged or names another file, else every record up to
/// the first damaged one.
pub fn decode(bytes: &[u8], hash: &[u8; 32]) -> Option<Vec<Record>> {
    if bytes.len() < HEADER_LEN || bytes[..HEADER_LEN] != header(hash)[..] {
        return None;
    }
    let mut out = Vec::new();
    let mut at = HEADER_LEN;
    while let Some(len) = bytes.get(at..at + 4) {
        let n = u32::from_le_bytes(len.try_into().ok()?) as usize;
        if n > MAX_RECORD {
            break;
        }
        let (Some(payload), Some(crc)) = (
            bytes.get(at + 4..at + 4 + n),
            bytes.get(at + 4 + n..at + 8 + n),
        ) else {
            break;
        };
        if crc32fast::hash(payload).to_le_bytes() != crc {
            break;
        }
        let Ok(r) = postcard::from_bytes::<Record>(payload) else {
            break;
        };
        out.push(r);
        at += 8 + n;
    }
    Some(out)
}

/// The records to replay onto the project file with `hash`, if a journal
/// for it exists at `path` and holds any.
pub fn recover(path: &Path, hash: &[u8; 32]) -> io::Result<Option<Vec<Record>>> {
    let mut bytes = Vec::new();
    match File::open(path) {
        Ok(mut f) => f.read_to_end(&mut bytes)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    Ok(decode(&bytes, hash).filter(|r| !r.is_empty()))
}

/// An open journal file.
#[derive(Debug)]
pub struct Journal {
    file: File,
    path: PathBuf,
    /// Records written since the last sync.
    dirty: bool,
    last_sync_ms: Option<u64>,
}

impl Journal {
    /// Starts a new journal for the project file with `hash`, replacing
    /// any old one.
    pub fn create(path: &Path, hash: &[u8; 32]) -> io::Result<Journal> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        file.write_all(&header(hash))?;
        file.sync_all()?;
        Ok(Journal {
            file,
            path: path.to_owned(),
            dirty: false,
            last_sync_ms: None,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one record (not yet synced).
    pub fn append(&mut self, r: &Record) -> io::Result<()> {
        self.file.write_all(&encode(r))?;
        self.dirty = true;
        Ok(())
    }

    /// Syncs if records were written and the last sync is at least
    /// [`SYNC_INTERVAL_MS`] old. Returns whether it synced.
    pub fn sync_if_due(&mut self, now_ms: u64) -> io::Result<bool> {
        let due = self
            .last_sync_ms
            .is_none_or(|t| now_ms.saturating_sub(t) >= SYNC_INTERVAL_MS);
        if !self.dirty || !due {
            return Ok(false);
        }
        self.sync()?;
        self.last_sync_ms = Some(now_ms);
        Ok(true)
    }

    pub fn sync(&mut self) -> io::Result<()> {
        self.file.sync_data()?;
        self.dirty = false;
        Ok(())
    }

    /// Empties the journal after a save, for the new file `hash`.
    pub fn reset(&mut self, hash: &[u8; 32]) -> io::Result<()> {
        *self = Journal::create(&self.path.clone(), hash)?;
        Ok(())
    }
}
