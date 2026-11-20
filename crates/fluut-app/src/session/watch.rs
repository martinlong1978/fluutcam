// SPDX-License-Identifier: MIT OR Apache-2.0

//! Change detection for linked models in the window (spec 02 §2.6.3,
//! D13 §8): `fluut_core::watch` decides when to check and what changed;
//! this runs the check on a thread of its own, so hashing a large file
//! never holds up a frame or waits behind a calculation.
//!
//! A change only raises the notice "Linked model changed: Update". Nothing
//! is re-read or recalculated until the user updates.

use fluut_core::watch::{Changed, Due, LinkedFile, Schedule, Stamps};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// A linked file that changed, as the notice shows it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChangedLink {
    pub source: String,
    pub path: PathBuf,
    /// The file's hash now; `None` when it cannot be read.
    pub hash: Option<String>,
}

impl From<Changed> for ChangedLink {
    fn from(c: Changed) -> ChangedLink {
        ChangedLink {
            source: c.source,
            path: c.path,
            hash: c.hash,
        }
    }
}

type Answer = (Stamps, Vec<Changed>);

/// The window's watch on its linked files.
#[derive(Debug)]
pub struct LinkWatch {
    schedule: Schedule,
    /// `None` while a check runs on its thread.
    stamps: Option<Stamps>,
    rx: Option<Receiver<Answer>>,
    /// What the last check found.
    changed: Vec<ChangedLink>,
    /// `(source, hash)` the user put off with "Later": not shown again
    /// until the file changes once more.
    later: BTreeSet<(String, Option<String>)>,
    /// The files the last check started for: a project opened, or an
    /// update that stored a new hash, checks at once.
    files: Vec<LinkedFile>,
}

impl Default for LinkWatch {
    fn default() -> Self {
        LinkWatch {
            schedule: Schedule::new(),
            stamps: Some(Stamps::new()),
            rx: None,
            changed: Vec::new(),
            later: BTreeSet::new(),
            files: Vec::new(),
        }
    }
}

impl LinkWatch {
    /// Called every frame with the window's focus and the linked files of
    /// the open project. Starts a check when one is due and none runs, and
    /// takes the answer of a finished one. Returns how long until the next
    /// check, for the repaint timer.
    pub fn tick(
        &mut self,
        focused: bool,
        now: Instant,
        files: &[LinkedFile],
        wake: impl Fn() + Send + 'static,
    ) -> Option<Duration> {
        self.receive();
        if files.is_empty() {
            self.changed.clear();
        }
        let mut due = self.schedule.tick(focused, now);
        if focused && due.is_none() && files != self.files.as_slice() {
            due = Some(Due::Focus);
        }
        if let Some(due) = due
            && !files.is_empty()
            && self.start(files.to_vec(), due, wake)
        {
            self.files = files.to_vec();
        }
        self.schedule.wait(now).filter(|_| !files.is_empty())
    }

    /// Checks `files` at once, here, without a thread (tests).
    pub fn check_now(&mut self, files: &[LinkedFile], due: Due) {
        self.receive();
        if let Some(st) = self.stamps.as_mut() {
            self.changed = st.check(files, due).into_iter().map(Into::into).collect();
        }
    }

    /// Starts a check on its thread; `false` when one still runs.
    fn start(
        &mut self,
        files: Vec<LinkedFile>,
        due: Due,
        wake: impl Fn() + Send + 'static,
    ) -> bool {
        let Some(mut st) = self.stamps.take() else {
            return false;
        };
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name("fluut-link-watch".into())
            .spawn(move || {
                let changed = st.check(&files, due);
                let _ = tx.send((st, changed));
                wake();
            });
        match spawned {
            Ok(_) => {
                self.rx = Some(rx);
                true
            }
            Err(_) => {
                self.stamps = Some(Stamps::new());
                false
            }
        }
    }

    fn receive(&mut self) {
        let Some(rx) = &self.rx else {
            return;
        };
        match rx.try_recv() {
            Ok((st, changed)) => {
                self.stamps = Some(st);
                self.changed = changed.into_iter().map(Into::into).collect();
                self.rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.stamps = Some(Stamps::new());
                self.rx = None;
            }
        }
    }

    /// The changed files the notice shows: not put off, and still in the
    /// project.
    pub fn changed(&self, files: &[LinkedFile]) -> Vec<&ChangedLink> {
        self.changed
            .iter()
            .filter(|c| !self.later.contains(&(c.source.clone(), c.hash.clone())))
            .filter(|c| {
                files
                    .iter()
                    .any(|f| f.source == c.source && f.hash != c.hash)
            })
            .collect()
    }

    /// "Later": hides the notice until a file changes again.
    pub fn later(&mut self, files: &[LinkedFile]) {
        let now: Vec<(String, Option<String>)> = self
            .changed(files)
            .iter()
            .map(|c| (c.source.clone(), c.hash.clone()))
            .collect();
        self.later.extend(now);
    }

    /// After an update of `sources`: their notice goes until the next
    /// check finds a new change.
    pub fn updated(&mut self, sources: &[String]) {
        self.changed.retain(|c| !sources.contains(&c.source));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(dir: &std::path::Path, hash: &[u8]) -> LinkedFile {
        LinkedFile {
            source: "g1".into(),
            path: dir.join("m.craftx"),
            hash: Some(fluut_model::hash::sha256_hex(hash)),
        }
    }

    #[test]
    fn a_change_is_noticed_put_off_and_noticed_again_when_it_moves_on() {
        let d = std::env::temp_dir().join(format!("fluut-app-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("m.craftx"), b"one").unwrap();
        let mut w = LinkWatch::default();
        let files = [file(&d, b"one")];
        w.check_now(&files, Due::Focus);
        assert!(w.changed(&files).is_empty());
        std::fs::write(d.join("m.craftx"), b"two!").unwrap();
        w.check_now(&files, Due::Focus);
        assert_eq!(w.changed(&files).len(), 1);
        w.later(&files);
        assert!(w.changed(&files).is_empty());
        std::fs::write(d.join("m.craftx"), b"three").unwrap();
        w.check_now(&files, Due::Focus);
        assert_eq!(w.changed(&files).len(), 1);
        // Once the project holds the new hash, it is quiet.
        let files = [file(&d, b"three")];
        assert!(w.changed(&files).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_check_runs_on_its_own_thread_and_comes_back() {
        let d = std::env::temp_dir().join(format!("fluut-app-watch-t-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("m.craftx"), b"new").unwrap();
        let files = [file(&d, b"old")];
        let mut w = LinkWatch::default();
        let now = Instant::now();
        assert!(w.tick(true, now, &files, || {}).is_some());
        for _ in 0..400 {
            w.tick(true, now, &files, || {});
            if !w.changed(&files).is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(w.changed(&files).len(), 1);
        w.updated(&["g1".into()]);
        assert!(w.changed(&files).is_empty());
        // No linked files: nothing to wait for.
        assert_eq!(w.tick(true, now, &[], || {}), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
