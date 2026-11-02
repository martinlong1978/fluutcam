// SPDX-License-Identifier: MIT OR Apache-2.0

//! `contour` (waterline, D09 §4.4, D15b §5.1) on the dome part (a block
//! with a cylinder boss under a hemisphere) and two boxes: contours against
//! closed forms, the flat level (CH-03), levels, scallop-adaptive levels,
//! exact cut-off points, directions, order, codes, gouges and translation
//! invariance.

mod support;

use fluut_cam::cutter::synth::{Builder, cuboid};
use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry};
use fluut_geom::{Point2, Point3, Surface};
use support::finish::*;
use support::step::{self, DOME_CENTRE, DOME_SHOULDER};

const TOL: f64 = 1e-6;

fn contour(tool: &str, groups: &str) -> fluut_model::project::Toolpath {
    tp("contour", tool, groups)
}

fn radial(p: Point3) -> f64 {
    libm::hypot(p.x() - DOME_CENTRE[0], p.y() - DOME_CENTRE[1])
}

/// Distance in XY from `p` to the 80 × 60 block outline (0 inside).
fn to_block(p: Point3) -> f64 {
    let dx = (0.0 - p.x()).max(p.x() - 80.0).max(0.0);
    let dy = (0.0 - p.y()).max(p.y() - 60.0).max(0.0);
    libm::hypot(dx, dy)
}

/// CL radius about the boss axis at tip height `z` for a tool of radius
/// 3 and corner `r`: the corner centre circle (radius 3 − r, height z + r)
/// on the sphere of radius 12 + r about the shoulder, or the cylinder.
fn boss_radius(z: f64, r: f64) -> f64 {
    let zc = z + r - DOME_SHOULDER;
    if zc <= 0.0 {
        15.0
    } else {
        let rr = 12.0 + r;
        (3.0 - r) + (rr * rr - zc * zc).sqrt()
    }
}

/// CL distance from the block outline at tip height `z < 10`.
fn block_offset(z: f64, r: f64) -> f64 {
    let zc = z + r - 10.0;
    if zc <= 0.0 {
        3.0
    } else {
        (3.0 - r) + (r * r - zc * zc).max(0.0).sqrt()
    }
}

fn levels(r: &fluut_cam::Generated) -> Vec<f64> {
    let mut zs: Vec<f64> = cut_points(r).iter().map(|p| p.z()).collect();
    zs.sort_by(|a, b| b.total_cmp(a));
    zs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    zs
}

#[test]
fn contours_match_closed_forms() {
    let g = dome();
    for (id, r) in [("b6", 3.0), ("f6", 0.0), ("r6", 1.0)] {
        let res = run(&g, &contour(id, "[passes]\nstep_down = 2.5\n"));
        let pts = cut_points(&res);
        assert!(pts.len() > 100, "{id}");
        let (mut boss, mut outer) = (0, 0);
        for p in pts {
            if radial(p) < 20.0 {
                boss += 1;
                // The contour is classified at z + δ (CH-03).
                let want = boss_radius(p.z() + 1e-6, r);
                assert!(
                    (radial(p) - want).abs() < TOL,
                    "{id} z {}: radius {} want {want}",
                    p.z(),
                    radial(p)
                );
            } else {
                outer += 1;
                assert!(p.z() < 10.0, "{id}: {p:?}");
                let want = block_offset(p.z() + 1e-6, r);
                assert!(
                    (to_block(p) - want).abs() < TOL,
                    "{id} z {}: offset {} want {want}",
                    p.z(),
                    to_block(p)
                );
            }
        }
        assert!(boss > 0 && outer > 0, "{id}");
    }
}

#[test]
fn flat_level_gives_the_wall_foot() {
    // CH-03: the level at the block top (Z 10) is tangent to that face.
    // The contour is the foot of the boss wall, nothing on the flat.
    let g = dome();
    for id in ["f6", "r6", "b6"] {
        let res = run(&g, &contour(id, "[passes]\nstep_down = 4\n"));
        let at10: Vec<Point3> = cut_points(&res)
            .into_iter()
            .filter(|p| (p.z() - 10.0).abs() < 1e-9)
            .collect();
        assert!(!at10.is_empty(), "{id}");
        for p in at10 {
            assert!((radial(p) - 15.0).abs() < TOL, "{id}: {p:?}");
        }
    }
}

#[test]
fn levels_are_even_with_flats() {
    let g = dome();
    let res = run(&g, &contour("b6", "[passes]\nstep_down = 4\n"));
    let want = [24.4, 20.8, 17.2, 13.6, 10.0, 20.0 / 3.0, 10.0 / 3.0, 0.0];
    let got = levels(&res);
    assert_eq!(got.len(), want.len(), "{got:?}");
    for (a, b) in got.iter().zip(want) {
        assert!((a - b).abs() < 1e-9, "{got:?}");
    }
}

#[test]
fn scallop_height_adds_levels_where_needed() {
    let g = dome();
    let base = levels(&run(&g, &contour("b6", "[passes]\nstep_down = 3\n")));
    let t = contour("b6", "[passes]\nstep_down = 3\nscallop_height = 0.02\n");
    let res = run(&g, &t);
    let got = levels(&res);
    assert!(got.len() > base.len(), "{base:?} {got:?}");
    for z in &base {
        assert!(got.iter().any(|g| (g - z).abs() < 1e-9), "{z} lost");
    }
    // An added level cuts only where the wall needed it: where
    // sin β · √(8rh) is below the gap it splits.
    let k = (8.0f64 * 3.0 * 0.02).sqrt();
    for p in cut_points(&res) {
        if base.iter().any(|z| (z - p.z()).abs() < 1e-9) || radial(p) > 20.0 {
            continue;
        }
        // Slope of a ball contact on the dome from the CL radius.
        let sin_b = (radial(p) / 15.0).min(1.0);
        let gap = base.windows(2).find(|w| w[0] > p.z() && w[1] < p.z());
        if let Some(w) = gap {
            assert!(sin_b * k < w[0] - w[1] + 1e-9, "{p:?}");
        }
    }
    assert_no_gouge(&g, &res, &tool("b6"), 0.0, 0.01 + 1e-9);
}

#[test]
fn slope_limits() {
    let g = dome();
    // Contact slope on the dome: sin β = CL radius / 15 (ball).
    let res = run(
        &g,
        &contour("b6", "[passes]\nstep_down = 2\nmin_slope_deg = 30\n"),
    );
    let pts = cut_points(&res);
    assert!(!pts.iter().any(|p| (p.z() - 26.0).abs() < 1e-9));
    assert!(pts.iter().any(|p| (p.z() - 24.0).abs() < 1e-9));
    for p in pts.iter().filter(|p| radial(**p) < 20.0) {
        assert!(radial(*p) >= 7.5 - TOL, "{p:?}");
    }
    let res = run(
        &g,
        &contour("b6", "[passes]\nstep_down = 2\nmax_slope_deg = 60\n"),
    );
    for p in cut_points(&res).iter().filter(|p| radial(**p) < 20.0) {
        assert!(radial(*p) <= 15.0 * 0.75f64.sqrt() + TOL, "{p:?}");
    }
}

#[test]
fn drive_faces_cut_off_exactly() {
    // Drive: the hemisphere and the front half cylinder (y < 30). At Z
    // 10.5 the ball touches the cylinder; the contour is cut on the front
    // half only, ending exactly over the seams at (25, 30) and (55, 30).
    // The wall is a cliff for the ball at this level, so the faces come
    // from the wall at the level, not from the drop's contact on top.
    let g = dome();
    let mut drive = faces_where(&g, |s| matches!(s, Surface::Sphere { .. }));
    drive.push(faces_where(&g, |s| matches!(s, Surface::Cylinder { .. }))[0].clone());
    let sel: Vec<String> = drive
        .iter()
        .map(|f| format!("{{ face = \"{f}\" }}"))
        .collect();
    let t = contour(
        "b6",
        &format!(
            "[geometry]\ndrive_faces = [{}]\n[heights]\nbottom = {{ ref = \"work_zero\", offset = 10.5 }}\n\
             [passes]\nstep_down = 2.5\n",
            sel.join(", ")
        ),
    );
    let res = run(&g, &t);
    let low: Vec<Vec<Point3>> = runs(&res)
        .into_iter()
        .filter(|r| (r[0].z() - 10.5).abs() < 1e-9)
        .collect();
    assert_eq!(low.len(), 1, "{low:?}");
    let r = &low[0];
    for p in r {
        assert!(p.y() <= 30.0 + TOL, "{p:?}");
        assert!((radial(*p) - 15.0).abs() < TOL, "{p:?}");
    }
    let ends = [r[0], r[r.len() - 1]];
    for e in ends {
        let d =
            libm::hypot(e.x() - 25.0, e.y() - 30.0).min(libm::hypot(e.x() - 55.0, e.y() - 30.0));
        assert!(d < TOL, "{e:?}");
    }
    // cam.contour.loop_open: a contour ends at a limit inside the region.
    assert!(warnings(&res).contains(&"cam.contour.loop_open"));
    let all = run(&g, &contour("b6", "[passes]\nstep_down = 2.5\n"));
    assert!(!warnings(&all).contains(&"cam.contour.loop_open"));
}

/// Signed area of a closed XY polyline (positive counter-clockwise).
fn area(r: &[Point3]) -> f64 {
    r.windows(2)
        .map(|w| w[0].x() * w[1].y() - w[1].x() * w[0].y())
        .sum::<f64>()
        * 0.5
}

#[test]
fn climb_and_conventional() {
    // Climb keeps the material on the right: clockwise around the boss.
    let g = dome();
    for (dir, sign) in [("climb", -1.0), ("conventional", 1.0)] {
        let res = run(
            &g,
            &contour(
                "b6",
                &format!("[passes]\nstep_down = 4\ndirection = \"{dir}\"\n"),
            ),
        );
        let loops: Vec<Vec<Point3>> = runs(&res)
            .into_iter()
            .filter(|r| radial(r[0]) < 20.0)
            .collect();
        assert!(!loops.is_empty());
        for l in loops {
            assert!(area(&l) * sign > 100.0, "{dir}: {}", area(&l));
        }
    }
}

fn two_boxes(shift: [f64; 3]) -> SetupGeometry {
    let mut b = Builder::new("boxes");
    let p = |x: f64, y: f64, z: f64| Point3::new(x + shift[0], y + shift[1], z + shift[2]);
    cuboid(&mut b, p(0.0, 0.0, 0.0), p(10.0, 10.0, 10.0));
    cuboid(&mut b, p(30.0, 0.0, 0.0), p(40.0, 10.0, 10.0));
    SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-1".into(),
            geom: BodyGeom::Brep(b.build()),
        }],
    }
}

#[test]
fn order_by_level_and_by_area() {
    let g = two_boxes([0.0; 3]);
    let box_of = |r: &Vec<Point3>| usize::from(r[0].x() > 20.0);
    let by_level = run(&g, &contour("f6", "[passes]\nstep_down = 2\n"));
    let rl = runs(&by_level);
    assert_eq!(rl.len(), 10);
    assert!(rl.windows(2).all(|w| w[0][0].z() >= w[1][0].z()));
    let switches = rl
        .windows(2)
        .filter(|w| box_of(&w[0]) != box_of(&w[1]))
        .count();
    assert!(switches > 1);
    let by_area = run(
        &g,
        &contour(
            "f6",
            "[passes]\nstep_down = 2\n[linking]\norder = \"by_area\"\n",
        ),
    );
    let ra = runs(&by_area);
    assert_eq!(ra.len(), 10);
    let switches = ra
        .windows(2)
        .filter(|w| box_of(&w[0]) != box_of(&w[1]))
        .count();
    assert_eq!(switches, 1);
    for half in ra.chunks(5) {
        assert!(half.windows(2).all(|w| w[0][0].z() > w[1][0].z()));
    }
}

#[test]
fn stock_to_leave_offsets_the_contour() {
    let g = dome();
    let res = run(
        &g,
        &contour("f6", "[passes]\nstep_down = 4\nstock_to_leave = 0.5\n"),
    );
    let at = cut_points(&res)
        .into_iter()
        .filter(|p| p.z() < 15.0 && p.z() > 10.0 && radial(*p) < 20.0)
        .collect::<Vec<_>>();
    assert!(!at.is_empty());
    for p in at {
        assert!((radial(p) - 15.5).abs() < TOL, "{p:?}");
    }
}

#[test]
fn validation_codes() {
    let g = dome();
    let p = |id: &str, s: &str| contour(id, &format!("[passes]\nstep_down = 5\n{s}\n"));
    codes_pass_fail(
        "param.contour.scallop.radius",
        &g,
        &p("r6", "scallop_height = 0.05"),
        &p("f6", "scallop_height = 0.05"),
    );
    codes_pass_fail(
        "param.contour.scallop.range",
        &g,
        &p("r6", "scallop_height = 0.5"),
        &p("r6", "scallop_height = 1"),
    );
    codes_pass_fail(
        "param.contour.slope.order",
        &g,
        &p("b6", "min_slope_deg = 10\nmax_slope_deg = 80"),
        &p("b6", "min_slope_deg = 80\nmax_slope_deg = 10"),
    );
    codes_pass_fail(
        "param.contour.slope.range",
        &g,
        &p("b6", "max_slope_deg = 90"),
        &p("b6", "max_slope_deg = 95"),
    );
    codes_pass_fail(
        "param.passes.step_down.flute",
        &g,
        &contour("b6", "[passes]\nstep_down = 25\n"),
        &contour("b6", "[passes]\nstep_down = 26\n"),
    );
    codes_pass_fail("cam.contour.tool_shape", &g, &p("f6", ""), &p("v6", ""));
}

#[test]
fn no_gouges_with_any_tool() {
    let g = dome();
    for id in ["b6", "f6", "r6"] {
        let t = contour(
            id,
            "[passes]\nstep_down = 3\n[linking]\nretract = \"stay_down\"\n",
        );
        let r = run(&g, &t);
        assert_no_gouge(&g, &r, &tool(id), 0.0, 0.01 + 1e-9);
    }
}

#[test]
fn translation_invariance() {
    let t = contour("b6", "[passes]\nstep_down = 3\nmin_slope_deg = 20\n");
    assert_translation_invariant(step::dome, &t);
}

#[test]
fn seeds_find_every_loop() {
    // Two boxes far apart and a level between their tops: both loops.
    let g = two_boxes([0.0; 3]);
    let res = run(&g, &contour("b6", "[passes]\nstep_down = 20\n"));
    let r = runs(&res);
    assert_eq!(r.len(), 2);
    let c: Vec<Point2> = r.iter().map(|l| Point2::new(l[0].x(), l[0].y())).collect();
    assert!((c[0].x() < 20.0) != (c[1].x() < 20.0));
}

#[test]
fn inside_corners_close() {
    // The pocket block: a pocket with sharp inside corners around an
    // island. Every contour closes, and each pocket loop keeps R from the
    // walls.
    let g = load_step(&step::pocket_block([0.0; 3]));
    for id in ["f6", "b6"] {
        let r = run(&g, &contour(id, "[passes]\nstep_down = 3\n"));
        assert!(!warnings(&r).contains(&"cam.contour.loop_open"), "{id}");
        for l in runs(&r) {
            assert_eq!(l.first(), l.last(), "{id}: open {l:?}");
        }
        let pocket: Vec<Point3> = cut_points(&r)
            .into_iter()
            .filter(|p| {
                p.z() >= 12.0
                    && p.z() <= 17.0
                    && p.x() > 5.0
                    && p.x() < 75.0
                    && p.y() > 5.0
                    && p.y() < 55.0
            })
            .filter(|p| !(p.x() > 30.0 && p.x() < 50.0 && p.y() > 20.0 && p.y() < 40.0))
            .collect();
        assert!(!pocket.is_empty(), "{id}");
        for p in pocket {
            let wall = (p.x() - 10.0)
                .min(70.0 - p.x())
                .min(p.y() - 10.0)
                .min(50.0 - p.y());
            assert!(wall >= 3.0 - 0.01, "{id}: {p:?}");
        }
        assert_no_gouge(&g, &r, &tool(id), 0.0, 0.01 + 1e-9);
    }
}

#[test]
fn sub_tolerance_steps_at_a_flat_level_are_not_lost() {
    // A wafer 0.9 µm high on a plate, like a wall's top edge standing a
    // micron above the flat it meets (a model gap on nist_ctc_02). The
    // level at the plate top must not report a lost contour.
    let mut b = Builder::new("wafer");
    cuboid(
        &mut b,
        Point3::new(0.0, 0.0, -10.0),
        Point3::new(60.0, 60.0, 0.0),
    );
    cuboid(
        &mut b,
        Point3::new(20.0, 20.0, 0.0),
        Point3::new(40.0, 40.0, 9e-4),
    );
    let g = SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-1".into(),
            geom: BodyGeom::Brep(b.build()),
        }],
    };
    for id in ["b6", "f6"] {
        let r = run(&g, &contour(id, "[passes]\nstep_down = 5\n"));
        assert!(!warnings(&r).contains(&"cam.contour.loop_open"), "{id}");
        assert_no_gouge(&g, &r, &tool(id), 0.0, 0.01 + 1e-9);
    }
}
