// SPDX-License-Identifier: MIT OR Apache-2.0

//! The output of the surface-following finishing types against fresh
//! drops (adaptive sampling, fu-8): every cut point is a real drop, never
//! an interpolated height, and every cut chord is within the machining
//! tolerance of fresh drops at its quarter, middle and three-quarter
//! points, on a free-form and on analytic parts, with ball, flat and
//! bull tools. The strategy tests' dense gouge check runs on the same
//! results.

mod support;

use fluut_cam::Generated;
use fluut_cam::cutter::synth::sculpted;
use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry};
use fluut_cam::surface::SurfaceQuery;
use fluut_geom::{Point3, Vec3};
use fluut_model::project::Toolpath;
use fluut_path::Motion;
use support::finish::*;
use support::step;

const TOL: f64 = 0.01;

fn freeform() -> SetupGeometry {
    SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-s".into(),
            geom: BodyGeom::Brep(sculpted(4, 4.0, 5)),
        }],
    }
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

/// The CL tip at `p` as the sampler takes it: the drop, raised to
/// `bottom`.
fn tip(q: &SurfaceQuery, p: Point3, bottom: f64) -> (f64, f64) {
    match q.drop(p.x(), p.y()) {
        Some(h) if h.tip >= bottom => (h.tip, h.slope),
        _ => (bottom, 0.0),
    }
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
/// chord at least `tol` long in XY is within `tol` (along the contact
/// normal) of fresh drops at 1/4, 1/2 and 3/4 of it, except where a level
/// chord runs along a wall (the error is sideways there, as in
/// [`assert_no_gouge`]). Returns the worst deviation, the number of
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
            let (z, slope) = tip(&q, p, bottom);
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
            let (z, slope) = tip(&q, x, bottom);
            let dev = (z - x.z()) * libm::cos(slope);
            worst = worst.max(dev.abs());
            assert!(
                dev.abs() <= tol,
                "{what}: chord {a:?}..{b:?} is {dev:.6} off the drop at {f}"
            );
        }
    }
    (worst, chords, placed)
}

fn all_tools(g: &SetupGeometry, name: &str) {
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
            assert!(worst <= TOL, "{what}: {worst}");
            assert_no_gouge(g, &r, &tool(tl), 0.0, TOL);
        }
    }
}

#[test]
fn freeform_chords_hold_the_tolerance() {
    all_tools(&freeform(), "sculpted");
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
