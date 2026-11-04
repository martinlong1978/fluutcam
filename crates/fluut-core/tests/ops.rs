// SPDX-License-Identifier: MIT OR Apache-2.0

//! Project operations: new project, save as, import, add Setup, add toolpath.

use fluut_core::ops::{DEFAULT_STOCK_MARGIN, default_tool};
use fluut_core::{JobState, PostOptions, Session, SetupFrame, Targets};
use fluut_model::project::{DisplayUnit, SetupKind, StockSpec, ZeroSpec, write};
use fluut_model::setup::resolve_setup;
use fluut_model::validate::{ValidateCx, validate};
use fluut_model::{Severity, ToolpathTypeId};
use std::path::{Path, PathBuf};

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("ops-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sample(project: &str, file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fluut-cli/tests/projects")
        .join(project)
        .join(file)
}

fn new(name: &str) -> (Session, PathBuf) {
    let d = dir(name);
    let s = Session::new_project(&d.join("np.fluut"), DisplayUnit::Mm).unwrap();
    (s, d)
}

fn boss(name: &str) -> (Session, PathBuf) {
    let (mut s, d) = new(name);
    s.import_model(&sample("boss", "boss.step")).unwrap();
    (s, d)
}

#[test]
fn new_project_is_a_valid_file() {
    let d = dir("new");
    let s = Session::new_project(&d.join("Widget"), DisplayUnit::Inch).unwrap();
    assert_eq!(s.path.as_deref(), Some(d.join("Widget.fluut").as_path()));
    assert!(!s.is_dirty());
    let o = Session::open(&d.join("Widget.fluut"), &[]).unwrap();
    assert_eq!(o.project.name, "Widget");
    assert_eq!(o.project.units, DisplayUnit::Inch);
    assert!(o.project.setups.is_empty() && o.project.geometry.is_empty());
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    // A missing folder is a diagnostic.
    let e = Session::new_project(&d.join("no/such/x.fluut"), DisplayUnit::Mm).unwrap_err();
    assert_eq!(e[0].code, "model.project.save");
}

#[test]
fn import_copies_a_file_from_outside_and_references_one_inside() {
    let (mut s, d) = new("import");
    let r = s.import_model(&sample("boss", "boss.step")).unwrap();
    assert!(r.copied && r.bodies == ["b-334"], "{r:?}");
    assert!(d.join("boss.step").is_file());
    assert_eq!(s.project.geometry[0].path, "boss.step");
    assert!(s.project.geometry[0].hash.is_some());
    assert!(s.is_dirty());
    assert_eq!(s.store.bodies().count(), 1);
    // The same file again is a clash of body ids, and nothing changes.
    let before = s.project.clone();
    let e = s.import_model(&sample("boss", "boss.step")).unwrap_err();
    assert_eq!(e[0].code, "model.import.body_ids");
    assert_eq!(s.project, before);
    // A file inside the folder is referenced where it is.
    let (mut s, d) = new("import-inside");
    std::fs::create_dir_all(d.join("parts")).unwrap();
    std::fs::copy(sample("block", "block.step"), d.join("parts/block.step")).unwrap();
    let r = s.import_model(&d.join("parts/block.step")).unwrap();
    assert!(!r.copied);
    assert_eq!(s.project.geometry[0].path, "parts/block.step");
}

#[test]
fn bad_files_are_diagnostics_and_change_nothing() {
    let (mut s, d) = new("bad");
    let before = s.project.clone();
    let bad = d.join("bad.step");
    std::fs::write(&bad, "this is not a STEP file").unwrap();
    std::fs::write(d.join("a.txt"), "x").unwrap();
    std::fs::write(d.join("cube.stl"), "solid x\nendsolid x\n").unwrap();
    for (f, code) in [
        (bad.clone(), "model.geometry.step"),
        (d.join("missing.step"), "model.import.read"),
        (d.join("a.txt"), "model.import.kind"),
    ] {
        let e = s.import_model(&f).unwrap_err();
        assert!(e.iter().any(|x| x.code == code), "{f:?}: {e:?}");
    }
    assert!(s.import_model(&d.join("cube.stl")).is_err());
    assert_eq!(s.project, before);
    assert!(s.store.bodies().next().is_none());
    // No project file yet.
    let mut loose = Session::open(&sample("boss", "boss.fluut"), &[]).unwrap();
    loose.path = None;
    assert_eq!(
        loose.import_model(&bad).unwrap_err()[0].code,
        "model.import.no_project"
    );
}

#[test]
fn a_new_three_axis_setup_has_the_documented_defaults() {
    let (mut s, _d) = boss("setup");
    let id = s.add_setup(SetupKind::ThreeAxis, None, "Setup 1").unwrap();
    assert_eq!(id, "s1");
    let st = s.project.setup("s1").unwrap();
    assert_eq!(st.name, "Setup 1");
    assert_eq!(st.machine.as_deref(), Some("fluut/generic-3axis-router@1"));
    assert_eq!(st.zero, Some(ZeroSpec::default()));
    let r = resolve_setup(&s.project, "s1").unwrap();
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    // The machine and its post resolve.
    assert!(
        s.machines().values().all(Result::is_ok),
        "{:?}",
        s.machines()
    );
    let f = SetupFrame::new(&r, s.store.bodies().cloned().collect()).unwrap();
    let m = DEFAULT_STOCK_MARGIN.map(|v| v as f64);
    let (mn, mx, sn, sx) = (f.model.min, f.model.max, f.stock.min, f.stock.max);
    // Work zero at the stock's top front-left corner.
    assert!(sn.x().abs() < 1e-9 && sn.y().abs() < 1e-9 && sx.z().abs() < 1e-9);
    // The stock is the model bounds plus the margin (X and Y both sides, top).
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
    assert!(near(mn.x() - sn.x(), m[0]) && near(sx.x() - mx.x(), m[0]));
    assert!(near(mn.y() - sn.y(), m[1]) && near(sx.y() - mx.y(), m[1]));
    assert!(near(sx.z() - mx.z(), m[2]) && near(sn.z(), mn.z()));
}

#[test]
fn based_on_inherits_and_checks_the_kind() {
    let (mut s, _d) = boss("based");
    s.add_setup(SetupKind::ThreeAxis, None, "Setup 1").unwrap();
    let id = s
        .add_setup(SetupKind::ThreeAxis, Some("s1"), "Setup 2")
        .unwrap();
    let c = s.project.setup(&id).unwrap();
    assert_eq!(c.based_on.as_deref(), Some("s1"));
    assert_eq!(c.stock, Some(StockSpec::FromPrevious));
    assert!(c.kind.is_none() && c.machine.is_none() && c.zero.is_none());
    assert!(c.toolpaths.is_empty());
    let r = resolve_setup(&s.project, &id).unwrap();
    assert_eq!(r.machine.as_deref(), Some("fluut/generic-3axis-router@1"));
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    let e = s
        .add_setup(SetupKind::Indexed, Some("s1"), "Setup 3")
        .unwrap_err();
    assert_eq!(e.code, "setup.add.kind");
    let e = s
        .add_setup(SetupKind::ThreeAxis, Some("nope"), "x")
        .unwrap_err();
    assert_eq!(e.code, "setup.missing");
}

#[test]
fn rotary_and_indexed_setups_get_a_cylinder_stock() {
    let (mut s, _d) = new("rotary");
    let e = s.add_setup(SetupKind::Rotary, None, "Setup 1").unwrap_err();
    assert_eq!(e.code, "setup.add.no_model");
    s.import_model(&sample("shaft", "shaft.step")).unwrap();
    s.add_setup(SetupKind::Rotary, None, "Setup 1").unwrap();
    s.add_setup(SetupKind::Indexed, Some("s1"), "Setup 2")
        .unwrap();
    for id in ["s1", "s2"] {
        let r = resolve_setup(&s.project, id).unwrap();
        assert!(r.diagnostics.is_empty(), "{id}: {:?}", r.diagnostics);
        SetupFrame::new(&r, s.store.bodies().cloned().collect()).unwrap();
    }
    assert!(
        s.machines().values().all(Result::is_ok),
        "{:?}",
        s.machines()
    );
}

#[test]
fn toolpath_defaults_by_type() {
    let (mut s, _d) = boss("tp");
    s.add_setup(SetupKind::ThreeAxis, None, "Setup 1").unwrap();
    for k in ToolpathTypeId::ALL {
        let t = default_tool(k);
        assert!(t.is_empty() == (k == ToolpathTypeId::ManualNc));
        assert!(t.is_empty() || s.tools.get(t).is_some(), "{k}: {t}");
    }
    let id = s
        .add_toolpath("s1", ToolpathTypeId::Parallel, "Parallel 1")
        .unwrap();
    assert_eq!(id, "t1");
    let (st, t) = s.project.toolpath("t1").unwrap();
    assert_eq!(t.tool, "fluut/starter#ball-6");
    let cx = ValidateCx {
        material: st.material.as_deref(),
        ..ValidateCx::new(s.tools.get(&t.tool))
    };
    let errors: Vec<_> = validate(t, &cx)
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    // A 2-D type has no selection yet, which validation says.
    s.add_toolpath("s1", ToolpathTypeId::Pocket2d, "Pocket 1")
        .unwrap();
    let (st, t) = s.project.toolpath("t2").unwrap();
    let cx = ValidateCx {
        material: st.material.as_deref(),
        ..ValidateCx::new(s.tools.get(&t.tool))
    };
    assert!(
        validate(t, &cx)
            .iter()
            .any(|d| d.code == "param.geometry.drive.empty")
    );
    assert_eq!(
        s.add_toolpath("nope", ToolpathTypeId::Face, "x")
            .unwrap_err()
            .code,
        "setup.missing"
    );
}

#[test]
fn new_import_setup_toolpath_calculate_post_save() {
    let (mut s, d) = boss("e2e");
    s.add_setup(SetupKind::ThreeAxis, None, "Setup 1").unwrap();
    s.add_toolpath("s1", ToolpathTypeId::Parallel, "Parallel 1")
        .unwrap();
    let reports = s.calculate(&Targets::All);
    assert!(
        reports
            .iter()
            .all(|r| matches!(r.state, JobState::Done { .. })),
        "{reports:?}"
    );
    let opts = PostOptions {
        date: "2026-11-04".into(),
        time: "12:00:00".into(),
        notes: String::new(),
    };
    let (out, _) = s
        .post(
            "fluut/linuxcnc",
            &fluut_post::builtin::builtin_store(),
            &opts,
        )
        .unwrap();
    assert!(out.files[0].contains("G1"), "{}", out.files[0]);
    // Save, reopen, save again: the same bytes.
    assert!(s.is_dirty());
    s.save().unwrap();
    assert!(!s.is_dirty());
    let path = d.join("np.fluut");
    let first = std::fs::read(&path).unwrap();
    assert_eq!(first, write(&s.project).into_bytes());
    let mut o = Session::open(&path, &[]).unwrap();
    assert_eq!(o.project, s.project);
    o.save().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), first);
}

#[test]
fn each_operation_is_one_undo_step() {
    let (mut s, _d) = new("undo");
    let mut states = vec![s.project.clone()];
    s.import_model(&sample("boss", "boss.step")).unwrap();
    states.push(s.project.clone());
    s.add_setup(SetupKind::ThreeAxis, None, "Setup 1").unwrap();
    states.push(s.project.clone());
    s.add_toolpath("s1", ToolpathTypeId::Parallel, "Parallel 1")
        .unwrap();
    states.push(s.project.clone());
    assert!(s.can_undo() && !s.can_redo());
    for i in (0..3).rev() {
        s.undo().unwrap();
        assert_eq!(s.project, states[i], "undo to {i}");
    }
    assert!(!s.can_undo() && s.can_redo());
    for (i, st) in states.iter().enumerate().skip(1) {
        s.redo().unwrap();
        assert_eq!(s.project, *st, "redo to {i}");
    }
}

#[test]
fn dirty_follows_the_saved_state() {
    let (mut s, _d) = boss("dirty");
    assert!(s.is_dirty());
    s.save().unwrap();
    assert!(!s.is_dirty());
    s.add_setup(SetupKind::ThreeAxis, None, "Setup 1").unwrap();
    assert!(s.is_dirty());
    s.undo().unwrap();
    assert!(!s.is_dirty());
    s.redo().unwrap();
    assert!(s.is_dirty());
}

#[test]
fn save_as_copies_what_the_project_needs_and_keeps_undo() {
    let (mut s, d) = boss("saveas");
    s.add_setup(SetupKind::ThreeAxis, None, "Setup 1").unwrap();
    // A project-layer tool library beside the project.
    std::fs::create_dir_all(d.join("tools")).unwrap();
    std::fs::copy(sample("boss", "tools/shop.toml"), d.join("tools/shop.toml")).unwrap();
    let other = d.join("elsewhere");
    s.save_as(&other.join("copy.fluut")).unwrap();
    assert!(!s.is_dirty());
    assert!(other.join("boss.step").is_file() && other.join("tools/shop.toml").is_file());
    let first = std::fs::read(other.join("copy.fluut")).unwrap();
    assert_eq!(first, write(&s.project).into_bytes());
    let mut o = Session::open(&other.join("copy.fluut"), &[]).unwrap();
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    assert_eq!(o.store.bodies().count(), 1);
    o.save().unwrap();
    assert_eq!(std::fs::read(other.join("copy.fluut")).unwrap(), first);
    // The history survives, and later saves go to the new file.
    s.add_toolpath("s1", ToolpathTypeId::Face, "Face 1")
        .unwrap();
    s.undo().unwrap();
    s.undo().unwrap();
    assert!(s.project.setups.is_empty() && s.is_dirty());
    s.save().unwrap();
    assert_eq!(
        Session::open(&other.join("copy.fluut"), &[])
            .unwrap()
            .project,
        s.project
    );
}
