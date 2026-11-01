// SPDX-License-Identifier: MIT OR Apache-2.0

//! 3-D roughing (D15b §4, D09 §4.1–§4.2): `pocket_3d` and `adaptive_3d` on
//! the pocket block and the ridge.

mod support;

use fluut_cam::Generated;
use fluut_cam::geometry::SetupGeometry;
use fluut_cam::region::{Inside, Region2};
use fluut_cam::section::Sectioner;
use fluut_cam::stock::LayeredStock;
use fluut_geom::Point2;
use support::cam::*;
use support::step;

fn block_stock() -> Stock {
    Stock::new([0.0, 0.0, 0.0], [80.0, 60.0, 20.0])
}

/// The ridge (crest at Z 15.36) in a stock 1 mm higher and 5 mm wider.
fn ridge_stock() -> Stock {
    Stock::new([-5.0, -5.0, 0.0], [65.0, 45.0, 16.5]).model(0.0, 15.36)
}

/// Every cut point keeps `clear` (R + stock to leave, less a little) from
/// the section of the part at its Z.
fn assert_clear_of_part(g: &SetupGeometry, r: &Generated, clear: f64) {
    let s = Sectioner::new(g, 0.01).unwrap();
    let mut cache: Vec<(f64, Region2)> = Vec::new();
    for seg in cuts(r) {
        for l in fluut_path::linearize(seg, 0.01) {
            for p in [start(&l), end(&l)] {
                let z = p.z();
                let i = match cache.iter().position(|(k, _)| *k == z) {
                    Some(i) => i,
                    None => {
                        let reg = s.section(z + 1e-4).unwrap().region;
                        cache.push((z, reg.offset(clear - 0.01)));
                        cache.len() - 1
                    }
                };
                assert_eq!(
                    cache[i].1.contains(xy(p)),
                    Inside::Out,
                    "cut point {p:?} is inside the part grown by {clear}"
                );
            }
        }
    }
}

fn cut_length(r: &Generated) -> f64 {
    cuts(r).iter().map(|s| s.length()).sum()
}

#[test]
fn pocket_3d_levels_with_flats() {
    let g = load(&step::pocket_block([0.0; 3]));
    // Even levels every 5 from 20, plus the floor at 12; nothing to cut
    // below the floor.
    let t = tp("pocket_3d", "f6", "[passes]\nstep_down = 5\n");
    let r = run(&g, &t, block_stock());
    assert_eq!(cut_levels(&r), vec![15.0, 12.0]);
    assert_clear_of_part(&g, &r, 3.0);
    // Without flat detection the floor is missed.
    let t = tp(
        "pocket_3d",
        "f6",
        "[passes]\nstep_down = 5\nflat_detection = false\n",
    );
    assert_eq!(cut_levels(&run(&g, &t, block_stock())), vec![15.0]);
    // Axial stock to leave lifts the flat level.
    let t = tp(
        "pocket_3d",
        "f6",
        "[passes]\nstep_down = 5\nstock_to_leave = 0.5\nstock_to_leave_axial = 0.25\n",
    );
    let r = run(&g, &t, block_stock());
    assert_eq!(cut_levels(&r), vec![15.0, 12.25]);
    assert_clear_of_part(&g, &r, 3.5);
}

#[test]
fn pocket_3d_codes() {
    let g = load(&step::pocket_block([0.0; 3]));
    // A 20 mm tool cannot pass beside the island (15 mm gaps).
    let t = tp("pocket_3d", "f20", "[passes]\nstep_down = 8\n");
    let r = run(&g, &t, block_stock());
    assert!(
        codes(&r).contains(&"cam.pocket_3d.region.unreachable".into()),
        "{:?}",
        codes(&r)
    );
    let t = tp("pocket_3d", "f6", "[passes]\nstep_down = 8\n");
    let r = run(&g, &t, block_stock());
    assert!(!codes(&r).contains(&"cam.pocket_3d.region.unreachable".into()));
}

#[test]
fn adaptive_3d_on_a_curved_top() {
    let g = load(&step::ridge([0.0; 3]));
    let t = tp(
        "adaptive_3d",
        "f6",
        "[passes]\noptimal_load = 1.5\nstep_down = 3\nstock_to_leave = 0.3\n",
    );
    let r = run(&g, &t, ridge_stock());
    // Down the curved top, then around the walls inside the wider stock.
    let levels = cut_levels(&r);
    assert_eq!(levels.last(), Some(&0.0), "{levels:?}");
    assert_clear_of_part(&g, &r, 3.3);
}

#[test]
fn adaptive_3d_fine_levels_cut_less() {
    let g = load(&step::ridge([0.0; 3]));
    let coarse = tp(
        "adaptive_3d",
        "f6",
        "[passes]\noptimal_load = 1.5\nstep_down = 4\n",
    );
    let fine = tp(
        "adaptive_3d",
        "f6",
        "[passes]\noptimal_load = 1.5\nstep_down = 4\nfine_step_down = 1\n",
    );
    let a = run(&g, &coarse, ridge_stock());
    let b = run(&g, &fine, ridge_stock());
    let (la, lb) = (cut_levels(&a), cut_levels(&b));
    assert!(lb.len() > la.len(), "{la:?} {lb:?}");
    assert_clear_of_part(&g, &b, 3.0);
    // A fine level clears only steps, so it cuts much less than a main one.
    let at = |r: &Generated, z: f64| -> f64 {
        cuts(r)
            .iter()
            .filter(|s| (end(s).z() - z).abs() < 1e-9)
            .map(|s| s.length())
            .sum()
    };
    let fine_z: Vec<f64> = lb.iter().filter(|z| !la.contains(z)).copied().collect();
    let main_cut = at(&b, la[0]);
    for z in fine_z {
        assert!(at(&b, z) < main_cut, "{z}: {} {main_cut}", at(&b, z));
    }
}

#[test]
fn adaptive_3d_incomplete_code() {
    let g = load(&step::pocket_block([0.0; 3]));
    let t = tp(
        "adaptive_3d",
        "f6",
        "[passes]\noptimal_load = 1.5\nstep_down = 8\nmin_cutting_radius = 40\n",
    );
    let r = run(&g, &t, block_stock());
    assert!(
        codes(&r).contains(&"cam.adaptive_3d.incomplete".into()),
        "{:?}",
        codes(&r)
    );
    // The ridge clears completely (the passing case is also in
    // `adaptive_3d_on_a_curved_top`).
    let g = load(&step::ridge([0.0; 3]));
    let t = tp(
        "adaptive_3d",
        "f6",
        "[passes]\noptimal_load = 1.5\nstep_down = 4\n",
    );
    let r = run(&g, &t, ridge_stock());
    assert!(
        !codes(&r).contains(&"cam.adaptive_3d.incomplete".into()),
        "{:?}",
        codes(&r)
    );
}

#[test]
fn rest_after_roughing_cuts_little() {
    let g = load(&step::pocket_block([0.0; 3]));
    let first = tp("pocket_3d", "f6", "[passes]\nstep_down = 4\n");
    let a = run(&g, &first, block_stock());
    let st = after(&LayeredStock::planar(&block_stock().aabb()), &a, "f6");
    let again = tp(
        "pocket_3d",
        "f6",
        "[geometry]\nrest_from = \"previous\"\n[passes]\nstep_down = 4\n",
    );
    let b = run_rest(&g, &again, block_stock(), &st);
    assert!(
        cut_length(&b) < 0.05 * cut_length(&a),
        "{} {}",
        cut_length(&b),
        cut_length(&a)
    );
    // A 6 mm rest after a 20 mm roughing cuts the gaps by the island.
    let big = tp("pocket_3d", "f20", "[passes]\nstep_down = 8\n");
    let st = after(
        &LayeredStock::planar(&block_stock().aabb()),
        &run(&g, &big, block_stock()),
        "f20",
    );
    let rest = tp(
        "adaptive_3d",
        "f6",
        "[geometry]\nrest_from = \"previous\"\n[passes]\noptimal_load = 1.5\nstep_down = 8\n",
    );
    let c = run_rest(&g, &rest, block_stock(), &st);
    let near = |p: Point2| {
        cuts(&c)
            .iter()
            .flat_map(|s| fluut_path::linearize(s, 0.01))
            .map(|s| xy(start(&s)).distance(p).min(xy(end(&s)).distance(p)))
            .fold(f64::INFINITY, f64::min)
    };
    assert!(near(Point2::new(40.0, 17.5)) < 3.0);
    assert!(near(Point2::new(20.0, 30.0)) > 4.0);
}
