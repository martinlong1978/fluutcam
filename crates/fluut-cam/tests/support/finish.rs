// SPDX-License-Identifier: MIT OR Apache-2.0

//! Helpers for the 3-D finishing type tests (D15b §5): tools, parts,
//! toolpaths, runs, and the gouge, code and translation checks.

#![allow(dead_code)]

use super::step;
use fluut_cam::cutter::Cutter;
use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry, face_id};
use fluut_cam::surface::{FaceSets, SurfaceQuery};
use fluut_cam::{Generated, StrategyInput, calculate};
use fluut_geom::{Affine, Point3, Surface, Vec3};
use fluut_model::project::{SetupKind, Toolpath, read};
use fluut_model::resolve::HeightEnv;
use fluut_model::tools::{Tool, ToolLibrary};
use fluut_path::{Motion, Segment};
use fluut_topo::BrepBody;

pub const SHIFT: [f64; 3] = [0.123457, -0.987651, 0.31];

pub fn tools() -> Vec<Tool> {
    let preset = "[[tool.preset]]\nmaterial = \"MDF\"\nrpm = 18000\nfeed = 2000\nplunge = 500\n\
                  stepover = 1.5\n";
    let src = format!(
        "format = \"fluut.tools/1\"\nid = \"lib\"\n\
         [[tool]]\nid = \"b6\"\nshape = \"ball\"\ndiameter = 6\nflute_length = 25\n{preset}\
         [[tool]]\nid = \"f6\"\nshape = \"flat\"\ndiameter = 6\n{preset}\
         [[tool]]\nid = \"r6\"\nshape = \"bull\"\ndiameter = 6\ncorner_radius = 1\n{preset}\
         [[tool]]\nid = \"v6\"\nshape = \"v\"\ndiameter = 6\n{preset}"
    );
    ToolLibrary::parse(&src).unwrap().0.tools
}

pub fn tool(id: &str) -> Tool {
    tools().into_iter().find(|t| t.id == id).unwrap()
}

pub fn load_step(text: &str) -> SetupGeometry {
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

pub fn dome() -> SetupGeometry {
    load_step(&step::dome([0.0; 3]))
}

pub fn brep(g: &SetupGeometry) -> &BrepBody {
    match &g.bodies[0].geom {
        BodyGeom::Brep(b) => b,
        BodyGeom::Wire(_) => panic!("wire"),
    }
}

/// Ids of the faces of body 0 whose surface matches.
pub fn faces_where(g: &SetupGeometry, f: impl Fn(&Surface) -> bool) -> Vec<String> {
    let b = brep(g);
    (0..b.faces.len())
        .filter(|&i| f(&b.surfaces[b.faces[i].surface as usize]))
        .map(|i| face_id(&g.bodies[0].id, i as u32))
        .collect()
}

/// A toolpath of `kind` with the tool `lib#tool` and the given groups.
pub fn tp(kind: &str, tool: &str, groups: &str) -> Toolpath {
    let groups = groups.replace("\n[", "\n[setup.toolpath.");
    let groups = match groups.strip_prefix('[') {
        Some(g) => format!("[setup.toolpath.{g}"),
        None => groups,
    };
    let src = format!(
        "format = \"fluut.project/1\"\n[[setup]]\nid = \"s\"\n[[setup.toolpath]]\n\
         id = \"t\"\ntype = \"{kind}\"\ntool = \"lib#{tool}\"\n{groups}"
    );
    read(&src)
        .unwrap_or_else(|e| panic!("{e}\n{src}"))
        .project
        .setups[0]
        .toolpaths[0]
        .clone()
}

pub fn env(g: &SetupGeometry) -> HeightEnv {
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

pub fn tool_of(t: &Toolpath) -> &str {
    t.tool.as_str().rsplit('#').next().unwrap()
}

pub fn input<'a>(g: &'a SetupGeometry, t: &'a Toolpath, tool: &'a Tool) -> StrategyInput<'a> {
    StrategyInput {
        toolpath: t,
        geometry: g,
        tool,
        material: Some("MDF"),
        setup_kind: SetupKind::ThreeAxis,
        env: env(g),
        to_work: Affine::identity(),
        machine: None,
        post: None,
    }
}

pub fn run(g: &SetupGeometry, t: &Toolpath) -> Generated {
    let tl = tool(tool_of(t));
    calculate(&input(g, t, &tl)).unwrap_or_else(|e| panic!("{e}"))
}

/// Every code reported, as a warning or an error.
pub fn codes(g: &SetupGeometry, t: &Toolpath) -> Vec<String> {
    let tl = tool(tool_of(t));
    match calculate(&input(g, t, &tl)) {
        Ok(r) => r.diagnostics.into_iter().map(|d| d.code).collect(),
        Err(e) => e.diagnostics().into_iter().map(|d| d.code).collect(),
    }
}

/// `code` is absent for `pass` and present for `fail`.
pub fn codes_pass_fail(code: &str, g: &SetupGeometry, pass: &Toolpath, fail: &Toolpath) {
    let p = codes(g, pass);
    assert!(!p.iter().any(|c| c == code), "{code} on the passing case");
    let f = codes(g, fail);
    assert!(f.iter().any(|c| c == code), "{code} not reported: {f:?}");
}

pub fn warnings(r: &Generated) -> Vec<&str> {
    r.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

pub fn with_motion(r: &Generated, m: Motion) -> Vec<&Segment> {
    r.result
        .segments()
        .filter(|s| s.motion().is_some_and(|x| x.motion == m))
        .collect()
}

/// Cut stretches: maximal chains of consecutive Cut segments.
pub fn runs(r: &Generated) -> Vec<Vec<Point3>> {
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

/// Every end point of every Cut segment.
pub fn cut_points(r: &Generated) -> Vec<Point3> {
    runs(r).into_iter().flatten().collect()
}

pub fn reference<'a>(g: &'a SetupGeometry, tl: &Tool, stock: f64) -> SurfaceQuery<'a> {
    let c = Cutter::new(tl.diameter, tl.corner_radius()).unwrap();
    SurfaceQuery::new(g, &FaceSets::default(), c, stock).unwrap()
}

/// Dense drops along every segment: the tool is never below the surface by
/// more than `tol` (D08 §11 gouge check). On a horizontal move (a
/// waterline) the error of a chord is sideways, so a point may instead be
/// within `tol` sideways of where the tool clears.
pub fn assert_no_gouge(g: &SetupGeometry, r: &Generated, tl: &Tool, stock: f64, tol: f64) {
    let q = reference(g, tl, stock);
    for s in r.result.segments() {
        for p in fluut_path::linearize(s, 1e-3) {
            let (a, b) = (p.start().unwrap().xyz, p.end().unwrap().xyz);
            let level = (a.z() - b.z()).abs() < 1e-12;
            for k in 0..=10 {
                let x = a.lerp(b, k as f64 / 10.0);
                if let Some(h) = q.drop(x.x(), x.y()) {
                    let sideways = level
                        && q.probe(x.x(), x.y()).is_some_and(|pr| {
                            let y = x - Vec3::new(pr.toward.x(), pr.toward.y(), 0.0) * tol;
                            q.drop(y.x(), y.y()).is_none_or(|h| h.tip <= x.z() + 1e-6)
                        });
                    assert!(
                        x.z() >= h.tip - tol || sideways,
                        "gouge {:.6} at {x:?} ({:?})",
                        h.tip - x.z(),
                        s.motion()
                    );
                }
            }
        }
    }
}

/// Moving the part by [`SHIFT`] moves every coordinate by the same offset,
/// within 1e-6 mm, with the same number of segments (spec 04 §4.12.7
/// (3)).
pub fn assert_translation_invariant(part: fn([f64; 3]) -> String, t: &Toolpath) {
    let a = load_step(&part([0.0; 3]));
    let b = load_step(&part(SHIFT));
    let (ra, rb) = (run(&a, t), run(&b, t));
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
