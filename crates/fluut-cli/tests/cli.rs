// SPDX-License-Identifier: MIT OR Apache-2.0

//! End to end: STEP or DXF file + TOML project → G-code from the `fluut`
//! binary.
//! Golden output per reference project, translation invariance (spec 04
//! §4.12.7 (3)), and the command line's exit status.
//!
//! `FLUUT_BLESS=1` rewrites the STEP inputs (from the test writer) and the
//! golden G-code after a deliberate change; review the diff by eye.

#[path = "../../fluut-cam/tests/support/step.rs"]
mod step;

use fluut_core::{PostOptions, Session, Targets};
use fluut_path::Segment;
use std::path::{Path, PathBuf};
use std::process::Command;

fn here(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(p)
}

fn bless() -> bool {
    std::env::var_os("FLUUT_BLESS").is_some()
}

fn check_file(path: &Path, text: &str) {
    if bless() {
        std::fs::write(path, text).unwrap();
        return;
    }
    let want = std::fs::read_to_string(path)
        .unwrap_or_else(|_| panic!("{} missing: run with FLUUT_BLESS=1", path.display()));
    if want != text {
        let line = text
            .lines()
            .zip(want.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        panic!(
            "{} differs at line {}:\n  got:  {:?}\n  want: {:?}",
            path.display(),
            line + 1,
            text.lines().nth(line),
            want.lines().nth(line)
        );
    }
}

fn fluut(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fluut"))
        .args(args)
        .output()
        .unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn step_inputs_match_the_writer() {
    check_file(&here("projects/boss/boss.step"), &step::boss([0.0; 3]));
    check_file(
        &here("projects/block/block.step"),
        &step::pocket_block([0.0; 3]),
    );
    check_file(&here("projects/ridge/ridge.step"), &step::ridge([0.0; 3]));
    check_file(
        &here("projects/plate2d/plate2d.step"),
        &step::plate([0.0; 3]),
    );
    check_file(&here("projects/holes/holes.step"), &step::holes([0.0; 3]));
    check_file(&here("projects/dome/dome.step"), &step::dome([0.0; 3]));
    check_file(
        &here("projects/rough3d/rough3d.step"),
        &step::ridge([0.0; 3]),
    );
    check_file(&here("projects/shaft/shaft.step"), &step::shaft([0.0; 3]));
    check_file(
        &here("projects/flipside/flipside.step"),
        &step::two_sided([0.0; 3]),
    );
}

#[test]
fn projects_are_canonical() {
    for p in [
        "projects/boss/boss.fluut",
        "projects/block/block.fluut",
        "projects/ridge/ridge.fluut",
        "projects/plate/plate.fluut",
        "projects/plate2d/plate2d.fluut",
        "projects/holes/holes.fluut",
        "projects/dome/dome.fluut",
        "projects/rough3d/rough3d.fluut",
        "projects/shaft/shaft.fluut",
        "projects/flipside/flipside.fluut",
    ] {
        let text = std::fs::read_to_string(here(p)).unwrap();
        let l = fluut_model::project::read(&text).unwrap();
        assert!(l.warnings.is_empty(), "{p}: {:?}", l.warnings);
        assert_eq!(fluut_model::project::write(&l.project), text, "{p}");
    }
}

/// Posts `project` and compares with the golden file. `warnings` are the
/// codes the run must report, in order.
fn golden(project: &str, post: &str, name: &str, warnings: &[&str]) {
    let out = tmp(&format!("golden-{name}")).join("out.nc");
    let o = fluut(&[
        "post",
        here(project).to_str().unwrap(),
        "--post",
        post,
        "-o",
        out.to_str().unwrap(),
        "--date",
        "2026-11-01",
        "--time",
        "12:00:00",
    ]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{name}: {err}");
    let codes: Vec<&str> = err
        .lines()
        .filter_map(|l| {
            l.split_whitespace()
                .find(|w| w.starts_with("param.") || w.starts_with("cam."))
        })
        .map(|c| c.trim_end_matches(':'))
        .collect();
    assert_eq!(codes, warnings, "{name}: {err}");
    let text = std::fs::read_to_string(&out).unwrap();
    check_file(&here(&format!("golden/{name}")), &text);
}

#[test]
fn golden_contour_2d() {
    golden(
        "projects/boss/boss.fluut",
        "fluut/linuxcnc",
        "boss-linuxcnc.ngc",
        &[],
    );
}

#[test]
fn golden_pocket_2d_and_cut_out() {
    // The cut-out goes 0.2 mm into the spoilboard, and the profile ramp
    // cannot run over the tabs on the last level, so that level plunges.
    let w = ["param.heights.below_stock", "cam.linking.ramp_fallback"];
    golden(
        "projects/block/block.fluut",
        "fluut/linuxcnc",
        "block-linuxcnc.ngc",
        &w,
    );
    golden(
        "projects/block/block.fluut",
        "fluut/grbl",
        "block-grbl.gcode",
        &w,
    );
}

#[test]
fn golden_parallel() {
    golden(
        "projects/ridge/ridge.fluut",
        "fluut/linuxcnc",
        "ridge-linuxcnc.ngc",
        &[],
    );
}

#[test]
fn golden_3d_finishing() {
    // One toolpath of each 3-D finishing type (D15b §5) on the dome.
    golden(
        "projects/dome/dome.fluut",
        "fluut/linuxcnc",
        "dome-linuxcnc.ngc",
        &[],
    );
    let o = fluut(&["calc", here("projects/dome/dome.fluut").to_str().unwrap()]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        out.lines()
            .filter(|l| l.contains(": calculated; cut "))
            .count(),
        DOME.toolpaths.len(),
        "{out}"
    );
}

#[test]
fn golden_dxf_outline() {
    // DXF layers drive both 2-D types: a rounded pocket and a hole on
    // POCKET, a cut-out of lines and arcs on OUTLINE.
    golden(
        "projects/plate/plate.fluut",
        "fluut/linuxcnc",
        "plate-linuxcnc.ngc",
        &[],
    );
    let o = fluut(&["calc", here("projects/plate/plate.fluut").to_str().unwrap()]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(out.contains("t1 \"Pocket and hole\": calculated"), "{out}");
    assert!(out.contains("t2 \"Cut out\": calculated"), "{out}");
}

#[test]
fn golden_2d_types() {
    golden(
        "projects/plate2d/plate2d.fluut",
        "fluut/linuxcnc",
        "plate2d-linuxcnc.ngc",
        &[],
    );
}

#[test]
fn golden_hole_types() {
    // LinuxCNC has a native peck cycle; Grbl has no cycles, so the drill
    // toolpath is expanded into moves (D10 §5).
    golden(
        "projects/holes/holes.fluut",
        "fluut/linuxcnc",
        "holes-linuxcnc.ngc",
        &[],
    );
    golden(
        "projects/holes/holes.fluut",
        "fluut/grbl",
        "holes-grbl.gcode",
        &["param.drill.cycle.expanded"],
    );
}

#[test]
fn golden_roughing_3d() {
    // An optimised-load roughing, then a rest roughing with a smaller tool that
    // starts from the in-process stock. Some rest areas are too small for
    // the helix.
    golden(
        "projects/rough3d/rough3d.fluut",
        "fluut/linuxcnc",
        "rough3d-linuxcnc.ngc",
        &["cam.linking.ramp_fallback"],
    );
}

#[test]
fn golden_rotary() {
    // A rotary Setup posted with a project post that adds the A axis to
    // LinuxCNC; the machine has a continuous A axis.
    golden(
        "projects/shaft/shaft.fluut",
        "shop/linuxcnc-4x",
        "shaft-linuxcnc-4x.ngc",
        &[],
    );
}

#[test]
fn golden_two_sided() {
    // An indexed Setup at 0° pockets the top and half the window; a child
    // at 180° carries its stock, pockets the bottom and finishes the window
    // from the in-process stock: its rest pocket stops where the top
    // Setup broke through. The post indexes A between the Setups.
    golden(
        "projects/flipside/flipside.fluut",
        "shop/linuxcnc-4x",
        "flipside-linuxcnc-4x.ngc",
        &[],
    );
    let g = std::fs::read_to_string(here("golden/flipside-linuxcnc-4x.ngc")).unwrap();
    let a0 = g.lines().position(|l| l == "G0 A0").unwrap();
    let a180 = g.lines().position(|l| l == "G0 A180").unwrap();
    assert!(a0 < a180);
    // Before the turn the tool rises clear of the block's corners
    // (y ±30, z 9 about the axis) by the clearance offset.
    let z: f64 = g.lines().nth(a180 - 1).unwrap()["G0 Z".len()..]
        .parse()
        .unwrap();
    assert!(
        (z - (30.0f64 * 30.0 + 9.0 * 9.0).sqrt() - 10.0).abs() < 1e-3,
        "{z}"
    );
}

#[test]
fn two_sided_turn_above_the_z_travel_fails() {
    let d = tmp("flip-travel");
    for sub in ["tools", "posts", "machines"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
    }
    for f in [
        "flipside.fluut",
        "flipside.step",
        "tools/shop.toml",
        "posts/linuxcnc-4x.post.toml",
    ] {
        std::fs::copy(here(&format!("projects/flipside/{f}")), d.join(f)).unwrap();
    }
    let m = std::fs::read_to_string(here("projects/flipside/machines/router-4x.machine.toml"))
        .unwrap()
        .replace("[spindle]", "[travel]\nz = [-100, 30]\n\n[spindle]");
    std::fs::write(d.join("machines/router-4x.machine.toml"), m).unwrap();
    let mut s = Session::open(&d.join("flipside.fluut"), &[]).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    let store = s.library.mem_store(fluut_model::library::DefKind::Post);
    let opts = PostOptions {
        date: "2026-11-01".into(),
        time: "12:00:00".into(),
        notes: String::new(),
    };
    let e = s.post("shop/linuxcnc-4x", &store, &opts).unwrap_err();
    assert!(
        e.iter().any(|d| d.code == "setup.clearance.index_travel"),
        "{e:?}"
    );
}

#[test]
fn two_sided_invariance_along_x() {
    // Moving the part along the rotary axis, with the zero on the stock
    // end, moves nothing in the program.
    let post = |shift: f64, name: &str| -> (Session, String) {
        let d = tmp(name);
        for sub in ["tools", "posts", "machines"] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
        }
        for f in [
            "flipside.fluut",
            "tools/shop.toml",
            "posts/linuxcnc-4x.post.toml",
            "machines/router-4x.machine.toml",
        ] {
            std::fs::copy(here(&format!("projects/flipside/{f}")), d.join(f)).unwrap();
        }
        std::fs::write(d.join("flipside.step"), step::two_sided([shift, 0.0, 0.0])).unwrap();
        let mut s = Session::open(&d.join("flipside.fluut"), &[]).unwrap();
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        let store = s.library.mem_store(fluut_model::library::DefKind::Post);
        let opts = PostOptions {
            date: "2026-11-01".into(),
            time: "12:00:00".into(),
            notes: String::new(),
        };
        let (out, _) = s.post("shop/linuxcnc-4x", &store, &opts).unwrap();
        (s, out.files.concat())
    };
    let (mut sa, ga) = post(0.0, "flip-a");
    let (mut sb, gb) = post(7.123457, "flip-b");
    assert_eq!(ga, gb);
    sa.calculate(&Targets::All);
    sb.calculate(&Targets::All);
    for id in ["t1", "t2", "t3", "t4"] {
        let (ra, rb) = (sa.result(id).unwrap(), sb.result(id).unwrap());
        assert_eq!(ra.segments().count(), rb.segments().count(), "{id}");
        for (p, q) in ra.segments().zip(rb.segments()) {
            for (u, v) in [(p.start(), q.start()), (p.end(), q.end())] {
                if let (Some(u), Some(v)) = (u, v) {
                    assert!(u.xyz.distance(v.xyz) <= 1e-6, "{id}");
                }
            }
        }
    }
}

/// The 4-axis post without a `manual_nc` block.
const NO_MANUAL_POST: &str = r#"format  = "fluut.post/1"
id      = "test/no-manual"
version = "1.0"
name    = "No manual NC"
extends = "shop/linuxcnc-4x@1"

[blocks.manual_nc]
lines = []
"#;

#[test]
fn manual_nc_needs_the_post_block() {
    let mut s = Session::open(&here("projects/shaft/shaft.fluut"), &[]).unwrap();
    let mut store = s.library.mem_store(fluut_model::library::DefKind::Post);
    store.insert_toml(NO_MANUAL_POST).unwrap();
    let opts = PostOptions {
        date: "2026-11-01".into(),
        time: "12:00:00".into(),
        notes: String::new(),
    };
    let e = s.post("test/no-manual", &store, &opts).unwrap_err();
    assert!(e.iter().any(|d| d.code == "param.manual_nc.post"), "{e:?}");
    let (out, _) = s.post("shop/linuxcnc-4x", &store, &opts).unwrap();
    let g = out.files.concat();
    // The stop comes after the roughing, before the next tool, with no tool
    // change of its own.
    let stop = g.lines().position(|l| l == "M0").unwrap();
    let t2 = g.lines().position(|l| l == "T2 M6").unwrap();
    assert!(stop < t2);
    assert_eq!(g.lines().filter(|l| l.ends_with(" M6")).count(), 2);
}

#[test]
fn rotary_invariance_along_x() {
    // Moving the part along the rotary axis, with the zero on the stock
    // end, moves nothing in the program.
    let post = |shift: f64, name: &str| -> (Session, String) {
        let d = tmp(name);
        for sub in ["tools", "posts", "machines"] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
        }
        for f in [
            "shaft.fluut",
            "tools/shop.toml",
            "posts/linuxcnc-4x.post.toml",
            "machines/router-4x.machine.toml",
        ] {
            std::fs::copy(here(&format!("projects/shaft/{f}")), d.join(f)).unwrap();
        }
        std::fs::write(d.join("shaft.step"), step::shaft([0.0, 0.0, shift])).unwrap();
        let mut s = Session::open(&d.join("shaft.fluut"), &[]).unwrap();
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        let store = s.library.mem_store(fluut_model::library::DefKind::Post);
        let opts = PostOptions {
            date: "2026-11-01".into(),
            time: "12:00:00".into(),
            notes: String::new(),
        };
        let (out, _) = s.post("shop/linuxcnc-4x", &store, &opts).unwrap();
        (s, out.files.concat())
    };
    let (mut sa, ga) = post(0.0, "rot-a");
    let (mut sb, gb) = post(7.123457, "rot-b");
    assert_eq!(ga, gb);
    sa.calculate(&Targets::All);
    sb.calculate(&Targets::All);
    for id in ["t1", "t2", "t3", "t4"] {
        let (ra, rb) = (sa.result(id).unwrap(), sb.result(id).unwrap());
        for (p, q) in ra.segments().zip(rb.segments()) {
            for (u, v) in [(p.start(), q.start()), (p.end(), q.end())] {
                if let (Some(u), Some(v)) = (u, v) {
                    assert!(u.xyz.distance(v.xyz) <= 1e-6, "{id}");
                    assert_eq!(u.a, v.a, "{id}");
                }
            }
        }
    }
}

/// A post that extends `fluut/gcode` with a tap cycle.
const TAP_POST: &str = r#"format  = "fluut.post/1"
id      = "test/tap"
version = "1.0"
name    = "Tap test"
extends = "fluut/gcode@1"

[cycles]
enabled   = true
supported = ["drill", "tap"]

[blocks.cycle_start]
lines = [{ when = "cycle.kind == 'tap'", line = "G84 [X] [Y] Z[CYCLE_Z] R[CYCLE_R] [F]" }]
"#;

#[test]
fn taps_feed_at_rpm_times_pitch() {
    let d = tmp("tap");
    std::fs::create_dir_all(d.join("tools")).unwrap();
    std::fs::copy(here("projects/holes/holes.step"), d.join("holes.step")).unwrap();
    let tools = std::fs::read_to_string(here("projects/holes/tools/shop.toml"))
        .unwrap()
        .replace(
            "shape         = \"drill\"",
            "shape         = \"drill\"\npitch         = 0.8",
        );
    std::fs::write(d.join("tools/shop.toml"), tools).unwrap();
    let text = std::fs::read_to_string(here("projects/holes/holes.fluut")).unwrap();
    let text = text
        .replace("cycle        = \"peck\"", "cycle        = \"tap\"")
        .replace("type = \"bore\"", "type = \"bore\"\n  enabled = false");
    std::fs::write(d.join("holes.fluut"), text).unwrap();
    let mut s = Session::open(&d.join("holes.fluut"), &[]).unwrap();
    let mut store = fluut_post::builtin::builtin_store();
    store.insert_toml(TAP_POST).unwrap();
    let opts = PostOptions {
        date: "2026-11-01".into(),
        time: "12:00:00".into(),
        notes: String::new(),
    };
    let (out, _) = s.post("test/tap", &store, &opts).unwrap();
    let g = out.files.concat();
    // 3000 rev/min × 0.8 mm.
    let tap = g.lines().find(|l| l.starts_with("G84")).unwrap();
    assert!(tap.ends_with("F2400"), "{tap}");
    // A post without a tap cycle cannot post it.
    let e = s
        .post(
            "fluut/linuxcnc",
            &fluut_post::builtin::builtin_store(),
            &opts,
        )
        .unwrap_err();
    assert!(e.iter().any(|d| d.code == "param.drill.tap.post"), "{e:?}");
}

#[test]
fn calc_reports_each_toolpath() {
    let o = fluut(&["calc", here("projects/block/block.fluut").to_str().unwrap()]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        out.lines()
            .filter(|l| l.contains(": calculated; cut "))
            .count(),
        3,
        "{out}"
    );
    let o = fluut(&["calc", here("projects/ridge/ridge.fluut").to_str().unwrap()]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        out.lines()
            .filter(|l| l.contains(": calculated; cut "))
            .count(),
        2,
        "{out}"
    );
}

#[test]
fn calc_prints_times_and_sim_verifies() {
    let o = fluut(&["calc", here("projects/boss/boss.fluut").to_str().unwrap()]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        out.contains("t1 \"Profile outside\": calculated; cut "),
        "{out}"
    );
    assert!(out.contains(", time 0:"), "{out}");
    let o = fluut(&["sim", here("projects/boss/boss.fluut").to_str().unwrap()]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(out.contains("s1 \"Top\": simulated at 0.2 mm"), "{out}");
}

/// Simulation errors a reference project may report, with the reason:
/// `(project, toolpath, code, reason)`. Anything else fails
/// `reference_projects_simulate_clean`.
const ALLOWED_FINDINGS: &[(&str, &str, &str, &str)] = &[
    (
        "plate2d",
        "t2",
        "sim.gouge",
        "the score line is cut 0.8 mm into the model's top face by design",
    ),
    (
        "plate2d",
        "t3",
        "sim.gouge",
        "the chamfer is cut into the model's sharp outer edge by design",
    ),
    (
        "shaft",
        "",
        "sim.setup.rotary",
        "rotary Setups are not simulated yet (D11 §10)",
    ),
];

#[test]
fn reference_projects_simulate_clean() {
    // Every reference project through the simulation's checks: rapids in
    // stock, gouges, holder and shank collisions. The default 0.2 mm rays:
    // the optimised-load pocket of `plate2d` once cut 0.15 mm into its island,
    // which 0.5 mm rays miss. One thread per project.
    let projects = [
        "boss", "block", "ridge", "plate", "plate2d", "holes", "dome", "rough3d", "shaft",
        "vcarve", "flipside",
    ];
    let outputs: Vec<(&str, String)> = std::thread::scope(|sc| {
        let hs: Vec<_> = projects
            .iter()
            .map(|&p| {
                sc.spawn(move || {
                    let path = here(&format!("projects/{p}/{p}.fluut"));
                    let o = fluut(&["sim", path.to_str().unwrap()]);
                    (p, String::from_utf8_lossy(&o.stderr).into_owned())
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut unexpected = Vec::new();
    for (p, err) in outputs {
        // `error [t2] code: message` or `error code: message`.
        for l in err.lines().filter(|l| l.starts_with("error ")) {
            let rest = &l["error ".len()..];
            let (tp, rest) = match rest.strip_prefix('[') {
                Some(r) => r.split_once("] ").unwrap(),
                None => ("", rest),
            };
            let code = rest.split(':').next().unwrap();
            if !ALLOWED_FINDINGS
                .iter()
                .any(|a| (a.0, a.1, a.2) == (p, tp, code))
            {
                unexpected.push(format!("{p}: {l}"));
            }
        }
    }
    assert!(unexpected.is_empty(), "{}", unexpected.join("\n"));
}

#[test]
fn errors_and_usage() {
    let d = tmp("errors");
    let text = std::fs::read_to_string(here("projects/boss/boss.fluut"))
        .unwrap()
        .replace("shop#flat-6", "shop#flat-8");
    std::fs::write(d.join("p.fluut"), text).unwrap();
    std::fs::copy(here("projects/boss/boss.step"), d.join("boss.step")).unwrap();
    let o = fluut(&[
        "calc",
        d.join("p.fluut").to_str().unwrap(),
        "--tools",
        here("projects/boss/tools/shop.toml").to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("param.tool.missing"));
    assert_eq!(fluut(&["calc"]).status.code(), Some(2));
    assert_eq!(fluut(&["post", "x.fluut"]).status.code(), Some(2));
    assert_eq!(fluut(&["frobnicate", "x"]).status.code(), Some(2));
    assert_eq!(
        fluut(&["calc", "/nonexistent/p.fluut"]).status.code(),
        Some(1)
    );
    assert!(fluut(&["--version"]).status.success());
}

#[test]
fn an_earlier_crash_report_is_announced_once() {
    let data = tmp("crash-data");
    let reports = data.join("crashes");
    std::fs::create_dir_all(&reports).unwrap();
    std::fs::write(reports.join("crash-1-2.txt"), "report").unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_fluut"))
            .arg("calc")
            .env("FLUUT_DATA_DIR", &data)
            .output()
            .unwrap()
    };
    let first = String::from_utf8_lossy(&run().stderr).into_owned();
    assert!(first.contains("the last run crashed"), "{first}");
    assert!(first.contains("Nothing was sent"), "{first}");
    assert!(reports.join("crash-1-2.seen.txt").is_file());
    let second = String::from_utf8_lossy(&run().stderr).into_owned();
    assert!(!second.contains("crashed"), "{second}");
}

#[test]
fn version_names_the_workspace_version() {
    let o = fluut(&["--version"]);
    assert_eq!(
        String::from_utf8_lossy(&o.stdout).trim(),
        format!("fluut {}", fluut_core::build_info::version_string())
    );
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("fluut 0.1.0"));
}

/// A reference project: its directory and STEP stem, the writer of its
/// part, the toolpaths, and the work zero (the front left stock top
/// corner) for an unshifted part.
struct Reference {
    name: &'static str,
    part: fn([f64; 3]) -> String,
    toolpaths: &'static [&'static str],
    zero: [f64; 3],
}

const BLOCK: Reference = Reference {
    name: "block",
    part: step::pocket_block,
    toolpaths: &["t1", "t2", "t3"],
    zero: [10.0, 5.0, 20.0],
};

const PLATE: Reference = Reference {
    name: "plate2d",
    part: step::plate,
    toolpaths: &["t1", "t4", "t2", "t3"],
    zero: [-5.0, -5.0, 21.0],
};

const HOLES: Reference = Reference {
    name: "holes",
    part: step::holes,
    toolpaths: &["t1", "t2"],
    zero: [0.0, 0.0, 15.0],
};

/// The stock top is 1 above the crest of the ridge (stock margin).
const RIDGE: Reference = Reference {
    name: "ridge",
    part: step::ridge,
    toolpaths: &["t1", "t2"],
    zero: [-5.0, -5.0, 16.358983848622453],
};

/// The stock top is 1 above the dome top (stock margin).
const DOME: Reference = Reference {
    name: "dome",
    part: step::dome,
    toolpaths: &["t1", "t2", "t3", "t4", "t5"],
    zero: [-5.0, -5.0, 29.0],
};

/// The ridge again, roughed in 3-D.
const ROUGH3D: Reference = Reference {
    name: "rough3d",
    part: step::ridge,
    toolpaths: &["t1", "t2"],
    zero: [-5.0, -5.0, 16.358983848622453],
};

/// The project `r` in `dir` with the model moved by `shift`, and the work
/// zero given as a point moved by the same offset when `point` is set.
fn shifted(r: &Reference, dir: &Path, shift: [f64; 3], point: bool) -> PathBuf {
    let n = r.name;
    std::fs::create_dir_all(dir.join("tools")).unwrap();
    std::fs::write(dir.join(format!("{n}.step")), (r.part)(shift)).unwrap();
    std::fs::copy(
        here(&format!("projects/{n}/tools/shop.toml")),
        dir.join("tools/shop.toml"),
    )
    .unwrap();
    let mut p = std::fs::read_to_string(here(&format!("projects/{n}/{n}.fluut"))).unwrap();
    if point {
        p = p.replace(
            "zero     = { method = \"stock_top_corner\", corner = \"front_left\" }",
            &format!(
                "zero     = {{ method = \"point\", at = [{:?}, {:?}, {:?}] }}",
                r.zero[0] + shift[0],
                r.zero[1] + shift[1],
                r.zero[2] + shift[2]
            ),
        );
        assert!(p.contains("\"point\""));
    }
    let path = dir.join(format!("{n}.fluut"));
    std::fs::write(&path, p).unwrap();
    path
}

fn post_text(path: &Path) -> (Session, String) {
    let mut s = Session::open(path, &[]).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    let opts = PostOptions {
        date: "2026-11-01".into(),
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
    (s, out.files.concat())
}

#[test]
fn translation_invariance() {
    // Spec 04 §4.12.7 (3): a non-round offset of model and work zero
    // together moves no toolpath coordinate by more than ε_s, and the
    // G-code is the same.
    let shift = [0.123457, -0.987651, 0.31];
    for r in [&BLOCK, &RIDGE, &PLATE, &HOLES, &DOME, &ROUGH3D] {
        for point in [true, false] {
            let n = r.name;
            let a = shifted(r, &tmp(&format!("inv-{n}-a-{point}")), [0.0; 3], point);
            let b = shifted(r, &tmp(&format!("inv-{n}-b-{point}")), shift, point);
            let (mut sa, ga) = post_text(&a);
            let (mut sb, gb) = post_text(&b);
            assert_eq!(ga, gb, "{n}: G-code differs (zero as point: {point})");
            sa.calculate(&Targets::All);
            sb.calculate(&Targets::All);
            for id in r.toolpaths {
                let (ra, rb) = (sa.result(id).unwrap(), sb.result(id).unwrap());
                let (xa, xb): (Vec<&Segment>, Vec<&Segment>) =
                    (ra.segments().collect(), rb.segments().collect());
                assert_eq!(xa.len(), xb.len(), "{n} {id}");
                let mut worst: f64 = 0.0;
                for (p, q) in xa.iter().zip(&xb) {
                    for (u, v) in [(p.start(), q.start()), (p.end(), q.end())] {
                        if let (Some(u), Some(v)) = (u, v) {
                            worst = worst.max(u.xyz.distance(v.xyz));
                        }
                    }
                }
                assert!(worst <= 1e-6, "{n} {id}: {worst}");
            }
        }
    }
}

/// Without an update key in the build the command stops before any network
/// use. (Once the maintainer adds the key, a run would use the network, so
/// the test then does nothing.)
#[test]
fn update_check_without_a_key_exits_3_and_says_so() {
    if fluut_core::update::has_key() {
        return;
    }
    let o = Command::new(env!("CARGO_BIN_EXE_fluut"))
        .arg("update-check")
        .env(
            "FLUUT_DATA_DIR",
            std::env::temp_dir().join("fluut-cli-update"),
        )
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&o.stderr).contains("no update key in this build"));
}
