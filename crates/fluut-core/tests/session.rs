// SPDX-License-Identifier: MIT OR Apache-2.0

//! Session, graph and frames on a project written to a temporary folder.

#[path = "../../fluut-cam/tests/support/step.rs"]
mod step;

use fluut_core::{JobState, NodeId, PostOptions, Session, SetupFrame, Targets};
use fluut_geom::Point3;
use fluut_model::params::{Num, PValue};
use fluut_model::project::{AxisDir, Corner, Orientation, Setup, ZeroSpec};
use fluut_post::builtin::builtin_store;
use std::path::PathBuf;

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
    assert_eq!(g1.changed(&g0), [NodeId::Post("t1".into())]);
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
        [NodeId::Result("t1".into()), NodeId::Post("t1".into())]
    );
    assert!(s.is_stale("t1"));
    // A Setup edit makes every toolpath of the Setup stale.
    let g2 = s.graph();
    s.project.setups[0].work_offset = Some("G55".into());
    assert!(s.graph().changed(&g2).contains(&NodeId::Setup("s1".into())));
}

#[test]
fn missing_tool_and_geometry() {
    let mut s = open("missing");
    s.project.setups[0].toolpaths[0].tool = "shop#nope".into();
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

fn frame(s: &Session, setup: &Setup) -> SetupFrame {
    SetupFrame::new(setup, s.store.bodies().cloned().collect()).unwrap()
}

#[test]
fn frames_place_the_zero_and_stock() {
    let s = open("frames");
    let mut setup = s.project.setups[0].clone();
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
    setup.orientation = Some(Orientation {
        tool_axis: AxisDir::NegY,
        x_axis: AxisDir::PosX,
    });
    let f = frame(&s, &setup);
    assert_eq!(
        f.to_work.apply_point(Point3::new(0.0, -1.0, 0.0)),
        Point3::new(0.0, 0.0, 1.0)
    );
    assert!((f.model.max.z() - 0.0).abs() < 1e-9 && (f.model.min.z() + 30.0).abs() < 1e-9);
    setup.orientation = Some(Orientation {
        tool_axis: AxisDir::PosZ,
        x_axis: AxisDir::NegZ,
    });
    assert!(SetupFrame::new(&setup, s.store.bodies().cloned().collect()).is_err());
}
