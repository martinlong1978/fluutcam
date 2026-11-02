// SPDX-License-Identifier: MIT OR Apache-2.0

//! `steep_and_shallow` (D09 §4.8, D15b §5.5) on the dome: the steep part
//! only where the slope reaches the threshold, the shallow region with its
//! overlap, both shallow patterns, `region_order`, codes, gouges and
//! translation invariance.

mod support;

use fluut_geom::Point3;
use support::finish::*;
use support::step::{self, DOME_CENTRE};

fn sns(tool: &str, passes: &str) -> fluut_model::project::Toolpath {
    tp("steep_and_shallow", tool, &format!("[passes]\n{passes}\n"))
}

fn radial(p: Point3) -> f64 {
    libm::hypot(p.x() - DOME_CENTRE[0], p.y() - DOME_CENTRE[1])
}

/// Steep (waterline) runs and the others: a steep run is level and its
/// middle point touches at 30° or steeper (or touches nothing: the outer
/// walls).
fn split(r: &fluut_cam::Generated) -> (Vec<Vec<Point3>>, Vec<Vec<Point3>>) {
    let g = dome();
    let q = reference(&g, &tool("b6"), 0.0);
    runs(r).into_iter().partition(|l| {
        let m = l[l.len() / 2];
        l.iter().all(|p| (p.z() - l[0].z()).abs() < 1e-12)
            && q.probe(m.x(), m.y())
                .is_none_or(|h| h.slope >= 30f64.to_radians() - 1e-6)
    })
}

#[test]
fn steep_and_shallow_parts_meet_at_the_threshold() {
    // Ball on the dome, threshold 30°: on the hemisphere the contact slope
    // is asin(ρ / 15) for a CL radius ρ, so the steep band is ρ ≥ 7.5 up
    // to the boss wall (ρ = 15), and the block top is shallow.
    let g = dome();
    let s = 1.5;
    let ov = 1.0;
    let r = run(
        &g,
        &sns(
            "b6",
            &format!(
                "threshold_deg = 30\noverlap = {ov}\nsteep = {{ step_down = 2 }}\n\
                 shallow = {{ stepover = {s} }}"
            ),
        ),
    );
    let (steep, shallow) = split(&r);
    assert!(!steep.is_empty() && !shallow.is_empty());
    for p in steep.iter().flatten().filter(|p| radial(**p) < 20.0) {
        assert!(radial(*p) >= 7.5 - 1e-6, "steep {p:?}");
    }
    // The shallow region reaches into the band by at most the overlap and
    // half a stepover (plus the tolerance).
    let lo = 7.5 + ov + 0.5 * s + 0.02;
    let hi = 15.0 - ov - 0.5 * s - 0.02;
    for p in shallow.iter().flatten() {
        let rr = radial(*p);
        assert!(!(rr > lo && rr < hi), "shallow {p:?} at {rr}");
    }
    assert_no_gouge(&g, &r, &tool("b6"), 0.0, 0.01 + 1e-9);
}

#[test]
fn overlap_grows_the_shallow_region() {
    let g = dome();
    let reach = |ov: f64| {
        let r = run(
            &g,
            &sns(
                "b6",
                &format!("threshold_deg = 30\noverlap = {ov}\nshallow = {{ stepover = 1.5 }}"),
            ),
        );
        // Largest CL radius of the shallow cut inside the dome's band.
        split(&r)
            .1
            .iter()
            .flatten()
            .map(|p| radial(*p))
            .filter(|r| *r < 11.0)
            .fold(0.0f64, f64::max)
    };
    let (a, b) = (reach(0.0), reach(2.0));
    assert!(b > a + 1.5, "{a} {b}");
}

#[test]
fn shallow_patterns_and_region_order() {
    let g = dome();
    let par = run(&g, &sns("b6", "shallow = { stepover = 2 }"));
    let sc = run(
        &g,
        &sns("b6", "shallow = { pattern = \"scallop\", stepover = 2 }"),
    );
    // The scallop pattern cuts loops over the surface: closed runs whose Z
    // varies (contours are closed but level; parallel passes never close).
    let surface_loop = |l: &Vec<Point3>| {
        l.first() == l.last() && l.len() > 4 && l.iter().any(|p| (p.z() - l[0].z()).abs() > 1e-6)
    };
    assert!(runs(&sc).iter().any(surface_loop));
    assert!(!runs(&par).iter().any(surface_loop));
    // Steep first: a closed contour; shallow first: a straight pass at
    // the pass angle 0 (one Y).
    let first = |r: &fluut_cam::Generated| runs(r)[0].clone();
    let f = first(&par);
    assert!(f.first() == f.last() && f.iter().all(|p| (p.z() - f[0].z()).abs() < 1e-12));
    let sf = run(
        &g,
        &sns(
            "b6",
            "region_order = \"shallow_first\"\nshallow = { stepover = 2 }",
        ),
    );
    let f = first(&sf);
    assert!(f.first() != f.last() && f.iter().all(|p| (p.y() - f[0].y()).abs() < 1e-12));
    assert_no_gouge(&g, &sc, &tool("b6"), 0.0, 0.01 + 1e-9);
}

#[test]
fn validation_codes() {
    let g = dome();
    codes_pass_fail(
        "param.steep_and_shallow.threshold.range",
        &g,
        &sns("b6", "threshold_deg = 60"),
        &sns("b6", "threshold_deg = 90"),
    );
    codes_pass_fail(
        "param.steep_and_shallow.shallow.stepover_and_scallop",
        &g,
        &sns("b6", "shallow = { scallop_height = 0.05 }"),
        &sns("b6", "shallow = { scallop_height = 0.05, stepover = 1 }"),
    );
    codes_pass_fail(
        "param.steep_and_shallow.scallop.radius",
        &g,
        &sns("r6", "steep = { scallop_height = 0.05 }"),
        &sns("f6", "steep = { scallop_height = 0.05 }"),
    );
    codes_pass_fail(
        "param.steep_and_shallow.scallop.range",
        &g,
        &sns("r6", "shallow = { scallop_height = 0.5 }"),
        &sns("r6", "shallow = { scallop_height = 1 }"),
    );
    codes_pass_fail(
        "param.steep_and_shallow.shallow.direction",
        &g,
        &sns("b6", "shallow = { pattern = \"scallop\", stepover = 2 }"),
        &sns(
            "b6",
            "shallow = { pattern = \"scallop\", stepover = 2, direction = \"zigzag\" }",
        ),
    );
    codes_pass_fail(
        "param.passes.stepover.max",
        &g,
        &sns("b6", "shallow = { stepover = 6 }"),
        &sns("b6", "shallow = { stepover = 6.5 }"),
    );
    codes_pass_fail(
        "cam.steep_and_shallow.tool_shape",
        &g,
        &sns("f6", "shallow = { stepover = 3 }"),
        &sns("v6", "shallow = { stepover = 3 }"),
    );
}

#[test]
fn no_gouges_with_any_tool() {
    let g = dome();
    for id in ["b6", "f6", "r6"] {
        let r = run(
            &g,
            &sns(
                id,
                "steep = { step_down = 3 }\nshallow = { stepover = 2.5 }\n[linking]\nretract = \"stay_down\"",
            ),
        );
        assert_no_gouge(&g, &r, &tool(id), 0.0, 0.01 + 1e-9);
    }
}

#[test]
fn translation_invariance() {
    let t = sns(
        "b6",
        "threshold_deg = 40\nsteep = { step_down = 3 }\nshallow = { stepover = 3, pass_angle_deg = 30 }",
    );
    assert_translation_invariant(step::dome, &t);
}

#[test]
fn rapids_stay_above_the_stock() {
    // The 3-D finishing linker cannot see what earlier toolpaths left, so
    // no rapid goes below stock top + safety distance (1); below that it
    // plunges (the rapid floor of D08 §8.2). The dome has no roughing.
    let g = dome();
    let floor = env(&g).stock_top + 1.0;
    for t in [
        tp(
            "steep_and_shallow",
            "b6",
            "[passes]\nsteep = { step_down = 3 }\nshallow = { stepover = 2 }\n",
        ),
        tp("parallel", "b6", "[passes]\nstepover = 2\n"),
        tp("contour", "b6", "[passes]\nstep_down = 3\n"),
    ] {
        let r = run(&g, &t);
        for s in with_motion(&r, fluut_path::Motion::Rapid) {
            let (a, b) = (s.start().unwrap().xyz, s.end().unwrap().xyz);
            if b.z() <= a.z() {
                assert!(b.z() >= floor - 1e-9, "{}: rapid to {b:?}", tool_of(&t));
            }
        }
    }
}
