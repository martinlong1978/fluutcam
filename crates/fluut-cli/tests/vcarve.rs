// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `vcarve` reference project: `engrave` on lettering-like outlines
//! from a DXF file (D15b §2.4). Golden G-code and translation invariance
//! (spec 04 §4.12.7 (3)), as `cli.rs` does for the other projects.
//!
//! `FLUUT_BLESS=1` rewrites the DXF input (from [`dxf`]) and the golden
//! G-code after a deliberate change; review the diff by eye.

use fluut_core::{PostOptions, Session, Targets};
use fluut_path::Segment;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn here(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(p)
}

fn check_file(path: &Path, text: &str) {
    if std::env::var_os("FLUUT_BLESS").is_some() {
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

fn tmp(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Closed outlines as bulge vertices: a curved stroke with round ends, a
/// bowl with a hole, a serifed stem, a V with sharp corners and a square
/// dot wide enough to leave a flat area.
fn outlines() -> Vec<Vec<(f64, f64, f64)>> {
    // tan(π / 8): quarter arcs.
    let b = 2f64.sqrt() - 1.0;
    vec![
        vec![
            (10.0, 6.0, 1.0),
            (10.0, 24.0, 1.0),
            (10.0, 21.5, -1.0),
            (10.0, 8.5, 1.0),
        ],
        vec![
            (32.0, 3.0, b),
            (39.0, 15.0, b),
            (32.0, 27.0, b),
            (25.0, 15.0, b),
        ],
        vec![
            (32.0, 7.0, b),
            (35.0, 15.0, b),
            (32.0, 23.0, b),
            (29.0, 15.0, b),
        ],
        vec![
            (45.0, 3.0, 0.0),
            (55.0, 3.0, 0.0),
            (55.0, 5.0, 0.0),
            (51.5, 6.0, 0.0),
            (51.5, 24.0, 0.0),
            (55.0, 25.0, 0.0),
            (55.0, 27.0, 0.0),
            (45.0, 27.0, 0.0),
            (45.0, 25.0, 0.0),
            (48.5, 24.0, 0.0),
            (48.5, 6.0, 0.0),
            (45.0, 5.0, 0.0),
        ],
        vec![
            (60.0, 27.0, 0.0),
            (64.0, 27.0, 0.0),
            (68.0, 10.0, 0.0),
            (72.0, 27.0, 0.0),
            (76.0, 27.0, 0.0),
            (70.0, 3.0, 0.0),
            (66.0, 3.0, 0.0),
        ],
        vec![
            (80.0, 3.0, 0.0),
            (86.0, 3.0, 0.0),
            (86.0, 9.0, 0.0),
            (80.0, 9.0, 0.0),
        ],
    ]
}

/// The outlines as closed `LWPOLYLINE`s on layer `TEXT`, moved by `shift`.
fn dxf(shift: [f64; 3]) -> String {
    let mut s = String::from(
        "  0\nSECTION\n  2\nHEADER\n  9\n$INSUNITS\n 70\n4\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n",
    );
    for l in outlines() {
        write!(
            s,
            "  0\nLWPOLYLINE\n  8\nTEXT\n 90\n{}\n 70\n1\n 38\n{:?}\n",
            l.len(),
            shift[2]
        )
        .unwrap();
        for (x, y, b) in l {
            write!(s, " 10\n{:?}\n 20\n{:?}\n", x + shift[0], y + shift[1]).unwrap();
            if b != 0.0 {
                write!(s, " 42\n{b:?}\n").unwrap();
            }
        }
    }
    s.push_str("  0\nENDSEC\n  0\nEOF\n");
    s
}

#[test]
fn dxf_input_matches_the_writer() {
    check_file(&here("projects/vcarve/vcarve.dxf"), &dxf([0.0; 3]));
}

#[test]
fn project_is_canonical() {
    let p = "projects/vcarve/vcarve.fluut";
    let text = std::fs::read_to_string(here(p)).unwrap();
    let l = fluut_model::project::read(&text).unwrap();
    assert!(l.warnings.is_empty(), "{p}: {:?}", l.warnings);
    assert_eq!(fluut_model::project::write(&l.project), text, "{p}");
}

#[test]
fn golden_engrave() {
    let out = tmp("golden-vcarve").join("out.nc");
    let o = Command::new(env!("CARGO_BIN_EXE_fluut"))
        .args([
            "post",
            here("projects/vcarve/vcarve.fluut").to_str().unwrap(),
            "--post",
            "fluut/linuxcnc",
            "-o",
            out.to_str().unwrap(),
            "--date",
            "2026-11-01",
            "--time",
            "12:00:00",
        ])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{err}");
    let codes: Vec<&str> = err
        .lines()
        .filter_map(|l| {
            l.split_whitespace()
                .find(|w| w.starts_with("param.") || w.starts_with("cam."))
        })
        .map(|c| c.trim_end_matches(':'))
        .collect();
    // The square dot is wider than the V at max depth.
    assert_eq!(codes, ["cam.engrave.flat_area"], "{err}");
    let text = std::fs::read_to_string(&out).unwrap();
    check_file(&here("golden/vcarve-linuxcnc.ngc"), &text);
}

/// The project in `dir` with the outlines moved by `shift`.
fn shifted(dir: &Path, shift: [f64; 3]) -> PathBuf {
    std::fs::create_dir_all(dir.join("tools")).unwrap();
    std::fs::write(dir.join("vcarve.dxf"), dxf(shift)).unwrap();
    std::fs::copy(
        here("projects/vcarve/tools/shop.toml"),
        dir.join("tools/shop.toml"),
    )
    .unwrap();
    let path = dir.join("vcarve.fluut");
    std::fs::copy(here("projects/vcarve/vcarve.fluut"), &path).unwrap();
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
    // A non-round offset of the outlines moves no toolpath coordinate by
    // more than 1e-6 mm in the work frame, and the G-code is the same.
    let shift = [0.123457, -0.987651, 0.31];
    let a = shifted(&tmp("inv-vcarve-a"), [0.0; 3]);
    let b = shifted(&tmp("inv-vcarve-b"), shift);
    let (mut sa, ga) = post_text(&a);
    let (mut sb, gb) = post_text(&b);
    assert_eq!(ga, gb, "G-code differs");
    sa.calculate(&Targets::All);
    sb.calculate(&Targets::All);
    let (ra, rb) = (sa.result("t1").unwrap(), sb.result("t1").unwrap());
    let (xa, xb): (Vec<&Segment>, Vec<&Segment>) =
        (ra.segments().collect(), rb.segments().collect());
    assert_eq!(xa.len(), xb.len());
    let mut worst: f64 = 0.0;
    for (p, q) in xa.iter().zip(&xb) {
        for (u, v) in [(p.start(), q.start()), (p.end(), q.end())] {
            if let (Some(u), Some(v)) = (u, v) {
                worst = worst.max(u.xyz.distance(v.xyz));
            }
        }
    }
    assert!(worst <= 1e-6, "{worst}");
}
