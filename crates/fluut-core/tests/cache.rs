// SPDX-License-Identifier: MIT OR Apache-2.0

//! The result cache sidecar (D06 §7.5) and the session's save, journal
//! and recovery (D06 §7).

#[path = "../../fluut-cam/tests/support/step.rs"]
mod step;

use fluut_core::cache::{DiskCache, KEEP_UNUSED, decode, encode};
use fluut_core::{JobState, OpenOptions, Session, Targets};
use fluut_geom::{Point3, UnitVec3};
use fluut_model::Diagnostic;
use fluut_model::edit::Edit;
use fluut_path::{
    AxisPos, CompSide, CycleKind, CycleParams, FeedRef, MarkerKind, Motion, MotionInfo, Pass,
    PassKind, PersistId, Segment, ToolpathResult,
};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn at(x: f64, y: f64, z: f64) -> AxisPos {
    AxisPos {
        xyz: Point3::new(x, y, z),
        a: Some(0.0),
        b: None,
    }
}

fn mi(motion: Motion, feed: FeedRef) -> MotionInfo {
    MotionInfo {
        motion,
        feed,
        tol: 0.01,
        face: Some(PersistId(u128::MAX - 7)),
    }
}

/// One result with every segment kind.
fn sample() -> ToolpathResult {
    let mut rot_end = at(0.0, 0.0, -1.0);
    rot_end.a = Some(90.0);
    let p1 = Pass::new(
        PassKind::Cut,
        vec![
            Segment::Line {
                a: at(0.1, -0.0, 5.0),
                b: at(0.1, 0.0, 0.0),
                m: mi(Motion::Plunge, FeedRef::Plunge),
            },
            Segment::Marker {
                kind: MarkerKind::Comment("hello (world)".into()),
            },
            Segment::Arc {
                a: at(0.1, 0.0, 0.0),
                b: at(10.1, 0.0, 0.0),
                centre: Point3::new(5.1, 0.0, 0.0),
                normal: UnitVec3::Z,
                sweep: std::f64::consts::PI,
                m: mi(Motion::Cut, FeedRef::Cut),
            },
            Segment::Helix {
                a: at(10.1, 0.0, 0.0),
                b: at(0.1, 0.0, -1.0),
                centre: Point3::new(5.1, 0.0, 0.0),
                normal: -UnitVec3::Z,
                sweep: -std::f64::consts::PI,
                m: mi(Motion::Ramp, FeedRef::Ramp),
            },
        ],
    )
    .with_comp(CompSide::Right);
    let p2 = Pass::new(
        PassKind::Link,
        vec![
            Segment::Marker {
                kind: MarkerKind::Dwell(0.5),
            },
            Segment::Line {
                a: at(0.1, 0.0, -1.0),
                b: at(0.0, 0.0, -1.0),
                m: mi(Motion::Link, FeedRef::Transition),
            },
            Segment::Rotary {
                a: at(0.0, 0.0, -1.0),
                b: rot_end,
                radius: 12.5,
                m: mi(Motion::Cut, FeedRef::Cut),
            },
            Segment::Cycle {
                at: rot_end,
                cycle: CycleParams {
                    kind: CycleKind::Peck,
                    top_z: 0.0,
                    depth_z: -5.0,
                    retract_z: 2.0,
                    peck: Some(1.0),
                    dwell_s: None,
                },
                m: mi(Motion::Cut, FeedRef::Plunge),
            },
            Segment::Marker {
                kind: MarkerKind::OptionalStop,
            },
            Segment::Marker {
                kind: MarkerKind::PassThrough("M7".into()),
            },
            Segment::Marker {
                kind: MarkerKind::Stop,
            },
        ],
    );
    ToolpathResult::new(vec![p1, p2]).unwrap()
}

fn diags() -> Vec<Diagnostic> {
    vec![
        Diagnostic::warning("cam.parallel.empty", "nothing").on("t1"),
        Diagnostic::error("x.y", "z").at("passes.stepover"),
    ]
}

#[test]
fn files_round_trip_bit_for_bit() {
    let r = sample();
    let bytes = encode("sha256:ab", &r, &diags()).unwrap();
    assert_eq!(&bytes[..4], b"FLUT");
    let (back, d) = decode(&bytes, "sha256:ab").unwrap();
    assert_eq!(back, r);
    assert_eq!(d, diags());
    // Bitwise: the sign of -0.0 survives.
    let first = back.segments().next().unwrap().start().unwrap();
    assert_eq!(first.xyz.y().to_bits(), (-0.0f64).to_bits());
}

#[test]
fn damaged_files_are_misses() {
    let bytes = encode("sha256:ab", &sample(), &diags()).unwrap();
    assert!(decode(&bytes, "sha256:cd").is_none());
    for cut in [0, 3, 6, bytes.len() / 2, bytes.len() - 1] {
        assert!(decode(&bytes[..cut], "sha256:ab").is_none(), "{cut}");
    }
    for i in [0, 4, 10, bytes.len() / 2, bytes.len() - 2] {
        let mut b = bytes.clone();
        b[i] ^= 1;
        assert!(decode(&b, "sha256:ab").is_none(), "{i}");
    }
    // Another schema, with a valid check: still a miss.
    let mut b = bytes[..bytes.len() - 4].to_vec();
    b[4] = 99;
    let crc = crc32fast::hash(&b);
    b.extend_from_slice(&crc.to_le_bytes());
    assert!(decode(&b, "sha256:ab").is_none());
}

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn gc_keeps_referenced_and_recent_files() {
    let c = DiskCache::new(dir("cache-gc").join("p.fluut-cache"));
    let r = sample();
    for h in ["sha256:01", "sha256:02", "sha256:03"] {
        assert!(c.put(h, &r, &[]).unwrap());
    }
    let old = SystemTime::now() - KEEP_UNUSED - Duration::from_secs(60);
    for h in ["sha256:01", "sha256:02"] {
        std::fs::File::options()
            .append(true)
            .open(c.file(h))
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
    let keep: BTreeSet<String> = ["sha256:01".to_owned()].into();
    assert_eq!(c.gc(&keep, SystemTime::now()).unwrap(), 1);
    assert!(c.file("sha256:01").exists(), "referenced");
    assert!(!c.file("sha256:02").exists(), "old and unreferenced");
    assert!(c.file("sha256:03").exists(), "recent");
    // A hit counts as use.
    std::fs::File::options()
        .append(true)
        .open(c.file("sha256:03"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    assert!(c.get("sha256:03").is_some());
    assert_eq!(c.gc(&BTreeSet::new(), SystemTime::now()).unwrap(), 1);
    assert!(c.file("sha256:03").exists());
    // No folder, nothing to do.
    assert_eq!(
        DiskCache::new(dir("cache-none").join("x"))
            .gc(&keep, SystemTime::now())
            .unwrap(),
        0
    );
}

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

/// The boss project in a fresh folder.
fn workspace(name: &str) -> PathBuf {
    let d = dir(name);
    std::fs::create_dir_all(d.join("tools")).unwrap();
    let text = step::boss([0.0; 3]);
    std::fs::write(d.join("boss.step"), &text).unwrap();
    std::fs::write(d.join("tools/shop.toml"), TOOLS).unwrap();
    let id = text
        .lines()
        .find(|l| l.contains("MANIFOLD_SOLID_BREP"))
        .and_then(|l| l.strip_prefix('#'))
        .and_then(|l| l.split('=').next())
        .unwrap()
        .to_owned();
    let project = format!(
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
    drive_curves = [{{ face = "f-{id}.8" }}]

  [[setup.toolpath]]
  id   = "t2"
  name = "Finish"
  type = "contour_2d"
  tool = "shop#flat-6"

    [setup.toolpath.geometry]
    drive_curves = [{{ face = "f-{id}.8" }}]

    [setup.toolpath.passes]
    step_down = 2.0
"#
    );
    std::fs::write(d.join("p.fluut"), project).unwrap();
    d.join("p.fluut")
}

fn opts() -> OpenOptions {
    OpenOptions {
        cache: true,
        journal: true,
        recover: true,
        ..OpenOptions::default()
    }
}

fn states(r: &[fluut_core::JobReport]) -> Vec<JobState> {
    r.iter().map(|r| r.state).collect()
}

#[test]
fn results_survive_a_reload_and_follow_the_r_p_split() {
    let path = workspace("cache-session");
    let mut s = Session::open_with(&path, &opts()).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    let r = s.calculate(&Targets::All);
    assert_eq!(states(&r), [JobState::Done { cached: false }; 2], "{r:?}");
    assert_eq!(s.calculated, 2);
    let t1 = s.result("t1").unwrap();
    drop(s);

    // Reload: both results come from the files, nothing is calculated.
    let mut s = Session::open_with(&path, &opts()).unwrap();
    let r = s.calculate(&Targets::All);
    assert_eq!(states(&r), [JobState::Done { cached: true }; 2]);
    assert_eq!(s.calculated, 0);
    assert_eq!(*s.result("t1").unwrap(), *t1);
    drop(s);

    // A P edit (cut feed) keeps the cache file.
    let mut s = Session::open_with(&path, &opts()).unwrap();
    s.apply(Edit::set_toolpath_field(
        "t1",
        "tool_params.feed.cut",
        Some("1500"),
    ))
    .unwrap();
    let r = s.calculate(&Targets::All);
    assert_eq!(states(&r), [JobState::Done { cached: true }; 2]);
    assert_eq!(s.calculated, 0);
    // An R edit (stock to leave) misses it; the other toolpath still hits.
    s.apply(Edit::set_toolpath_field(
        "t1",
        "passes.stock_to_leave",
        Some("0.5"),
    ))
    .unwrap();
    let r = s.calculate(&Targets::All);
    assert_eq!(
        states(&r),
        [
            JobState::Done { cached: false },
            JobState::Done { cached: true }
        ]
    );
    assert_eq!(s.calculated, 1);
    // Undo makes the first result current again, from memory.
    s.undo().unwrap();
    assert!(!s.is_stale("t1"));
}

#[test]
fn save_collects_garbage_and_resets_the_journal() {
    let path = workspace("cache-save");
    let mut s = Session::open_with(&path, &opts()).unwrap();
    s.calculate(&Targets::All);
    let cache = DiskCache::for_project(&path);
    let n = std::fs::read_dir(cache.dir()).unwrap().count();
    assert_eq!(n, 2);
    // A stale file from an earlier edit, unused for 8 days.
    let stale = cache.file("sha256:dead");
    std::fs::copy(
        std::fs::read_dir(cache.dir())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path(),
        &stale,
    )
    .unwrap();
    std::fs::File::options()
        .append(true)
        .open(&stale)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(8 * 24 * 3600))
        .unwrap();
    s.apply(Edit::SetName("Saved".into())).unwrap();
    assert_eq!(Session::pending_recovery(&path), Some(1));
    s.save().unwrap();
    assert!(!stale.exists());
    assert_eq!(std::fs::read_dir(cache.dir()).unwrap().count(), 2);
    // Saved: nothing to recover; the file has the edit.
    assert_eq!(Session::pending_recovery(&path), None);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("name   = \"Saved\""), "{text}");
    assert!(fluut_model::persist::backup_path(&path).exists());
}

#[test]
fn a_crash_is_recovered_on_open() {
    let path = workspace("cache-crash");
    let mut s = Session::open_with(&path, &opts()).unwrap();
    s.apply(Edit::set_setup_field("s1", "material", Some("\"Oak\"")))
        .unwrap();
    s.apply(Edit::RemoveToolpath("t2".into())).unwrap();
    s.apply(Edit::SetName("A".into())).unwrap();
    s.undo().unwrap();
    let want = s.project.clone();
    // Crash: the session goes away without saving.
    drop(s);
    assert_eq!(Session::pending_recovery(&path), Some(4));
    let s = Session::open_with(&path, &opts()).unwrap();
    assert_eq!(s.project, want);
    assert!(
        s.warnings
            .iter()
            .any(|w| w.code == "model.journal.recovered")
    );
    drop(s);
    // The recovered edits are in the new journal: a second crash keeps them.
    let s = Session::open_with(&path, &opts()).unwrap();
    assert_eq!(s.project, want);
    // Without `recover`, the file is opened as saved.
    let s = Session::open_with(
        &path,
        &OpenOptions {
            recover: false,
            ..opts()
        },
    )
    .unwrap();
    assert_eq!(s.project.setups[0].material.as_deref(), Some("MDF"));
}

#[test]
fn every_reference_result_is_stored_exactly() {
    let root = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fluut-cli/tests/projects"
    ));
    for name in ["block", "boss", "plate", "ridge"] {
        let path = root.join(name).join(format!("{name}.fluut"));
        let mut s = Session::open(&path, &[]).unwrap();
        let reports = s.calculate(&Targets::All);
        for r in reports {
            let res = s.result(&r.toolpath).unwrap();
            let bytes = encode("sha256:00", &res, &r.diagnostics)
                .unwrap_or_else(|| panic!("{name} {}: not cacheable", r.toolpath));
            let (back, d) = decode(&bytes, "sha256:00").unwrap();
            assert_eq!(back, *res, "{name} {}", r.toolpath);
            assert_eq!(d, r.diagnostics);
        }
    }
}
