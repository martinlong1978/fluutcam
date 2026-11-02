// SPDX-License-Identifier: MIT OR Apache-2.0

//! `drill` (D15b §3.1, D09 §3.1) on the holes block.

mod support;

use fluut_cam::geometry::face_id;
use fluut_cam::region::Region2;
use fluut_cam::stock::LayeredStock;
use fluut_geom::Point2;
use fluut_path::{CycleKind, CycleParams, Motion, Segment};
use support::cam::*;
use support::step;

fn stock() -> Stock {
    Stock::new([0.0, 0.0, 0.0], [80.0, 50.0, 15.0])
}

const FIVE: &str = "[geometry]\nhole_diameter = { min = 4.9, max = 5.1 }\n";

/// The cycles in order: (x, y, params).
fn cycles(r: &fluut_cam::Generated) -> Vec<(Point2, CycleParams)> {
    r.result
        .segments()
        .filter_map(|s| match s {
            Segment::Cycle { at, cycle, .. } => Some((xy(at.xyz), *cycle)),
            _ => None,
        })
        .collect()
}

#[test]
fn holes_by_diameter_in_shortest_order() {
    let g = load(&step::holes([0.0; 3]));
    let r = run(&g, &tp("drill", "d5", FIVE), stock());
    assert!(r.diagnostics.is_empty(), "{:?}", codes(&r));
    let c = cycles(&r);
    let at: Vec<Point2> = c.iter().map(|x| x.0).collect();
    // From the work zero: (15, 15), (15, 35), then the Ø5 hole under the
    // counterbore at (40, 25).
    assert_eq!(
        at,
        [
            Point2::new(15.0, 15.0),
            Point2::new(15.0, 35.0),
            Point2::new(40.0, 25.0)
        ]
    );
    // Through holes from the top at 15 to the bottom at 0; the hole under
    // the counterbore starts at its own top, Z 12. R plane: top + 2, but
    // never below the stock top + the safety distance: nothing machines
    // the counterbore, and the cycle rapids to the R plane (D08 §8.2).
    let (_, p) = c[0];
    assert_eq!(
        (p.kind, p.top_z, p.depth_z, p.retract_z),
        (CycleKind::Drill, 15.0, 0.0, 17.0)
    );
    assert_eq!((c[2].1.top_z, c[2].1.retract_z), (12.0, 16.0));
    assert!(
        moves(&r, Motion::Rapid)
            .iter()
            .all(|s| start(s).z().min(end(s).z()) >= 16.0)
    );
    // Retract mode `retract`: between holes at the retract height (stock
    // top + 5).
    let rapid_z: Vec<f64> = moves(&r, Motion::Rapid)
        .iter()
        .map(|s| end(s).z())
        .collect();
    assert!(rapid_z.contains(&20.0));
    // Selection order keeps the found order.
    let t = tp(
        "drill",
        "d5",
        &format!("{FIVE}[passes]\nhole_order = \"selection\"\n"),
    );
    assert_eq!(cycles(&run(&g, &t, stock())).len(), 3);
}

#[test]
fn counterbore_stack_and_depth_options() {
    let g = load(&step::holes([0.0; 3]));
    let b = g.bodies[0].id.clone();
    let ids: Vec<String> = [10, 11, 13, 14]
        .iter()
        .map(|&i| format!("{{ face = \"{}\" }}", face_id(&b, i)))
        .collect();
    let geo = format!("[geometry]\ndrive_faces = [{}]\n", ids.join(", "));
    let r = run(&g, &tp("drill", "d5", &geo), stock());
    let c = cycles(&r);
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].1.top_z, c[0].1.depth_z), (15.0, 0.0));
    // Full diameter: deeper by (D/2) / tan(118° / 2).
    let t = tp(
        "drill",
        "d5",
        &format!("{geo}[passes]\ndepth_to = \"full_diameter\"\ncycle = \"peck\"\npeck_depth = 3\n"),
    );
    let c = cycles(&run(&g, &t, stock()));
    let extra = 2.5 / libm::tan(59f64.to_radians());
    assert!((c[0].1.depth_z + extra).abs() < 1e-12);
    assert_eq!((c[0].1.kind, c[0].1.peck), (CycleKind::Peck, Some(3.0)));
    // The Ø10 counterbore alone: a 5 mm drill is the wrong tool.
    let t = tp(
        "drill",
        "d5",
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n",
            face_id(&b, 10)
        ),
    );
    assert!(codes(&run(&g, &t, stock())).contains(&"cam.drill.diameter_mismatch".into()));
    // The flute is 30 long; a hole 31 deep warns.
    let t = tp(
        "drill",
        "d5",
        &format!("{FIVE}[heights]\nbottom = {{ ref = \"selection\", offset = -16 }}\n"),
    );
    assert!(codes(&run(&g, &t, stock())).contains(&"cam.drill.depth.flute".into()));
}

#[test]
fn retract_modes_and_dwell() {
    let g = load(&step::holes([0.0; 3]));
    let t = tp(
        "drill",
        "d5",
        &format!("{FIVE}[passes]\nretract_mode = \"feed\"\ncycle = \"dwell\"\n"),
    );
    let r = run(&g, &t, stock());
    // Between holes at the R plane: never above 17 after the first cycle.
    let segs: Vec<&Segment> = r.result.segments().collect();
    let first = segs
        .iter()
        .position(|s| matches!(s, Segment::Cycle { .. }))
        .unwrap();
    let last = segs
        .iter()
        .rposition(|s| matches!(s, Segment::Cycle { .. }))
        .unwrap();
    assert!(
        segs[first..last]
            .iter()
            .filter_map(|s| s.end())
            .all(|p| p.xyz.z() <= 17.0)
    );
    let c = cycles(&r);
    assert!(
        c.iter()
            .all(|x| x.1.kind == CycleKind::Spot && x.1.dwell_s == Some(0.5))
    );
}

#[test]
fn circles_boundary_and_errors() {
    // A circle takes its depth from the heights.
    let w = wire_circle(20.0, 20.0, 10.0, 2.5);
    let t = tp(
        "drill",
        "d5",
        "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n\
         [heights]\nbottom = { ref = \"selection\", offset = -6 }\n",
    );
    let c = cycles(&run(&w, &t, stock()));
    assert_eq!((c[0].1.top_z, c[0].1.depth_z), (10.0, 4.0));
    let line = wire(&[[0.0, 0.0, 10.0], [10.0, 0.0, 10.0]]);
    assert!(run_err(&line, &t, stock()).contains(&"cam.drill.not_circle".into()));
    // A boundary keeps the holes whose axis is inside it.
    let mut g = load(&step::holes([0.0; 3]));
    let wb = wire(&[
        [0.0, 0.0, 15.0],
        [30.0, 0.0, 15.0],
        [30.0, 50.0, 15.0],
        [0.0, 50.0, 15.0],
        [0.0, 0.0, 15.0],
    ]);
    g.bodies.extend(wb.bodies);
    let t = tp(
        "drill",
        "d5",
        &format!("{FIVE}geometry.boundary = {{ curves = [{{ body = \"b-w\" }}] }}\n")
            .replace("geometry.boundary", "boundary"),
    );
    let c = cycles(&run(&g, &t, stock()));
    assert_eq!(c.len(), 2);
    // A cylinder along X is skipped with a warning.
    let ridge = load(&step::ridge([0.0; 3]));
    let rb = ridge.bodies[0].id.clone();
    let t = tp(
        "drill",
        "d5",
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n",
            face_id(&rb, 5)
        ),
    );
    let e = run_err(&ridge, &t, Stock::new([0.0, 0.0, 0.0], [60.0, 40.0, 16.0]));
    assert!(e.contains(&"cam.drill.axis".into()), "{e:?}");
}

#[test]
fn r_plane_over_the_in_process_stock() {
    // An earlier toolpath cleared the Ø10 counterbore over the hole at
    // (40, 25) down to Z 12: its R plane is its top + 2, no longer the
    // stock top + the safety distance. The other holes keep theirs.
    let g = load(&step::holes([0.0; 3]));
    let mut st = LayeredStock::planar(&stock().aabb());
    st.remove_swept(12.0, &Region2::circle(40.0, 25.0, 5.0));
    let r = run_rest(&g, &tp("drill", "d5", FIVE), stock(), &st);
    let c = cycles(&r);
    assert_eq!((c[0].1.retract_z, c[2].1.retract_z), (17.0, 14.0));
    // A counterbore cleared to less than the tool radius round the hole
    // leaves material within reach.
    let mut st = LayeredStock::planar(&stock().aabb());
    st.remove_swept(12.0, &Region2::circle(40.0, 27.0, 2.0));
    let r = run_rest(&g, &tp("drill", "d5", FIVE), stock(), &st);
    assert_eq!(cycles(&r)[2].1.retract_z, 16.0);
}
