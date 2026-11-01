// SPDX-License-Identifier: MIT OR Apache-2.0

//! `adaptive_2d` (D15b §2.2, D09 §2.3) on the plate pocket and wire
//! regions.

mod support;

use fluut_cam::Region2;
use fluut_cam::geometry::face_id;
use fluut_cam::spike::pocket::swept_path;
use fluut_geom::Point2;
use fluut_path::{Motion, Segment};
use support::cam::*;
use support::step;

fn plate_stock() -> Stock {
    Stock::new([-5.0, -5.0, 0.0], [105.0, 75.0, 20.0])
}

fn pocket(g: &fluut_cam::geometry::SetupGeometry, extra: &str) -> fluut_model::project::Toolpath {
    let b = g.bodies[0].id.clone();
    tp(
        "adaptive_2d",
        "f6",
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n\
             [passes]\noptimal_load = 1.5\nstep_down = 8\n{extra}",
            face_id(&b, 14)
        ),
    )
}

/// The region swept by the tool (R 3) along every cut and ramp move.
fn swept(r: &fluut_cam::Generated) -> Region2 {
    let mut parts = Vec::new();
    for s in r.result.segments() {
        let Some(m) = s.motion() else { continue };
        if !matches!(m.motion, Motion::Cut | Motion::Link | Motion::Ramp) {
            continue;
        }
        if (end(s).z() - 12.0).abs() > 1e-9 {
            continue;
        }
        for l in fluut_path::linearize(s, 1e-4) {
            parts.push(swept_path(&[xy(start(&l)), xy(end(&l))], 3.0));
        }
    }
    while parts.len() > 1 {
        parts = parts
            .chunks(2)
            .map(|c| match c {
                [a, b] => a.union(b),
                [a] => a.clone(),
                _ => unreachable!(),
            })
            .collect();
    }
    parts.pop().unwrap_or_default()
}

#[test]
fn clears_the_plate_pocket() {
    let g = load(&step::plate([0.0; 3]));
    let r = run(&g, &pocket(&g, ""), plate_stock());
    {
        let pocket_shape = Region2::rect(21.0, 21.0, 59.0, 49.0)
            .offset(6.0)
            .subtract(&Region2::circle(40.0, 35.0, 6.0));
        let left = pocket_shape.subtract(&swept(&r));
        for l in &left.loops {
            let one = Region2 {
                loops: vec![l.clone()],
            };
            let (lo, hi) = one.bbox();
            eprintln!(
                "LEFT {:.4} thick {:.4} {:?} {:?}",
                l.area(),
                one.offset(-0.05).area(),
                lo,
                hi
            );
        }
    }
    assert!(r.diagnostics.is_empty(), "{:?}", codes(&r));
    // One level, at the floor.
    assert_eq!(cut_levels(&r), [12.0]);
    // The entry is a helix down from the stock top.
    let helix: Vec<&Segment> = moves(&r, Motion::Ramp)
        .into_iter()
        .filter(|s| matches!(s, Segment::Helix { .. }))
        .collect();
    assert!(!helix.is_empty());
    // Every point of the pocket is cut (its corner radii are larger than
    // the tool radius, so nothing is out of reach).
    // The pocket: 15–65 × 15–55 with 6 mm corner radii.
    let pocket_shape = Region2::rect(21.0, 21.0, 59.0, 49.0)
        .offset(6.0)
        .subtract(&Region2::circle(40.0, 35.0, 6.0));
    let left = pocket_shape.subtract(&swept(&r));
    // Thin slivers between passes may stay, but none 0.1 mm thick (the
    // `cam.adaptive_2d.incomplete` limit at the default tolerance).
    assert!(left.area() < 0.2, "{} mm² left", left.area());
    assert!(left.offset(-0.05).is_empty());
    // No cut runs outside the tool-centre region.
    let centres = pocket_shape.offset(-3.0 + 1e-6);
    for (pi, p) in r.result.passes().iter().enumerate() {
        for (si, s) in p.segments.iter().enumerate() {
            if s.motion().is_some_and(|m| m.motion == Motion::Cut)
                && centres.contains(xy(end(s))) == fluut_cam::Inside::Out
            {
                eprintln!(
                    "pass {pi} of {} seg {si}/{} {:?} -> {:?}",
                    r.result.passes().len(),
                    p.segments.len(),
                    start(s),
                    end(s)
                );
            }
        }
    }
    for s in cuts(&r) {
        assert!(
            centres.contains(xy(end(s))) != fluut_cam::Inside::Out,
            "{:?}",
            end(s)
        );
    }
    // Nor does any move between them at the floor: a straight entry into a
    // pass or a sliver move can cut across the island (within the
    // machining tolerance, for chords of the path around it).
    let reach = pocket_shape.offset(-3.0 + 0.01);
    for s in r.result.segments() {
        if s.motion().is_none_or(|m| m.motion == Motion::Rapid)
            || (start(s).z() - 12.0).abs() > 1e-9
            || (end(s).z() - 12.0).abs() > 1e-9
        {
            continue;
        }
        for l in fluut_path::linearize(s, 1e-4) {
            let (a, b) = (xy(start(&l)), xy(end(&l)));
            let n = (a.distance(b) / 0.05).ceil().max(1.0) as usize;
            for k in 0..=n {
                let q = a + (b - a) * (k as f64 / n as f64);
                assert!(
                    reach.contains(q) != fluut_cam::Inside::Out,
                    "{q:?} on {a:?} -> {b:?} ({:?})",
                    s.motion()
                );
            }
        }
    }
}

#[test]
fn climb_and_conventional_final_contours() {
    let g = load(&step::plate([0.0; 3]));
    let last_loop_area = |dir: &str| -> f64 {
        let r = run(
            &g,
            &pocket(&g, &format!("direction = \"{dir}\"\n")),
            plate_stock(),
        );
        // The final contour of the outer boundary: the cut pass with the
        // largest XY extent; its signed area tells its orientation.
        let passes = r.result.passes();
        let mut best = (0.0f64, 0.0f64);
        for p in passes {
            let pts: Vec<Point2> = p
                .segments
                .iter()
                .filter(|s| s.motion().is_some_and(|m| m.motion == Motion::Cut))
                .map(|s| xy(end(s)))
                .collect();
            if pts.len() < 3 {
                continue;
            }
            let a: f64 = (0..pts.len())
                .map(|i| {
                    let (u, v) = (pts[i], pts[(i + 1) % pts.len()]);
                    0.5 * (u.x() * v.y() - v.x() * u.y())
                })
                .sum();
            if a.abs() > best.0.abs() {
                best = (a, a);
            }
        }
        best.1
    };
    // Climb in a pocket: the outer wall on the right, counter-clockwise.
    assert!(last_loop_area("climb") > 1000.0);
    assert!(last_loop_area("conventional") < -1000.0);
}

#[test]
fn open_pockets_enter_from_outside_the_stock() {
    // A 40 × 20 slot whose left end runs 10 mm past the stock edge.
    let w = wire(&[
        [-10.0, 0.0, 10.0],
        [30.0, 0.0, 10.0],
        [30.0, 20.0, 10.0],
        [-10.0, 20.0, 10.0],
        [-10.0, 0.0, 10.0],
    ]);
    let s = Stock::new([0.0, -10.0, 0.0], [50.0, 30.0, 15.0]);
    let t = tp(
        "adaptive_2d",
        "f6",
        "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n\
         [heights]\nbottom = { ref = \"selection\", offset = -5 }\n\
         [passes]\noptimal_load = 1.2\nstep_down = 10\n",
    );
    let r = run(&w, &t, s);
    assert!(
        !r.result
            .segments()
            .any(|x| matches!(x, Segment::Helix { .. }))
    );
    let plunge = moves(&r, Motion::Plunge);
    assert!(xy(end(plunge[0])).x() < 0.0, "{:?}", end(plunge[0]));
}

#[test]
fn calculation_codes() {
    // An open chain cannot bound a region.
    let w = wire(&[[0.0, 0.0, 10.0], [30.0, 0.0, 10.0]]);
    let t = tp(
        "adaptive_2d",
        "f6",
        "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n\
         [heights]\nbottom = { ref = \"selection\", offset = -2 }\n",
    );
    let s = Stock::new([-20.0, -20.0, 0.0], [50.0, 50.0, 10.0]);
    assert!(run_err(&w, &t, s).contains(&"cam.pocket_2d.region.open".into()));
    // A 4 mm square is narrower than a 6 mm tool.
    let sq = |n: f64| {
        wire(&[
            [0.0, 0.0, 10.0],
            [n, 0.0, 10.0],
            [n, n, 10.0],
            [0.0, n, 10.0],
            [0.0, 0.0, 10.0],
        ])
    };
    let r = run(&sq(4.0), &t, s);
    assert!(codes(&r).contains(&"cam.adaptive_2d.region.unreachable".into()));
    let r = run(&sq(20.0), &t, s);
    assert!(!codes(&r).contains(&"cam.adaptive_2d.region.unreachable".into()));
    assert!(!codes(&r).contains(&"cam.linking.ramp_fallback".into()));
    // A 6.5 mm square fits the tool but not the helix.
    let r = run(&sq(6.5), &t, s);
    assert!(
        codes(&r).contains(&"cam.linking.ramp_fallback".into()),
        "{:?}",
        codes(&r)
    );
    // A large minimum cutting radius leaves material.
    let big = tp(
        "adaptive_2d",
        "f6",
        "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n\
         [heights]\nbottom = { ref = \"selection\", offset = -2 }\n\
         [passes]\nmin_cutting_radius = 40\n",
    );
    let r = run(&sq(30.0), &big, s);
    assert!(
        codes(&r).contains(&"cam.adaptive_2d.incomplete".into()),
        "{:?}",
        codes(&r)
    );
    let r = run(&sq(30.0), &t, s);
    assert!(
        !codes(&r).contains(&"cam.adaptive_2d.incomplete".into()),
        "{:?}",
        codes(&r)
    );
}
