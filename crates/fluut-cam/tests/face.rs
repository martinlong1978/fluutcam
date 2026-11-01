// SPDX-License-Identifier: MIT OR Apache-2.0

//! `face` (D15b §2.1, D09 §2.1) on the boss part.

mod support;

use fluut_cam::geometry::face_id;
use fluut_geom::Point3;
use fluut_path::Motion;
use support::cam::*;
use support::step;

/// The 40 × 30 × 10 boss in a stock 5 mm larger on each side and 2 mm
/// higher.
fn stock() -> Stock {
    Stock::new([-5.0, -5.0, 0.0], [45.0, 35.0, 12.0]).model(0.0, 10.0)
}

#[test]
fn levels_reach_the_model_top_exactly() {
    let g = load(&step::boss([0.0; 3]));
    let r = run(
        &g,
        &tp("face", "f20", "[passes]\nstep_down = 0.8\n"),
        stock(),
    );
    // 2 mm in even steps of at most 0.8: three levels.
    let l = cut_levels(&r);
    assert_eq!(l.len(), 3, "{l:?}");
    assert_eq!(l[2], 10.0);
    assert!((l[0] - (12.0 - 2.0 / 3.0)).abs() < 1e-12);
    // With a final step-down, the last level is that far above the floor.
    let r = run(
        &g,
        &tp(
            "face",
            "f20",
            "[passes]\nstep_down = 1.5\nfinal_step_down = 0.3\nstock_to_leave_axial = 0.1\n",
        ),
        stock(),
    );
    let l = cut_levels(&r);
    assert_eq!(l.len(), 3, "{l:?}");
    assert_eq!(l[2], 10.1);
    assert!((l[1] - 10.4).abs() < 1e-12, "{l:?}");
    assert!((l[0] - 11.2).abs() < 1e-12, "{l:?}");
}

#[test]
fn passes_run_past_the_stock_by_the_overlap() {
    let g = load(&step::boss([0.0; 3]));
    let r = run(
        &g,
        &tp("face", "f20", "[passes]\nstep_down = 5\noverlap = 12\n"),
        stock(),
    );
    let c = cuts(&r);
    // Lines along X: every pass starts or ends at x = −5 − 12 or 45 + 12,
    // or on the rounded ends of the grown area.
    let xs: Vec<f64> = c.iter().flat_map(|s| [start(s).x(), end(s).x()]).collect();
    let lo = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (lo + 17.0).abs() < 1e-9 && (hi - 57.0).abs() < 1e-9,
        "{lo} {hi}"
    );
    // Passes cover the stock across Y: the first and last are within R
    // (+ the overlap) of the stock edges, and no gap exceeds the stepover.
    let mut ys: Vec<f64> = c.iter().map(|s| start(s).y()).collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    assert!(
        ys[0] - 10.0 <= -5.0 && ys[ys.len() - 1] + 10.0 >= 35.0,
        "{ys:?}"
    );
    assert!(ys.windows(2).all(|w| w[1] - w[0] <= 14.0 + 1e-9));
    // Zig-zag: one level stays down between passes.
    assert!(!moves(&r, Motion::Link).is_empty());
    assert!(moves(&r, Motion::Retract).len() == 1);
    assert!(r.diagnostics.is_empty(), "{:?}", codes(&r));
}

#[test]
fn directions() {
    let g = load(&step::boss([0.0; 3]));
    let along = |dir: &str| -> Vec<f64> {
        let r = run(
            &g,
            &tp(
                "face",
                "f20",
                &format!("[passes]\nstep_down = 5\ndirection = \"{dir}\"\n"),
            ),
            stock(),
        );
        cuts(&r).iter().map(|s| end(s).x() - start(s).x()).collect()
    };
    let z = along("zigzag");
    assert!(z[0] > 0.0 && z[1] < 0.0 && z[2] > 0.0);
    // Passes step towards +Y, so the uncut material is on the left of +X
    // travel. Climb keeps it on the right (spindle M3): travel −X.
    assert!(along("climb").iter().all(|d| *d < 0.0));
    assert!(along("conventional").iter().all(|d| *d > 0.0));
}

#[test]
fn pass_angle_and_drive_faces() {
    let g = load(&step::boss([0.0; 3]));
    let r = run(
        &g,
        &tp(
            "face",
            "f20",
            "[passes]\nstep_down = 5\npass_angle_deg = 90\n",
        ),
        stock(),
    );
    for s in cuts(&r) {
        assert!((end(s).x() - start(s).x()).abs() < 1e-9);
    }
    // A drive face sets `selection`: the top face of the boss at Z 10.
    let b = g.bodies[0].id.clone();
    let t = tp(
        "face",
        "f20",
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n[heights]\nbottom = {{ ref = \"selection\" }}\n\
             [passes]\nstep_down = 5\n",
            face_id(&b, 9)
        ),
    );
    let r = run(&g, &t, stock());
    assert_eq!(cut_levels(&r), [10.0]);
    // A face that is not planar and horizontal is an error.
    let t = tp(
        "face",
        "f20",
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n",
            face_id(&b, 0)
        ),
    );
    assert!(run_err(&g, &t, stock()).contains(&"param.geometry.drive_faces.kind".into()));
    // The entry is outside the stock and above it.
    let first = r.result.segments().next().unwrap();
    let p: Point3 = start(first);
    assert!(p.x() < -5.0 || p.x() > 45.0 || p.y() < -5.0 || p.y() > 35.0);
}

#[test]
fn reference_parts_load() {
    // The M6 reference parts: face counts as documented on the writers.
    for (text, n) in [(step::plate([0.0; 3]), 18), (step::holes([0.0; 3]), 18)] {
        let g = load(&text);
        let fluut_cam::geometry::BodyGeom::Brep(b) = &g.bodies[0].geom else {
            panic!()
        };
        assert_eq!(b.faces.len(), n);
    }
}
