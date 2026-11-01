// SPDX-License-Identifier: MIT OR Apache-2.0

//! Re-import identity (spec 03 §3.5).

#[path = "../../fluut-cam/tests/support/step.rs"]
mod step;

use fluut_core::Session;
use fluut_core::reimport::MatchedBy;
use std::path::PathBuf;
use step::{Profile, Writer};

/// The 40 × 30 boss of the shared test parts, `h` high. Faces 0–7 are the
/// walls, 8 the bottom, 9 the top.
fn boss(h: f64) -> String {
    let mut w = Writer::new([0.0; 3]);
    let p = Profile::rounded_rect(0.0, 0.0, 40.0, 30.0, 5.0);
    w.walls(&p, 0.0, h, true);
    w.cap(&p, &[], 0.0, false);
    w.cap(&p, &[], h, true);
    w.finish("boss")
}

/// Adds `by` to every entity number, as a re-export from CAD would
/// renumber the file.
fn renumber(text: &str, by: u64) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('#') {
        out.push_str(&rest[..=i]);
        rest = &rest[i + 1..];
        let n = rest.bytes().take_while(u8::is_ascii_digit).count();
        let v: u64 = rest[..n].parse().unwrap();
        out.push_str(&(v + by).to_string());
        rest = &rest[n..];
    }
    out.push_str(rest);
    out
}

fn body_id(text: &str) -> String {
    text.lines()
        .find(|l| l.contains("MANIFOLD_SOLID_BREP"))
        .and_then(|l| l.strip_prefix('#'))
        .and_then(|l| l.split('=').next())
        .unwrap()
        .to_owned()
}

const TOOLS: &str = "format = \"fluut.tools/1\"\nid = \"shop\"\n[[tool]]\nid = \"flat-6\"\nshape = \"flat\"\ndiameter = 6\n";

fn workspace(name: &str) -> (PathBuf, String) {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("tools")).unwrap();
    std::fs::write(d.join("tools/shop.toml"), TOOLS).unwrap();
    let text = boss(10.0);
    std::fs::write(d.join("boss.step"), &text).unwrap();
    let b = body_id(&text);
    let project = format!(
        r#"format = "fluut.project/1"
name   = "Boss"
units  = "mm"

[[geometry]]
id   = "g1"
path = "boss.step"
kind = "step"

  [[geometry.body]]
  id   = "b-{b}"
  name = "boss"

[[setup]]
id          = "s1"
name        = "Top"
kind        = "three_axis"
geometry    = {{ bodies = ["b-{b}"] }}
orientation = {{ tool_axis = {{ face = "f-{b}.9" }}, x_axis = "+X" }}

  [[setup.toolpath]]
  id   = "t1"
  name = "Top outline"
  type = "contour_2d"
  tool = "shop#flat-6"

    [setup.toolpath.geometry]
    drive_curves = [{{ face = "f-{b}.9" }}]

  [[setup.toolpath]]
  id   = "t2"
  name = "Walls"
  type = "contour_2d"
  tool = "shop#flat-6"

    [setup.toolpath.geometry]
    drive_faces = [{{ face = "f-{b}.0" }}, {{ face = "f-{b}.4" }}, {{ face = "f-{b}.8" }}]
"#
    );
    std::fs::write(d.join("p.fluut"), project).unwrap();
    (d, b)
}

#[test]
fn one_changed_face_is_reported_and_the_rest_follow() {
    let (d, b) = workspace("reimport-changed");
    let mut s = Session::open(&d.join("p.fluut"), &[]).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    let before = s.project.clone();
    // The CAD model changes: the top is 2 mm higher, and the file is
    // renumbered.
    let new = renumber(&boss(12.0), 1000);
    let nb = body_id(&new);
    assert_ne!(nb, b);
    std::fs::write(d.join("boss.step"), &new).unwrap();
    let r = s.reimport("g1").unwrap();
    let m = &r.matching;
    assert_eq!(
        m.ids[&format!("b-{b}")],
        (format!("b-{nb}"), MatchedBy::Name)
    );
    // The bottom is unchanged; the walls kept their surfaces but grew.
    assert_eq!(
        m.ids[&format!("f-{b}.8")],
        (format!("f-{nb}.8"), MatchedBy::Signature)
    );
    for i in 0..8 {
        assert_eq!(
            m.ids[&format!("f-{b}.{i}")],
            (format!("f-{nb}.{i}"), MatchedBy::Surface),
            "wall {i}"
        );
    }
    assert!(m.new_id(&format!("f-{b}.9")).is_none());
    // The top is reported for the toolpath and the Setup that use it.
    let un: Vec<(&str, &str)> = r
        .unmatched
        .iter()
        .map(|u| (u.owner.as_str(), u.field.as_str()))
        .collect();
    assert_eq!(un, [("s1", "orientation"), ("t1", "geometry.drive_curves")]);
    let w: Vec<_> = r
        .diagnostics
        .iter()
        .filter(|d| d.code == "model.reimport.unmatched")
        .collect();
    assert_eq!(w.len(), 2);
    assert_eq!(w[1].toolpath.as_deref(), Some("t1"));
    // Selections of matched faces are rewritten; the others are kept.
    let text = fluut_model::project::write(&s.project);
    assert!(
        text.contains(&format!(
            "drive_faces = [{{ face = \"f-{nb}.0\" }}, {{ face = \"f-{nb}.4\" }}, {{ face = \"f-{nb}.8\" }}]"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("drive_curves = [{{ face = \"f-{b}.9\" }}]")),
        "{text}"
    );
    assert!(text.contains(&format!("bodies = [\"b-{nb}\"]")), "{text}");
    assert!(
        text.contains(&format!("  id   = \"b-{nb}\"\n  name = \"boss\"")),
        "{text}"
    );
    assert!(text.contains("hash = \"sha256:"), "{text}");
    // The new geometry is loaded: the walls now reach Z 12.
    let walls = s.store.bodies().next().unwrap();
    assert_eq!(walls.id, format!("b-{nb}"));
    // One undo step restores the old selections.
    s.undo().unwrap();
    assert_eq!(s.project, before);
}

#[test]
fn an_unchanged_file_maps_every_id_to_itself() {
    let (d, b) = workspace("reimport-same");
    let mut s = Session::open(&d.join("p.fluut"), &[]).unwrap();
    let r = s.reimport("g1").unwrap();
    assert!(r.unmatched.is_empty());
    assert!(r.matching.renames().is_empty());
    // 1 body, 10 faces and every edge, all by id.
    let edges = r
        .matching
        .ids
        .keys()
        .filter(|k| k.starts_with("e-"))
        .count();
    assert!(edges > 10, "{edges}");
    assert_eq!(r.matching.ids.len(), 1 + 10 + edges);
    assert!(r.matching.ids.values().all(|(_, by)| *by == MatchedBy::Id));
    assert_eq!(
        r.matching.new_id(&format!("f-{b}.9")),
        Some(format!("f-{b}.9").as_str())
    );
    // Only the hash was added.
    let p = &s.project;
    assert!(p.source("g1").unwrap().hash.is_some());
    assert!(s.reimport("g9").is_err());
}

#[test]
fn renumbering_alone_loses_nothing() {
    let (d, b) = workspace("reimport-renumber");
    let mut s = Session::open(&d.join("p.fluut"), &[]).unwrap();
    std::fs::write(d.join("boss.step"), renumber(&boss(10.0), 77)).unwrap();
    let r = s.reimport("g1").unwrap();
    assert!(r.unmatched.is_empty(), "{:?}", r.unmatched);
    let nb = (b.parse::<u64>().unwrap() + 77).to_string();
    for i in 0..10 {
        assert_eq!(
            r.matching.ids[&format!("f-{b}.{i}")],
            (format!("f-{nb}.{i}"), MatchedBy::Signature)
        );
    }
}
