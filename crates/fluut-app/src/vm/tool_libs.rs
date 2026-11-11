// SPDX-License-Identifier: MIT OR Apache-2.0

//! The tool libraries a project refers to (design D06 §7.2.1): the rows the
//! browser lists, and the edits that add and remove a reference. No `egui`.
//!
//! A reference is stored relative to the project file where it can be (see
//! `fluut_model::library::tool_ref_for`). Each change is one
//! `SetToolLibraries` edit, so it is one undo step and marks the project
//! as changed.

use crate::text::{data, tr, tr_args};
use fluut_model::Diagnostic;
use fluut_model::edit::Edit;
use fluut_model::library::{check_tool_ref, names_tool_file, resolve_tool_ref, tool_ref_for};
use fluut_model::project::Project;
use std::path::Path;

/// One listed library.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ToolLibRow {
    /// As written in the project file.
    pub reference: String,
    /// The file name, shown as the label.
    pub name: String,
    /// Why the library is not available, if it is not.
    pub problem: Option<String>,
}

/// The rows for the project's references. `dir` is the project folder;
/// `warnings` are the session's load findings, which say what did not load.
pub fn rows(project: &Project, dir: &Path, warnings: &[Diagnostic]) -> Vec<ToolLibRow> {
    project
        .tool_libraries
        .iter()
        .map(|r| {
            let quoted = format!("`{r}`");
            let file = resolve_tool_ref(dir, r);
            let shown = file.display().to_string();
            let problem = warnings.iter().find_map(|d| {
                let about_it = match d.code.as_str() {
                    "project.tools.missing" | "project.tools.rejected" => {
                        d.message.contains(&quoted)
                    }
                    "model.tools.load" => d.message.starts_with(&shown),
                    _ => false,
                };
                about_it.then(|| match d.code.as_str() {
                    "project.tools.missing" => tr("browser-tool-library-missing"),
                    "project.tools.rejected" => tr("browser-tool-library-rejected"),
                    _ => tr("browser-tool-library-broken"),
                })
            });
            let name = Path::new(r.as_str())
                .file_name()
                .map_or_else(|| r.clone(), |n| n.to_string_lossy().into_owned());
            ToolLibRow {
                reference: r.clone(),
                name: data(name),
                problem,
            }
        })
        .collect()
}

/// Why a library cannot be added.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AddError {
    /// The project already refers to the file.
    Listed,
    /// The path is not usable as a reference (text for the user).
    Refused(String),
}

impl AddError {
    pub fn text(&self) -> String {
        match self {
            AddError::Listed => tr("browser-tool-library-listed"),
            AddError::Refused(why) => tr_args("browser-tool-library-refused", &[("reason", why)]),
        }
    }
}

/// The edit that adds the library file `file` to the project in `dir`: its
/// reference is relative to the project file where possible, and goes last
/// (it wins over the earlier ones on the same library id).
pub fn add_edit(project: &Project, dir: &Path, file: &Path) -> Result<Edit, AddError> {
    if project
        .tool_libraries
        .iter()
        .any(|r| names_tool_file(dir, r, file))
    {
        return Err(AddError::Listed);
    }
    let reference = tool_ref_for(dir, file);
    check_tool_ref(&reference).map_err(|why| AddError::Refused(why.to_owned()))?;
    let mut all = project.tool_libraries.clone();
    all.push(reference);
    Ok(Edit::SetToolLibraries(all))
}

/// The edit that removes the reference at `index`.
pub fn remove_edit(project: &Project, index: usize) -> Option<Edit> {
    (index < project.tool_libraries.len()).then(|| {
        let mut all = project.tool_libraries.clone();
        all.remove(index);
        Edit::SetToolLibraries(all)
    })
}

#[cfg(test)]
mod tests;
