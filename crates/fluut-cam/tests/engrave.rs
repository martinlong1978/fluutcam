// SPDX-License-Identifier: MIT OR Apache-2.0

//! `engrave` (D09 §2.6, D15b §2.4): V-carving along the exact medial axis.

mod support;

use fluut_cam::{Generated, Region2, StrategyInput, calculate, strategy::Engrave};
use fluut_geom::{Affine, Point2};
use fluut_model::project::{SetupKind, Toolpath};
use fluut_model::tools::{Tool, ToolLibrary};
use fluut_path::{Motion, Segment};
use support::cam::*;

const W: &str = "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n";

const TOP: f64 = 20.0;

fn stock() -> Stock {
    Stock::new([-20.0, -20.0, 0.0], [60.0, 60.0, TOP])
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> fluut_cam::geometry::SetupGeometry {
    wire(&[
        [x0, y0, TOP],
        [x1, y0, TOP],
        [x1, y1, TOP],
        [x0, y1, TOP],
        [x0, y0, TOP],
    ])
}

/// A 60° V tool (half-angle 30°) with a sharp tip and short flutes.
const V60: &str = r#"
format = "fluut.tools/1"
id = "lib"
[[tool]]
id = "v60"
shape = "v"
diameter = 10
half_angle_deg = 30
flute_length = 4
  [[tool.preset]]
  material = "MDF"
  rpm = 18000
  feed = 1500
  plunge = 500
"#;

fn v60() -> Tool {
    ToolLibrary::parse(V60).unwrap().0.tools.remove(0)
}

fn run_tool(
    g: &fluut_cam::geometry::SetupGeometry,
    t: &Toolpath,
    tool: &Tool,
) -> Result<Generated, Vec<String>> {
    let s = stock();
    let input = StrategyInput {
        toolpath: t,
        geometry: g,
        tool,
        material: Some("MDF"),
        setup_kind: SetupKind::ThreeAxis,
        env: s.env(),
        stock: Some(s.aabb()),
        to_work: Affine::identity(),
        machine: None,
        post: None,
    };
    calculate(&input).map_err(|e| e.diagnostics().into_iter().map(|d| d.code).collect())
}

/// Every end point of the cut moves.
fn cut_points(r: &Generated) -> Vec<fluut_geom::Point3> {
    cuts(r).iter().flat_map(|s| [start(s), end(s)]).collect()
}

/// No cut point is deeper than `max_depth`, and the cone at every cut
/// point stays inside the shape: Rt + depth · tan α ≤ the distance to the
/// boundary.
fn assert_inside(r: &Generated, shape: &Region2, rt: f64, tan: f64, max_depth: f64) {
    for p in cut_points(r) {
        let d = TOP - p.z();
        assert!(d <= max_depth + 1e-9, "{p:?} deeper than {max_depth}");
        let q = Point2::new(p.x(), p.y());
        let room = shape.boundary_distance(q);
        if d > 1e-9 {
            assert!(
                rt + d * tan <= room + 0.01,
                "{p:?}: cone {} vs {room}",
                rt + d * tan
            );
        }
    }
}

#[test]
fn a_narrow_stroke_is_cut_to_its_full_depth() {
    // 30 × 4: the axis is the midline (clearance 2) plus four corner
    // branches. v90: Rt 0.1, α 45°, so the midline is 1.9 deep.
    let g = rect(0.0, 0.0, 30.0, 4.0);
    let r = run(&g, &tp("engrave", "v90", W), stock());
    assert!(codes(&r).is_empty(), "{:?}", codes(&r));
    let shape = Region2::rect(0.0, 0.0, 30.0, 4.0);
    assert_inside(&r, &shape, 0.1, 1.0, 5.9);
    let pts = cut_points(&r);
    let deepest = pts.iter().map(|p| p.z()).fold(f64::INFINITY, f64::min);
    assert!((deepest - (TOP - 1.9)).abs() < 1e-9, "{deepest}");
    // The midline is cut at full depth from x = 2 to x = 28.
    let mid: Vec<_> = pts
        .iter()
        .filter(|p| (p.z() - (TOP - 1.9)).abs() < 1e-9)
        .collect();
    let xs = mid.iter().map(|p| p.x());
    let (x0, x1) = xs.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| {
        (a.min(x), b.max(x))
    });
    assert!(
        (x0 - 2.0).abs() < 1e-6 && (x1 - 28.0).abs() < 1e-6,
        "{x0} {x1}"
    );
    // The corner branches rise to the tip radius: depth 0 at 0.1 from the
    // corner along the bisector.
    let near_corner = pts
        .iter()
        .map(|p| Point2::new(p.x(), p.y()).distance(Point2::new(0.0, 0.0)))
        .fold(f64::INFINITY, f64::min);
    assert!(
        (near_corner - 0.1 * 2f64.sqrt()).abs() < 1e-6,
        "{near_corner}"
    );
    // One plunge per path: the stroke is one path from corner to corner and
    // two short branches each side.
    assert!(moves(&r, Motion::Plunge).len() <= 3);
}

#[test]
fn depth_follows_the_half_angle() {
    // A 60° tool with a sharp tip: depth = ρ / tan 30°.
    let g = rect(0.0, 0.0, 30.0, 2.0);
    let t = tp("engrave", "v60", W);
    let r = run_tool(&g, &t, &v60()).unwrap();
    let deepest = cut_points(&r)
        .iter()
        .map(|p| p.z())
        .fold(f64::INFINITY, f64::min);
    let want = 1.0 / libm::tan(30f64.to_radians());
    assert!((TOP - deepest - want).abs() < 1e-9, "{}", TOP - deepest);
    // Flute 4 is shorter than the cone (5 / tan 30° = 8.66): a warning.
    assert_eq!(codes(&r), ["param.engrave.max_depth.flute"]);
    let shape = Region2::rect(0.0, 0.0, 30.0, 2.0);
    assert_inside(&r, &shape, 0.0, libm::tan(30f64.to_radians()), 8.67);
}

#[test]
fn wide_shapes_leave_a_flat_area() {
    // 30 × 30 with max_depth 4: ρmax = 0.1 + 4 = 4.1, so the ring is the
    // square shrunk by 4.1, cut at depth 4.
    let g = rect(0.0, 0.0, 30.0, 30.0);
    let t = tp("engrave", "v90", &format!("{W}[passes]\nmax_depth = 4\n"));
    let r = run(&g, &t, stock());
    assert_eq!(codes(&r), ["cam.engrave.flat_area"]);
    let shape = Region2::rect(0.0, 0.0, 30.0, 30.0);
    assert_inside(&r, &shape, 0.1, 1.0, 4.0);
    let ring: Vec<_> = cut_points(&r)
        .into_iter()
        .filter(|p| (p.z() - (TOP - 4.0)).abs() < 1e-9)
        .collect();
    assert!(
        ring.iter()
            .any(|p| (p.x() - 4.1).abs() < 1e-9 && (p.y() - 4.1).abs() < 1e-9)
    );
    for p in &ring {
        let q = Point2::new(p.x(), p.y());
        assert!((shape.boundary_distance(q) - 4.1).abs() < 1e-6, "{p:?}");
    }
    // The flat area for a clearing toolpath.
    let s = stock();
    let tool = tool("v90");
    let input = StrategyInput {
        toolpath: &t,
        geometry: &g,
        tool: &tool,
        material: Some("MDF"),
        setup_kind: SetupKind::ThreeAxis,
        env: s.env(),
        stock: Some(s.aabb()),
        to_work: Affine::identity(),
        machine: None,
        post: None,
    };
    let flat = Engrave::flat_areas(&input).unwrap();
    assert!((flat.area() - 21.8 * 21.8).abs() < 1e-6, "{}", flat.area());
    // A narrow stroke has none.
    let r = run(&rect(0.0, 0.0, 30.0, 4.0), &t, stock());
    assert!(codes(&r).is_empty());
}

#[test]
fn corner_sharpening_reaches_into_flat_area_corners() {
    let g = rect(0.0, 0.0, 30.0, 30.0);
    let near = |r: &Generated| {
        cut_points(r)
            .iter()
            .map(|p| Point2::new(p.x(), p.y()).distance(Point2::new(0.0, 0.0)))
            .fold(f64::INFINITY, f64::min)
    };
    let off = run(
        &g,
        &tp("engrave", "v90", &format!("{W}[passes]\nmax_depth = 4\n")),
        stock(),
    );
    let on = run(
        &g,
        &tp(
            "engrave",
            "v90",
            &format!("{W}[passes]\nmax_depth = 4\ncorner_sharpening = true\n"),
        ),
        stock(),
    );
    // Off: the nearest cut to the corner is the ring corner at (4.1, 4.1).
    assert!(
        (near(&off) - 4.1 * 2f64.sqrt()).abs() < 1e-6,
        "{}",
        near(&off)
    );
    // On: the branch runs up to depth 0 at the tip radius.
    assert!(
        (near(&on) - 0.1 * 2f64.sqrt()).abs() < 1e-6,
        "{}",
        near(&on)
    );
    let shape = Region2::rect(0.0, 0.0, 30.0, 30.0);
    assert_inside(&on, &shape, 0.1, 1.0, 4.0);
}

#[test]
fn step_down_clips_each_level() {
    // A stroke 3 deep (ρ 3.1) in two levels of 1.5.
    let g = rect(0.0, 0.0, 40.0, 6.2);
    let one = run(&g, &tp("engrave", "v90", W), stock());
    let t = tp("engrave", "v90", &format!("{W}[passes]\nstep_down = 2\n"));
    let r = run(&g, &t, stock());
    let shape = Region2::rect(0.0, 0.0, 40.0, 6.2);
    assert_inside(&r, &shape, 0.1, 1.0, 5.9);
    let zs: Vec<f64> = cut_points(&r).iter().map(|p| p.z()).collect();
    let deepest = zs.iter().copied().fold(f64::INFINITY, f64::min);
    assert!((TOP - deepest - 3.0).abs() < 1e-9);
    // The first level is clipped at 1.5 and the second goes to 3.
    assert!(zs.iter().any(|&z| (z - (TOP - 1.5)).abs() < 1e-9));
    assert_eq!(
        moves(&r, Motion::Plunge).len(),
        2 * moves(&one, Motion::Plunge).len()
    );
}

#[test]
fn holes_and_arcs() {
    // A ring: outer circle radius 10, hole radius 6. The axis is the
    // circle of radius 8 at clearance 2: one closed path at depth 1.9.
    let mut g = wire_circle(10.0, 10.0, TOP, 10.0);
    let hole = wire_circle(10.0, 10.0, TOP, 6.0);
    let fluut_cam::geometry::BodyGeom::Wire(w) = &mut g.bodies[0].geom else {
        unreachable!()
    };
    let fluut_cam::geometry::BodyGeom::Wire(h) = &hole.bodies[0].geom else {
        unreachable!()
    };
    w.curves.extend(h.curves.iter().cloned());
    let r = run(&g, &tp("engrave", "v90", W), stock());
    let pts = cut_points(&r);
    for p in &pts {
        let q = Point2::new(p.x(), p.y()).distance(Point2::new(10.0, 10.0));
        assert!((q - 8.0).abs() < 0.01, "{p:?}");
        assert!((TOP - p.z() - 1.9).abs() < 1e-6, "{p:?}");
    }
    assert_eq!(moves(&r, Motion::Plunge).len(), 1);
}

#[test]
fn codes_pass_and_fail() {
    let square = rect(0.0, 0.0, 10.0, 10.0);
    // param.engrave.tool_shape
    let r = run(&square, &tp("engrave", "v90", W), stock());
    assert!(codes(&r).is_empty());
    let t = tp("engrave", "f6", W);
    assert!(run_err(&square, &t, stock()).contains(&"param.engrave.tool_shape".into()));
    // param.engrave.max_depth.cone: v90's cone is 5.9.
    let t = tp("engrave", "v90", &format!("{W}[passes]\nmax_depth = 5.9\n"));
    assert!(codes(&run(&square, &t, stock())).is_empty());
    let t = tp("engrave", "v90", &format!("{W}[passes]\nmax_depth = 6\n"));
    assert!(run_err(&square, &t, stock()).contains(&"param.engrave.max_depth.cone".into()));
    // param.engrave.max_depth.flute: v60's flute is 4.
    let tool = v60();
    let t = tp("engrave", "v60", &format!("{W}[passes]\nmax_depth = 4\n"));
    assert!(
        !run_tool(&square, &t, &tool)
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.code == "param.engrave.max_depth.flute")
    );
    let t = tp("engrave", "v60", &format!("{W}[passes]\nmax_depth = 4.5\n"));
    let r = run_tool(&rect(0.0, 0.0, 30.0, 2.0), &t, &tool).unwrap();
    assert!(codes(&r).contains(&"param.engrave.max_depth.flute".into()));
    // cam.pocket_2d.region.open
    let line = wire(&[[0.0, 0.0, TOP], [10.0, 0.0, TOP]]);
    let t = tp("engrave", "v90", W);
    assert!(run_err(&line, &t, stock()).contains(&"cam.pocket_2d.region.open".into()));
    // cam.engrave.flat_area: the 10 × 10 square at the full cone depth
    // has none (ρ 5 < 6); at max_depth 2 it has.
    let t = tp("engrave", "v90", &format!("{W}[passes]\nmax_depth = 2\n"));
    assert_eq!(codes(&run(&square, &t, stock())), ["cam.engrave.flat_area"]);
}

#[test]
fn every_entry_plunges_and_cuts_follow_the_axis() {
    // An L-shape: reflex corner, parabolic edges.
    let g = wire(&[
        [0.0, 0.0, TOP],
        [20.0, 0.0, TOP],
        [20.0, 6.0, TOP],
        [6.0, 6.0, TOP],
        [6.0, 20.0, TOP],
        [0.0, 20.0, TOP],
        [0.0, 0.0, TOP],
    ]);
    let r = run(&g, &tp("engrave", "v90", W), stock());
    let shape = Region2::polygon(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 6.0),
        (6.0, 6.0),
        (6.0, 20.0),
        (0.0, 20.0),
    ]);
    assert_inside(&r, &shape, 0.1, 1.0, 5.9);
    // Every cut point is on the axis: its depth matches its clearance.
    for p in cut_points(&r) {
        let q = Point2::new(p.x(), p.y());
        let want = ((shape.boundary_distance(q) - 0.1) / 1.0).max(0.0);
        assert!((TOP - p.z() - want).abs() < 0.01, "{p:?}: {want}");
    }
    assert!(
        r.result
            .segments()
            .filter(|s| s.motion().is_some_and(|m| m.motion == Motion::Ramp))
            .count()
            == 0
    );
    assert!(
        r.result
            .segments()
            .all(|s| !matches!(s, Segment::Helix { .. }))
    );
}

/// A wire body `b-w` of the loops given as bulge vertices, at Z = TOP.
fn wire_loops(loops: &[Vec<(f64, f64, f64)>]) -> fluut_cam::geometry::SetupGeometry {
    use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry};
    use fluut_geom::{Curve3, Frame, Interval, Point3, UnitVec3, Vec3};
    let mut curves = Vec::new();
    for l in loops {
        let lp = fluut_cam::Loop2 {
            verts: l
                .iter()
                .map(|&(x, y, b)| fluut_cam::Vertex::new(x, y, b))
                .collect(),
        };
        for s in lp.segs() {
            let a = Point3::new(s.a.x(), s.a.y(), TOP);
            let b = Point3::new(s.b.x(), s.b.y(), TOP);
            let wc = match s.arc() {
                None => fluut_topo::WireCurve {
                    curve: Curve3::line(a, UnitVec3::new(b - a).unwrap()),
                    range: Interval {
                        lo: 0.0,
                        hi: a.distance(b),
                    },
                },
                Some(g) => {
                    let up = g.sweep > 0.0;
                    let f = Frame::new(
                        Point3::new(g.centre.x(), g.centre.y(), TOP),
                        Vec3::new(0.0, 0.0, if up { 1.0 } else { -1.0 }),
                        Vec3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap();
                    let (lo, hi) = if up {
                        (g.start, g.start + g.sweep)
                    } else {
                        (-g.start, -g.start - g.sweep)
                    };
                    fluut_topo::WireCurve {
                        curve: Curve3::circle(f, g.radius).unwrap(),
                        range: Interval { lo, hi },
                    }
                }
            };
            curves.push(wc);
        }
    }
    SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-w".into(),
            geom: BodyGeom::Wire(fluut_topo::WireBody {
                name: "w".into(),
                tol_model: 1e-6,
                curves,
            }),
        }],
    }
}

/// Lettering-like loops: a curved stroke with round ends, a bowl with a
/// hole, a serifed stem.
fn letters() -> Vec<Vec<(f64, f64, f64)>> {
    let b = libm::tan(std::f64::consts::PI / 8.0);
    vec![
        vec![
            (0.0, -6.0, 1.0),
            (0.0, 6.0, 1.0),
            (0.0, 4.0, -1.0),
            (0.0, -4.0, 1.0),
        ],
        vec![
            (14.0, 0.0, b),
            (18.0, 6.0, b),
            (14.0, 12.0, b),
            (10.0, 6.0, b),
        ],
        vec![
            (14.0, 2.0, -b),
            (12.0, 6.0, -b),
            (14.0, 10.0, -b),
            (16.0, 6.0, -b),
        ],
        vec![
            (21.0, 0.0, 0.0),
            (27.0, 0.0, 0.0),
            (27.0, 1.0, 0.0),
            (25.0, 1.5, 0.0),
            (25.0, 10.5, 0.0),
            (27.0, 11.0, 0.0),
            (27.0, 12.0, 0.0),
            (21.0, 12.0, 0.0),
            (21.0, 11.0, 0.0),
            (23.0, 10.5, 0.0),
            (23.0, 1.5, 0.0),
            (21.0, 1.0, 0.0),
        ],
    ]
}

#[test]
fn lettering_stays_inside_its_outline() {
    let loops = letters();
    let g = wire_loops(&loops);
    let mut shape = Region2::empty();
    for (i, l) in loops.iter().enumerate() {
        let lp = fluut_cam::Loop2 {
            verts: l
                .iter()
                .map(|&(x, y, b)| fluut_cam::Vertex::new(x, y, b))
                .collect(),
        };
        // The bowl's hole is the third loop.
        let lp = if (i == 2) == lp.is_ccw() {
            lp.reversed()
        } else {
            lp
        };
        shape.loops.push(lp);
    }
    for (groups, rt, md) in [
        (W.to_string(), 0.1, 5.9),
        (format!("{W}[passes]\nmax_depth = 0.6\n"), 0.1, 0.6),
        (
            format!("{W}[passes]\nmax_depth = 0.6\ncorner_sharpening = true\nstep_down = 0.25\n"),
            0.1,
            0.6,
        ),
    ] {
        let r = run(&g, &tp("engrave", "v90", &groups), stock());
        assert_inside(&r, &shape, rt, 1.0, md);
        assert!(!cuts(&r).is_empty());
    }
    // Every shape is carved: some cut deeper than 0.5 in each.
    let r = run(&g, &tp("engrave", "v90", W), stock());
    for (x0, x1) in [(-1.0, 7.0), (9.0, 19.0), (20.0, 28.0)] {
        assert!(
            cut_points(&r)
                .iter()
                .any(|p| p.x() > x0 && p.x() < x1 && TOP - p.z() > 0.5),
            "{x0}"
        );
    }
}
