// SPDX-License-Identifier: MIT OR Apache-2.0

//! The open document: the project root and its undo history (D06 §6.2).
//!
//! An undo entry holds the root from before the edit. Setups are shared
//! `Arc`s, so an entry costs only the Setups the edit changed. The history
//! is unlimited by count and capped by an estimate of those unshared
//! Setups' size (256 MB by default); the oldest entries go first.
//!
//! A group (a script run, a multi-step UI action) makes many edits one
//! step: [`Document::begin_group`] and [`Document::end_group`].

use crate::edit::{Changed, Edit, EditError, apply};
use crate::journal::{Journal, Record};
use crate::project::{Project, Setup, write_setup};
use std::collections::VecDeque;
use std::sync::Arc;

/// Default cap on the undo history (spec 08 §8.8).
pub const UNDO_CAP_BYTES: usize = 256 << 20;

/// Edits to the same field within this time merge into one undo step.
pub const COALESCE_MS: u64 = 750;

#[derive(Clone, Debug)]
struct Entry {
    label: String,
    /// The root on the other side of this step.
    root: Arc<Project>,
    changed: Changed,
    bytes: usize,
    /// `(target, key, time)` of a field edit, for coalescing.
    coalesce: Option<(String, String, u64)>,
}

/// What an apply, undo or redo changed.
#[derive(Clone, PartialEq, Debug)]
pub struct Outcome {
    pub label: String,
    pub changed: Changed,
}

/// An open group: the root before it and what its edits changed.
#[derive(Clone, Debug)]
struct OpenGroup {
    label: String,
    depth: usize,
    start: Arc<Project>,
    changed: Changed,
}

#[derive(Debug)]
pub struct Document {
    root: Arc<Project>,
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
    undo_bytes: usize,
    cap: usize,
    journal: Option<Journal>,
    group: Option<OpenGroup>,
}

/// Rough size of the Setups of `old` that `new` does not share.
fn unshared_bytes(old: &Project, new: &Project) -> usize {
    let mut n = 64;
    for s in &old.setups {
        if !new.setups.iter().any(|t| Arc::ptr_eq(s, t)) {
            n += setup_bytes(s);
        }
    }
    n
}

fn setup_bytes(s: &Setup) -> usize {
    let mut text = String::new();
    write_setup(&mut text, s);
    text.len() * 4
}

impl Document {
    pub fn new(project: Project) -> Document {
        Document {
            root: Arc::new(project),
            undo: VecDeque::new(),
            redo: Vec::new(),
            undo_bytes: 0,
            cap: UNDO_CAP_BYTES,
            journal: None,
            group: None,
        }
    }

    /// Records every later apply, undo and redo in `journal`.
    pub fn with_journal(mut self, journal: Journal) -> Document {
        self.journal = Some(journal);
        self
    }

    /// Ends the document, handing back its journal.
    pub fn into_journal(self) -> Option<Journal> {
        self.journal
    }

    pub fn journal_mut(&mut self) -> Option<&mut Journal> {
        self.journal.as_mut()
    }

    /// Sets the undo cap in bytes (tests use small caps).
    pub fn set_cap(&mut self, bytes: usize) {
        self.cap = bytes;
        self.trim();
    }

    pub fn project(&self) -> &Project {
        &self.root
    }

    pub fn root(&self) -> Arc<Project> {
        self.root.clone()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Labels of the undo steps, newest first.
    pub fn undo_labels(&self) -> impl Iterator<Item = &str> {
        self.undo.iter().rev().map(|e| e.label.as_str())
    }

    /// Applies an edit as a new undo step.
    pub fn apply(&mut self, edit: Edit) -> Result<Outcome, EditError> {
        self.apply_inner(edit, None)
    }

    /// Applies an edit made at `now_ms`. A field edit on the same target
    /// and key as the previous step, within [`COALESCE_MS`], joins that
    /// step (scrubbing a value gives one undo step).
    pub fn apply_at(&mut self, edit: Edit, now_ms: u64) -> Result<Outcome, EditError> {
        self.apply_inner(edit, Some(now_ms))
    }

    /// Opens a group: the edits until the matching [`Document::end_group`]
    /// become one undo step labelled `label`. Groups nest; only the
    /// outermost counts.
    pub fn begin_group(&mut self, label: &str) {
        match &mut self.group {
            Some(g) => g.depth += 1,
            None => {
                self.group = Some(OpenGroup {
                    label: label.to_owned(),
                    depth: 1,
                    start: self.root.clone(),
                    changed: Changed::new(),
                });
            }
        }
    }

    pub fn in_group(&self) -> bool {
        self.group.is_some()
    }

    /// Closes the innermost group. Closing the outermost one adds its undo
    /// step and returns it; `None` when a group stays open or the group
    /// changed nothing.
    pub fn end_group(&mut self) -> Option<Outcome> {
        let g = self.group.as_mut()?;
        g.depth -= 1;
        if g.depth > 0 {
            return None;
        }
        let g = self.group.take()?;
        if Arc::ptr_eq(&g.start, &self.root) {
            return None;
        }
        let bytes = unshared_bytes(&g.start, &self.root);
        self.undo_bytes += bytes;
        self.undo.push_back(Entry {
            label: g.label.clone(),
            root: g.start,
            changed: g.changed.clone(),
            bytes,
            coalesce: None,
        });
        self.trim();
        Some(Outcome {
            label: g.label,
            changed: g.changed,
        })
    }

    /// Closes every open level of a group.
    fn close_groups(&mut self) {
        if let Some(g) = &mut self.group {
            g.depth = 1;
        }
        self.end_group();
    }

    fn apply_inner(&mut self, edit: Edit, now: Option<u64>) -> Result<Outcome, EditError> {
        let (next, changed) = apply(&self.root, &edit)?;
        if let Some(j) = &mut self.journal {
            j.append(&Record::Edit(edit.clone()))
                .map_err(|e| EditError(format!("journal: {e}")))?;
        }
        let next = Arc::new(next);
        let label = edit.label();
        if let Some(g) = &mut self.group {
            g.changed.extend(changed.iter().cloned());
            self.root = next;
            self.redo.clear();
            return Ok(Outcome { label, changed });
        }
        let key = match (edit.coalesce_key(), now) {
            (Some((t, k)), Some(ms)) => Some((t.to_owned(), k.to_owned(), ms)),
            _ => None,
        };
        let joins = match (&key, self.undo.back()) {
            (
                Some((t, k, ms)),
                Some(Entry {
                    coalesce: Some((t0, k0, ms0)),
                    ..
                }),
            ) => t == t0 && k == k0 && ms.saturating_sub(*ms0) <= COALESCE_MS,
            _ => false,
        };
        let old = std::mem::replace(&mut self.root, next);
        self.redo.clear();
        if joins {
            let top = self.undo.back_mut().expect("checked above");
            top.coalesce = key;
            top.changed.extend(changed.iter().cloned());
        } else {
            let bytes = unshared_bytes(&old, &self.root);
            self.undo_bytes += bytes;
            self.undo.push_back(Entry {
                label: label.clone(),
                root: old,
                changed: changed.clone(),
                bytes,
                coalesce: key,
            });
            self.trim();
        }
        Ok(Outcome { label, changed })
    }

    fn trim(&mut self) {
        while self.undo_bytes > self.cap && self.undo.len() > 1 {
            if let Some(e) = self.undo.pop_front() {
                self.undo_bytes -= e.bytes;
            }
        }
    }

    /// Steps back; `None` when there is nothing to undo. An open group is
    /// closed first.
    pub fn undo(&mut self) -> Option<Outcome> {
        self.close_groups();
        let mut e = self.undo.pop_back()?;
        self.undo_bytes -= e.bytes;
        if let Some(j) = &mut self.journal {
            // A failed journal write must not lose the step: the journal
            // then simply ends earlier.
            let _ = j.append(&Record::Undo);
        }
        std::mem::swap(&mut self.root, &mut e.root);
        e.coalesce = None;
        let out = Outcome {
            label: e.label.clone(),
            changed: e.changed.clone(),
        };
        self.redo.push(e);
        Some(out)
    }

    /// Steps forward again; `None` when there is nothing to redo.
    pub fn redo(&mut self) -> Option<Outcome> {
        self.close_groups();
        let mut e = self.redo.pop()?;
        if let Some(j) = &mut self.journal {
            let _ = j.append(&Record::Redo);
        }
        std::mem::swap(&mut self.root, &mut e.root);
        let out = Outcome {
            label: e.label.clone(),
            changed: e.changed.clone(),
        };
        self.undo_bytes += e.bytes;
        self.undo.push_back(e);
        self.trim();
        Some(out)
    }

    /// Replays journal records (recovery, D06 §7.3). Records that no longer
    /// apply are skipped; the count of applied records is returned.
    pub fn replay(&mut self, records: &[Record]) -> usize {
        let mut n = 0;
        for r in records {
            let ok = match r {
                Record::Edit(e) => self.apply(e.clone()).is_ok(),
                Record::Undo => self.undo().is_some(),
                Record::Redo => self.redo().is_some(),
            };
            n += usize::from(ok);
        }
        n
    }
}
