// SPDX-License-Identifier: MIT OR Apache-2.0

//! `parallel` (D09 §4.3, D15 §5) on the ridge part (a convex cylinder top)
//! and synthetic bodies: validation codes, layout, directions, slope
//! limits, boundary, stock to leave, heights, links, leads, scallop height,
//! gouges and translation invariance (D08 §11).

mod support;

use fluut_cam::cutter::Cutter;
use fluut_cam::cutter::synth::{Builder, line};
use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry, face_id};
use fluut_cam::surface::{FaceSets, SurfaceQuery};
use fluut_cam::{Generated, StrategyInput, calculate};
use fluut_geom::{Affine, Frame, Interval, Point2, Point3, Surface, UvBox, Vec3};
use fluut_model::project::{SetupKind, Toolpath, read};
use fluut_model::resolve::HeightEnv;
use fluut_model::tools::{Tool, ToolLibrary};
use fluut_path::{Motion, Segment};
use fluut_topo::{BrepBody, WireBody, WireCurve};
use support::step::{self, RIDGE_RADIUS, ridge_axis_z};

const SHIFT: [f64; 3] = [0.123457, -0.987651, 0.31];

fn tools() -> Vec<Tool> {
    let src = "format = \"fluut.tools/1\"\nid = \"lib\"\n\
               [[tool]]\nid = \"b6\"\nshape = \"ball\"\ndiameter = 6\nflute_length = 25\n\
               [[tool.preset]]\nmaterial = \"MDF\"\nrpm = 18000\nfeed = 2000\nplunge = 500\nstepover = 1\n\
               [[tool]]\nid = \"f6\"\nshape = \"flat\"\ndiameter = 6\n\
               [[tool.preset]]\nmaterial = \"MDF\"\nrpm = 18000\nfeed = 2000\nplunge = 500\nstepover = 1\n\
               [[tool]]\nid = \"r6\"\nshape = \"bull\"\ndiameter = 6\ncorner_radius = 1\n\
               [[tool.preset]]\nmaterial = \"MDF\"\nrpm = 18000\nfeed = 2000\nplunge = 500\nstepover = 1\n";
    ToolLibrary::parse(src).unwrap().0.tools
}

fn tool(id: &str) -> Tool {
    tools().into_iter().find(|t| t.id == id).unwrap()
}

fn load_step(text: &str) -> SetupGeometry {
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

fn ridge() -> SetupGeometry {
    load_step(&step::ridge([0.0; 3]))
}

fn brep(g: &SetupGeometry) -> &BrepBody {
    match &g.bodies[0].geom {
        BodyGeom::Brep(b) => b,
        BodyGeom::Wire(_) => panic!("wire"),
    }
}

/// Id of the first face of body 0 whose surface matches.
fn face_where(g: &SetupGeometry, f: impl Fn(&Surface) -> bool) -> String {
    let b = brep(g);
    let i = b
        .faces
        .iter()
        .position(|x| f(&b.surfaces[x.surface as usize]))
        .unwrap();
    face_id(&g.bodies[0].id, i as u32)
}

fn cylinder_face(g: &SetupGeometry) -> String {
    face_where(g, |s| matches!(s, Surface::Cylinder { .. }))
}

fn bottom_face(g: &SetupGeometry) -> String {
    face_where(g, |s| match s {
        Surface::Plane { frame } => frame.origin.z().abs() < 1e-9 && frame.z.get().z().abs() > 0.5,
        _ => false,
    })
}

fn tp(tool: &str, groups: &str) -> Toolpath {
    let groups = groups.replace("\n[", "\n[setup.toolpath.");
    let groups = match groups.strip_prefix('[') {
        Some(g) => format!("[setup.toolpath.{g}"),
        None => groups,
    };
    let src = format!(
        "format = \"fluut.project/1\"\n[[setup]]\nid = \"s\"\n[[setup.toolpath]]\n\
         id = \"t\"\ntype = \"parallel\"\ntool = \"lib#{tool}\"\n{groups}"
    );
    read(&src)
        .unwrap_or_else(|e| panic!("{e}\n{src}"))
        .project
        .setups[0]
        .toolpaths[0]
        .clone()
}

fn env(g: &SetupGeometry) -> HeightEnv {
    let b = g.bounds(1e-3);
    HeightEnv {
        setup_clearance: b.max.z() + 20.0,
        stock_top: b.max.z() + 1.0,
        stock_bottom: b.min.z(),
        model_top: b.max.z(),
        model_bottom: b.min.z(),
        selection: None,
    }
}

fn input<'a>(g: &'a SetupGeometry, t: &'a Toolpath, tool: &'a Tool) -> StrategyInput<'a> {
    StrategyInput {
        toolpath: t,
        geometry: g,
        tool,
        material: Some("MDF"),
        setup_kind: SetupKind::ThreeAxis,
        env: env(g),
        stock: None,
        to_work: Affine::identity(),
        machine: None,
        post: None,
        in_process: None,
        rotary: None,
    }
}

fn run(g: &SetupGeometry, t: &Toolpath) -> Generated {
    let tl = tool(tool_of(t));
    calculate(&input(g, t, &tl)).unwrap_or_else(|e| panic!("{e}"))
}

fn tool_of(t: &Toolpath) -> &str {
    t.tool.as_str().rsplit('#').next().unwrap()
}

fn run_err(g: &SetupGeometry, t: &Toolpath) -> Vec<String> {
    let tl = tool(tool_of(t));
    calculate(&input(g, t, &tl))
        .unwrap_err()
        .diagnostics()
        .into_iter()
        .map(|d| d.code)
        .collect()
}

fn warnings(r: &Generated) -> Vec<&str> {
    r.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn with_motion(r: &Generated, m: Motion) -> Vec<&Segment> {
    r.result
        .segments()
        .filter(|s| s.motion().is_some_and(|x| x.motion == m))
        .collect()
}

/// Cut stretches: maximal chains of consecutive Cut segments.
fn runs(r: &Generated) -> Vec<Vec<Point3>> {
    let mut out: Vec<Vec<Point3>> = Vec::new();
    let mut open = false;
    for s in r.result.segments() {
        let cut = s.motion().is_some_and(|x| x.motion == Motion::Cut);
        if cut {
            let (a, b) = (s.start().unwrap().xyz, s.end().unwrap().xyz);
            if !open {
                out.push(vec![a]);
                open = true;
            }
            out.last_mut().unwrap().push(b);
        } else {
            open = false;
        }
    }
    out
}

fn reference<'a>(g: &'a SetupGeometry, tl: &Tool, stock: f64) -> SurfaceQuery<'a> {
    let c = Cutter::new(tl.diameter, tl.corner_radius()).unwrap();
    SurfaceQuery::new(g, &FaceSets::default(), c, stock).unwrap()
}

/// Dense drops along every segment: the tool is never below the surface by
/// more than `tol` (D08 §11 gouge check).
fn assert_no_gouge(g: &SetupGeometry, r: &Generated, tl: &Tool, stock: f64, tol: f64) {
    let q = reference(g, tl, stock);
    for s in r.result.segments() {
        let pts = fluut_path::linearize(s, 1e-3);
        for p in pts {
            let (a, b) = (p.start().unwrap().xyz, p.end().unwrap().xyz);
            for k in 0..=10 {
                let x = a.lerp(b, k as f64 / 10.0);
                if let Some(h) = q.drop(x.x(), x.y()) {
                    assert!(
                        x.z() >= h.tip - tol,
                        "gouge {:.6} at {x:?} ({:?})",
                        h.tip - x.z(),
                        s.motion()
                    );
                }
            }
        }
    }
}

// ── validation codes (D15 §5.2) ──────────────────────────────────────

fn codes_pass_fail(code: &str, g: &SetupGeometry, pass: &Toolpath, fail: &Toolpath) {
    let ok = run(g, pass);
    assert!(!warnings(&ok).contains(&code), "{code} on the passing case");
    let tl = tool(tool_of(fail));
    let got: Vec<String> = match calculate(&input(g, fail, &tl)) {
        Ok(r) => r.diagnostics.into_iter().map(|d| d.code).collect(),
        Err(e) => e.diagnostics().into_iter().map(|d| d.code).collect(),
    };
    assert!(
        got.iter().any(|c| c == code),
        "{code} not reported: {got:?}"
    );
}

#[test]
fn validation_codes() {
    let g = ridge();
    let p = |tool: &str, s: &str| tp(tool, &format!("[passes]\n{s}\n"));
    codes_pass_fail(
        "param.parallel.stepover_and_scallop",
        &g,
        &p("b6", "scallop_height = 0.05"),
        &p("b6", "scallop_height = 0.05\nstepover = 1"),
    );
    codes_pass_fail(
        "param.parallel.scallop.radius",
        &g,
        &p("r6", "scallop_height = 0.05"),
        &p("f6", "scallop_height = 0.05"),
    );
    codes_pass_fail(
        "param.parallel.scallop.range",
        &g,
        &p("r6", "scallop_height = 0.5"),
        &p("r6", "scallop_height = 1"),
    );
    codes_pass_fail(
        "param.passes.stepover.max",
        &g,
        &p("b6", "stepover = 6"),
        &p("b6", "stepover = 6.5"),
    );
    codes_pass_fail(
        "param.parallel.slope.order",
        &g,
        &p("b6", "stepover = 3\nmin_slope_deg = 10\nmax_slope_deg = 80"),
        &p("b6", "stepover = 3\nmin_slope_deg = 80\nmax_slope_deg = 10"),
    );
    codes_pass_fail(
        "param.parallel.slope.range",
        &g,
        &p("b6", "stepover = 3\nmax_slope_deg = 90"),
        &p("b6", "stepover = 3\nmax_slope_deg = 95"),
    );
    // A computed stepover is at most 2r ≤ D, so it is always accepted.
    let r = run(&g, &p("b6", "scallop_height = 2.9"));
    assert!(!warnings(&r).contains(&"param.passes.stepover.max"));
}

/// A tetrahedron of planar triangles, as a mesh import would give.
fn mesh() -> SetupGeometry {
    let v = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(20.0, 0.0, 0.0),
        Point3::new(0.0, 20.0, 0.0),
        Point3::new(5.0, 5.0, 8.0),
    ];
    let mut b = Builder::new("mesh");
    let mut e = std::collections::BTreeMap::new();
    for (i, j) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
        let (c, r) = line(v[i], v[j]).unwrap();
        e.insert((i, j), b.edge(Some(c), r, v[i]));
    }
    let edge = |i: usize, j: usize| {
        if i < j {
            (e[&(i, j)], true)
        } else {
            (e[&(j, i)], false)
        }
    };
    for [i, j, k] in [[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]] {
        let n = (v[j] - v[i]).cross(v[k] - v[j]);
        let f = Frame::new(v[i], n, v[j] - v[i]).unwrap();
        b.face(
            Surface::plane(f),
            true,
            &[vec![edge(i, j), edge(j, k), edge(k, i)]],
        );
    }
    SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-m".into(),
            geom: BodyGeom::Brep(b.build()),
        }],
    }
}

#[test]
fn mesh_scallop_warning() {
    let s = "[passes]\nscallop_height = 0.05\n";
    let r = run(&ridge(), &tp("b6", s));
    assert!(!warnings(&r).contains(&"param.parallel.drive_faces.mesh_scallop"));
    let r = run(&mesh(), &tp("b6", s));
    assert!(warnings(&r).contains(&"param.parallel.drive_faces.mesh_scallop"));
}

/// A bowl: the inside of a sphere of radius `r`, centre at height `r`.
fn bowl(r: f64) -> SetupGeometry {
    let f = Frame::new(
        Point3::new(0.0, 0.0, r),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, 0.0),
    )
    .unwrap();
    let mut b = Builder::new("bowl");
    let fi = b.patch(
        Surface::sphere(f, r).unwrap(),
        UvBox::new(
            Interval { lo: 0.0, hi: 6.0 },
            Interval { lo: -1.4, hi: -0.3 },
        ),
    );
    let mut body = b.build();
    // The material is outside the sphere: the face looks inwards.
    body.faces[fi as usize].same_sense = false;
    SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-w".into(),
            geom: BodyGeom::Brep(body),
        }],
    }
}

#[test]
fn concave_rest_warning() {
    let s = "[passes]\nstepover = 1\n";
    let r = run(&bowl(10.0), &tp("b6", s));
    assert!(!warnings(&r).contains(&"cam.parallel.scallop.concave"));
    let r = run(&bowl(2.5), &tp("b6", s));
    assert!(warnings(&r).contains(&"cam.parallel.scallop.concave"));
}

#[test]
fn tool_shape_and_missing_faces() {
    let g = ridge();
    let t = tp(
        "b6",
        "[geometry]\ndrive_faces = [{ face = \"f-nope.1\" }]\n[passes]\nstepover = 1\n",
    );
    assert!(run_err(&g, &t).contains(&"param.geometry.selection.missing".to_string()));
}

// ── layout and directions ────────────────────────────────────────────

/// Distinct pass offsets along n of the cut points, sorted.
fn offsets(r: &Generated, n: Vec3) -> Vec<f64> {
    let mut v: Vec<f64> = runs(r)
        .iter()
        .flatten()
        .map(|p| p.x() * n.x() + p.y() * n.y())
        .collect();
    v.sort_by(f64::total_cmp);
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    v
}

#[test]
fn passes_span_the_region_at_any_angle() {
    let g = ridge();
    for ang in [0.0f64, 30.0, 90.0, 135.0] {
        let t = tp(
            "b6",
            &format!("[passes]\nstepover = 2.5\npass_angle_deg = {ang}\n"),
        );
        let r = run(&g, &t);
        let (s, c) = libm::sincos(ang.to_radians());
        let n = Vec3::new(-s, c, 0.0);
        let off = offsets(&r, n);
        assert!(off.len() > 10, "{ang}: {}", off.len());
        for w in off.windows(2) {
            assert!(w[1] - w[0] <= 2.5 + 1e-9, "{ang}: gap {}", w[1] - w[0]);
        }
        // The extreme passes are at the region's limits.
        let corners = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)];
        let ext: Vec<f64> = corners
            .iter()
            .map(|&(x, y)| x * n.x() + y * n.y())
            .collect();
        let (lo, hi) = (
            ext.iter().copied().fold(f64::INFINITY, f64::min),
            ext.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        // A pass through a corner alone touches one point and is empty.
        let tight = ang % 90.0 == 0.0;
        let slack = if tight { 1e-9 } else { 2.5 };
        assert!(
            off[0] - lo >= -1e-9 && off[0] - lo <= slack,
            "{ang}: {} {lo}",
            off[0]
        );
        let last = off[off.len() - 1];
        assert!(
            hi - last >= -1e-9 && hi - last <= slack,
            "{ang}: {last} {hi}"
        );
        // Every run lies along the pass direction.
        for run in runs(&r) {
            let d = run[run.len() - 1] - run[0];
            let along = d.x() * c + d.y() * s;
            assert!((d.x() * n.x() + d.y() * n.y()).abs() < 1e-9 && along.abs() > 0.0);
        }
    }
}

/// Travel direction (+1 along u, −1 against) of each pass in order.
fn travel(r: &Generated) -> Vec<f64> {
    runs(r)
        .iter()
        .map(|run| (run[run.len() - 1].x() - run[0].x()).signum())
        .collect()
}

#[test]
fn zigzag_climb_and_conventional() {
    let g = ridge();
    let t = |dir: &str| {
        tp(
            "b6",
            &format!(
                "[passes]\nstepover = 5\ndirection = \"{dir}\"\n[linking]\nretract = \"full\"\n"
            ),
        )
    };
    let z = travel(&run(&g, &t("zigzag")));
    assert!(z.len() >= 9);
    assert!(z.windows(2).all(|w| w[0] == -w[1]), "{z:?}");
    assert_eq!(z[0], 1.0);
    // Passes advance in +Y. Climb (spindle M3) keeps the next pass on the
    // right of travel: along −X.
    let c = travel(&run(&g, &t("climb")));
    assert!(c.iter().all(|&d| d == -1.0), "{c:?}");
    let v = travel(&run(&g, &t("conventional")));
    assert!(v.iter().all(|&d| d == 1.0), "{v:?}");
    for rr in [run(&g, &t("climb")), run(&g, &t("zigzag"))] {
        let ys: Vec<f64> = runs(&rr).iter().map(|r| r[0].y()).collect();
        assert!(ys.windows(2).all(|w| w[1] > w[0]), "{ys:?}");
    }
}

// ── slope limits ─────────────────────────────────────────────────────

#[test]
fn slope_limits_cut_off_exactly() {
    // Passes along Y climb the cylinder. A ball of radius 3 on a cylinder
    // of radius 40 has slope asin(|y − 20| / 43) at the contact.
    let g = ridge();
    let rr = RIDGE_RADIUS + 3.0;
    let t = tp(
        "b6",
        "[passes]\nstepover = 5\npass_angle_deg = 90\nmax_slope_deg = 20\n",
    );
    let r = run(&g, &t);
    let lim = rr * libm::sin(20f64.to_radians());
    let rs = runs(&r);
    assert!(!rs.is_empty());
    for run in &rs {
        for end in [run[0], run[run.len() - 1]] {
            assert!(
                ((end.y() - 20.0).abs() - lim).abs() < 1e-6,
                "end at y {} (want |y − 20| = {lim})",
                end.y()
            );
        }
    }
    // A minimum slope leaves the crest uncut: two runs per pass.
    let t = tp(
        "b6",
        "[passes]\nstepover = 5\npass_angle_deg = 90\nmin_slope_deg = 10\nmax_slope_deg = 20\n",
    );
    let r = run(&g, &t);
    let lo = rr * libm::sin(10f64.to_radians());
    let rs = runs(&r);
    assert!(rs.len() >= 2 * 13, "{}", rs.len());
    for run in &rs {
        for p in run {
            let d = (p.y() - 20.0).abs();
            assert!(d >= lo - 1e-6 && d <= lim + 1e-6, "{p:?}");
        }
        let ends: Vec<f64> = [run[0], run[run.len() - 1]]
            .iter()
            .map(|p| (p.y() - 20.0).abs())
            .collect();
        assert!(ends.iter().any(|d| (d - lo).abs() < 1e-6));
        assert!(ends.iter().any(|d| (d - lim).abs() < 1e-6));
    }
}

// ── boundary, stock to leave, heights ────────────────────────────────

fn x_extent(r: &Generated) -> (f64, f64) {
    let pts: Vec<Point3> = runs(r).into_iter().flatten().collect();
    (
        pts.iter().map(|p| p.x()).fold(f64::INFINITY, f64::min),
        pts.iter().map(|p| p.x()).fold(f64::NEG_INFINITY, f64::max),
    )
}

#[test]
fn boundary_with_tool_containment() {
    let g = ridge();
    let b = bottom_face(&g);
    let t = |c: &str| {
        tp(
            "b6",
            &format!(
                "[geometry]\nboundary = {{ curves = [{{ face = \"{b}\" }}], tool_containment = \"{c}\" }}\n\
                 [passes]\nstepover = 5\n"
            ),
        )
    };
    let (lo, hi) = x_extent(&run(&g, &t("inside")));
    assert!(
        (lo - 3.0).abs() < 1e-9 && (hi - 57.0).abs() < 1e-9,
        "{lo} {hi}"
    );
    let (lo, hi) = x_extent(&run(&g, &t("centre")));
    assert!(lo.abs() < 1e-9 && (hi - 60.0).abs() < 1e-9, "{lo} {hi}");
    let (lo2, hi2) = x_extent(&run(&g, &t("outside")));
    assert!(lo2 <= lo && hi2 >= hi);
    // Outside a 20 × 10 rectangle (centre): the passes through it break.
    let mut g2 = ridge();
    g2.bodies.push(rect_wire(20.0, 15.0, 40.0, 25.0));
    let t = tp(
        "b6",
        "[geometry]\ndrive_faces = [{ body = \"%\" }]\n\
         boundary = { curves = [{ body = \"b-w\" }], side = \"outside\", tool_containment = \"centre\" }\n\
         [passes]\nstepover = 2.5\n"
            .replace('%', &g.bodies[0].id)
            .as_str(),
    );
    let r = run(&g2, &t);
    for p in runs(&r).iter().flatten() {
        let strictly_in = p.x() > 20.0 + 1e-9
            && p.x() < 40.0 - 1e-9
            && p.y() > 15.0 + 1e-9
            && p.y() < 25.0 - 1e-9;
        assert!(!strictly_in, "{p:?}");
    }
    let broken = runs(&r)
        .iter()
        .filter(|run| (run[0].x() - 20.0).abs() < 1e-9 || (run[0].x() - 40.0).abs() < 1e-9)
        .count();
    assert!(broken >= 3, "{broken}");
}

fn rect_wire(x0: f64, y0: f64, x1: f64, y1: f64) -> SetupBody {
    let pts = [
        Point3::new(x0, y0, 0.0),
        Point3::new(x1, y0, 0.0),
        Point3::new(x1, y1, 0.0),
        Point3::new(x0, y1, 0.0),
    ];
    let curves = (0..4)
        .map(|i| {
            let (c, range) = line(pts[i], pts[(i + 1) % 4]).unwrap();
            WireCurve { curve: c, range }
        })
        .collect();
    SetupBody {
        id: "b-w".into(),
        geom: BodyGeom::Wire(WireBody {
            name: "w".into(),
            tol_model: 1e-6,
            curves,
        }),
    }
}

#[test]
fn stock_to_leave_is_a_normal_offset() {
    // On the crest (y = 20) the surface is level: the CL is s above it.
    let g = ridge();
    let crest = ridge_axis_z() + RIDGE_RADIUS;
    for s in [0.0, 0.5, -1.0] {
        let t = tp(
            "b6",
            &format!(
                "[passes]\nstepover = 5\nstock_to_leave = {s}\n[heights]\ntop = {{ ref = \"model_top\", offset = 2 }}\n"
            ),
        );
        let r = run(&g, &t);
        let on_crest: Vec<Point3> = runs(&r)
            .into_iter()
            .flatten()
            .filter(|p| (p.y() - 20.0).abs() < 1e-9)
            .collect();
        assert!(!on_crest.is_empty());
        for p in on_crest {
            assert!((p.z() - (crest + s)).abs() < 1e-7, "s {s}: {p:?}");
        }
        let tl = tool("b6");
        assert_no_gouge(&g, &r, &tl, s, 0.01 + 1e-9);
    }
    // Negative stock: the cut goes past the model (a warning).
    let r = run(
        &g,
        &tp("b6", "[passes]\nstepover = 5\nstock_to_leave = -1\n"),
    );
    assert!(warnings(&r).contains(&"param.passes.stock_to_leave.negative"));
    // At or below −r it is an error.
    let e = run_err(
        &g,
        &tp("r6", "[passes]\nstepover = 5\nstock_to_leave = -1\n"),
    );
    assert!(e.contains(&"param.passes.stock_to_leave.range".to_string()));
}

#[test]
fn heights_clamp_and_skip() {
    let g = ridge();
    let crest = ridge_axis_z() + RIDGE_RADIUS;
    // Bottom raised to 12: lower CL points sit at 12.
    let t = tp(
        "b6",
        "[passes]\nstepover = 2.5\n[heights]\nbottom = { ref = \"model_bottom\", offset = 12 }\n",
    );
    let r = run(&g, &t);
    let pts: Vec<Point3> = runs(&r).into_iter().flatten().collect();
    assert!(pts.iter().all(|p| p.z() >= 12.0));
    assert!(pts.iter().any(|p| p.z() == 12.0));
    // Top 2 below the crest: nothing above it is cut, and the passes near
    // the crest are skipped.
    let t = tp(
        "b6",
        "[passes]\nstepover = 2.5\npass_angle_deg = 90\n[heights]\ntop = { ref = \"model_top\", offset = -2 }\n",
    );
    let r = run(&g, &t);
    let top = crest - 2.0;
    let rs = runs(&r);
    for p in rs.iter().flatten() {
        assert!(p.z() <= top + 1e-6, "{p:?}");
    }
    // Each pass is split at the crest and linked over it.
    assert!(rs.len() >= 2 * 20, "{}", rs.len());
    let tl = tool("b6");
    assert_no_gouge(&g, &r, &tl, 0.0, 0.01 + 1e-9);
}

// ── links and leads ──────────────────────────────────────────────────

#[test]
fn links_follow_the_policy() {
    let g = ridge();
    let t = |retract: &str| {
        tp(
            "b6",
            &format!("[passes]\nstepover = 2.5\n[linking]\nretract = \"{retract}\"\n"),
        )
    };
    let tl = tool("b6");
    let full = run(&g, &t("full"));
    assert!(with_motion(&full, Motion::Link).is_empty());
    assert_no_gouge(&g, &full, &tl, 0.0, 0.01 + 1e-9);
    // Full retract links rise to the retract height (stock top + 5).
    let retract = env(&g).stock_top + 5.0;
    let tops: Vec<f64> = with_motion(&full, Motion::Retract)
        .iter()
        .map(|s| s.end().unwrap().xyz.z())
        .collect();
    assert!(tops.iter().any(|&z| (z - retract).abs() < 1e-9), "{tops:?}");
    // Stay-down (and minimum, for these short steps): the zig-zag ends are
    // joined along the surface, so there is one entry and one retract.
    for policy in ["stay_down", "minimum"] {
        let r = run(&g, &t(policy));
        let links = with_motion(&r, Motion::Link);
        assert!(!links.is_empty(), "{policy}");
        assert_eq!(with_motion(&r, Motion::Plunge).len(), 1, "{policy}");
        assert_no_gouge(&g, &r, &tl, 0.0, 0.01 + 1e-9);
    }
    // Minimum retract over the uncut crest stays low: safety distance above
    // the geometry bound, below the retract height.
    let t = tp(
        "b6",
        "[passes]\nstepover = 5\npass_angle_deg = 90\nmin_slope_deg = 10\n[linking]\nretract = \"minimum\"\n",
    );
    let r = run(&g, &t);
    let crest = ridge_axis_z() + RIDGE_RADIUS;
    let ups: Vec<f64> = with_motion(&r, Motion::Retract)
        .iter()
        .map(|s| s.end().unwrap().xyz.z())
        .filter(|&z| z < env(&g).setup_clearance - 1e-9)
        .collect();
    assert!(!ups.is_empty());
    assert!(
        ups.iter()
            .all(|&z| z >= crest + 1.0 - 1e-9 && z <= retract + 1e-9),
        "{ups:?}"
    );
    assert_no_gouge(&g, &r, &tl, 0.0, 0.01 + 1e-9);
}

#[test]
fn vertical_leads() {
    let g = ridge();
    let t = tp(
        "b6",
        "[passes]\nstepover = 5\n[linking]\nretract = \"full\"\nlead_in = { kind = \"arc\", radius = 2, length = 1 }\n",
    );
    let r = run(&g, &t);
    assert!(!warnings(&r).contains(&"cam.parallel.lead.dropped"));
    let leads: Vec<&Segment> = r
        .result
        .segments()
        .filter(|s| {
            s.motion()
                .is_some_and(|m| matches!(m.motion, Motion::LeadIn | Motion::LeadOut))
        })
        .collect();
    let arcs: Vec<&&Segment> = leads
        .iter()
        .filter(|s| matches!(s, Segment::Arc { .. }))
        .collect();
    assert!(arcs.len() >= 2 * 9, "{}", arcs.len());
    for a in arcs {
        let Segment::Arc { normal, .. } = a else {
            unreachable!()
        };
        assert!(normal.get().z().abs() < 1e-12);
    }
    let tl = tool("b6");
    assert_no_gouge(&g, &r, &tl, 0.0, 0.01 + 1e-9);
    // In a bowl, a run that starts at the slope limit heads down into the
    // middle; a straight lead along its start tangent would cut into the
    // steeper wall behind it, so it is left out.
    let g = bowl(10.0);
    let t = tp(
        "b6",
        "[passes]\nstepover = 2\nmax_slope_deg = 30\n\
         [linking]\nretract = \"full\"\nlead_in = { kind = \"linear\", length = 5 }\n",
    );
    let r = run(&g, &t);
    assert!(warnings(&r).contains(&"cam.parallel.lead.dropped"));
    assert_no_gouge(&g, &r, &tl, 0.0, 0.01 + 1e-9);
}

// ── scallop height ───────────────────────────────────────────────────

#[test]
fn scallop_height_is_met_on_a_ball_tool() {
    // Passes along X lie at constant Y on the cylinder, so the cross
    // section is a circle problem: neighbouring ball centres (radius 3)
    // meet at the cusp; its height above the surface must not exceed h.
    let g = ridge();
    let zc = ridge_axis_z();
    for h in [0.01, 0.05] {
        let t = tp(
            "b6",
            &format!("[passes]\nscallop_height = {h}\nmax_slope_deg = 20\n"),
        );
        let r = run(&g, &t);
        // The CL of each pass (straight along X).
        let mut cl: Vec<(f64, f64)> = runs(&r)
            .iter()
            .map(|run| {
                assert!(run.iter().all(|p| (p.z() - run[0].z()).abs() < 1e-9));
                (run[0].y(), run[0].z())
            })
            .collect();
        cl.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert!(cl.len() > 10, "{}", cl.len());
        let mut worst: f64 = 0.0;
        for w in cl.windows(2) {
            let (c0, c1) = (
                Point2::new(w[0].0, w[0].1 + 3.0),
                Point2::new(w[1].0, w[1].1 + 3.0),
            );
            let d = (c1 - c0).length();
            assert!(d < 6.0);
            // Lower intersection of the two circles of radius 3.
            let m = c0 + (c1 - c0) * 0.5;
            let k = (9.0 - d * d / 4.0).sqrt();
            let perp = Point2::new(c1.y() - c0.y(), c0.x() - c1.x());
            let perp = (perp - Point2::new(0.0, 0.0)) * (1.0 / d);
            let cusp = if perp.y() < 0.0 {
                m + perp * k
            } else {
                m + perp * -k
            };
            let height = libm::hypot(cusp.x() - 20.0, cusp.y() - zc) - RIDGE_RADIUS;
            worst = worst.max(height);
        }
        assert!(worst <= h + 1e-9, "h {h}: cusp {worst}");
        assert!(worst >= 0.5 * h, "h {h}: cusp {worst} (stepover too small)");
    }
}

// ── tools, gouges, translation ───────────────────────────────────────

#[test]
fn no_gouges_with_any_tool() {
    let g = ridge();
    for id in ["b6", "f6", "r6"] {
        let t = tp(id, "[passes]\nstepover = 2\npass_angle_deg = 30\n");
        let r = run(&g, &t);
        assert_no_gouge(&g, &r, &tool(id), 0.0, 0.01 + 1e-9);
    }
}

#[test]
fn translation_invariance() {
    // Spec 04 §4.12.7 (3): moving the part moves every coordinate by the
    // same offset, within ε_s.
    let a = ridge();
    let b = load_step(&step::ridge(SHIFT));
    let t = tp(
        "b6",
        "[passes]\nscallop_height = 0.05\npass_angle_deg = 45\nmax_slope_deg = 25\n",
    );
    let (ra, rb) = (run(&a, &t), run(&b, &t));
    let (xa, xb): (Vec<&Segment>, Vec<&Segment>) = (
        ra.result.segments().collect(),
        rb.result.segments().collect(),
    );
    assert_eq!(xa.len(), xb.len());
    let d = Vec3::new(SHIFT[0], SHIFT[1], SHIFT[2]);
    let mut worst: f64 = 0.0;
    for (p, q) in xa.iter().zip(&xb) {
        for (u, v) in [(p.start(), q.start()), (p.end(), q.end())] {
            if let (Some(u), Some(v)) = (u, v) {
                worst = worst.max((v.xyz - (u.xyz + d)).length());
            }
        }
    }
    assert!(worst < 1e-6, "{worst}");
}

#[test]
fn drive_faces_limit_what_is_cut() {
    // Only the cylinder drives; the side walls are check faces, so the
    // tool never cuts them but the cylinder is cut right to its edges.
    let g = ridge();
    let cyl = cylinder_face(&g);
    let t = tp(
        "f6",
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{cyl}\" }}]\n[passes]\nstepover = 2\npass_angle_deg = 90\n"
        ),
    );
    let r = run(&g, &t);
    assert!(!runs(&r).is_empty());
    assert_no_gouge(&g, &r, &tool("f6"), 0.0, 0.01 + 1e-9);
}
