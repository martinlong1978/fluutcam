// SPDX-License-Identifier: MIT OR Apache-2.0

//! The output of the surface-following finishing types against fresh
//! drops (adaptive sampling, fu-8; crease checks, fu-10): every cut point
//! is a real drop, never an interpolated height, and every cut chord is
//! within [`BOUND`] of the machining tolerance of fresh drops at its
//! quarter, middle and three-quarter points (along the CL surface's
//! normal, or, into the part, by how deep the tool cuts), on a free-form,
//! a rough faceted and analytic parts, with ball, flat and bull tools. The
//! strategy tests' dense gouge check runs on the same results.

mod support;

use fluut_cam::Generated;
use fluut_cam::cutter::Cutter;
use fluut_cam::cutter::synth::{sculpted, tiled};
use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry};
use fluut_cam::surface::{FaceSets, SurfaceQuery};
use fluut_geom::{Point3, Vec3};
use fluut_model::project::Toolpath;
use fluut_path::Motion;
use support::finish::*;
use support::step;

const TOL: f64 = 0.01;

/// The share of the tolerance a chord may leave the CL by: `tol/4` from
/// the merge (every drop within `tol/4` of the output chord) and `tol/2`
/// from the sampler's crease check (the CL within `tol/2` of a sampler
/// chord between its ends, by their gradients).
const BOUND: f64 = 0.75;

fn body(b: fluut_topo::BrepBody) -> SetupGeometry {
    SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-s".into(),
            geom: BodyGeom::Brep(b),
        }],
    }
}

fn freeform() -> SetupGeometry {
    body(sculpted(4, 4.0, 5))
}

fn types() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "parallel",
            "[passes]\nstepover = 1\nmachining_tolerance = 0.01\n",
        ),
        (
            "parallel",
            "[passes]\nstepover = 1.5\npass_angle_deg = 30\nmachining_tolerance = 0.01\n",
        ),
        (
            "spiral",
            "[passes]\nstepover = 1\nmachining_tolerance = 0.01\n",
        ),
        (
            "scallop",
            "[passes]\nstepover = 1\nmachining_tolerance = 0.01\n",
        ),
    ]
}

/// The CL tip at `p` as the sampler takes it (the drop, raised to
/// `bottom`), its slope and the length of its gradient.
fn tip(q: &SurfaceQuery, p: Point3, bottom: f64) -> (f64, f64, f64) {
    match q.drop(p.x(), p.y()) {
        Some(h) if h.tip >= bottom => (h.tip, h.slope, h.grad.length()),
        _ => (bottom, 0.0, 0.0),
    }
}

/// Whether the tool with its tip at `x` cuts less than `s` into the part:
/// the tool shrunk by `s` all round (its tip `s` higher) touches nothing,
/// so whatever the tool overlaps is within `s` of its surface.
fn shallower(g: &SetupGeometry, t: &Toolpath, x: Point3, s: f64) -> bool {
    let tl = tool(tool_of(t));
    let c = Cutter::new(tl.diameter - 2.0 * s, (tl.corner_radius() - s).max(0.0)).unwrap();
    let q = SurfaceQuery::new(g, &FaceSets::default(), c, 0.0).unwrap();
    q.drop(x.x(), x.y()).is_none_or(|h| h.tip <= x.z() + s)
}

/// Which cut points need not be drops.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Points {
    /// Every one is a drop.
    Drops,
    /// Level chords are a waterline's (contour points, not drops) and are
    /// left out.
    Waterline,
    /// A `scallop` loop starts at its point nearest the end of the last
    /// loop and a `continuous` one ends one step early: those points are
    /// put on an accepted chord, not dropped (as before adaptive sampling);
    /// they must be within `tol` of the drop along the contact normal.
    LoopEnds,
}

/// Every end of a cut segment is a real drop (see [`Points`]); every cut
/// chord at least `tol` long in XY is within `BOUND·tol` of fresh drops at
/// 1/4, 1/2 and 3/4 of it, along the normal of the CL surface (`(τ − z) /
/// √(1 + |∇τ|²)`; the tool's normal at the contact, which the gouge check
/// uses, is not the CL's where a flat tool's rim touches an edge), or,
/// into the part, the tool there cuts less than `BOUND·tol` deep; except
/// where a level chord runs along a wall (the error is sideways there, as
/// in [`assert_no_gouge`]). Returns the worst deviation, the number of
/// chords checked and the number of cut points that are not drops.
fn check(
    g: &SetupGeometry,
    r: &Generated,
    t: &Toolpath,
    tol: f64,
    points: Points,
    what: &str,
) -> (f64, usize, usize) {
    let tl = tool(tool_of(t));
    let q = reference(g, &tl, 0.0);
    let bottom = env(g).model_bottom;
    let mut worst: f64 = 0.0;
    let mut chords = 0;
    let mut placed = 0;
    for s in r.result.segments() {
        if s.motion().is_none_or(|m| m.motion != Motion::Cut) {
            continue;
        }
        let (a, b) = (s.start().unwrap().xyz, s.end().unwrap().xyz);
        let level = (a.z() - b.z()).abs() < 1e-12;
        if points == Points::Waterline && level {
            continue;
        }
        for p in [a, b] {
            let (z, slope, _) = tip(&q, p, bottom);
            let off = (z - p.z()).abs();
            if off > 1e-9 {
                placed += 1;
                assert!(
                    points == Points::LoopEnds && off * libm::cos(slope) <= tol,
                    "{what}: cut point {p:?} is not a drop ({z})"
                );
            }
        }
        if libm::hypot(a.x() - b.x(), a.y() - b.y()) < tol {
            continue;
        }
        chords += 1;
        for f in [0.25, 0.5, 0.75] {
            let x = a.lerp(b, f);
            let sideways = level
                && q.probe(x.x(), x.y()).is_some_and(|pr| {
                    let y = x - Vec3::new(pr.toward.x(), pr.toward.y(), 0.0) * tol;
                    q.drop(y.x(), y.y()).is_none_or(|h| h.tip <= x.z() + 1e-6)
                });
            if sideways {
                continue;
            }
            let (z, _, grad) = tip(&q, x, bottom);
            let dev = (z - x.z()) / libm::sqrt(1.0 + grad * grad);
            let lim = BOUND * tol;
            assert!(
                dev.abs() <= lim || dev > 0.0 && shallower(g, t, x, lim),
                "{what}: chord {a:?}..{b:?} is {dev:.6} off the drop at {f}"
            );
            worst = worst.max(dev.abs().min(lim));
        }
    }
    (worst, chords, placed)
}

/// The strategy tests' dense check, with the tool's cut depth in place of
/// its sideways tests: at 11 points of every segment the tool is within
/// `tol` of the drop along the contact normal, or cuts less than `tol`
/// deep ([`shallower`]). For the rough tiles, whose flat-tool walls (steps
/// a few µm wide in XY) the sideways tests do not recognise.
fn assert_shallow(g: &SetupGeometry, r: &Generated, t: &Toolpath, tol: f64) {
    let q = reference(g, &tool(tool_of(t)), 0.0);
    for s in r.result.segments() {
        for p in fluut_path::linearize(s, 1e-3) {
            let (a, b) = (p.start().unwrap().xyz, p.end().unwrap().xyz);
            for k in 0..=10 {
                let x = a.lerp(b, k as f64 / 10.0);
                if let Some(h) = q.drop(x.x(), x.y()) {
                    let depth = (h.tip - x.z()) * libm::cos(h.slope);
                    assert!(
                        depth <= tol || shallower(g, t, x, tol),
                        "gouge {depth:.6} at {x:?} on {a:?}..{b:?} ({:?})",
                        s.motion()
                    );
                }
            }
        }
    }
}

fn all_tools(g: &SetupGeometry, name: &str) {
    all_tools_with(g, name, false);
}

fn all_tools_with(g: &SetupGeometry, name: &str, depth: bool) {
    for (kind, groups) in types() {
        for tl in ["b6", "f6", "r6"] {
            let t = tp(kind, tl, groups);
            let r = run(g, &t);
            let what = format!("{name} {kind} {tl} {groups:?}");
            let pts = if kind == "scallop" {
                Points::LoopEnds
            } else {
                Points::Drops
            };
            let (worst, chords, _) = check(g, &r, &t, TOL, pts, &what);
            assert!(chords > 20, "{what}: {chords} chords");
            assert!(worst <= BOUND * TOL, "{what}: {worst}");
            if depth {
                assert_shallow(g, &r, &t, TOL);
            } else {
                assert_no_gouge(g, &r, &tool(tl), 0.0, TOL);
            }
        }
    }
}

#[test]
fn freeform_chords_hold_the_tolerance() {
    all_tools(&freeform(), "sculpted");
}

/// The rough tiles of the drop benchmarks (B-spline bumps at random
/// heights, open edges all round): creases of flat and bull CLs everywhere.
#[test]
fn tiles_chords_hold_the_tolerance() {
    all_tools_with(&body(tiled(2, true)), "tiles", true);
}

#[test]
fn dome_chords_hold_the_tolerance() {
    all_tools(&dome(), "dome");
}

#[test]
fn ridge_chords_hold_the_tolerance() {
    all_tools(&load_step(&step::ridge([0.0; 3])), "ridge");
}

/// The toolpaths of the golden G-code projects (`fluut-cli` tests,
/// `dome` and `ridge`) whose output changed with adaptive sampling, on the
/// whole part (the projects pick faces by id).
#[test]
fn golden_project_settings_hold_the_tolerance() {
    let dome = dome();
    let ridge = load_step(&step::ridge([0.0; 3]));
    let tol = 0.02;
    let cases: [(&SetupGeometry, &str, &str, Points); 4] = [
        (
            &dome,
            "spiral",
            "[passes]
machining_tolerance = 0.02
stepover = 1.5
direction = \"outside_in\"
",
            Points::Drops,
        ),
        (
            &dome,
            "scallop",
            "[passes]
machining_tolerance = 0.02
stepover = 2
continuous = true
",
            Points::LoopEnds,
        ),
        (
            &dome,
            "steep_and_shallow",
            "[passes]
machining_tolerance = 0.02
threshold_deg = 40
overlap = 1
steep = { step_down = 2 }
shallow = { stepover = 3, pass_angle_deg = 45 }
",
            Points::Waterline,
        ),
        (
            &ridge,
            "parallel",
            "[passes]
machining_tolerance = 0.02
scallop_height = 0.1
pass_angle_deg = 45
max_slope_deg = 25
",
            Points::Drops,
        ),
    ];
    for (g, kind, groups, pts) in cases {
        let t = tp(kind, "b6", groups);
        let r = run(g, &t);
        let what = format!("{kind} {groups:?}");
        let (_, chords, _) = check(g, &r, &t, tol, pts, &what);
        assert!(chords > 20, "{what}: {chords} chords");
        assert_no_gouge(g, &r, &tool("b6"), 0.0, tol);
    }
}
