// SPDX-License-Identifier: MIT OR Apache-2.0

//! `bore` (D15b §2.6, D09 §2.8) on the holes block.

mod support;

use fluut_cam::geometry::face_id;
use fluut_geom::Point3;
use fluut_path::{Motion, Segment};
use std::f64::consts::TAU;
use support::cam::*;
use support::step;

fn stock() -> Stock {
    Stock::new([0.0, 0.0, 0.0], [80.0, 50.0, 15.0])
}

const BLIND: &str = "[geometry]\nhole_diameter = { min = 21.9, max = 22.1 }\n";

/// Helix and arc segments: (radius about their centre, start Z, end Z,
/// sweep).
fn turns(r: &fluut_cam::Generated) -> Vec<(f64, f64, f64, f64, bool)> {
    r.result
        .segments()
        .filter_map(|s| match s {
            Segment::Helix {
                a,
                b,
                centre,
                sweep,
                ..
            }
            | Segment::Arc {
                a,
                b,
                centre,
                sweep,
                ..
            } if s.motion().is_some_and(|m| m.motion == Motion::Cut) => Some((
                libm::hypot(a.xyz.x() - centre.x(), a.xyz.y() - centre.y()),
                a.xyz.z(),
                b.xyz.z(),
                *sweep,
                matches!(s, Segment::Helix { .. }),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn helix_with_even_pitch_ends_at_the_bottom() {
    let g = load(&step::holes([0.0; 3]));
    let r = run(&g, &tp("bore", "f6", BLIND), stock());
    // The Ø22 blind hole, Z 7–15, tool R 3: helix radius 8 (a core of
    // Ø10 is left: warning). Pitch 0.6 → 14 turns of 8/14.
    assert_eq!(codes(&r), ["cam.bore.core_left"]);
    let t = turns(&r);
    let helices: Vec<_> = t.iter().filter(|x| x.4).collect();
    assert_eq!(helices.len(), 14);
    for (k, h) in helices.iter().enumerate() {
        assert!((h.0 - 8.0).abs() < 1e-9);
        assert!((h.1 - h.2 - 8.0 / 14.0).abs() < 1e-9, "{k}: {h:?}");
        assert_eq!(h.3, TAU, "climb in a hole runs counter-clockwise");
    }
    assert_eq!(helices[13].2, 7.0);
    // The bottom circle.
    let arcs: Vec<_> = t.iter().filter(|x| !x.4).collect();
    assert_eq!(arcs.len(), 1);
    assert_eq!((arcs[0].1, arcs[0].2), (7.0, 7.0));
    // The lead-in starts at the hole centre, the lead-out ends there.
    let li = moves(&r, Motion::LeadIn);
    assert_eq!(xy(start(li[0])), fluut_geom::Point2::new(62.0, 25.0));
    let lo = moves(&r, Motion::LeadOut);
    assert_eq!(xy(end(lo[0])), fluut_geom::Point2::new(62.0, 25.0));
}

#[test]
fn rapids_stay_above_the_stock_when_top_is_lower() {
    // `top` 3 below the hole's top (15): material the bore cannot see may
    // lie above it, so the approach rapids to the stock top + 1 and feeds
    // from there (D08 §8.2).
    let g = load(&step::holes([0.0; 3]));
    let t = tp(
        "bore",
        "f6",
        &format!(
            "{BLIND}[heights]
top = {{ ref = \"selection\", offset = -3 }}
"
        ),
    );
    let r = run(&g, &t, stock());
    let rapids = moves(&r, Motion::Rapid);
    assert!(!rapids.is_empty());
    assert!(rapids.iter().all(|s| start(s).z().min(end(s).z()) >= 16.0));
    let p = moves(&r, Motion::Plunge);
    assert_eq!((start(p[0]).z(), end(p[0]).z()), (16.0, 12.0));
}

#[test]
fn finishing_pass_stock_and_direction() {
    let g = load(&step::holes([0.0; 3]));
    let t = tp(
        "bore",
        "f6",
        &format!(
            "{BLIND}[passes]\npitch = 2\nfinishing_pass = true\nfinishing_stepover = 0.2\n\
             stock_to_leave = 0.1\nstock_to_leave_axial = 0.05\ndirection = \"conventional\"\n"
        ),
    );
    let r = run(&g, &t, stock());
    let t = turns(&r);
    // Helix at 11 − 3 − 0.1 − 0.2; bottom circle there; final circle at
    // 11 − 3 − 0.1. Conventional: clockwise.
    assert!(t.iter().filter(|x| x.4).all(|x| (x.0 - 7.7).abs() < 1e-9));
    assert!(t.iter().all(|x| x.3 == -TAU));
    let arcs: Vec<_> = t.iter().filter(|x| !x.4).collect();
    assert_eq!(arcs.len(), 2);
    assert!((arcs[0].0 - 7.7).abs() < 1e-9 && (arcs[1].0 - 7.9).abs() < 1e-9);
    assert_eq!(arcs[1].1, 7.05);
}

#[test]
fn faces_circles_and_bosses() {
    let g = load(&step::holes([0.0; 3]));
    let b = g.bodies[0].id.clone();
    // The two half faces of the blind hole give one hole.
    let t = tp(
        "bore",
        "f6",
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{}\" }}, {{ face = \"{}\" }}]\n",
            face_id(&b, 15),
            face_id(&b, 16)
        ),
    );
    let r = run(&g, &t, stock());
    assert_eq!(turns(&r).iter().filter(|x| !x.4).count(), 1);
    // A Ø5 hole is too small for a 6 mm tool.
    let t = tp(
        "bore",
        "f6",
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n",
            face_id(&b, 6)
        ),
    );
    assert!(run_err(&g, &t, stock()).contains(&"cam.bore.hole_small".into()));
    // A face that is not cylindrical.
    let t = tp(
        "bore",
        "f6",
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n",
            face_id(&b, 5)
        ),
    );
    assert!(run_err(&g, &t, stock()).contains(&"param.geometry.drive_faces.kind".into()));
    // A circle as a boss: the helix runs outside it, clockwise for climb.
    let w = wire_circle(20.0, 20.0, 10.0, 5.0);
    let t = tp(
        "bore",
        "f6",
        "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n\
         [heights]\nbottom = { ref = \"selection\", offset = -4 }\n\
         [passes]\nfeature = \"boss\"\npitch = 2\n",
    );
    let r = run(&w, &t, stock());
    let tt = turns(&r);
    assert!(tt.iter().all(|x| (x.0 - 8.0).abs() < 1e-9 && x.3 == -TAU));
    assert_eq!(tt.iter().filter(|x| x.4).count(), 2);
    let li = moves(&r, Motion::LeadIn);
    let p0 = xy(start(li[0]));
    assert!(p0.distance(fluut_geom::Point2::new(20.0, 20.0)) > 8.0);
    // A Ø10 hole leaves no core for a 6 mm tool (helix radius 2 < R).
    let small = wire_circle(20.0, 20.0, 10.0, 5.0);
    let t = tp(
        "bore",
        "f6",
        "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n\
         [heights]\nbottom = { ref = \"selection\", offset = -4 }\n",
    );
    assert!(run(&small, &t, stock()).diagnostics.is_empty());
    // A curve that is not a circle.
    let line = wire(&[[0.0, 0.0, 10.0], [10.0, 0.0, 10.0]]);
    let t = tp(
        "bore",
        "f6",
        "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n[heights]\nbottom = { ref = \"selection\", offset = -4 }\n",
    );
    assert!(run_err(&line, &t, stock()).contains(&"cam.bore.not_circle".into()));
}

#[test]
fn axis_not_parallel() {
    // The ridge's top is a cylinder along X.
    let g = load(&step::ridge([0.0; 3]));
    let b = g.bodies[0].id.clone();
    let t = tp(
        "bore",
        "f6",
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n",
            face_id(&b, 5)
        ),
    );
    let s = Stock::new([0.0, 0.0, 0.0], [60.0, 40.0, 16.0]);
    assert!(run_err(&g, &t, s).contains(&"cam.bore.axis".into()));
    let _ = Point3::new(0.0, 0.0, 0.0);
}
