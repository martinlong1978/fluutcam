// SPDX-License-Identifier: MIT OR Apache-2.0

//! `scallop` (D09 §4.7, D15b §5.4) on the dome's hemisphere and the ridge:
//! the 3-D stepover, inserted passes, loop order and travel direction,
//! `continuous`, slope limits, codes, gouges and translation invariance.

mod support;

use fluut_geom::{Point3, Surface};
use fluut_path::Motion;
use support::finish::*;
use support::step;

fn on_sphere(tool: &str, passes: &str) -> fluut_model::project::Toolpath {
    let g = dome();
    let sphere = faces_where(&g, |s| matches!(s, Surface::Sphere { .. }));
    tp(
        "scallop",
        tool,
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n[passes]\n{passes}\n",
            sphere[0]
        ),
    )
}

fn ridge() -> fluut_cam::geometry::SetupGeometry {
    load_step(&step::ridge([0.0; 3]))
}

/// The ridge's cylinder top drives: the region is 0..60 × 0..40.
fn on_ridge(tool: &str, passes: &str) -> fluut_model::project::Toolpath {
    let g = ridge();
    let cyl = faces_where(&g, |s| matches!(s, Surface::Cylinder { .. }));
    tp(
        "scallop",
        tool,
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n[passes]\n{passes}\n",
            cyl[0]
        ),
    )
}

/// XY offset of `p` from the edges of the box `lo`–`hi`.
fn inset(p: Point3, lo: (f64, f64), hi: (f64, f64)) -> f64 {
    (p.x() - lo.0)
        .min(hi.0 - p.x())
        .min(p.y() - lo.1)
        .min(hi.1 - p.y())
}

fn seg_dist(p: Point3, a: Point3, b: Point3) -> f64 {
    let d = b - a;
    let l2 = d.dot(d);
    let t = if l2 > 0.0 {
        ((p - a).dot(d) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + d * t - p).length()
}

/// The runs grouped by their pass (the inset of their first point).
fn passes(
    r: &fluut_cam::Generated,
    lo: (f64, f64),
    hi: (f64, f64),
) -> Vec<(f64, Vec<Vec<Point3>>)> {
    let mut out: Vec<(f64, Vec<Vec<Point3>>)> = Vec::new();
    for l in runs(r) {
        let d = l
            .iter()
            .map(|p| inset(*p, lo, hi))
            .fold(f64::INFINITY, f64::min);
        match out.iter_mut().find(|(k, _)| (k - d).abs() < 1e-6) {
            Some((_, v)) => v.push(l),
            None => out.push((d, vec![l])),
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

#[test]
fn stepover_is_measured_along_the_surface() {
    // On the dome every cut point of a pass is within s (3-D) of the
    // previous pass, and the passes are closer in XY where it is steep.
    let g = dome();
    let s = 1.5;
    let r = run(&g, &on_sphere("b6", &format!("stepover = {s}")));
    let ps = passes(&r, (28.0, 18.0), (52.0, 42.0));
    assert!(ps.len() > 5);
    let tol = 0.01;
    let mut narrow = false;
    for w in ps.windows(2) {
        let (prev, cur) = (&w[0].1, &w[1].1);
        if w[1].0 - w[0].0 < s - 1e-9 {
            narrow = true;
        }
        for l in cur {
            for p in l {
                let d = prev
                    .iter()
                    .flat_map(|q| q.windows(2).map(|s| seg_dist(*p, s[0], s[1])))
                    .fold(f64::INFINITY, f64::min);
                assert!(d <= s + tol, "{p:?}: {d}");
            }
        }
    }
    assert!(narrow);
    assert_no_gouge(&g, &r, &tool("b6"), 0.0, tol + 1e-9);
}

#[test]
fn extra_passes_where_the_next_pass_is_steeper() {
    // Outside in on the ridge, each pass is less steep than the last: no
    // inserted passes. With every face of the dome driving, the outer
    // passes run on the flat block top and the inner ones climb the boss.
    let r = run(&ridge(), &on_ridge("b6", "stepover = 2"));
    assert!(!warnings(&r).contains(&"cam.scallop.extra_passes"));
    let r = run(&dome(), &tp("scallop", "b6", "[passes]\nstepover = 3\n"));
    assert!(warnings(&r).contains(&"cam.scallop.extra_passes"));
}

/// Signed area of a closed XY polyline (positive counter-clockwise).
fn area(r: &[Point3]) -> f64 {
    r.windows(2)
        .map(|w| w[0].x() * w[1].y() - w[1].x() * w[0].y())
        .sum::<f64>()
        * 0.5
}

#[test]
fn direction_and_cut_direction() {
    let g = ridge();
    let lo_hi = ((0.0, 0.0), (60.0, 40.0));
    for (dir, cut, sign) in [
        ("outside_in", "climb", -1.0),
        ("outside_in", "conventional", 1.0),
        ("inside_out", "climb", 1.0),
        ("inside_out", "conventional", -1.0),
    ] {
        let r = run(
            &g,
            &on_ridge(
                "b6",
                &format!("stepover = 3\ndirection = \"{dir}\"\ncut_direction = \"{cut}\""),
            ),
        );
        let rs = runs(&r);
        let insets: Vec<f64> = rs.iter().map(|l| inset(l[0], lo_hi.0, lo_hi.1)).collect();
        if dir == "outside_in" {
            assert!(
                insets[0] < 1e-6 && insets[insets.len() - 1] > 10.0,
                "{insets:?}"
            );
        } else {
            assert!(
                insets[0] > 10.0 && insets[insets.len() - 1] < 1e-6,
                "{insets:?}"
            );
        }
        for l in rs.iter().filter(|l| l.first() == l.last() && l.len() > 4) {
            assert!(area(l) * sign > 0.0, "{dir} {cut}");
        }
    }
}

#[test]
fn continuous_has_no_retracts() {
    let g = ridge();
    let r = run(&g, &on_ridge("b6", "stepover = 3\ncontinuous = true"));
    assert_eq!(with_motion(&r, Motion::Plunge).len(), 1);
    assert_eq!(with_motion(&r, Motion::Retract).len(), 1);
    assert!(!with_motion(&r, Motion::Link).is_empty());
    assert_no_gouge(&g, &r, &tool("b6"), 0.0, 0.01 + 1e-9);
    let r = run(
        &g,
        &on_ridge("b6", "stepover = 3\n[linking]\nretract = \"full\""),
    );
    assert!(with_motion(&r, Motion::Plunge).len() > 1);
}

#[test]
fn slope_limits() {
    let g = dome();
    let r = run(&g, &on_sphere("b6", "stepover = 2\nmax_slope_deg = 45"));
    let q = reference(&g, &tool("b6"), 0.0);
    let pts = cut_points(&r);
    assert!(!pts.is_empty());
    for p in pts {
        let h = q.drop(p.x(), p.y()).unwrap();
        assert!(h.slope <= 45f64.to_radians() + 1e-6, "{p:?}");
    }
}

#[test]
fn validation_codes() {
    let g = dome();
    codes_pass_fail(
        "param.scallop.stepover_and_scallop",
        &g,
        &on_sphere("b6", "scallop_height = 0.05"),
        &on_sphere("b6", "scallop_height = 0.05\nstepover = 1"),
    );
    codes_pass_fail(
        "param.scallop.scallop.radius",
        &g,
        &on_sphere("r6", "scallop_height = 0.05"),
        &on_sphere("f6", "scallop_height = 0.05"),
    );
    codes_pass_fail(
        "param.scallop.scallop.range",
        &g,
        &on_sphere("r6", "scallop_height = 0.5"),
        &on_sphere("r6", "scallop_height = 1"),
    );
    codes_pass_fail(
        "param.scallop.slope.order",
        &g,
        &on_sphere("b6", "stepover = 3\nmin_slope_deg = 10\nmax_slope_deg = 80"),
        &on_sphere("b6", "stepover = 3\nmin_slope_deg = 80\nmax_slope_deg = 10"),
    );
    codes_pass_fail(
        "param.scallop.slope.range",
        &g,
        &on_sphere("b6", "stepover = 3\nmax_slope_deg = 90"),
        &on_sphere("b6", "stepover = 3\nmax_slope_deg = 95"),
    );
    codes_pass_fail(
        "param.passes.stepover.max",
        &g,
        &on_sphere("b6", "stepover = 6"),
        &on_sphere("b6", "stepover = 6.5"),
    );
    codes_pass_fail(
        "cam.scallop.tool_shape",
        &g,
        &on_sphere("b6", "stepover = 3"),
        &on_sphere("v6", "stepover = 3"),
    );
    // cam.scallop.extra_passes: extra_passes_where_the_next_pass_is_steeper.
}

#[test]
fn no_gouges_with_any_tool() {
    let g = dome();
    for id in ["b6", "f6", "r6"] {
        let r = run(
            &g,
            &on_sphere(id, "stepover = 2\n[linking]\nretract = \"stay_down\""),
        );
        assert_no_gouge(&g, &r, &tool(id), 0.0, 0.01 + 1e-9);
    }
}

#[test]
fn translation_invariance() {
    let t = tp(
        "scallop",
        "b6",
        "[passes]\nstepover = 4\ncontinuous = true\n",
    );
    assert_translation_invariant(step::dome, &t);
}
