// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use fluut_model::Diagnostic;
use std::path::PathBuf;

fn dir() -> PathBuf {
    std::env::temp_dir().join("fluut-app-toollibs/jobs/one")
}

fn project(refs: &[&str]) -> Project {
    let mut p = Project::new("P");
    p.tool_libraries = refs.iter().map(|s| (*s).to_owned()).collect();
    p
}

#[test]
fn a_library_is_added_last_with_a_path_relative_to_the_project() {
    let d = dir();
    let shared = d.join("../../shared/fusion.toml");
    let e = add_edit(&project(&["a.toml"]), &d, &shared).unwrap();
    assert_eq!(
        e,
        Edit::SetToolLibraries(vec!["a.toml".into(), "../../shared/fusion.toml".into()])
    );
}

#[test]
fn a_library_already_listed_is_not_added_again_however_it_is_spelled() {
    let d = dir();
    let p = project(&["../../shared/fusion.toml"]);
    assert_eq!(
        add_edit(&p, &d, &d.join("../../shared/fusion.toml")),
        Err(AddError::Listed)
    );
    assert_eq!(
        add_edit(&p, &d, &d.join("./x/../../../shared/fusion.toml")),
        Err(AddError::Listed)
    );
    assert!(!AddError::Listed.text().is_empty());
}

#[test]
fn a_reference_is_removed_by_position() {
    let p = project(&["a.toml", "b.toml", "c.toml"]);
    assert_eq!(
        remove_edit(&p, 1),
        Some(Edit::SetToolLibraries(vec![
            "a.toml".into(),
            "c.toml".into()
        ]))
    );
    assert_eq!(remove_edit(&p, 3), None);
}

#[test]
fn rows_show_the_file_name_and_what_did_not_load() {
    let d = dir();
    let p = project(&[
        "../shared/gone.toml",
        "ok.toml",
        "https://x/y.toml",
        "bad.toml",
    ]);
    let broken = resolve_tool_ref(&d, "bad.toml").display().to_string();
    let w = vec![
        Diagnostic::warning(
            "project.tools.missing",
            "tool library `../shared/gone.toml` was not found (x); its tools are not available",
        ),
        Diagnostic::warning(
            "project.tools.rejected",
            "tool library reference `https://x/y.toml` is not used: URLs are not allowed",
        ),
        Diagnostic::error("model.tools.load", format!("{broken}: not valid")),
    ];
    let rows = rows(&p, &d, &w);
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].name, "gone.toml");
    assert_eq!(rows[0].reference, "../shared/gone.toml");
    assert!(rows[0].problem.as_ref().unwrap().contains("not found"));
    assert_eq!(rows[1].problem, None);
    assert!(
        rows[2]
            .problem
            .as_ref()
            .unwrap()
            .contains("plain file path")
    );
    assert!(
        rows[3]
            .problem
            .as_ref()
            .unwrap()
            .contains("could not be read")
    );
}
