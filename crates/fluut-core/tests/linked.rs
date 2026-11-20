// SPDX-License-Identifier: MIT OR Apache-2.0

//! Linked models: a `.craftx` file is imported, picks store face names,
//! and an update re-finds every pick by name with RFC 0001's outcomes
//! (spec 03 §3.5 item 4, D06 §5.1).

#[path = "../../fluut-io-craftx/tests/support/craftx.rs"]
mod craftx;
#[path = "../../fluut-cam/tests/support/step.rs"]
mod step;

use craftx::Craftx;
use fluut_core::names::Outcome;
use fluut_core::store::craftx_body_id;
use fluut_core::{JobState, Session, Targets};
use fluut_model::edit::Edit;
use std::path::PathBuf;

const TOOLS: &str = "format = \"fluut.tools/1\"\nid = \"shop\"\n[[tool]]\nid = \"flat-6\"\nshape = \"flat\"\ndiameter = 6\n";

/// A folder with `model.craftx` (revision `c`) and a project with a
/// pocket and a contour toolpath on it, no picks yet.
fn workspace(name: &str, c: &Craftx) -> (PathBuf, String) {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("tools")).unwrap();
    std::fs::write(d.join("tools/shop.toml"), TOOLS).unwrap();
    std::fs::write(d.join("model.craftx"), c.bytes()).unwrap();
    let b = craftx_body_id(&c.source_id, 0, 0);
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
revision  = "{rev}"

  [[geometry.body]]
  id   = "{b}"
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
  tool = "shop#flat-6"

    [setup.toolpath.tool_params]
    spindle_rpm = 18000
    feed        = {{ cut = 1200, plunge = 300 }}

    [setup.toolpath.passes]
    stepover  = 2.4
    step_down = 4.0

  [[setup.toolpath]]
  id   = "t2"
  name = "Edge"
  type = "contour_2d"
  tool = "shop#flat-6"
"#,
        sid = c.source_id,
        hash = fluut_model::hash::sha256_hex(&c.bytes()),
        rev = c.revision
    );
    std::fs::write(d.join("p.fluut"), project).unwrap();
    (d, b)
}

fn stem(b: &str) -> &str {
    b.strip_prefix("b-").unwrap()
}

/// Opens the project and picks the face named `face` for t1 and the edge
/// named `edge` for t2, by their ids in the loaded body.
fn open_with_picks(d: &std::path::Path, b: &str, face: &str, edge: Option<&str>) -> Session {
    let mut s = Session::open(&d.join("p.fluut"), &[]).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    let body = fluut_core::names::brep(s.store.bodies(), b)
        .unwrap()
        .clone();
    let f = body.names.faces_named(face)[0];
    s.apply(Edit::set_toolpath_field(
        "t1",
        "geometry.drive_faces",
        Some(&format!("[{{ face = \"f-{}.{f}\" }}]", stem(b))),
    ))
    .unwrap();
    if let Some(edge) = edge {
        let e = fluut_core::names::find_edge(&body, edge, None).entities[0];
        s.apply(Edit::set_toolpath_field(
            "t2",
            "geometry.drive_curves",
            Some(&format!("[{{ edge = \"e-{}.{e}\" }}]", stem(b))),
        ))
        .unwrap();
    }
    s
}

fn field(s: &Session, tp: &str, path: &str) -> String {
    let (_, t) = s.project.toolpath(tp).unwrap();
    t.params.field_toml(path).unwrap_or_default()
}

fn update(
    d: &std::path::Path,
    s: &mut Session,
    c: &Craftx,
) -> fluut_core::reimport::ReimportReport {
    std::fs::write(d.join("model.craftx"), c.bytes()).unwrap();
    s.reimport("g1").unwrap()
}

#[test]
fn picks_store_names_and_round_trip() {
    let (d, b) = workspace("linked-names", &craftx::rev1());
    let mut s = open_with_picks(&d, &b, "F2:floor", Some("F1:side:l1|F1:side:l2"));
    let st = stem(&b);
    assert_eq!(
        field(&s, "t1", "geometry.drive_faces"),
        format!("[{{ face = \"f-{st}.10\", name = \"F2:floor\" }}]")
    );
    assert!(
        field(&s, "t2", "geometry.drive_curves").contains("name = \"F1:side:l1|F1:side:l2\""),
        "{}",
        field(&s, "t2", "geometry.drive_curves")
    );
    // Naming is part of the pick's undo step.
    assert_eq!(
        s.undo_label().as_deref(),
        Some("Set geometry.drive_curves of t2")
    );
    s.save().unwrap();
    let text = std::fs::read_to_string(d.join("p.fluut")).unwrap();
    assert!(text.starts_with("format = \"fluut.project/1.2\""), "{text}");
    assert!(text.contains("name = \"F2:floor\""), "{text}");
    assert!(text.contains("source_id = \"3f6c0d4e-design\""), "{text}");
    let again = Session::open(&d.join("p.fluut"), &[]).unwrap();
    assert_eq!(again.project, s.project);
    // The link and the design's parameters are there for the Model panel.
    let link = again.store.source("g1").unwrap().link.clone().unwrap();
    assert_eq!(link.source_id, "3f6c0d4e-design");
    assert_eq!(link.revision, "1");
    assert_eq!(link.params[0].name, "wall");
}

#[test]
fn exact_picks_follow_their_faces_when_faces_move() {
    let (d, b) = workspace("linked-exact", &craftx::rev1());
    let mut s = open_with_picks(&d, &b, "F2:floor", Some("F1:side:l1|F1:side:l2"));
    let r = update(&d, &mut s, &craftx::hole());
    let st = stem(&b);
    let outcomes: Vec<_> = r
        .picks
        .iter()
        .map(|p| (p.owner.as_str(), p.outcome))
        .collect();
    assert_eq!(outcomes, [("t1", Outcome::Exact), ("t2", Outcome::Exact)]);
    // The floor is face 12 now.
    assert_eq!(r.picks[0].ids, [format!("f-{st}.12")]);
    assert_eq!(
        field(&s, "t1", "geometry.drive_faces"),
        format!("[{{ face = \"f-{st}.12\", name = \"F2:floor\" }}]")
    );
    assert!(s.broken_picks("t1").is_empty());
    assert!(s.broken_picks("t2").is_empty());
    assert_eq!(
        s.project
            .source("g1")
            .unwrap()
            .link
            .as_ref()
            .unwrap()
            .revision,
        "2"
    );
    let reports = s.calculate(&Targets::Toolpaths(vec!["t1".into()]));
    assert!(
        matches!(reports[0].state, JobState::Done { .. }),
        "{:?}",
        reports[0]
    );
}

#[test]
fn a_split_face_becomes_all_its_pieces_and_an_edge_survives_it() {
    let (d, b) = workspace("linked-split", &craftx::rev1());
    let mut s = open_with_picks(&d, &b, "F2:floor", Some("F2:side:l1|F2:side:l4"));
    let r = update(&d, &mut s, &craftx::split());
    let st = stem(&b);
    assert_eq!(r.picks[0].outcome, Outcome::Split);
    assert_eq!(r.picks[0].ids, [format!("f-{st}.10"), format!("f-{st}.15")]);
    assert!(r.diagnostics.iter().any(|d| d.code == "model.pick.split"));
    assert_eq!(
        field(&s, "t1", "geometry.drive_faces"),
        format!(
            "[{{ face = \"f-{st}.10\", name = \"F2:floor\" }}, {{ face = \"f-{st}.15\", name = \"F2:floor\" }}]"
        )
    );
    // The pocket corner at (20, 15): the wall at low Y is split, and the
    // corner is the edge of its left piece.
    assert_eq!(r.picks[1].outcome, Outcome::Exact);
    assert!(s.broken_picks("t1").is_empty());
    assert!(s.broken_picks("t2").is_empty());
}

#[test]
fn merged_pieces_follow_the_face_that_answers_to_them() {
    let (d, b) = workspace("linked-merged", &craftx::split());
    let mut s = open_with_picks(&d, &b, "F2:floor#[F6:side:l4]", None);
    let r = update(&d, &mut s, &craftx::merged());
    let st = stem(&b);
    assert_eq!(r.picks[0].outcome, Outcome::Merged);
    assert_eq!(r.picks[0].ids, [format!("f-{st}.10")]);
    assert!(r.diagnostics.iter().any(|d| d.code == "model.pick.merged"));
    // The pick keeps the name it was made with.
    assert_eq!(
        field(&s, "t1", "geometry.drive_faces"),
        format!("[{{ face = \"f-{st}.10\", name = \"F2:floor#[F6:side:l4]\" }}]")
    );
    assert!(s.broken_picks("t1").is_empty());
}

#[test]
fn a_name_found_by_geometry_is_kept_and_flagged() {
    let (d, b) = workspace("linked-inexact", &craftx::rev1());
    let mut s = open_with_picks(&d, &b, "F2:floor", None);
    let r = update(&d, &mut s, &craftx::inexact());
    assert_eq!(r.picks[0].outcome, Outcome::ByGeometry);
    let w: Vec<_> = r
        .diagnostics
        .iter()
        .filter(|d| d.code == "model.pick.by_geometry")
        .collect();
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].toolpath.as_deref(), Some("t1"));
    assert!(s.broken_picks("t1").is_empty());
    let _ = b;
}

#[test]
fn a_missing_pick_breaks_its_toolpath_until_it_is_picked_again() {
    let (d, b) = workspace("linked-missing", &craftx::rev1());
    let mut s = open_with_picks(&d, &b, "F2:floor", Some("F1:side:l1|F1:side:l2"));
    let first = s.calculate(&Targets::Toolpaths(vec!["t1".into()]));
    assert!(
        matches!(first[0].state, JobState::Done { .. }),
        "{:?}",
        first[0]
    );
    let before = s.result("t1").unwrap();
    // The fillet takes the edge away.
    let r = update(&d, &mut s, &craftx::fillet());
    assert_eq!(r.picks[1].outcome, Outcome::Missing);
    assert_eq!(r.broken(), ["t2"]);
    assert_eq!(s.broken_picks("t2").len(), 1);
    // The floor goes too: its feature was replaced.
    let mut c = craftx::rev1();
    c.revision = 7;
    c.faces[10].name = "F9:floor".into();
    let st = stem(&b);
    let r = update(&d, &mut s, &c);
    assert_eq!(r.picks[0].outcome, Outcome::Missing);
    let broken = s.broken_picks("t1");
    assert_eq!(broken.len(), 1);
    assert_eq!(broken[0].name, "F2:floor");
    // The pick is kept, never dropped: on face 11, where the fillet
    // revision had the floor.
    assert_eq!(
        field(&s, "t1", "geometry.drive_faces"),
        format!("[{{ face = \"f-{st}.11\", name = \"F2:floor\" }}]")
    );
    // The last result stays for viewing; calculating and posting refuse.
    assert!(std::sync::Arc::ptr_eq(
        &s.last_result("t1").unwrap(),
        &before
    ));
    let rep = s.calculate(&Targets::Toolpaths(vec!["t1".into()]));
    assert_eq!(rep[0].state, JobState::Broken);
    let e = &rep[0].diagnostics[0];
    assert_eq!(e.code, "model.pick.missing");
    assert!(e.is_error());
    for part in ["F2:floor", "Pocket", "40.0, 30.0, 12.0"] {
        assert!(e.message.contains(part), "{part}: {}", e.message);
    }
    let posts = fluut_post::builtin::builtin_store();
    let err = s
        .post("fluut/grbl", &posts, &Default::default())
        .expect_err("posting must refuse a broken toolpath");
    assert!(
        err.iter().any(|d| d.code == "model.pick.missing"),
        "{err:?}"
    );
    // Picking the floor again (removing the pick, then picking) clears it.
    s.apply(Edit::set_toolpath_field("t1", "geometry.drive_faces", None))
        .unwrap();
    s.apply(Edit::set_toolpath_field(
        "t1",
        "geometry.drive_faces",
        Some(&format!("[{{ face = \"f-{st}.10\" }}]")),
    ))
    .unwrap();
    assert!(s.broken_picks("t1").is_empty());
    assert_eq!(
        field(&s, "t1", "geometry.drive_faces"),
        format!("[{{ face = \"f-{st}.10\", name = \"F9:floor\" }}]")
    );
}

#[test]
fn an_update_is_one_undo_step_and_brings_the_old_model_back() {
    let (d, b) = workspace("linked-undo", &craftx::rev1());
    let mut s = open_with_picks(&d, &b, "F2:floor", None);
    let before = s.project.clone();
    update(&d, &mut s, &craftx::hole());
    let after = s.project.clone();
    assert_ne!(before, after);
    s.undo().unwrap();
    assert_eq!(s.project, before);
    // The bodies of revision 1 are back, so the pick holds.
    let body = fluut_core::names::brep(s.store.bodies(), &b).unwrap();
    assert_eq!(body.faces.len(), 11);
    assert!(s.broken_picks("t1").is_empty());
    s.redo().unwrap();
    assert_eq!(s.project, after);
    let body = fluut_core::names::brep(s.store.bodies(), &b).unwrap();
    assert_eq!(body.faces.len(), 13);
    assert!(s.broken_picks("t1").is_empty());
}

#[test]
fn a_changed_file_opened_before_its_update_shows_broken_picks() {
    let (d, b) = workspace("linked-stale", &craftx::rev1());
    let mut s = open_with_picks(&d, &b, "F2:floor", None);
    s.save().unwrap();
    // The design changes while the project is closed: face 10 is now a
    // pocket wall.
    std::fs::write(d.join("model.craftx"), craftx::hole().bytes()).unwrap();
    let mut s = Session::open(&d.join("p.fluut"), &[]).unwrap();
    assert_eq!(s.broken_picks("t1").len(), 1);
    let rep = s.calculate(&Targets::Toolpaths(vec!["t1".into()]));
    assert_eq!(rep[0].state, JobState::Broken);
    // The update re-finds the floor by its stored name.
    let r = s.reimport("g1").unwrap();
    assert_eq!(r.picks[0].outcome, Outcome::Exact);
    assert_eq!(r.picks[0].ids, [format!("f-{}.12", stem(&b))]);
    assert!(s.broken_picks("t1").is_empty());
}

#[test]
fn a_file_that_is_not_the_linked_design_is_reported() {
    let (d, _) = workspace("linked-other", &craftx::rev1());
    let mut other = craftx::rev1();
    other.source_id = "another-design".into();
    std::fs::write(d.join("model.craftx"), other.bytes()).unwrap();
    let s = Session::open(&d.join("p.fluut"), &[]).unwrap();
    assert!(
        s.warnings.iter().any(|w| w.code == "model.link.source_id"),
        "{:?}",
        s.warnings
    );
}

#[test]
fn a_damaged_file_is_refused_by_name() {
    let (d, _) = workspace("linked-damaged", &craftx::rev1());
    let mut c = craftx::rev1();
    c.format = "craft-exchange/2".into();
    std::fs::write(d.join("model.craftx"), c.bytes()).unwrap();
    let s = Session::open(&d.join("p.fluut"), &[]).unwrap();
    assert!(
        s.warnings
            .iter()
            .any(|w| w.code == "import.craftx.format" && w.is_error()),
        "{:?}",
        s.warnings
    );
}

#[test]
fn import_model_links_a_craftx_file() {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("linked-import");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("block.craftx"), craftx::rev1().bytes()).unwrap();
    let mut s =
        Session::new_project(&d.join("p.fluut"), fluut_model::project::DisplayUnit::Mm).unwrap();
    s.import_model(&d.join("block.craftx")).unwrap();
    let g = &s.project.geometry[0];
    assert_eq!(g.kind, fluut_model::project::SourceKind::Craftx);
    let link = g.link.as_ref().unwrap();
    assert_eq!(
        (link.source_id.as_str(), link.revision.as_str()),
        ("3f6c0d4e-design", "1")
    );
    assert_eq!(g.bodies[0].id, craftx_body_id("3f6c0d4e-design", 0, 0));
}

/// Projects made before names keep every id: each CLI test project opens,
/// and re-importing its unchanged files maps every selection to itself
/// and changes no Setup or toolpath.
#[test]
fn projects_without_names_keep_their_ids() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fluut-cli/tests/projects");
    let mut seen = 0;
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let Some(file) = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|x| x == "fluut"))
        else {
            continue;
        };
        let mut s = Session::open(&file, &[]).unwrap();
        let before = s.project.clone();
        for b in s.store.bodies() {
            if let fluut_cam::geometry::BodyGeom::Brep(x) = &b.geom {
                assert!(x.names.is_empty(), "{}: {}", file.display(), b.id);
            }
        }
        let refs = fluut_core::reimport::references(&before);
        for g in before.geometry.iter().map(|g| g.id.clone()) {
            let r = s.reimport(&g).unwrap();
            assert!(r.picks.is_empty(), "{}", file.display());
            assert!(
                r.unmatched.is_empty(),
                "{}: {:?}",
                file.display(),
                r.unmatched
            );
            for x in &refs {
                if let Some(n) = r.matching.new_id(&x.id) {
                    assert_eq!(n, x.id, "{}", file.display());
                }
            }
        }
        assert_eq!(s.project.setups, before.setups, "{}", file.display());
        seen += 1;
    }
    assert!(seen >= 10, "{seen}");
}

#[test]
fn the_update_report_names_each_outcome_and_toolpath_in_plain_words() {
    let (d, b) = workspace("linked-report", &craftx::rev1());
    let mut s = open_with_picks(&d, &b, "F2:floor", Some("F1:side:l1|F1:side:l2"));
    let r = update(&d, &mut s, &craftx::fillet());
    assert_eq!(
        (r.revision_before.as_deref(), r.revision.as_deref()),
        (Some("1"), Some("3"))
    );
    assert!(!r.unchanged);
    assert_eq!(r.picks[0].owner_name, "Pocket");
    let text = r.text();
    for part in [
        "g1 model.craftx: revision 1 -> 3",
        "exact       t1 \"Pocket\" (geometry.drive_faces): the face `F2:floor` was found by its name",
        "missing     t2 \"Edge\" (geometry.drive_curves): the edge `F1:side:l1|F1:side:l2` is not in the model any more",
        "the toolpath is broken until you pick it again",
        "2 picks: 1 exact, 1 missing; 1 toolpath broken",
    ] {
        assert!(text.contains(part), "{part}\n{text}");
    }
    // Updating again from the same file changes nothing and says so.
    let r = s.reimport("g1").unwrap();
    assert!(r.unchanged);
    assert!(
        r.text().contains("the file has not changed"),
        "{}",
        r.text()
    );
}

#[test]
fn the_outcome_words_cover_split_merged_and_by_geometry() {
    let (d, b) = workspace("linked-words", &craftx::rev1());
    let mut s = open_with_picks(&d, &b, "F2:floor", None);
    let r = update(&d, &mut s, &craftx::split());
    assert!(
        r.picks[0]
            .text()
            .contains("is now 2 pieces; the pick uses all of them"),
        "{}",
        r.picks[0].text()
    );
    let r = update(&d, &mut s, &craftx::inexact());
    assert!(
        r.picks[0].text().contains("found by its shape only"),
        "{}",
        r.picks[0].text()
    );
    let (d, b) = workspace("linked-words-merged", &craftx::split());
    let mut s = open_with_picks(&d, &b, "F2:floor#[F6:side:l4]", None);
    let r = update(&d, &mut s, &craftx::merged());
    assert!(
        r.picks[0].text().contains("merged into another face"),
        "{}",
        r.picks[0].text()
    );
    assert_eq!(
        r.counts(),
        [(Outcome::Merged, 1)],
        "only outcomes that occurred are counted"
    );
}

#[test]
fn an_update_from_another_file_relinks_the_source_in_the_same_step() {
    let (d, b) = workspace("linked-relink", &craftx::rev1());
    let mut s = open_with_picks(&d, &b, "F2:floor", None);
    std::fs::create_dir_all(d.join("sent")).unwrap();
    std::fs::write(d.join("sent/block.craftx"), craftx::hole().bytes()).unwrap();
    let before = s.project.clone();
    let r = s
        .reimport_from("g1", Some(&d.join("sent/block.craftx")))
        .unwrap();
    assert_eq!(r.picks[0].outcome, Outcome::Exact);
    assert_eq!(s.project.source("g1").unwrap().path, "sent/block.craftx");
    assert_eq!(r.path, "sent/block.craftx");
    s.undo().unwrap();
    assert_eq!(s.project, before);
}

#[test]
fn a_linked_file_is_referenced_where_it_is_and_never_copied() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("linked-in-place");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("design")).unwrap();
    std::fs::create_dir_all(root.join("cam")).unwrap();
    std::fs::write(root.join("design/block.craftx"), craftx::rev1().bytes()).unwrap();
    let mut s = Session::new_project(
        &root.join("cam/p.fluut"),
        fluut_model::project::DisplayUnit::Mm,
    )
    .unwrap();
    let r = s.import_model(&root.join("design/block.craftx")).unwrap();
    assert!(!r.copied);
    assert_eq!(s.project.geometry[0].path, "../design/block.craftx");
    assert!(!root.join("cam/block.craftx").exists());
    // Save as elsewhere: the link still names the design's file.
    s.save_as(&root.join("cam/sub/q.fluut")).unwrap();
    assert_eq!(s.project.geometry[0].path, "../../design/block.craftx");
    assert!(!root.join("cam/sub/block.craftx").exists());
    // Change detection sees the design's next save.
    let mut stamps = fluut_core::watch::Stamps::new();
    let files = s.linked_files();
    assert!(
        stamps
            .check(&files, fluut_core::watch::Due::Focus)
            .is_empty()
    );
    std::fs::write(root.join("design/block.craftx"), craftx::hole().bytes()).unwrap();
    let changed = stamps.check(&files, fluut_core::watch::Due::Interval);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].source, "g1");
    // The file tells which design it holds, and the project links it.
    let link = fluut_core::store::read_link(&root.join("design/block.craftx")).unwrap();
    assert_eq!(s.linked_source(&link.source_id), Some("g1"));
    assert_eq!(s.linked_source("another"), None);
    // The time the design was saved is known for the Model panel.
    assert!(s.store.source("g1").unwrap().modified.is_some());
}
