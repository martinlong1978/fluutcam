// SPDX-License-Identifier: MIT OR Apache-2.0

//! `horizontal` (D09 §4.5, D15b §5.2) on the dome part and the pocket
//! block: flats only, walls kept at R + stock, offset and raster, flats at
//! equal Z together, codes, gouges and translation invariance.

mod support;

use fluut_geom::{Point3, Surface};
use support::finish::*;
use support::step::{self, DOME_CENTRE};

fn horizontal(tool: &str, groups: &str) -> fluut_model::project::Toolpath {
    tp("horizontal", tool, groups)
}

fn radial(p: Point3) -> f64 {
    libm::hypot(p.x() - DOME_CENTRE[0], p.y() - DOME_CENTRE[1])
}

#[test]
fn flats_only_at_their_height() {
    let g = dome();
    let r = run(&g, &horizontal("f6", "[passes]\nstepover = 3\n"));
    let pts = cut_points(&r);
    assert!(pts.len() > 20);
    for p in &pts {
        assert!((p.z() - 10.0).abs() < 1e-9, "{p:?}");
    }
    let r = run(
        &g,
        &horizontal("f6", "[passes]\nstepover = 3\nstock_to_leave_axial = 0.2\n"),
    );
    for p in cut_points(&r) {
        assert!((p.z() - 10.2).abs() < 1e-9, "{p:?}");
    }
}

#[test]
fn walls_are_kept_at_radius_plus_stock() {
    let g = dome();
    let tol = 0.01;
    for stl in [0.0, 0.5] {
        let r = run(
            &g,
            &horizontal(
                "f6",
                &format!("[passes]\nstepover = 3\nstock_to_leave = {stl}\n"),
            ),
        );
        let pts = cut_points(&r);
        let near = pts.iter().map(|p| radial(*p)).fold(f64::INFINITY, f64::min);
        assert!(near >= 15.0 + stl - tol / 2.0, "{stl}: {near}");
        assert!(near <= 15.0 + stl + 1e-3, "{stl}: {near}");
        // The tool stays on the flat at the block's edges.
        for p in &pts {
            assert!(p.x() >= 3.0 - 1e-9 && p.x() <= 77.0 + 1e-9, "{p:?}");
            assert!(p.y() >= 3.0 - 1e-9 && p.y() <= 57.0 + 1e-9, "{p:?}");
        }
        assert_no_gouge(&g, &r, &tool("f6"), stl, tol + 1e-9);
    }
}

/// Signed area of a closed XY polyline (positive counter-clockwise).
fn area(r: &[Point3]) -> f64 {
    r.windows(2)
        .map(|w| w[0].x() * w[1].y() - w[1].x() * w[0].y())
        .sum::<f64>()
        * 0.5
}

#[test]
fn offset_rings_outside_in_and_climb() {
    let g = dome();
    for (dir, sign) in [("climb", 1.0), ("conventional", -1.0)] {
        let r = run(
            &g,
            &horizontal(
                "f6",
                &format!("[passes]\nstepover = 3\ndirection = \"{dir}\"\n"),
            ),
        );
        let rs = runs(&r);
        // The first ring is the area boundary: the outer loop, travelled
        // with the wall on the right for climb (counter-clockwise).
        let first = &rs[0];
        assert!(first.iter().any(|p| (p.x() - 3.0).abs() < 1e-6));
        assert!(area(first) * sign > 0.0, "{dir}");
    }
}

#[test]
fn raster_lines_are_a_stepover_apart() {
    let g = dome();
    let r = run(
        &g,
        &horizontal("f6", "[passes]\npattern = \"raster\"\nstepover = 3\n"),
    );
    let mut ys: Vec<f64> = runs(&r)
        .iter()
        .filter(|l| {
            l.len() == 2 && (l[0].y() - l[1].y()).abs() < 1e-9 && (l[0].x() - l[1].x()).abs() > 10.0
        })
        .map(|l| l[0].y())
        .collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert!(ys.len() > 10);
    // A line on the area boundary may clip to nothing; the boundary ring
    // cuts that edge.
    assert!(
        (ys[0] - 3.0).abs() < 1e-9 && 57.0 - ys[ys.len() - 1] <= 3.0 + 1e-9,
        "{ys:?}"
    );
    assert!(ys.windows(2).all(|w| w[1] - w[0] <= 3.0 + 1e-9));
}

#[test]
fn flats_at_equal_height_together_top_first() {
    // Pocket block: the block top and the island top at Z 20, the pocket
    // floor at Z 12.
    let g = load_step(&step::pocket_block([0.0; 3]));
    let r = run(&g, &horizontal("f6", "[passes]\nstepover = 3\n"));
    let zs: Vec<f64> = runs(&r).iter().map(|l| l[0].z()).collect();
    assert!(zs.contains(&20.0) && zs.contains(&12.0));
    let first12 = zs.iter().position(|z| *z == 12.0).unwrap();
    assert!(zs[first12..].iter().all(|z| *z == 12.0), "{zs:?}");
    // Both flats at 20: the block rim and the island.
    let at20: Vec<Point3> = runs(&r)
        .into_iter()
        .filter(|l| l[0].z() == 20.0)
        .flatten()
        .collect();
    assert!(
        at20.iter()
            .any(|p| p.x() > 35.0 && p.x() < 45.0 && p.y() > 25.0 && p.y() < 35.0)
    );
    assert!(at20.iter().any(|p| p.x() < 10.0));
}

#[test]
fn validation_codes() {
    let g = dome();
    let p = |id: &str, s: &str| horizontal(id, &format!("[passes]\nstepover = 3\n{s}\n"));
    codes_pass_fail(
        "param.horizontal.tool_shape",
        &g,
        &p("r6", ""),
        &p("b6", ""),
    );
    codes_pass_fail(
        "param.passes.stepover.max",
        &g,
        &horizontal("f6", "[passes]\nstepover = 6\n"),
        &horizontal("f6", "[passes]\nstepover = 6.5\n"),
    );
    codes_pass_fail(
        "param.passes.stepover.flat_width",
        &g,
        &horizontal("r6", "[passes]\nstepover = 4\n"),
        &horizontal("r6", "[passes]\nstepover = 4.5\n"),
    );
    let sphere = faces_where(&g, |s| matches!(s, Surface::Sphere { .. }));
    codes_pass_fail(
        "cam.horizontal.no_flats",
        &g,
        &p("f6", ""),
        &horizontal(
            "f6",
            &format!(
                "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n[passes]\nstepover = 3\n",
                sphere[0]
            ),
        ),
    );
    // param.horizontal.drive_faces.mesh: mesh_drive_faces_warn.
}

#[test]
fn mesh_drive_faces_warn() {
    let r = run(&mesh(), &horizontal("f6", "[passes]\nstepover = 3\n"));
    assert!(warnings(&r).contains(&"param.horizontal.drive_faces.mesh"));
    let r = run(&dome(), &horizontal("f6", "[passes]\nstepover = 3\n"));
    assert!(!warnings(&r).contains(&"param.horizontal.drive_faces.mesh"));
}

#[test]
fn no_gouges_with_any_tool() {
    let g = dome();
    for id in ["f6", "r6", "b6"] {
        let r = run(
            &g,
            &horizontal(
                id,
                "[passes]\nstepover = 2\n[linking]\nretract = \"stay_down\"\n",
            ),
        );
        assert_no_gouge(&g, &r, &tool(id), 0.0, 0.01 + 1e-9);
    }
}

#[test]
fn translation_invariance() {
    let t = horizontal("f6", "[passes]\nstepover = 3\nstock_to_leave = 0.2\n");
    assert_translation_invariant(step::dome, &t);
}
