// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tool libraries outside the project's `tools/` folder (design D06
//! §7.2.1): the native dialog that picks a library file, and the use of a
//! file as a reference. The list itself is in the browser. There is no
//! path text box: the paths come from the system dialog only.

use super::UiState;
use crate::dialogs::{DialogKind, Dialogs, Picked};
use crate::jobs::{Edit, Jobs};
use crate::vm::tool_libs::{self, AddError};
use std::path::{Path, PathBuf};

/// The folder of the open project's file.
fn project_dir(jobs: &Jobs) -> Option<PathBuf> {
    jobs.snapshot
        .as_ref()
        .and_then(|s| s.path.parent().map(PathBuf::from))
}

/// The ribbon command, or the button of the list: asks for the file.
pub fn ask(jobs: &Jobs, dialogs: &mut Dialogs) {
    if jobs.snapshot.is_none() || dialogs.is_open() {
        return;
    }
    dialogs.request(DialogKind::AddToolLibrary, project_dir(jobs), "");
}

/// The dialog gave a library file.
pub fn picked(jobs: &mut Jobs, state: &mut UiState, p: Picked) {
    if p.kind == DialogKind::AddToolLibrary {
        add(jobs, state, &p.path);
    }
}

/// What using a library file did.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Used {
    /// A reference was added (one undo step).
    Added,
    /// The project had the file already; its libraries were read again.
    Reloaded,
    /// Nothing happened; the reason is in `UiState::tool_lib_note`.
    Refused,
}

/// Adds a reference to `file`. A file the project lists already is read
/// again instead.
pub fn add(jobs: &mut Jobs, state: &mut UiState, file: &Path) -> Used {
    let (Some(snap), Some(dir)) = (jobs.snapshot.clone(), project_dir(jobs)) else {
        return Used::Refused;
    };
    state.tool_lib_note = None;
    match tool_libs::add_edit(&snap.project, &dir, file) {
        Ok(e) => {
            jobs.edit(Edit::Model(e));
            Used::Added
        }
        Err(AddError::Listed) => {
            jobs.edit(Edit::ReloadToolLibraries);
            state.tool_lib_note = Some(AddError::Listed.text());
            Used::Reloaded
        }
        Err(e) => {
            state.tool_lib_note = Some(e.text());
            Used::Refused
        }
    }
}

/// Removes the reference at `index`.
pub fn remove(jobs: &mut Jobs, state: &mut UiState, index: usize) {
    state.tool_lib_note = None;
    let Some(snap) = jobs.snapshot.clone() else {
        return;
    };
    if let Some(e) = tool_libs::remove_edit(&snap.project, index) {
        jobs.edit(Edit::Model(e));
    }
}
