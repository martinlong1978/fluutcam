// SPDX-License-Identifier: MIT OR Apache-2.0

//! `fluut calc`, `sim` and `post` honour the tool libraries a project
//! refers to (D06 §7.2.1); `--tools` still adds on top.

use std::path::{Path, PathBuf};
use std::process::Command;

fn here(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(p)
}

fn fluut(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fluut"))
        .args(args)
        .output()
        .unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// The `block` project in `<name>/jobs/one` with its library `shop` moved
/// to `<name>/shared/shop.toml` (not in `tools/`). `refs` is the
/// `tool_libraries` value, if any.
fn workspace(name: &str, refs: Option<&str>) -> (PathBuf, PathBuf) {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    let job = d.join("jobs/one");
    std::fs::create_dir_all(&job).unwrap();
    std::fs::create_dir_all(d.join("shared")).unwrap();
    std::fs::copy(here("projects/block/block.step"), job.join("block.step")).unwrap();
    std::fs::copy(
        here("projects/block/tools/shop.toml"),
        d.join("shared/shop.toml"),
    )
    .unwrap();
    let mut p = std::fs::read_to_string(here("projects/block/block.fluut")).unwrap();
    if let Some(r) = refs {
        p = p.replacen(
            "format = \"fluut.project/1\"",
            "format = \"fluut.project/1.1\"",
            1,
        );
        p = p.replacen(
            "units  = \"mm\"\n",
            &format!("units  = \"mm\"\ntool_libraries = [{r}]\n"),
            1,
        );
    }
    let file = job.join("block.fluut");
    std::fs::write(&file, p).unwrap();
    (d, file)
}

#[test]
fn calc_uses_a_library_the_project_refers_to() {
    let (_, p) = workspace("cli-tl-calc", Some("\"../../shared/shop.toml\""));
    let o = fluut(&["calc", p.to_str().unwrap()]);
    let out = text(&o.stdout);
    assert!(o.status.success(), "{}", text(&o.stderr));
    assert_eq!(
        out.lines()
            .filter(|l| l.contains(": calculated; cut "))
            .count(),
        3,
        "{out}"
    );
    assert!(!text(&o.stderr).contains("tools."), "{}", text(&o.stderr));
}

#[test]
fn without_the_reference_the_tool_is_missing() {
    let (_, p) = workspace("cli-tl-none", None);
    let o = fluut(&["calc", p.to_str().unwrap()]);
    assert!(!o.status.success(), "{}", text(&o.stdout));
}

#[test]
fn a_missing_library_file_is_a_warning_naming_the_file() {
    let (_, p) = workspace(
        "cli-tl-missing",
        Some("\"../../shared/gone.toml\", \"../../shared/shop.toml\""),
    );
    let o = fluut(&["calc", p.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o.stderr));
    let err = text(&o.stderr);
    assert!(err.contains("project.tools.missing"), "{err}");
    assert!(err.contains("gone.toml"), "{err}");
}

#[test]
fn tools_on_the_command_line_still_adds_on_top() {
    let (d, p) = workspace("cli-tl-top", Some("\"../../shared/shop.toml\""));
    // A library with the same id and version, but a different tool set:
    // it replaces the referenced one, so the toolpaths lose their tool.
    std::fs::write(
        d.join("extra.toml"),
        "format = \"fluut.tools/1\"\nid = \"shop\"\n[[tool]]\nid = \"other\"\nname = \"Other\"\n\
         number = 9\nshape = \"flat\"\ndiameter = 3\nflute_length = 10\n",
    )
    .unwrap();
    let o = fluut(&[
        "calc",
        p.to_str().unwrap(),
        "--tools",
        d.join("extra.toml").to_str().unwrap(),
    ]);
    assert!(!o.status.success(), "{}", text(&o.stdout));
    // Without it the reference is enough.
    assert!(fluut(&["calc", p.to_str().unwrap()]).status.success());
}

#[test]
fn sim_and_post_honour_the_reference_too() {
    let (d, p) = workspace("cli-tl-post", Some("\"../../shared/shop.toml\""));
    let out = d.join("out.ngc");
    let o = fluut(&[
        "post",
        p.to_str().unwrap(),
        "--post",
        "fluut/linuxcnc",
        "-o",
        out.to_str().unwrap(),
        "--date",
        "2026-11-11",
        "--time",
        "12:00:00",
    ]);
    assert!(o.status.success(), "{}", text(&o.stderr));
    assert!(std::fs::read_to_string(&out).unwrap().contains("G"));
    let o = fluut(&["sim", p.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o.stderr));
}
