// SPDX-License-Identifier: MIT OR Apache-2.0

//! Change detection for linked models (spec 02 §2.6.3, D06 §5.2): the
//! window checks each linked file's hash when it gains focus and every
//! [`INTERVAL`] while it has focus. No file watcher: polling one or two
//! files is cheap, works the same on every system and on network drives,
//! and needs no crate.
//!
//! [`Schedule`] says when a check is due; [`Stamps::changed`] does the
//! check. A file is hashed again only when its size or modification time
//! moved, or when the window has just gained focus. A change is only
//! reported; nothing is re-read or recalculated until the user updates.

use fluut_model::hash::sha256_hex;
use fluut_model::project::{Project, SourceKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How often a focused window checks its linked files.
pub const INTERVAL: Duration = Duration::from_secs(2);

/// Why a check is due.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Due {
    /// The window has just gained focus: every file is hashed.
    Focus,
    /// [`INTERVAL`] has passed with focus: files whose size or time moved
    /// are hashed.
    Interval,
}

/// When to check: on gaining focus, then every [`INTERVAL`] while focused.
#[derive(Clone, Debug, Default)]
pub struct Schedule {
    focused: bool,
    last: Option<Instant>,
}

impl Schedule {
    pub fn new() -> Schedule {
        Schedule::default()
    }

    /// Called every frame with the window's focus. A check is due when
    /// focus has just come back, or [`INTERVAL`] after the last one while
    /// the window has focus. A window without focus checks nothing.
    pub fn tick(&mut self, focused: bool, now: Instant) -> Option<Due> {
        let gained = focused && !self.focused;
        self.focused = focused;
        let due = if gained {
            Some(Due::Focus)
        } else if focused && self.last.is_none_or(|l| now.duration_since(l) >= INTERVAL) {
            Some(Due::Interval)
        } else {
            None
        };
        if due.is_some() {
            self.last = Some(now);
        }
        due
    }

    /// How long until the next check, while the window has focus.
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        if !self.focused {
            return None;
        }
        Some(
            self.last
                .map_or(Duration::ZERO, |l| INTERVAL.saturating_sub(now - l)),
        )
    }
}

/// A linked model's file and the hash the project holds for it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LinkedFile {
    /// The geometry source id.
    pub source: String,
    pub path: PathBuf,
    /// `sha256:<hex>` from the project, if it has one.
    pub hash: Option<String>,
}

/// The linked models of `project` (kind `craftx`, or a stored link), with
/// their files relative to `dir`.
pub fn linked_files(project: &Project, dir: &Path) -> Vec<LinkedFile> {
    project
        .geometry
        .iter()
        .filter(|g| g.kind == SourceKind::Craftx || g.link.is_some())
        .map(|g| LinkedFile {
            source: g.id.clone(),
            path: dir.join(&g.path),
            hash: g.hash.clone(),
        })
        .collect()
}

/// A linked file that no longer matches the project.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Changed {
    pub source: String,
    pub path: PathBuf,
    /// The file's hash now; `None` when it cannot be read (moved or
    /// deleted).
    pub hash: Option<String>,
}

#[derive(Clone, Debug)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    hash: Option<String>,
}

/// What was seen of each file at the last check, so an unchanged file is
/// not hashed again.
#[derive(Clone, Debug, Default)]
pub struct Stamps {
    seen: BTreeMap<PathBuf, Stamp>,
}

impl Stamps {
    pub fn new() -> Stamps {
        Stamps::default()
    }

    /// The hash of `path` now: from the last check when its size and time
    /// have not moved and `force` is off, else read and hashed. `None`
    /// when the file cannot be read.
    fn hash(&mut self, path: &Path, force: bool) -> Option<String> {
        let Ok(meta) = std::fs::metadata(path) else {
            self.seen.remove(path);
            return None;
        };
        let (len, modified) = (meta.len(), meta.modified().ok());
        if !force
            && let Some(s) = self.seen.get(path)
            && s.len == len
            && s.modified == modified
            && modified.is_some()
        {
            return s.hash.clone();
        }
        let hash = std::fs::read(path).ok().map(|b| sha256_hex(&b));
        self.seen.insert(
            path.to_path_buf(),
            Stamp {
                len,
                modified,
                hash: hash.clone(),
            },
        );
        hash
    }

    /// The files of `files` whose hash differs from the project's. `force`
    /// hashes every file (the window has just gained focus).
    pub fn changed(&mut self, files: &[LinkedFile], force: bool) -> Vec<Changed> {
        let mut out = Vec::new();
        for f in files {
            let now = self.hash(&f.path, force);
            if now != f.hash {
                out.push(Changed {
                    source: f.source.clone(),
                    path: f.path.clone(),
                    hash: now,
                });
            }
        }
        out
    }

    /// Checks `files` for a due check: everything on [`Due::Focus`].
    pub fn check(&mut self, files: &[LinkedFile], due: Due) -> Vec<Changed> {
        self.changed(files, due == Due::Focus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_is_due_on_focus_and_every_two_seconds_with_focus() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut s = Schedule::new();
        // No focus: nothing.
        assert_eq!(s.tick(false, at(0)), None);
        assert_eq!(s.wait(at(0)), None);
        // Gaining focus checks at once.
        assert_eq!(s.tick(true, at(100)), Some(Due::Focus));
        assert_eq!(s.tick(true, at(1000)), None);
        assert_eq!(s.wait(at(1000)), Some(Duration::from_millis(1100)));
        assert_eq!(s.tick(true, at(2100)), Some(Due::Interval));
        assert_eq!(s.tick(true, at(3000)), None);
        // Losing focus stops the checks; coming back checks again.
        assert_eq!(s.tick(false, at(9000)), None);
        assert_eq!(s.tick(false, at(12000)), None);
        assert_eq!(s.tick(true, at(12500)), Some(Due::Focus));
        assert_eq!(s.tick(true, at(13000)), None);
    }

    #[test]
    fn a_changed_or_missing_file_is_reported_and_an_unchanged_one_not() {
        let d = std::env::temp_dir().join(format!("fluut-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("m.craftx");
        std::fs::write(&path, b"one").unwrap();
        let file = |h: &[u8]| LinkedFile {
            source: "g1".into(),
            path: path.clone(),
            hash: Some(sha256_hex(h)),
        };
        let mut st = Stamps::new();
        assert!(st.check(&[file(b"one")], Due::Focus).is_empty());
        assert!(st.check(&[file(b"one")], Due::Interval).is_empty());
        std::fs::write(&path, b"two, longer").unwrap();
        let c = st.check(&[file(b"one")], Due::Interval);
        assert_eq!(c.len(), 1);
        assert_eq!(
            c[0].hash.as_deref(),
            Some(sha256_hex(b"two, longer").as_str())
        );
        // After an update the project holds the new hash: quiet again.
        assert!(st.check(&[file(b"two, longer")], Due::Interval).is_empty());
        std::fs::remove_file(&path).unwrap();
        let c = st.check(&[file(b"two, longer")], Due::Interval);
        assert_eq!(c[0].hash, None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn only_linked_sources_are_watched() {
        let p = fluut_model::project::read(
            r#"format = "fluut.project/1.2"
name = "x"
units = "mm"

[[geometry]]
id = "g1"
path = "a.step"
kind = "step"

[[geometry]]
id = "g2"
path = "b.craftx"
kind = "craftx"
hash = "sha256:00"
source_id = "d"
revision = "1"
"#,
        )
        .unwrap()
        .project;
        let f = linked_files(&p, Path::new("dir"));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].source, "g2");
        assert_eq!(f[0].path, Path::new("dir").join("b.craftx"));
    }
}
