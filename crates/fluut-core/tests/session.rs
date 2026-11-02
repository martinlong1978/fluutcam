// SPDX-License-Identifier: MIT OR Apache-2.0

//! Session, graph and frames on a project written to a temporary folder.

#[path = "../../fluut-cam/tests/support/step.rs"]
mod step;

use fluut_core::{JobState, NodeId, PostOptions, Session, SetupFrame, Targets};
use fluut_geom::Point3;
use fluut_model::params::{Num, PValue};
use fluut_model::project::{AxisDir, Corner, Orientation, Setup, ZeroSpec};
use fluut_model::setup::resolve_setup;
use fluut_post::builtin::builtin_store;
use std::path::PathBuf;
use std::sync::Arc;

const TOOLS: &str = r#"format = "fluut.tools/1"
id     = "shop"
[[tool]]
id = "flat-6"
name = "6 mm flat"
number = 1
shape = "flat"
diameter = 6
flute_length = 25
  [[tool.preset]]
  material = "MDF"
  rpm = 18000
  feed = 2000
  plunge = 600
  stepover = 2.4
  step_down = 4
"#;

fn project(body: &str) -> String {
    format!(
        r#"format = "fluut.project/1"
name   = "Boss"
units  = "mm"

[[geometry]]
id   = "g1"
path = "boss.step"
kind = "step"

[[setup]]
id       = "s1"
name     = "Top"
kind     = "three_axis"
material = "MDF"
stock    = {{ kind = "block", margin = [5, 5, 1] }}

  [[setup.toolpath]]
  id   = "t1"
  name = "Profile"
  type = "contour_2d"
  tool = "shop#flat-6"

    [setup.toolpath.geometry]
    drive_curves = [{{ face = "f-{body}.8" }}]
"#
    )
}

fn workspace(name: &str) -> (PathBuf, String) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tools")).unwrap();
    let text = step::boss([0.0; 3]);
    std::fs::write(dir.join("boss.step"), &text).unwrap();
    std::fs::write(dir.join("tools/shop.toml"), TOOLS).unwrap();
    // The body id is the STEP id of the solid.
    let id = text
        .lines()
        .find(|l| l.contains("MANIFOLD_SOLID_BREP"))
        .and_then(|l| l.strip_prefix('#'))
        .and_then(|l| l.split('=').next())
        .unwrap()
        .to_owned();
    (dir, id)
}

fn open(name: &str) -> Session {
    let (dir, id) = workspace(name);
    std::fs::write(dir.join("p.fluut"), project(&id)).unwrap();
    Session::open(&dir.join("p.fluut"), &[]).unwrap()
}

#[test]
fn calculation_is_cached_by_result_hash() {
    let mut s = open("cached");
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    assert!(s.is_stale("t1"));
    let r = s.calculate(&Targets::All);
    assert_eq!(
        r[0].state,
        JobState::Done { cached: false },
        "{:?}",
        r[0].diagnostics
    );
    assert!(!s.is_stale("t1"));
    let r = s.calculate(&Targets::All);
    assert_eq!(r[0].state, JobState::Done { cached: true });
    // A P edit (cut feed) keeps the result and changes the post node.
    let g0 = s.graph();
    s.project
        .toolpath_mut("t1")
        .unwrap()
        .params
        .set("tool_params.feed.cut", Some(PValue::Num(Num::int(1500))))
        .unwrap();
    let g1 = s.graph();
    assert_eq!(
        g1.changed(&g0),
        [NodeId::Post("t1".into()), NodeId::Simulation("s1".into())]
    );
    assert!(!s.is_stale("t1"));
    // An R edit (stock to leave) makes it stale.
    s.project
        .toolpath_mut("t1")
        .unwrap()
        .params
        .set("passes.stock_to_leave", Some(PValue::Num(Num::float(0.5))))
        .unwrap();
    assert_eq!(
        s.graph().changed(&g1),
        [
            NodeId::Result("t1".into()),
            NodeId::Post("t1".into()),
            NodeId::Simulation("s1".into())
        ]
    );
    assert!(s.is_stale("t1"));
    // A Setup edit makes every toolpath of the Setup stale.
    let g2 = s.graph();
    s.project.setup_mut("s1").unwrap().work_offset = Some("G55".into());
    assert!(s.graph().changed(&g2).contains(&NodeId::Setup("s1".into())));
}

#[test]
fn missing_tool_and_geometry() {
    let mut s = open("missing");
    s.project.toolpath_mut("t1").unwrap().tool = "shop#nope".into();
    let r = s.calculate(&Targets::All);
    assert_eq!(r[0].state, JobState::Failed);
    assert_eq!(r[0].diagnostics[0].code, "param.tool.missing");
    let (dir, id) = workspace("missing-file");
    std::fs::write(
        dir.join("p.fluut"),
        project(&id).replace("boss.step", "gone.step"),
    )
    .unwrap();
    let mut s = Session::open(&dir.join("p.fluut"), &[]).unwrap();
    assert_eq!(s.warnings[0].code, "model.geometry.missing");
    let r = s.calculate(&Targets::All);
    assert_eq!(r[0].diagnostics[0].code, "setup.geometry.empty");
    // A changed file is a warning.
    let p = project(&id).replace("kind = \"step\"", "kind = \"step\"\nhash = \"sha256:00\"");
    std::fs::write(dir.join("p.fluut"), p).unwrap();
    let s = Session::open(&dir.join("p.fluut"), &[]).unwrap();
    assert_eq!(s.warnings[0].code, "model.geometry.changed");
}

#[test]
fn post_writes_gcode() {
    let mut s = open("post");
    let opts = PostOptions {
        date: "2026-11-01".into(),
        time: "12:00:00".into(),
        notes: String::new(),
    };
    let (out, reports) = s.post("fluut/linuxcnc", &builtin_store(), &opts).unwrap();
    assert_eq!(reports.len(), 1);
    let g = &out.files[0];
    assert!(g.contains("(Profile)"), "{g}");
    assert!(g.contains("S18000 M3"), "{g}");
    assert!(g.contains("F2000"), "{g}");
    // Controller compensation needs the post's blocks: grbl has none.
    s.project
        .toolpath_mut("t1")
        .unwrap()
        .params
        .set("passes.compensation", Some(PValue::Enum("control".into())))
        .unwrap();
    let e = s.post("fluut/grbl", &builtin_store(), &opts).unwrap_err();
    assert!(
        e.iter()
            .any(|d| d.code == "param.contour_2d.compensation.post"),
        "{e:?}"
    );
    assert!(s.post("fluut/nope", &builtin_store(), &opts).is_err());
}

fn try_frame(s: &Session, setup: &Setup) -> Result<SetupFrame, fluut_model::Diagnostic> {
    let mut p = s.project.clone();
    let i = p.setup_index(&setup.id).unwrap();
    p.setups[i] = Arc::new(setup.clone());
    let r = resolve_setup(&p, &setup.id)?;
    SetupFrame::new(&r, s.store.bodies().cloned().collect())
}

fn frame(s: &Session, setup: &Setup) -> SetupFrame {
    try_frame(s, setup).unwrap()
}

#[test]
fn frames_place_the_zero_and_stock() {
    let s = open("frames");
    let mut setup = (*s.project.setups[0]).clone();
    // Default zero: stock top, front left. Boss 40 × 30 × 10, margins 5, 5, 1.
    let f = frame(&s, &setup);
    assert_eq!(f.stock.min, Point3::new(0.0, 0.0, -11.0));
    assert_eq!(f.stock.max, Point3::new(50.0, 40.0, 0.0));
    assert_eq!(f.clearance, 10.0);
    let env = f.height_env();
    assert_eq!((env.model_top, env.model_bottom), (-1.0, -11.0));
    setup.zero = Some(ZeroSpec::StockBottomCorner(Corner::BackRight));
    let f = frame(&s, &setup);
    assert_eq!(f.stock.max, Point3::new(0.0, 0.0, 11.0));
    setup.zero = Some(ZeroSpec::Point([Num::int(1), Num::int(2), Num::int(3)]));
    let f = frame(&s, &setup);
    assert_eq!(
        f.to_work.apply_point(Point3::new(1.0, 2.0, 3.0)),
        Point3::new(0.0, 0.0, 0.0)
    );
    // Tool axis along −Y of the model: the boss's back face is on top.
    setup.zero = Some(ZeroSpec::Point([Num::int(0); 3]));
    setup.orientation = Some(Orientation::ThreeAxis {
        tool_axis: AxisDir::NegY.into(),
        x_axis: AxisDir::PosX.into(),
    });
    let f = frame(&s, &setup);
    assert_eq!(
        f.to_work.apply_point(Point3::new(0.0, -1.0, 0.0)),
        Point3::new(0.0, 0.0, 1.0)
    );
    assert!((f.model.max.z() - 0.0).abs() < 1e-9 && (f.model.min.z() + 30.0).abs() < 1e-9);
    setup.orientation = Some(Orientation::ThreeAxis {
        tool_axis: AxisDir::PosZ.into(),
        x_axis: AxisDir::NegZ.into(),
    });
    assert!(try_frame(&s, &setup).is_err());
}

/// The project with a second Setup based on the first.
fn chained(name: &str) -> Session {
    let (dir, id) = workspace(name);
    let text = format!(
        "{}\n[[setup]]\nid       = \"s2\"\nname     = \"Again\"\nbased_on = \"s1\"\nstock    = {{ kind = \"from_previous\" }}\n\n  [[setup.toolpath]]\n  id   = \"t2\"\n  name = \"Profile\"\n  type = \"contour_2d\"\n  tool = \"shop#flat-6\"\n\n    [setup.toolpath.geometry]\n    drive_curves = [{{ face = \"f-{id}.8\" }}]\n",
        project(&id)
    );
    std::fs::write(dir.join("p.fluut"), text).unwrap();
    Session::open(&dir.join("p.fluut"), &[]).unwrap()
}

#[test]
fn child_setups_inherit_frames_and_go_stale_with_the_parent() {
    let mut s = chained("chained");
    let r = s.calculate(&Targets::All);
    assert!(
        r.iter()
            .all(|r| r.state == JobState::Done { cached: false }),
        "{r:?}"
    );
    let a = s.result("t1").unwrap();
    let b = s.result("t2").unwrap();
    // Same tool, parameters and inherited frame: the same path.
    assert_eq!(a, b);
    let p2 = (*s.project.setups[1]).clone();
    let f = frame(&s, &p2);
    assert_eq!(f.stock.max, Point3::new(50.0, 40.0, 0.0));
    // A parent edit restales the child.
    s.project.setup_mut("s1").unwrap().stock = Some(fluut_model::project::StockSpec::Block {
        margin: Some([Num::int(6), Num::int(6), Num::int(1)]),
        size: None,
    });
    assert!(s.is_stale("t2"));
    // A broken chain fails the child's toolpaths.
    s.project.setup_mut("s2").unwrap().based_on = Some("s9".into());
    let r = s.calculate(&Targets::Toolpaths(vec!["t2".into()]));
    assert_eq!(r[0].state, JobState::Failed);
    assert_eq!(r[0].diagnostics[0].code, "setup.based_on.missing");
}

#[test]
fn frames_take_scale_size_and_face_picks() {
    let s = open("frames2");
    let body = s.store.bodies().next().unwrap().id.clone();
    let stem = body.strip_prefix("b-").unwrap();
    let mut setup = (*s.project.setups[0]).clone();
    setup.geometry = Some(fluut_model::project::GeometrySel {
        scale: Some([Num::int(2); 3]),
        mirror: Some([false, true, false]),
        ..Default::default()
    });
    setup.stock = Some(fluut_model::project::StockSpec::Block {
        margin: None,
        size: Some([Num::int(100), Num::int(70), Num::int(25)]),
    });
    setup.zero = Some(ZeroSpec::StockTopCentre);
    let f = frame(&s, &setup);
    // The boss is 80 × 60 × 20 after scaling (mirrored in Y), centred in a
    // 100 × 70 × 25 box standing on the model bottom.
    assert_eq!(f.stock.max.z(), 0.0);
    assert_eq!(f.stock.min.z(), -25.0);
    assert!((f.stock.max.x() - 50.0).abs() < 1e-9 && (f.stock.min.y() + 35.0).abs() < 1e-9);
    assert!((f.model.max.x() - 40.0).abs() < 1e-9, "{:?}", f.model);
    assert!((f.model.max.y() - 30.0).abs() < 1e-9, "{:?}", f.model);
    assert!((f.model.min.z() + 25.0).abs() < 1e-9, "{:?}", f.model);
    // The top face of the boss as the tool axis: the same as +Z.
    let top = (0..20).map(|i| format!("f-{stem}.{i}")).find(|id| {
        let mut t = (*s.project.setups[0]).clone();
        t.orientation = Some(Orientation::ThreeAxis {
            tool_axis: fluut_model::project::AxisPick::Face(id.clone()),
            x_axis: AxisDir::PosX.into(),
        });
        try_frame(&s, &t).is_ok_and(|f| f.to_work == frame(&s, &s.project.setups[0]).to_work)
    });
    assert!(top.is_some());
    let mut t = (*s.project.setups[0]).clone();
    t.orientation = Some(Orientation::ThreeAxis {
        tool_axis: fluut_model::project::AxisPick::Face("f-nope.1".into()),
        x_axis: AxisDir::PosX.into(),
    });
    assert_eq!(
        try_frame(&s, &t).unwrap_err().code,
        "setup.orientation.pick"
    );
    t.kind = Some(fluut_model::project::SetupKind::Rotary);
    assert!(try_frame(&s, &t).is_err());
}

#[test]
fn machines_and_posts_come_from_the_library() {
    let (dir, id) = workspace("library");
    let user = dir.join("user-defs");
    std::fs::create_dir_all(user.join("posts")).unwrap();
    std::fs::create_dir_all(dir.join("machines")).unwrap();
    // A user post extending a built-in one, and a project machine that
    // sets one post word.
    std::fs::write(
        user.join("posts/mine.post.toml"),
        "format = \"fluut.post/1\"\nid = \"user/mine\"\nversion = \"1.0\"\nextends = \"fluut/linuxcnc@1\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("machines/router.machine.toml"),
        "format = \"fluut.machine/1\"\nid = \"project/router\"\nversion = \"1.0\"\n\
         name = \"Shop router\"\nextends = \"fluut/generic-3axis-router@1\"\n\
         [post_overrides]\n\"words.F.prefix\" = \"F=\"\n",
    )
    .unwrap();
    let text = project(&id).replace(
        "material = \"MDF\"",
        "machine  = \"project/router@1\"\nmaterial = \"MDF\"",
    );
    std::fs::write(dir.join("p.fluut"), text).unwrap();
    let opts = fluut_core::OpenOptions {
        tool_files: Vec::new(),
        user_dir: Some(user.clone()),
        ..Default::default()
    };
    let mut s = Session::open_with(&dir.join("p.fluut"), &opts).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    let m = s.machines();
    assert_eq!(
        m["project/router@1"].as_ref().unwrap().fields.name,
        "Shop router"
    );
    let posts = s.library.mem_store(fluut_model::library::DefKind::Post);
    let popts = PostOptions {
        date: "2026-11-01".into(),
        time: "12:00:00".into(),
        notes: String::new(),
    };
    let (out, _) = s.post("user/mine", &posts, &popts).unwrap();
    let g = &out.files[0];
    assert!(g.contains(" F=2000"), "{g}");
    assert!(g.contains("project/router@1.0"), "{g}");
    // A machine that is not in the library is a warning at open.
    let text = project(&id).replace(
        "material = \"MDF\"",
        "machine  = \"user/gone@1\"\nmaterial = \"MDF\"",
    );
    std::fs::write(dir.join("p.fluut"), text).unwrap();
    let s = Session::open_with(&dir.join("p.fluut"), &opts).unwrap();
    assert_eq!(s.warnings[0].code, "setup.machine.missing");
    assert!(!s.warnings[0].is_error());
}

#[test]
fn rest_toolpaths_depend_on_earlier_stock_updates() {
    let (dir, id) = workspace("rest-hash");
    let extra = format!(
        r#"
  [[setup.toolpath]]
  id   = "t2"
  name = "Finish"
  type = "parallel"
  tool = "shop#flat-6"

  [[setup.toolpath]]
  id   = "t3"
  name = "Rest"
  type = "pocket_2d"
  tool = "shop#flat-6"

    [setup.toolpath.geometry]
    drive_curves = [{{ face = "f-{id}.8" }}]
    rest_from    = "previous"
"#
    );
    std::fs::write(dir.join("p.fluut"), project(&id) + &extra).unwrap();
    let mut s = Session::open(&dir.join("p.fluut"), &[]).unwrap();
    let g0 = s.graph();
    assert!(
        g0.nodes[&NodeId::Result("t3".into())]
            .deps
            .contains(&NodeId::Result("t1".into()))
    );
    // An R edit of t1 (which updates stock) reaches t3, not t2 (parallel
    // does not read the stock).
    s.project
        .toolpath_mut("t1")
        .unwrap()
        .params
        .set("passes.stock_to_leave", Some(PValue::Num(Num::float(0.5))))
        .unwrap();
    let g1 = s.graph();
    let changed = g1.changed(&g0);
    assert!(
        changed.contains(&NodeId::Result("t3".into())),
        "{changed:?}"
    );
    assert!(
        !changed.contains(&NodeId::Result("t2".into())),
        "{changed:?}"
    );
    // A P edit of t1 does not; disabling it does.
    s.project
        .toolpath_mut("t1")
        .unwrap()
        .params
        .set("tool_params.feed.cut", Some(PValue::Num(Num::int(1500))))
        .unwrap();
    assert!(
        !s.graph()
            .changed(&g1)
            .contains(&NodeId::Result("t3".into()))
    );
    let g2 = s.graph();
    s.project.toolpath_mut("t1").unwrap().enabled = Some(false);
    assert!(
        s.graph()
            .changed(&g2)
            .contains(&NodeId::Result("t3".into()))
    );
    // Editing t2 (no stock update) does not reach t3.
    let g3 = s.graph();
    s.project
        .toolpath_mut("t2")
        .unwrap()
        .params
        .set("passes.stock_to_leave", Some(PValue::Num(Num::float(0.5))))
        .unwrap();
    assert!(
        !s.graph()
            .changed(&g3)
            .contains(&NodeId::Result("t3".into()))
    );
}

#[test]
fn an_edit_group_is_one_undo_step() {
    use fluut_model::edit::Edit;
    let mut s = open("group");
    let before = s.project.clone();
    s.begin_group("Script job.rhai");
    s.apply(Edit::SetName("A".into())).unwrap();
    s.apply(Edit::set_toolpath_field("t1", "name", Some("\"B\"")))
        .unwrap();
    assert_eq!(s.project.name, "A");
    let out = s.end_group().unwrap();
    assert_eq!(out.label, "Script job.rhai");
    assert_eq!(s.undo().unwrap().label, "Script job.rhai");
    assert_eq!(s.project, before);
    assert!(s.undo().is_none());
}

#[test]
fn post_one_setup() {
    let (dir, id) = workspace("post_setups");
    let mut text = project(&id);
    text.push_str(
        r#"
[[setup]]
id       = "s2"
name     = "Again"
based_on = "s1"

  [[setup.toolpath]]
  id   = "t2"
  name = "Second"
  type = "contour_2d"
  tool = "shop#flat-6"

    [setup.toolpath.geometry]
"#,
    );
    text.push_str(&format!("    drive_curves = [{{ face = \"f-{id}.8\" }}]\n"));
    std::fs::write(dir.join("p.fluut"), text).unwrap();
    let mut s = Session::open(&dir.join("p.fluut"), &[]).unwrap();
    let opts = PostOptions::default();
    let (all, _) = s.post("fluut/linuxcnc", &builtin_store(), &opts).unwrap();
    assert!(all.files[0].contains("(Profile)") && all.files[0].contains("(Second)"));
    let (one, reports) = s
        .post_setups(
            "fluut/linuxcnc",
            &builtin_store(),
            &opts,
            Some(&["s2".to_owned()]),
        )
        .unwrap();
    assert_eq!(reports.len(), 1);
    assert!(!one.files[0].contains("(Profile)") && one.files[0].contains("(Second)"));
    let e = s
        .post_setups(
            "fluut/linuxcnc",
            &builtin_store(),
            &opts,
            Some(&["x".into()]),
        )
        .unwrap_err();
    assert_eq!(e[0].code, "post.setup.missing");
}

#[test]
fn linking_toolpaths_depend_on_earlier_stock_updates() {
    // A 2-D toolpath's rapids stop above the in-process stock (D08 §8.2),
    // so its result follows the earlier stock-updating toolpaths, without
    // `rest_from`. A 3-D finishing toolpath's does not.
    let (dir, id) = workspace("link-hash");
    let extra = format!(
        r#"
  [[setup.toolpath]]
  id   = "t2"
  name = "Finish"
  type = "parallel"
  tool = "shop#flat-6"

  [[setup.toolpath]]
  id   = "t3"
  name = "Again"
  type = "contour_2d"
  tool = "shop#flat-6"

    [setup.toolpath.geometry]
    drive_curves = [{{ face = "f-{id}.8" }}]
"#
    );
    std::fs::write(dir.join("p.fluut"), project(&id) + &extra).unwrap();
    let mut s = Session::open(&dir.join("p.fluut"), &[]).unwrap();
    let g0 = s.graph();
    assert!(
        g0.nodes[&NodeId::Result("t3".into())]
            .deps
            .contains(&NodeId::Result("t1".into()))
    );
    s.project
        .toolpath_mut("t1")
        .unwrap()
        .params
        .set("passes.stock_to_leave", Some(PValue::Num(Num::float(0.5))))
        .unwrap();
    let changed = s.graph().changed(&g0);
    assert!(
        changed.contains(&NodeId::Result("t3".into())),
        "{changed:?}"
    );
    assert!(
        !changed.contains(&NodeId::Result("t2".into())),
        "{changed:?}"
    );
    // Calculating t3 alone calculates t1 first.
    let r = s.calculate(&Targets::Toolpaths(vec!["t3".into()]));
    let ids: Vec<&str> = r.iter().map(|r| r.toolpath.as_str()).collect();
    assert_eq!(ids, ["t1", "t2", "t3"]);
}

#[test]
fn a_child_setup_starts_from_the_parent_stock() {
    // Setup 2 (`from_previous`) profiles the boss again from 5 mm below
    // the stock top. Setup 1 cut the ring round the boss, so the child's
    // rapids go down to its feed height there, not to the stock top +
    // the safety distance.
    let (dir, id) = workspace("child-stock");
    let text = format!(
        "{}\n[[setup]]\nid       = \"s2\"\nname     = \"Again\"\nbased_on = \"s1\"\nstock    = {{ kind = \"from_previous\" }}\n\n  [[setup.toolpath]]\n  id   = \"t2\"\n  name = \"Lower\"\n  type = \"contour_2d\"\n  tool = \"shop#flat-6\"\n\n    [setup.toolpath.geometry]\n    drive_curves = [{{ face = \"f-{id}.8\" }}]\n\n    [setup.toolpath.heights]\n    top = {{ ref = \"stock_top\", offset = -5 }}\n",
        project(&id)
    );
    std::fs::write(dir.join("p.fluut"), &text).unwrap();
    let mut s = Session::open(&dir.join("p.fluut"), &[]).unwrap();
    let g0 = s.graph();
    assert!(
        g0.nodes[&NodeId::Result("t2".into())]
            .deps
            .contains(&NodeId::Result("t1".into()))
    );
    // Calculating the child alone calculates the parent first.
    let r = s.calculate(&Targets::Toolpaths(vec!["t2".into()]));
    let ids: Vec<&str> = r.iter().map(|r| r.toolpath.as_str()).collect();
    assert_eq!(ids, ["t1", "t2"]);
    assert!(
        r.iter().all(|r| matches!(r.state, JobState::Done { .. })),
        "{r:?}"
    );
    let low = |s: &Session| {
        s.result("t2")
            .unwrap()
            .segments()
            .filter(|x| {
                x.motion()
                    .is_some_and(|m| m.motion == fluut_path::Motion::Rapid)
            })
            .map(|x| x.end().unwrap().xyz.z())
            .fold(f64::INFINITY, f64::min)
    };
    // Stock top 0 in W (the zero is on it): the feed height −3.
    assert_eq!(low(&s), -3.0);
    // A parent edit restales the child.
    s.project
        .toolpath_mut("t1")
        .unwrap()
        .params
        .set("passes.stock_to_leave", Some(PValue::Num(Num::float(0.5))))
        .unwrap();
    assert!(s.is_stale("t2"));
    // Without the parent's toolpath the child sees the whole stock.
    s.project.toolpath_mut("t1").unwrap().enabled = Some(false);
    s.calculate(&Targets::All);
    assert_eq!(low(&s), 1.0);
}
