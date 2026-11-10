// SPDX-License-Identifier: MIT OR Apache-2.0

//! STL import end to end (D05 §2.5): a mesh becomes bodies that a 3-axis
//! Setup machines, simulates and posts.

use fluut_core::fluut_sim::{Check, Quiet};
use fluut_core::{JobState, PostOptions, Session, SimSettings, Targets};
use fluut_model::ToolpathTypeId;
use fluut_model::project::{DisplayUnit, SetupKind};
use std::fmt::Write;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("stl-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// ASCII STL text of triangles.
fn stl(tris: &[[[f64; 3]; 3]]) -> String {
    let mut s = String::from("solid t\n");
    for t in tris {
        s.push_str("facet normal 0 0 0\nouter loop\n");
        for v in t {
            writeln!(s, "vertex {} {} {}", v[0], v[1], v[2]).unwrap();
        }
        s.push_str("endloop\nendfacet\n");
    }
    s.push_str("endsolid t\n");
    s
}

/// A closed `w × d` block `base` high whose top carries a smooth bump of
/// height `bump` and radius `r` in the middle, on a grid of 1 mm.
fn bumpy_block(w: usize, d: usize, base: f64, bump: f64, r: f64) -> Vec<[[f64; 3]; 3]> {
    let z = |i: usize, j: usize| {
        let (x, y) = (i as f64 - w as f64 / 2.0, j as f64 - d as f64 / 2.0);
        base + bump * (1.0 - (x * x + y * y) / (r * r)).max(0.0)
    };
    let p = |i: usize, j: usize| [i as f64, j as f64, z(i, j)];
    let q = |i: usize, j: usize| [i as f64, j as f64, 0.0];
    let mut t = Vec::new();
    for j in 0..d {
        for i in 0..w {
            t.push([p(i, j), p(i + 1, j), p(i + 1, j + 1)]);
            t.push([p(i, j), p(i + 1, j + 1), p(i, j + 1)]);
        }
    }
    // Sides: each border step as a quad, counter-clockwise from outside.
    for i in 0..w {
        t.push([p(i, 0), q(i, 0), q(i + 1, 0)]);
        t.push([p(i, 0), q(i + 1, 0), p(i + 1, 0)]);
        t.push([p(i + 1, d), q(i + 1, d), q(i, d)]);
        t.push([p(i + 1, d), q(i, d), p(i, d)]);
    }
    for j in 0..d {
        t.push([p(w, j), q(w, j), q(w, j + 1)]);
        t.push([p(w, j), q(w, j + 1), p(w, j + 1)]);
        t.push([p(0, j + 1), q(0, j + 1), q(0, j)]);
        t.push([p(0, j + 1), q(0, j), p(0, j)]);
    }
    let c = |x: f64, y: f64| [x, y, 0.0];
    let (w, d) = (w as f64, d as f64);
    t.push([c(0.0, 0.0), c(w, d), c(w, 0.0)]);
    t.push([c(0.0, 0.0), c(0.0, d), c(w, d)]);
    t
}

fn session(name: &str, text: &str) -> (Session, PathBuf) {
    let d = dir(name);
    let mut s = Session::new_project(&d.join("np.fluut"), DisplayUnit::Mm).unwrap();
    let f = d.join("part.stl");
    std::fs::write(&f, text).unwrap();
    s.import_model(&f).unwrap();
    (s, d)
}

#[test]
fn a_mesh_is_machined_simulated_and_posted() {
    let (mut s, _) = session("e2e", &stl(&bumpy_block(40, 30, 6.0, 8.0, 12.0)));
    let bodies: Vec<_> = s.store.bodies().map(|b| b.id.clone()).collect();
    assert_eq!(bodies, ["b-1"]);
    s.add_setup(SetupKind::ThreeAxis, None, "Setup 1").unwrap();
    for (ty, name) in [
        (ToolpathTypeId::OptimisedLoad3d, "Rough"),
        (ToolpathTypeId::Parallel, "Finish"),
    ] {
        s.add_toolpath("s1", ty, name).unwrap();
    }
    let reports = s.calculate(&Targets::All);
    assert!(
        reports
            .iter()
            .all(|r| matches!(r.state, JobState::Done { .. })),
        "{:?}",
        reports.iter().map(|r| &r.diagnostics).collect::<Vec<_>>()
    );
    for r in &reports {
        assert!(r.stats.is_some_and(|st| st.cut_length > 0.0), "{r:?}");
    }
    let sim = s
        .simulate(
            "s1",
            &SimSettings {
                error: 0.25,
                ..SimSettings::default()
            },
            &Quiet,
        )
        .unwrap();
    let gouges: Vec<_> = sim
        .sim
        .findings
        .iter()
        .filter(|f| f.check == Check::Gouge)
        .collect();
    assert!(gouges.is_empty(), "{gouges:?}");
    let opts = PostOptions {
        date: "2026-11-11".into(),
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
}

#[test]
fn separate_shells_are_separate_bodies() {
    let mut a = bumpy_block(10, 10, 4.0, 0.0, 5.0);
    let b: Vec<_> = bumpy_block(10, 10, 4.0, 0.0, 5.0)
        .into_iter()
        .map(|t| t.map(|v| [v[0] + 30.0, v[1], v[2]]))
        .collect();
    a.extend(b);
    let (s, _) = session("two", &stl(&a));
    let bodies: Vec<_> = s.store.bodies().map(|b| b.id.clone()).collect();
    assert_eq!(bodies, ["b-1", "b-2"]);
}

#[test]
fn bad_meshes_import_with_warnings_or_fail_cleanly() {
    // One lone triangle: open, but a body.
    let d = dir("bad");
    let mut s = Session::new_project(&d.join("np.fluut"), DisplayUnit::Mm).unwrap();
    let f = d.join("one.stl");
    std::fs::write(
        &f,
        stl(&[[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 5.0]]]),
    )
    .unwrap();
    let r = s.import_model(&f).unwrap();
    assert!(
        r.diagnostics
            .iter()
            .any(|n| n.code == "model.geometry.mesh.open"),
        "{:?}",
        r.diagnostics
    );
    // Garbage, and a file with only degenerate triangles, are errors.
    let g = d.join("junk.stl");
    std::fs::write(&g, "solid x\nfacet normal 1 0 0\nvertex a b c\n").unwrap();
    assert!(s.import_model(&g).is_err());
    let z = d.join("flat.stl");
    std::fs::write(&z, stl(&[[[1.0, 1.0, 1.0]; 3]])).unwrap();
    assert!(s.import_model(&z).is_err());
}

#[test]
fn reimport_keeps_every_id() {
    let (mut s, _) = session("re", &stl(&bumpy_block(8, 8, 3.0, 2.0, 4.0)));
    let r = s.reimport("g1").unwrap();
    assert!(r.unmatched.is_empty());
    assert!(r.matching.renames().is_empty());
    assert!(r.matching.ids.contains_key("f-1.0"));
}
