// SPDX-License-Identifier: MIT OR Apache-2.0

//! `fluut model update`: re-reads a linked model, prints the update report
//! and saves the project; exit 0 when every pick resolved, 1 when one is
//! missing, 2 on an error.

#[path = "../../fluut-io-craftx/tests/support/craftx.rs"]
mod craftx;
#[path = "../../fluut-cam/tests/support/step.rs"]
mod step;

use craftx::Craftx;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fluut(args: &[&str]) -> (u8, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fluut"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.code().unwrap_or(-1) as u8,
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// The id of body 0 of the test design, without its `b-`.
fn stem(c: &Craftx) -> String {
    let h = fluut_model::hash::sha256_hex(c.source_id.as_bytes());
    let h = h.strip_prefix("sha256:").unwrap_or(&h).to_owned();
    format!("x{}-0", &h[..8])
}

/// A folder with `model.craftx` at revision 1 and a project whose pocket
/// toolpath picks the floor (`F2:floor`, face 10) by name.
fn workspace(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let c = craftx::rev1();
    std::fs::write(d.join("model.craftx"), c.bytes()).unwrap();
    let st = stem(&c);
    let project = format!(
        r#"format = "fluut.project/1.2"
name   = "Linked"
units  = "mm"

[[geometry]]
id        = "g1"
path      = "model.craftx"
kind      = "craftx"
hash      = "{hash}"
source_id = "{sid}"
revision  = "1"

  [[geometry.body]]
  id   = "b-{st}"
  name = "block"

[[setup]]
id    = "s1"
name  = "Top"
kind  = "three_axis"
stock = {{ kind = "block", margin = [2, 2, 0] }}
zero  = {{ method = "stock_top_corner", corner = "front_left" }}

  [[setup.toolpath]]
  id   = "t1"
  name = "Pocket"
  type = "pocket_2d"
  tool = "fluut/starter#flat-6"

    [setup.toolpath.geometry]
    drive_faces = [{{ face = "f-{st}.10", name = "F2:floor" }}]
"#,
        sid = c.source_id,
        hash = fluut_model::hash::sha256_hex(&c.bytes()),
    );
    std::fs::write(d.join("p.fluut"), project).unwrap();
    d
}

fn p(d: &Path) -> String {
    d.join("p.fluut").to_string_lossy().into_owned()
}

#[test]
fn an_update_that_finds_every_pick_prints_the_report_saves_and_exits_0() {
    let d = workspace("model-update-exact");
    std::fs::write(d.join("model.craftx"), craftx::hole().bytes()).unwrap();
    let (code, out, err) = fluut(&["model", "update", &p(&d)]);
    assert_eq!(code, 0, "{out}\n{err}");
    for part in [
        "g1 model.craftx: revision 1 -> 2",
        "exact       t1 \"Pocket\" (geometry.drive_faces): the face `F2:floor` was found by its name",
        "1 picks: 1 exact; 0 toolpaths broken",
        "saved ",
    ] {
        assert!(out.contains(part), "{part}\n{out}");
    }
    // The floor is face 12 in revision 2, and the project says so.
    let text = std::fs::read_to_string(d.join("p.fluut")).unwrap();
    assert!(
        text.contains(&format!("face = \"f-{}.12\"", stem(&craftx::rev1()))),
        "{text}"
    );
    assert!(
        text.contains("revision  = \"2\"") || text.contains("revision = \"2\""),
        "{text}"
    );
    // Nothing changed since: the report says so, and it is still 0.
    let (code, out, _) = fluut(&["model", "update", &p(&d), "--model", "g1"]);
    assert_eq!(code, 0);
    assert!(out.contains("the file has not changed"), "{out}");
}

#[test]
fn a_missing_pick_is_reported_by_name_and_exits_1() {
    let d = workspace("model-update-missing");
    let mut c = craftx::rev1();
    c.revision = 7;
    c.faces[10].name = "F9:floor".into();
    std::fs::write(d.join("model.craftx"), c.bytes()).unwrap();
    let (code, out, err) = fluut(&["model", "update", &p(&d)]);
    assert_eq!(code, 1, "{out}\n{err}");
    for part in [
        "missing     t1 \"Pocket\" (geometry.drive_faces): the face `F2:floor` is not in the model any more",
        "the toolpath is broken until you pick it again",
        "1 toolpath broken",
    ] {
        assert!(out.contains(part), "{part}\n{out}");
    }
    // The pick is kept in the saved project, and calc refuses the toolpath.
    let text = std::fs::read_to_string(d.join("p.fluut")).unwrap();
    assert!(text.contains("name = \"F2:floor\""), "{text}");
    let (code, out, err) = fluut(&["calc", &p(&d)]);
    assert_eq!(code, 1, "{out}\n{err}");
    assert!(out.contains("broken"), "{out}");
    assert!(err.contains("model.pick.missing"), "{err}");
}

#[test]
fn split_and_by_geometry_outcomes_resolve_and_exit_0() {
    let d = workspace("model-update-split");
    std::fs::write(d.join("model.craftx"), craftx::split().bytes()).unwrap();
    let (code, out, _) = fluut(&["model", "update", &p(&d)]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("split       t1"), "{out}");
    assert!(out.contains("is now 2 pieces"), "{out}");
    std::fs::write(d.join("model.craftx"), craftx::inexact().bytes()).unwrap();
    let (code, out, _) = fluut(&["model", "update", &p(&d)]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("by_geometry t1"), "{out}");
}

#[test]
fn errors_exit_2() {
    let d = workspace("model-update-errors");
    // An unknown source.
    let (code, _, err) = fluut(&["model", "update", &p(&d), "--model", "g9"]);
    assert_eq!(code, 2);
    assert!(err.contains("`g9` is not in the project"), "{err}");
    assert!(err.contains("the project's models are: g1"), "{err}");
    // A damaged file.
    let mut c = craftx::rev1();
    c.format = "craft-exchange/2".into();
    std::fs::write(d.join("model.craftx"), c.bytes()).unwrap();
    let (code, _, err) = fluut(&["model", "update", &p(&d)]);
    assert_eq!(code, 2);
    assert!(err.contains("import.craftx.format"), "{err}");
    // No project, and bad arguments.
    let (code, _, _) = fluut(&["model", "update", "nope.fluut"]);
    assert_eq!(code, 2);
    let (code, _, err) = fluut(&["model", "refresh", &p(&d)]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown model command"), "{err}");
    // A project with no linked model.
    let plain = d.join("plain.fluut");
    std::fs::write(
        &plain,
        "format = \"fluut.project/1\"\nname = \"x\"\nunits = \"mm\"\n",
    )
    .unwrap();
    let (code, _, err) = fluut(&["model", "update", &plain.to_string_lossy()]);
    assert_eq!(code, 2);
    assert!(err.contains("model.link.none"), "{err}");
}
