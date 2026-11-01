// SPDX-License-Identifier: MIT OR Apache-2.0

//! contour_2d and pocket_2d on hand-written STEP parts (D08 §11, D09 §2).

mod support;

use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry, edge_id, face_id};
use fluut_cam::{Generated, Region2, StrategyInput, calculate};
use fluut_geom::{Affine, Curve3, Interval, Point2, Point3, UnitVec3};
use fluut_model::project::{SetupKind, Toolpath, read};
use fluut_model::resolve::HeightEnv;
use fluut_model::tools::{Tool, ToolLibrary};
use fluut_path::{CompSide, Motion, Segment};
use fluut_topo::{WireBody, WireCurve};
use support::step;

fn tool() -> Tool {
    let src = "format = \"fluut.tools/1\"\nid = \"lib\"\n[[tool]]\nid = \"f6\"\nshape = \"flat\"\n\
               diameter = 6\nflute_length = 25\n[[tool.preset]]\nmaterial = \"MDF\"\nrpm = 18000\n\
               feed = 2000\nplunge = 500\nstepover = 2.4\nstep_down = 4\n";
    ToolLibrary::parse(src).unwrap().0.tools.remove(0)
}

fn load(text: &str) -> SetupGeometry {
    let m = fluut_io_step::parse(text.as_bytes()).unwrap();
    let bodies = fluut_io_step::map_bodies(&m)
        .into_iter()
        .map(|b| {
            let b = b.unwrap();
            assert!(b.skipped.is_empty(), "{:?}", b.skipped);
            SetupBody {
                id: format!("b-{}", b.id),
                geom: BodyGeom::Brep(b.body),
            }
        })
        .collect::<Vec<_>>();
    SetupGeometry { bodies }
}

fn body_id(g: &SetupGeometry) -> String {
    g.bodies[0].id.clone()
}

fn tp(kind: &str, groups: &str) -> Toolpath {
    let groups = groups.replace("\n[", "\n[setup.toolpath.");
    let groups = match groups.strip_prefix('[') {
        Some(g) => format!("[setup.toolpath.{g}"),
        None => groups,
    };
    let src = format!(
        "format = \"fluut.project/1\"\n[[setup]]\nid = \"s\"\n[[setup.toolpath]]\n\
         id = \"t\"\ntype = \"{kind}\"\ntool = \"lib#f6\"\n{groups}"
    );
    read(&src)
        .unwrap_or_else(|e| panic!("{e}\n{src}"))
        .project
        .setups[0]
        .toolpaths[0]
        .clone()
}

fn env(top: f64) -> HeightEnv {
    HeightEnv {
        setup_clearance: top + 10.0,
        stock_top: top,
        stock_bottom: 0.0,
        model_top: top,
        model_bottom: 0.0,
        selection: None,
    }
}

fn run(g: &SetupGeometry, t: &Toolpath, top: f64) -> Generated {
    let tool = tool();
    let input = StrategyInput {
        toolpath: t,
        geometry: g,
        tool: &tool,
        material: Some("MDF"),
        setup_kind: SetupKind::ThreeAxis,
        env: env(top),
        to_work: Affine::identity(),
        machine: None,
        post: None,
    };
    calculate(&input).unwrap_or_else(|e| panic!("{e}"))
}

fn run_err(g: &SetupGeometry, t: &Toolpath) -> Vec<String> {
    let tool = tool();
    let input = StrategyInput {
        toolpath: t,
        geometry: g,
        tool: &tool,
        material: Some("MDF"),
        setup_kind: SetupKind::ThreeAxis,
        env: env(10.0),
        to_work: Affine::identity(),
        machine: None,
        post: None,
    };
    calculate(&input)
        .unwrap_err()
        .diagnostics()
        .into_iter()
        .map(|d| d.code)
        .collect()
}

/// Cut segments (motion Cut), in order.
fn cuts(r: &Generated) -> Vec<&Segment> {
    r.result
        .segments()
        .filter(|s| s.motion().is_some_and(|m| m.motion == Motion::Cut))
        .collect()
}

fn xy(p: Point3) -> Point2 {
    Point2::new(p.x(), p.y())
}

fn boss_region() -> Region2 {
    let p = step::Profile::rounded_rect(0.0, 0.0, 40.0, 30.0, 5.0);
    let mut verts = Vec::new();
    for (i, k) in p.kinds.iter().enumerate() {
        let b = match k {
            step::Kind::Line => 0.0,
            step::Kind::Arc { .. } => libm::tan(std::f64::consts::PI / 8.0),
        };
        verts.push(fluut_cam::Vertex::new(p.pts[i][0], p.pts[i][1], b));
    }
    Region2 {
        loops: vec![fluut_cam::Loop2 { verts }],
    }
}

/// Signed area of the XY polygon of a pass's segment ends.
fn signed_area(segs: &[&Segment]) -> f64 {
    let pts: Vec<Point2> = segs.iter().map(|s| xy(s.end().unwrap().xyz)).collect();
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            0.5 * (a.x() * b.y() - b.x() * a.y())
        })
        .sum()
}

fn walls(g: &SetupGeometry) -> String {
    let b = body_id(g);
    let ids: Vec<String> = (0..8)
        .map(|i| format!("{{ face = \"{}\" }}", face_id(&b, i)))
        .collect();
    format!("[geometry]\ndrive_faces = [{}]\n", ids.join(", "))
}

#[test]
fn faces_are_where_the_writer_puts_them() {
    let g = load(&step::boss([0.0; 3]));
    let BodyGeom::Brep(b) = &g.bodies[0].geom else {
        panic!()
    };
    assert_eq!(b.faces.len(), 10);
    let p = load(&step::pocket_block([0.0; 3]));
    let BodyGeom::Brep(b) = &p.bodies[0].geom else {
        panic!()
    };
    assert_eq!(b.faces.len(), 16);
}

#[test]
fn contour_walls_edges_and_floor_loops_agree() {
    let g = load(&step::boss([0.0; 3]));
    let b = body_id(&g);
    let from_walls = run(
        &g,
        &tp(
            "contour_2d",
            &format!("{}[passes]\nstep_down = 20\n", walls(&g)),
        ),
        10.0,
    );
    let from_face = run(
        &g,
        &tp(
            "contour_2d",
            &format!(
                "[geometry]\ndrive_curves = [{{ face = \"{}\" }}]\n[passes]\nstep_down = 20\n",
                face_id(&b, 8)
            ),
        ),
        10.0,
    );
    assert_eq!(cuts(&from_walls).len(), cuts(&from_face).len());
    let a: f64 = cuts(&from_walls).iter().map(|s| s.length()).sum();
    let c: f64 = cuts(&from_face).iter().map(|s| s.length()).sum();
    // Outline offset by R = 3: 2·(30 + 20) + 2π·(5 + 3).
    let want = 100.0 + std::f64::consts::TAU * 8.0;
    assert!(
        (a - want).abs() < 1e-9 && (c - want).abs() < 1e-9,
        "{a} {c} {want}"
    );
    assert!(
        from_walls.diagnostics.is_empty(),
        "{:?}",
        from_walls.diagnostics
    );
}

#[test]
fn contour_offsets_and_endpoints_are_exact() {
    let g = load(&step::boss([0.0; 3]));
    let r = run(
        &g,
        &tp(
            "contour_2d",
            &format!("{}[passes]\nstep_down = 4\n", walls(&g)),
        ),
        10.0,
    );
    let boss = boss_region();
    for s in cuts(&r) {
        for p in [s.start().unwrap(), s.end().unwrap()] {
            let d = boss.boundary_distance(xy(p.xyz));
            assert!((d - 3.0).abs() < 1e-9, "{d}");
        }
    }
    // Levels: 10 mm in steps of at most 4: 6.667, 3.333, 0.
    let mut zs: Vec<f64> = cuts(&r).iter().map(|s| s.end().unwrap().xyz.z()).collect();
    zs.dedup();
    assert_eq!(zs, [10.0 - 10.0 / 3.0, 10.0 - 20.0 / 3.0, 0.0]);
}

#[test]
fn contour_direction_per_side() {
    let g = load(&step::boss([0.0; 3]));
    let cases = [
        ("outside", "climb", -1.0),
        ("outside", "conventional", 1.0),
        ("inside", "climb", 1.0),
        ("inside", "conventional", -1.0),
    ];
    for (side, dir, sign) in cases {
        let t = tp(
            "contour_2d",
            &format!(
                "{}[passes]\nstep_down = 20\nside = \"{side}\"\ndirection = \"{dir}\"\n",
                walls(&g)
            ),
        );
        let r = run(&g, &t, 10.0);
        let a = signed_area(&cuts(&r));
        assert!(a * sign > 0.0, "{side} {dir}: {a}");
    }
}

#[test]
fn contour_finishing_and_spring_passes() {
    let g = load(&step::boss([0.0; 3]));
    let t = tp(
        "contour_2d",
        &format!(
            "{}[passes]\nstep_down = 6\nfinishing_pass_count = 2\nfinishing_stepover = 0.5\nspring_pass_count = 1\n",
            walls(&g)
        ),
    );
    let r = run(&g, &t, 10.0);
    let boss = boss_region();
    // One distance per cut pass, in order.
    let dists: Vec<f64> = r
        .result
        .passes()
        .iter()
        .filter(|p| {
            p.segments
                .iter()
                .any(|s| s.motion().is_some_and(|m| m.motion == Motion::Cut))
        })
        .map(|p| {
            let s = p
                .segments
                .iter()
                .find(|s| s.motion().unwrap().motion == Motion::Cut)
                .unwrap();
            (boss.boundary_distance(xy(s.end().unwrap().xyz)) * 1e6).round() / 1e6
        })
        .collect();
    assert_eq!(dists, [3.0, 4.0, 3.5, 3.0, 3.0]);
}

#[test]
fn contour_tabs_rise_with_exact_ends() {
    let g = load(&step::boss([0.0; 3]));
    let t = tp(
        "contour_2d",
        &format!(
            "{}[passes]\nstep_down = 20\ntabs = {{ mode = \"count\", count = 4, width = 4, height = 2, ramp_angle_deg = 45 }}\n",
            walls(&g)
        ),
    );
    let r = run(&g, &t, 10.0);
    let segs = cuts(&r);
    let top: Vec<&&Segment> = segs
        .iter()
        .filter(|s| s.start().unwrap().xyz.z() == 2.0 && s.end().unwrap().xyz.z() == 2.0)
        .collect();
    let flat: f64 = top.iter().map(|s| s.length()).sum();
    // Four flats of width + D = 10 mm each.
    assert!((flat - 40.0).abs() < 1e-9, "{flat}");
    let ramps: Vec<&&Segment> = segs
        .iter()
        .filter(|s| s.start().unwrap().xyz.z() != s.end().unwrap().xyz.z())
        .collect();
    assert_eq!(ramps.len(), 8);
    for s in ramps {
        let dz = (s.end().unwrap().xyz.z() - s.start().unwrap().xyz.z()).abs();
        assert_eq!(dz, 2.0);
    }
    assert!(segs.iter().all(|s| s.end().unwrap().xyz.z() <= 2.0));
}

#[test]
fn contour_control_compensation() {
    let g = load(&step::boss([0.0; 3]));
    let t = tp(
        "contour_2d",
        &format!(
            "{}[passes]\nstep_down = 20\ncompensation = \"control\"\n[linking]\nlead_in = {{ kind = \"arc\", radius = 4, length = 3 }}\n",
            walls(&g)
        ),
    );
    let r = run(&g, &t, 10.0);
    let boss = boss_region();
    for s in cuts(&r) {
        assert!(boss.boundary_distance(xy(s.end().unwrap().xyz)) < 1e-9);
    }
    let comp: Vec<_> = r.result.passes().iter().filter_map(|p| p.comp).collect();
    assert_eq!(comp, [CompSide::Left]);
    let wear = run(
        &g,
        &tp(
            "contour_2d",
            &format!(
                "{}[passes]\nstep_down = 20\ncompensation = \"wear\"\ndirection = \"conventional\"\n",
                walls(&g)
            ),
        ),
        10.0,
    );
    let comp: Vec<_> = wear.result.passes().iter().filter_map(|p| p.comp).collect();
    assert_eq!(comp, [CompSide::Right]);
    assert!(
        wear.diagnostics
            .iter()
            .any(|d| d.code == "param.contour_2d.compensation.lead")
    );
}

fn wire(pts: &[[f64; 3]]) -> SetupGeometry {
    let curves = pts
        .windows(2)
        .map(|w| {
            let a = Point3::new(w[0][0], w[0][1], w[0][2]);
            let b = Point3::new(w[1][0], w[1][1], w[1][2]);
            WireCurve {
                curve: Curve3::line(a, UnitVec3::new(b - a).unwrap()),
                range: Interval {
                    lo: 0.0,
                    hi: a.distance(b),
                },
            }
        })
        .collect();
    SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-w".into(),
            geom: BodyGeom::Wire(WireBody {
                name: "w".into(),
                tol_model: 1e-6,
                curves,
            }),
        }],
    }
}

#[test]
fn contour_open_chains_left_and_right() {
    let g = wire(&[[0.0, 0.0, 0.0], [30.0, 0.0, 0.0]]);
    let base = "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n[heights]\nbottom = { ref = \"work_zero\", offset = -1 }\n[passes]\nstep_down = 20\n";
    for (side, y, x_start) in [("left", 3.0, 0.0), ("right", -3.0, 30.0)] {
        let r = run(
            &g,
            &tp("contour_2d", &format!("{base}side = \"{side}\"\n")),
            5.0,
        );
        let c = cuts(&r);
        assert!(
            c.iter()
                .all(|s| (s.end().unwrap().xyz.y() - y).abs() < 1e-9),
            "{side}"
        );
        // Climb: material on the right of travel.
        assert_eq!(c[0].start().unwrap().xyz.x(), x_start, "{side}");
    }
    let e = run_err(
        &g,
        &tp("contour_2d", &format!("{base}side = \"outside\"\n")),
    );
    assert!(
        e.contains(&"param.contour_2d.side.closure".to_owned()),
        "{e:?}"
    );
    let ok = run(
        &g,
        &tp(
            "contour_2d",
            &format!("{base}side = \"right\"\nstart_point = [0, 0, 0]\n"),
        ),
        5.0,
    );
    assert!(
        ok.diagnostics
            .iter()
            .any(|d| d.code == "param.contour_2d.start_point.open")
    );
    // Edge ids select single wire curves.
    let t = tp(
        "contour_2d",
        &format!(
            "[geometry]\ndrive_curves = [{{ edge = \"{}\" }}]\n[passes]\nstep_down = 20\nside = \"on\"\n",
            edge_id("b-w", 0)
        ),
    );
    let r = run(&g, &t, 5.0);
    assert!(cuts(&r).iter().all(|s| s.end().unwrap().xyz.y() == 0.0));
}

#[test]
fn contour_geometry_errors() {
    let g = load(&step::boss([0.0; 3]));
    let b = body_id(&g);
    let top = format!(
        "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n",
        face_id(&b, 9)
    );
    assert_eq!(
        run_err(&g, &tp("contour_2d", &top)),
        ["param.geometry.drive_faces.kind"]
    );
    let missing = "[geometry]\ndrive_faces = [{ face = \"f-999.0\" }]\n";
    assert_eq!(
        run_err(&g, &tp("contour_2d", missing)),
        ["param.geometry.selection.missing"]
    );
    let tiny = wire(&[
        [0.0, 0.0, 0.0],
        [4.0, 0.0, 0.0],
        [4.0, 4.0, 0.0],
        [0.0, 4.0, 0.0],
        [0.0, 0.0, 0.0],
    ]);
    let t = tp(
        "contour_2d",
        "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n[passes]\nside = \"inside\"\nstep_down = 20\n",
    );
    let r = run(&tiny, &t, 5.0);
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == "cam.contour_2d.offset_empty")
    );
    let eight = wire(&[
        [0.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [10.0, 0.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, 0.0, 0.0],
    ]);
    assert!(run_err(&eight, &t).contains(&"cam.geometry.chain_self_intersects".to_owned()));
}

fn pocket_floor(g: &SetupGeometry) -> String {
    format!(
        "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n",
        face_id(&body_id(g), 10)
    )
}

#[test]
fn pocket_offset_rings_keep_the_island_and_floor() {
    let g = load(&step::pocket_block([0.0; 3]));
    let t = tp(
        "pocket_2d",
        &format!(
            "{}[passes]\nstepover = 2.5\nstep_down = 5\nstock_to_leave = 0.5\nstock_to_leave_axial = 0.25\n",
            pocket_floor(&g)
        ),
    );
    let r = run(&g, &t, 20.0);
    // Small inner rings cannot take the helix: a fallback warning only.
    assert!(
        r.diagnostics
            .iter()
            .all(|d| d.code == "cam.linking.ramp_fallback")
    );
    let clear =
        Region2::rect(10.0, 10.0, 70.0, 50.0).with_hole(&Region2::rect(35.0, 25.0, 45.0, 35.0));
    let segs = cuts(&r);
    for s in &segs {
        let p = xy(s.end().unwrap().xyz);
        assert!(clear.boundary_distance(p) >= 3.5 - 1e-9);
    }
    let mut zs: Vec<f64> = segs.iter().map(|s| s.end().unwrap().xyz.z()).collect();
    zs.sort_by(f64::total_cmp);
    zs.dedup();
    // From the stock top (20) to the floor + axial stock (12.25) in 2 levels.
    assert_eq!(zs, [12.25, 20.0 - 7.75 / 2.0]);
    // The outermost ring touches the walls at R + stock to leave.
    let min_d = segs
        .iter()
        .map(|s| clear.boundary_distance(xy(s.end().unwrap().xyz)))
        .fold(f64::INFINITY, f64::min);
    assert!((min_d - 3.5).abs() < 1e-9);
}

#[test]
fn pocket_stepover_never_exceeded() {
    // Every point of the cleared area lies within stepover/2 + R of a cut
    // when rings are at most `stepover` apart: sample the centre region.
    let g = load(&step::pocket_block([0.0; 3]));
    for pattern in ["offset", "raster"] {
        let t = tp(
            "pocket_2d",
            &format!(
                "{}[passes]\npattern = \"{pattern}\"\nstepover = 2.5\nstep_down = 10\npass_angle_deg = 30\n",
                pocket_floor(&g)
            ),
        );
        let r = run(&g, &t, 20.0);
        let lines: Vec<fluut_cam::Seg2> = cuts(&r)
            .iter()
            .map(|s| fluut_cam::Seg2 {
                a: xy(s.start().unwrap().xyz),
                b: xy(s.end().unwrap().xyz),
                bulge: match s {
                    Segment::Arc { sweep, .. } => libm::tan(sweep / 4.0),
                    _ => 0.0,
                },
            })
            .collect();
        let centre = Region2::rect(13.0, 13.0, 67.0, 47.0);
        let mut worst: f64 = 0.0;
        for i in 0..=54 {
            for j in 0..=34 {
                let q = Point2::new(13.0 + i as f64, 13.0 + j as f64);
                if (32.0..=48.0).contains(&q.x()) && (22.0..=38.0).contains(&q.y()) {
                    continue;
                }
                let d = lines
                    .iter()
                    .map(|sg| fluut_cam::region::seg_distance(sg, q))
                    .fold(f64::INFINITY, f64::min);
                worst = worst.max(d);
            }
        }
        let _ = centre;
        // Raster lines are at most a stepover apart: every point is within
        // s/2 of a pass. Offset rings are a stepover apart along their
        // normals; where rings from the island and from the wall merge
        // (here at 90°), the corner of the merged ring is up to
        // (s/2)/sin 45° from the nearest pass. Both stay well under R.
        let bound = if pattern == "raster" {
            1.25
        } else {
            1.25 * std::f64::consts::SQRT_2
        };
        assert!(worst <= bound + 1e-9, "{pattern}: {worst}");
    }
}

#[test]
fn pocket_raster_lines_end_on_the_region() {
    let g = load(&step::pocket_block([0.0; 3]));
    let t = tp(
        "pocket_2d",
        &format!(
            "{}[passes]\npattern = \"raster\"\nstepover = 2\nstep_down = 10\n",
            pocket_floor(&g)
        ),
    );
    let r = run(&g, &t, 20.0);
    let centre = Region2::rect(13.0, 13.0, 67.0, 47.0)
        .subtract(&Region2::rect(35.0, 25.0, 45.0, 35.0).offset(3.0));
    let mut n = 0;
    for s in cuts(&r) {
        if let Segment::Line { a, b, .. } = s
            && a.xyz.y() == b.xyz.y()
        {
            for p in [a, b] {
                assert!(centre.boundary_distance(xy(p.xyz)) < 1e-9);
            }
            n += 1;
        }
    }
    assert!(n > 20);
}

#[test]
fn pocket_from_closed_wire_with_island_and_open_chain_error() {
    let mut g = wire(&[
        [0.0, 0.0, -5.0],
        [40.0, 0.0, -5.0],
        [40.0, 30.0, -5.0],
        [0.0, 30.0, -5.0],
        [0.0, 0.0, -5.0],
    ]);
    let island = wire(&[
        [15.0, 10.0, -5.0],
        [25.0, 10.0, -5.0],
        [25.0, 20.0, -5.0],
        [15.0, 20.0, -5.0],
        [15.0, 10.0, -5.0],
    ]);
    let BodyGeom::Wire(w) = &island.bodies[0].geom else {
        panic!()
    };
    if let BodyGeom::Wire(x) = &mut g.bodies[0].geom {
        x.curves.extend(w.curves.iter().cloned());
    }
    let t = tp(
        "pocket_2d",
        "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n[heights]\ntop = { ref = \"work_zero\" }\n[passes]\nstep_down = 5\n",
    );
    let r = run(&g, &t, 0.0);
    let island = Region2::rect(15.0, 10.0, 25.0, 20.0);
    for s in cuts(&r) {
        assert!(island.boundary_distance(xy(s.end().unwrap().xyz)) >= 3.0 - 1e-9);
        assert_eq!(s.end().unwrap().xyz.z(), -5.0);
    }
    let open = wire(&[[0.0, 0.0, -5.0], [40.0, 0.0, -5.0], [40.0, 30.0, -5.0]]);
    let e = run_err(&open, &t);
    assert!(e.contains(&"cam.pocket_2d.region.open".to_owned()), "{e:?}");
}

#[test]
fn pocket_helix_entry_or_fallback() {
    let g = load(&step::pocket_block([0.0; 3]));
    let t = tp(
        "pocket_2d",
        &format!("{}[passes]\nstep_down = 10\n", pocket_floor(&g)),
    );
    let r = run(&g, &t, 20.0);
    assert!(
        r.result
            .segments()
            .any(|s| matches!(s, Segment::Helix { .. }))
    );
    let big = tp(
        "pocket_2d",
        &format!(
            "{}[passes]\nstep_down = 10\npattern = \"raster\"\n[linking]\nramp = {{ kind = \"helix\", radius = 30 }}\n",
            pocket_floor(&g)
        ),
    );
    let r = run(&g, &big, 20.0);
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == "cam.linking.ramp_fallback")
    );
    assert!(
        !r.result
            .segments()
            .any(|s| matches!(s, Segment::Helix { .. }))
    );
}

#[test]
fn parallel_is_registered_but_unavailable() {
    let g = load(&step::boss([0.0; 3]));
    assert_eq!(run_err(&g, &tp("parallel", "")), ["cam.type.unavailable"]);
    assert_eq!(fluut_cam::registry().len(), 3);
}
