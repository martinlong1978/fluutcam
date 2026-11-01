// SPDX-License-Identifier: MIT OR Apache-2.0

//! End to end: STEP file + TOML project → G-code from the `fluut` binary.
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
}

#[test]
fn projects_are_canonical() {
    for p in ["projects/boss/boss.fluut", "projects/block/block.fluut"] {
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

/// The block project in `dir` with the model moved by `shift`, and the work
/// zero given as a point moved by the same offset when `point` is set.
fn shifted(dir: &Path, shift: [f64; 3], point: bool) -> PathBuf {
    std::fs::create_dir_all(dir.join("tools")).unwrap();
    std::fs::write(dir.join("block.step"), step::pocket_block(shift)).unwrap();
    std::fs::copy(
        here("projects/block/tools/shop.toml"),
        dir.join("tools/shop.toml"),
    )
    .unwrap();
    let mut p = std::fs::read_to_string(here("projects/block/block.fluut")).unwrap();
    if point {
        p = p.replace(
            "zero     = { method = \"stock_top_corner\", corner = \"front_left\" }",
            &format!(
                "zero     = {{ method = \"point\", at = [{:?}, {:?}, {:?}] }}",
                10.0 + shift[0],
                5.0 + shift[1],
                20.0 + shift[2]
            ),
        );
        assert!(p.contains("\"point\""));
    }
    let path = dir.join("block.fluut");
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
    for point in [true, false] {
        let a = shifted(&tmp(&format!("inv-a-{point}")), [0.0; 3], point);
        let b = shifted(&tmp(&format!("inv-b-{point}")), shift, point);
        let (mut sa, ga) = post_text(&a);
        let (mut sb, gb) = post_text(&b);
        assert_eq!(ga, gb, "G-code differs (zero as point: {point})");
        sa.calculate(&Targets::All);
        sb.calculate(&Targets::All);
        for id in ["t1", "t2", "t3"] {
            let (ra, rb) = (sa.result(id).unwrap(), sb.result(id).unwrap());
            let (xa, xb): (Vec<&Segment>, Vec<&Segment>) =
                (ra.segments().collect(), rb.segments().collect());
            assert_eq!(xa.len(), xb.len(), "{id}");
            let mut worst: f64 = 0.0;
            for (p, q) in xa.iter().zip(&xb) {
                for (u, v) in [(p.start(), q.start()), (p.end(), q.end())] {
                    if let (Some(u), Some(v)) = (u, v) {
                        worst = worst.max(u.xyz.distance(v.xyz));
                    }
                }
            }
            assert!(worst <= 1e-6, "{id}: {worst}");
        }
    }
}
