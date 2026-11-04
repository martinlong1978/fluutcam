// SPDX-License-Identifier: MIT OR Apache-2.0

//! The part's distance query for the rest colours (D11 §10): a ball-nosed
//! drop on the exact faces, against a box whose distances are known.

use fluut_cam::cutter::synth::{Builder, cuboid};
use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry};
use fluut_core::fluut_sim::{Clearance, Part};
use fluut_core::sim::DropPart;
use fluut_geom::Point3;
use glam::DVec3;

/// A box `0..10 × 0..10 × 0..5`.
fn boxed() -> DropPart {
    let mut b = Builder::new("box");
    cuboid(
        &mut b,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(10.0, 10.0, 5.0),
    );
    DropPart::new(&SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-box".into(),
            geom: BodyGeom::Brep(b.build()),
        }],
    })
}

/// Whether `p` is at least `r` from the box, by the query.
fn clear(c: &dyn Clearance, p: DVec3, r: f64) -> bool {
    c.clear_height(p.x, p.y, r).is_none_or(|h| p.z >= h)
}

#[test]
fn clear_is_the_distance_to_the_box() {
    let part = boxed();
    let c = part.clearance().expect("a query");
    let c = c.as_ref();
    let r = 0.2;
    // Above the top: clear from r up.
    assert!(!clear(c, DVec3::new(5.0, 5.0, 5.1), r));
    assert!(clear(c, DVec3::new(5.0, 5.0, 5.3), r));
    // Beside the wall at 0.1 and 0.3 mm, below the top.
    assert!(!clear(c, DVec3::new(10.1, 5.0, 3.0), r));
    assert!(clear(c, DVec3::new(10.3, 5.0, 3.0), r));
    // Past the top edge: the distance is to the edge, not the vertical gap.
    // 0.1 beside the edge and 0.15 over it is 0.18 away; 0.2 beside, 0.25.
    assert!(!clear(c, DVec3::new(10.1, 5.0, 5.15), r));
    assert!(clear(c, DVec3::new(10.2, 5.0, 5.15), r));
    // Inside the part is never clear; beside its footprint is.
    assert!(!clear(c, DVec3::new(5.0, 5.0, 2.0), r));
    assert!(clear(c, DVec3::new(20.0, 5.0, 2.0), r));
    // The offset surface itself.
    let h = c.clear_height(10.1, 5.0, r).unwrap();
    assert!(
        (h - (5.0 + (0.2f64 * 0.2 - 0.1 * 0.1).sqrt())).abs() < 1e-6,
        "{h}"
    );
}

#[test]
fn the_query_outlives_the_part_that_made_it() {
    let c = {
        let part = boxed();
        part.clearance().unwrap()
    };
    assert!(clear(c.as_ref(), DVec3::new(5.0, 5.0, 6.0), 0.5));
    assert!(!clear(c.as_ref(), DVec3::new(5.0, 5.0, 5.2), 0.5));
}
