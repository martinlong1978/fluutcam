// SPDX-License-Identifier: MIT OR Apache-2.0

//! `spiral` (D09 §4.6, D15b §5.3) on the dome's hemisphere: an even pitch
//! that reaches r_max, centre, rotation and direction, exact region clips,
//! `machine_corners`, the scallop height, codes, gouges and translation
//! invariance.

mod support;

use fluut_geom::{Point3, Surface};
use std::f64::consts::TAU;
use support::finish::*;
use support::step::{self, DOME_CENTRE};

/// The hemisphere's drive box is 28..52 × 18..42.
fn spiral(tool: &str, passes: &str) -> fluut_model::project::Toolpath {
    let g = dome();
    let sphere = faces_where(&g, |s| matches!(s, Surface::Sphere { .. }));
    tp(
        "spiral",
        tool,
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n[passes]\n{passes}\n",
            sphere[0]
        ),
    )
}

fn polar(p: Point3) -> (f64, f64) {
    let (dx, dy) = (p.x() - DOME_CENTRE[0], p.y() - DOME_CENTRE[1]);
    (libm::hypot(dx, dy), libm::atan2(dy, dx))
}

const R_MAX: f64 = 12.0 * std::f64::consts::SQRT_2;

/// How far `p` is off the spiral of pitch `s` (ccw or cw), in mm.
fn off_spiral(p: Point3, s: f64, ccw: bool) -> f64 {
    let (r, th) = polar(p);
    let th = if ccw { th } else { -th };
    let k = r / s - th / TAU;
    (k - k.round()).abs() * s
}

#[test]
fn even_pitch_reaches_r_max() {
    let g = dome();
    let r = run(&g, &spiral("b6", "stepover = 2"));
    let s = R_MAX / (R_MAX / 2.0f64).ceil();
    let pts = cut_points(&r);
    assert!(pts.len() > 100);
    let mut rmax: f64 = 0.0;
    for p in &pts {
        assert!(off_spiral(*p, s, true) < 1e-6, "{p:?}");
        rmax = rmax.max(polar(*p).0);
        // Clipped to the region.
        assert!(p.x() >= 28.0 - 1e-6 && p.x() <= 52.0 + 1e-6, "{p:?}");
        assert!(p.y() >= 18.0 - 1e-6 && p.y() <= 42.0 + 1e-6, "{p:?}");
    }
    // Beyond about 15 the ball rides on the block top, a check face.
    assert!(rmax > 14.0, "{rmax}");
    // With every face driving, the last turn reaches r_max (the region is
    // the 80 × 60 block, centred on the boss).
    let all = run(&g, &tp("spiral", "b6", "[passes]\nstepover = 3\n"));
    let r_all = 50.0;
    let s = r_all / (r_all / 3.0f64).ceil();
    let mut far: f64 = 0.0;
    for p in cut_points(&all) {
        assert!(off_spiral(p, s, true) < 1e-6, "{p:?}");
        far = far.max(polar(p).0);
    }
    assert!(far > r_all - s, "{far}");
}

#[test]
fn clips_are_exact() {
    // Run ends lie on the region boundary (or at the centre / the end).
    let g = dome();
    let r = run(&g, &spiral("b6", "stepover = 2"));
    for l in runs(&r) {
        for e in [l[0], l[l.len() - 1]] {
            let on_edge = [e.x() - 28.0, 52.0 - e.x(), e.y() - 18.0, 42.0 - e.y()]
                .iter()
                .any(|d| d.abs() < 1e-6);
            let at_centre = polar(e).0 < 1e-6;
            assert!(on_edge || at_centre, "{e:?}");
        }
    }
}

#[test]
fn rotation_direction_and_centre() {
    let g = dome();
    let s = R_MAX / (R_MAX / 2.0f64).ceil();
    let cw = run(&g, &spiral("b6", "stepover = 2\nrotation = \"cw\""));
    for p in cut_points(&cw) {
        assert!(off_spiral(p, s, false) < 1e-6, "{p:?}");
    }
    // Inside out starts at the centre; outside in ends there.
    let io = cut_points(&run(&g, &spiral("b6", "stepover = 2")));
    assert!(polar(io[0]).0 < 1e-6);
    let oi = cut_points(&run(
        &g,
        &spiral("b6", "stepover = 2\ndirection = \"outside_in\""),
    ));
    assert!(polar(oi[oi.len() - 1]).0 < 1e-6);
    assert!(polar(oi[0]).0 > 10.0);
    // A centre given in the model frame.
    let c = run(&g, &spiral("b6", "stepover = 2\ncentre = [40, 30, 0]"));
    assert_eq!(cut_points(&c), io);
    let off = run(&g, &spiral("b6", "stepover = 2\ncentre = [45, 30, 0]"));
    assert!(polar(cut_points(&off)[0]).0 > 4.0);
}

#[test]
fn machine_corners_adds_the_boundary() {
    // Every face drives: the region is the block, 0..80 × 0..60.
    let g = dome();
    let a = run(&g, &tp("spiral", "b6", "[passes]\nstepover = 3\n"));
    let b = run(
        &g,
        &tp(
            "spiral",
            "b6",
            "[passes]\nstepover = 3\nmachine_corners = true\n",
        ),
    );
    let last = runs(&b).pop().unwrap();
    assert!(cut_points(&b).len() > cut_points(&a).len());
    for p in &last {
        let edge = [p.x(), 80.0 - p.x(), p.y(), 60.0 - p.y()]
            .iter()
            .any(|d| d.abs() < 1e-6);
        assert!(edge, "{p:?}");
    }
    for (x, y) in [(0.0, 0.0), (80.0, 0.0), (80.0, 60.0), (0.0, 60.0)] {
        assert!(
            last.iter()
                .any(|p| (p.x() - x).abs() < 1e-6 && (p.y() - y).abs() < 1e-6),
            "corner {x} {y}"
        );
    }
}

#[test]
fn scallop_height_sets_the_pitch() {
    // Ball on the sphere (κ = 1/12): the exact two-ball cusp formula.
    let g = dome();
    let h: f64 = 0.05;
    let (r, k) = (3.0, 1.0 / 12.0);
    let c1 = (2.0 * r * h - h * h) / (2.0 * (1.0 + k * h) * (1.0 + k * r));
    let x = k * k * c1;
    let want = 2.0 * (x * (2.0 - x)).sqrt() / k;
    let s = R_MAX / (R_MAX / want).ceil();
    let res = run(&g, &spiral("b6", &format!("scallop_height = {h}")));
    for p in cut_points(&res) {
        assert!(off_spiral(p, s, true) < 1e-6, "{p:?}");
    }
}

#[test]
fn validation_codes() {
    let g = dome();
    codes_pass_fail(
        "param.spiral.stepover_and_scallop",
        &g,
        &spiral("b6", "scallop_height = 0.05"),
        &spiral("b6", "scallop_height = 0.05\nstepover = 1"),
    );
    codes_pass_fail(
        "param.spiral.scallop.radius",
        &g,
        &spiral("r6", "scallop_height = 0.05"),
        &spiral("f6", "scallop_height = 0.05"),
    );
    codes_pass_fail(
        "param.spiral.scallop.range",
        &g,
        &spiral("r6", "scallop_height = 0.5"),
        &spiral("r6", "scallop_height = 1"),
    );
    codes_pass_fail(
        "param.passes.stepover.max",
        &g,
        &spiral("b6", "stepover = 6"),
        &spiral("b6", "stepover = 6.5"),
    );
    codes_pass_fail(
        "cam.spiral.centre_outside",
        &g,
        &spiral("b6", "stepover = 3\ncentre = [50, 40, 0]"),
        &spiral("b6", "stepover = 3\ncentre = [60, 30, 0]"),
    );
    codes_pass_fail(
        "cam.spiral.tool_shape",
        &g,
        &spiral("b6", "stepover = 3"),
        &spiral("v6", "stepover = 3"),
    );
}

#[test]
fn no_gouges_with_any_tool() {
    let g = dome();
    for id in ["b6", "f6", "r6"] {
        let r = run(
            &g,
            &spiral(id, "stepover = 2\n[linking]\nretract = \"stay_down\""),
        );
        assert_no_gouge(&g, &r, &tool(id), 0.0, 0.01 + 1e-9);
    }
    // Every face drives: the whole block.
    let r = run(&g, &tp("spiral", "b6", "[passes]\nstepover = 3\n"));
    assert_no_gouge(&g, &r, &tool("b6"), 0.0, 0.01 + 1e-9);
}

#[test]
fn translation_invariance() {
    let t = tp(
        "spiral",
        "b6",
        "[passes]\nstepover = 3\nmachine_corners = true\n",
    );
    assert_translation_invariant(step::dome, &t);
}
