// SPDX-License-Identifier: MIT OR Apache-2.0

//! `fluut model update <project> [--model <id>] [--tools <file>]...`
//! (spec 03 §3.5 item 4, D06 §5.1): re-reads a linked model's file,
//! re-finds every named pick by its name and saves the project. It prints
//! the update report: one line per named pick with its outcome (`exact`,
//! `merged`, `split`, `by_geometry`, `missing`), the toolpath and what it
//! means.
//!
//! Without `--model`, every linked model of the project is updated. Exit
//! status: 0 when every pick resolved, 1 when a pick is missing (its
//! toolpath is broken), 2 on an error (bad arguments, a project or file
//! that cannot be read, a project that cannot be saved).

use fluut_core::Session;
use fluut_model::{Diagnostic, Severity};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Default)]
pub struct UpdateArgs {
    pub project: PathBuf,
    /// The geometry source to update; every linked model when `None`.
    pub model: Option<String>,
    pub tools: Vec<PathBuf>,
}

/// Parses the arguments after `model`.
pub fn parse(mut it: impl Iterator<Item = String>) -> Result<UpdateArgs, String> {
    match it.next().as_deref() {
        Some("update") => {}
        Some(s) => return Err(format!("unknown model command {s}")),
        None => return Err("no model command (update)".into()),
    }
    let mut a = UpdateArgs::default();
    let mut project = None;
    while let Some(arg) = it.next() {
        let mut value = |n: &str| it.next().ok_or(format!("{n} needs a value"));
        match arg.as_str() {
            "--model" => a.model = Some(value("--model")?),
            "--tools" => a.tools.push(value("--tools")?.into()),
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s if project.is_none() => project = Some(PathBuf::from(s)),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    a.project = project.ok_or("no project file")?;
    Ok(a)
}

fn print(d: &[Diagnostic]) {
    for x in d {
        eprintln!("{x}");
    }
}

/// The findings that are not about a pick: the report already says those.
fn others(d: &[Diagnostic]) -> Vec<Diagnostic> {
    d.iter()
        .filter(|x| !x.code.starts_with("model.pick.") && x.code != "model.reimport.unmatched")
        .cloned()
        .collect()
}

pub fn run(a: &UpdateArgs) -> ExitCode {
    let mut s = match Session::open(&a.project, &a.tools) {
        Ok(s) => s,
        Err(d) => {
            print(&d);
            return ExitCode::from(2);
        }
    };
    // Load findings about the linked files are what this command fixes;
    // the others still matter.
    print(
        &s.warnings
            .iter()
            .filter(|w| w.severity == Severity::Error || w.code != "model.geometry.changed")
            .cloned()
            .collect::<Vec<_>>(),
    );
    let ids: Vec<String> = match &a.model {
        Some(id) => {
            if s.project.source(id).is_none() {
                let known: Vec<&str> = s.project.geometry.iter().map(|g| g.id.as_str()).collect();
                print(&[Diagnostic::error(
                    "model.geometry.missing",
                    format!(
                        "geometry `{id}` is not in the project (it has: {})",
                        if known.is_empty() {
                            "none".to_owned()
                        } else {
                            known.join(", ")
                        }
                    ),
                )]);
                return ExitCode::from(2);
            }
            vec![id.clone()]
        }
        None => s.linked_files().into_iter().map(|f| f.source).collect(),
    };
    if ids.is_empty() {
        print(&[Diagnostic::error(
            "model.link.none",
            "the project has no linked model; give one with --model <id>",
        )]);
        return ExitCode::from(2);
    }
    let mut missing = false;
    for id in &ids {
        match s.reimport(id) {
            Ok(r) => {
                print!("{}", r.text());
                print(&others(&r.diagnostics));
                missing |= !r.broken().is_empty();
            }
            Err(d) => {
                print(&d);
                return ExitCode::from(2);
            }
        }
    }
    if let Err(d) = s.save() {
        print(&[d]);
        return ExitCode::from(2);
    }
    println!("saved {}", a.project.display());
    if missing {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Result<UpdateArgs, String> {
        parse(v.iter().map(|s| s.to_string()))
    }

    #[test]
    fn arguments() {
        let a = args(&["update", "p.fluut", "--model", "g2", "--tools", "t.toml"]).unwrap();
        assert_eq!(a.project, PathBuf::from("p.fluut"));
        assert_eq!(a.model.as_deref(), Some("g2"));
        assert_eq!(a.tools, [PathBuf::from("t.toml")]);
        assert_eq!(args(&["update", "p.fluut"]).unwrap().model, None);
        assert!(args(&[]).unwrap_err().contains("no model command"));
        assert!(args(&["fix", "p.fluut"]).unwrap_err().contains("unknown"));
        assert!(args(&["update"]).unwrap_err().contains("no project"));
        assert!(
            args(&["update", "p", "--model"])
                .unwrap_err()
                .contains("needs a value")
        );
        assert!(
            args(&["update", "p", "--x"])
                .unwrap_err()
                .contains("unknown option")
        );
        assert!(
            args(&["update", "p", "q"])
                .unwrap_err()
                .contains("unexpected")
        );
    }
}
