// SPDX-License-Identifier: MIT OR Apache-2.0

//! The command line of the window (spec 02 §2.6.4):
//!
//! ```text
//! fluutcam [project.fluut] [file.craftx | --open file.craftx]
//! ```
//!
//! A `.craftx` file opens in a new project; when a project given with it
//! already links the same design, the window offers to update that model
//! instead. A running window does not take the file over yet: each launch
//! is its own window (plan 2026-11-14-craft-bridge-import, Task 6 step 2).

use std::ffi::OsString;
use std::path::PathBuf;

/// What to open at start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    pub project: Option<PathBuf>,
    /// A linked model file to open.
    pub linked: Option<PathBuf>,
}

fn is_linked(p: &std::path::Path) -> bool {
    p.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("craftx"))
}

/// Reads the arguments after the program name. Unknown options and extra
/// files are ignored, so a file manager's arguments never stop the start.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Launch {
    let mut l = Launch::default();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let path = if a == "--open" {
            match it.next() {
                Some(p) => PathBuf::from(p),
                None => break,
            }
        } else if a.to_string_lossy().starts_with("--") {
            continue;
        } else {
            PathBuf::from(a)
        };
        if is_linked(&path) {
            l.linked.get_or_insert(path);
        } else {
            l.project.get_or_insert(path);
        }
    }
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Launch {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn a_craftx_file_is_a_linked_model_and_anything_else_a_project() {
        assert_eq!(p(&[]), Launch::default());
        assert_eq!(
            p(&["a.fluut"]),
            Launch {
                project: Some("a.fluut".into()),
                linked: None
            }
        );
        assert_eq!(p(&["b.craftx"]).linked, Some("b.craftx".into()));
        assert_eq!(p(&["B.CRAFTX"]).linked, Some("B.CRAFTX".into()));
        assert_eq!(p(&["--open", "b.craftx"]).linked, Some("b.craftx".into()));
        assert_eq!(
            p(&["a.fluut", "--open", "b.craftx"]),
            Launch {
                project: Some("a.fluut".into()),
                linked: Some("b.craftx".into())
            }
        );
        // Unknown options are skipped; a dangling --open is nothing.
        assert_eq!(
            p(&["--verbose", "a.fluut", "--open"]).project,
            Some("a.fluut".into())
        );
    }
}
