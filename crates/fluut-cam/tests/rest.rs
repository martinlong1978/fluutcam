// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rest machining from the in-process stock (D08 §7, D15 §2.3.2) in the
//! 2-D roughing types: a 20 mm pocket, then a 6 mm rest pocket.

mod support;

use fluut_cam::geometry::face_id;
use fluut_cam::stock::LayeredStock;
use fluut_geom::Point2;
use support::cam::*;
use support::step;

fn stock() -> Stock {
    Stock::new([0.0, 0.0, 0.0], [80.0, 60.0, 20.0])
}

fn floor(g: &fluut_cam::geometry::SetupGeometry) -> String {
    format!(
        "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n",
        face_id(&g.bodies[0].id, 10)
    )
}

/// The block stock after a 20 mm pocket of the floor in one level. The
/// tool cannot pass between the island and the long walls (15 mm gaps).
fn after_big(g: &fluut_cam::geometry::SetupGeometry) -> (LayeredStock, fluut_cam::Generated) {
    let t = tp(
        "pocket_2d",
        "f20",
        &format!("{}[passes]\nstep_down = 8\n", floor(g)),
    );
    let r = run(g, &t, stock());
    (after(&LayeredStock::planar(&stock().aabb()), &r, "f20"), r)
}

/// Distance from `p` to the nearest point of a cut move.
fn nearest_cut(r: &fluut_cam::Generated, p: Point2) -> f64 {
    cuts(r)
        .iter()
        .flat_map(|s| fluut_path::linearize(s, 0.01))
        .map(|s| {
            let (a, b) = (xy(start(&s)), xy(end(&s)));
            let d = b - a;
            let t = ((p - a).dot(d) / d.dot(d).max(1e-300)).clamp(0.0, 1.0);
            (a + d * t).distance(p)
        })
        .fold(f64::INFINITY, f64::min)
}

fn cut_length(r: &fluut_cam::Generated) -> f64 {
    cuts(r).iter().map(|s| s.length()).sum()
}

#[test]
fn rest_pocket_cuts_only_what_the_big_tool_left() {
    let g = load(&step::pocket_block([0.0; 3]));
    let (st, _) = after_big(&g);
    let rest = tp(
        "pocket_2d",
        "f6",
        &format!(
            "{}rest_from = \"previous\"\n[passes]\nstep_down = 8\n",
            floor(&g)
        ),
    );
    let full = run(&g, &rest, stock());
    let r = run_rest(&g, &rest, stock(), &st);
    assert!(
        cut_length(&r) < 0.7 * cut_length(&full),
        "{} {}",
        cut_length(&r),
        cut_length(&full)
    );
    // Not in the middle of the area the big tool cleared…
    assert!(nearest_cut(&r, Point2::new(20.0, 30.0)) > 4.0);
    // …but in the gap beside the island it could not enter.
    assert!(nearest_cut(&r, Point2::new(40.0, 17.5)) < 1.0);
    // Without in-process stock (first toolpath) it is a full pocket.
    assert_eq!(cut_length(&full), cut_length(&run(&g, &rest, stock())));
}

#[test]
fn repeating_a_roughing_cuts_nothing() {
    let g = load(&step::pocket_block([0.0; 3]));
    let (st, _) = after_big(&g);
    let again = tp(
        "pocket_2d",
        "f20",
        &format!(
            "{}rest_from = \"previous\"\n[passes]\nstep_down = 8\n",
            floor(&g)
        ),
    );
    let r = run_rest(&g, &again, stock(), &st);
    assert!(cut_length(&r) < 1.0, "{}", cut_length(&r));
}

#[test]
fn rest_levels_follow_the_stock() {
    // The big pocket goes to Z 16 only; a rest pocket to the floor in
    // 4 mm levels cuts the whole floor at Z 12 but only the rest at Z 16.
    let g = load(&step::pocket_block([0.0; 3]));
    let big = tp(
        "pocket_2d",
        "f20",
        &format!(
            "{}[heights]\nbottom = {{ ref = \"selection\", offset = 4 }}\n[passes]\nstep_down = 8\n",
            floor(&g)
        ),
    );
    let st = after(
        &LayeredStock::planar(&stock().aabb()),
        &run(&g, &big, stock()),
        "f20",
    );
    let rest = tp(
        "pocket_2d",
        "f6",
        &format!(
            "{}rest_from = \"previous\"\n[passes]\nstep_down = 4\n",
            floor(&g)
        ),
    );
    let r = run_rest(&g, &rest, stock(), &st);
    assert_eq!(cut_levels(&r), vec![16.0, 12.0]);
    let at = |z: f64| -> f64 {
        cuts(&r)
            .iter()
            .filter(|s| (end(s).z() - z).abs() < 1e-9)
            .map(|s| s.length())
            .sum()
    };
    assert!(at(16.0) < 0.6 * at(12.0), "{} {}", at(16.0), at(12.0));
}

#[test]
fn optimised_load_rest_clears_the_gaps_only() {
    let g = load(&step::pocket_block([0.0; 3]));
    let (st, _) = after_big(&g);
    let t = tp(
        "optimised_load_2d",
        "f6",
        &format!(
            "{}rest_from = \"previous\"\n[passes]\noptimal_load = 1.5\nstep_down = 8\n",
            floor(&g)
        ),
    );
    let r = run_rest(&g, &t, stock(), &st);
    assert!(nearest_cut(&r, Point2::new(20.0, 30.0)) > 4.0);
    // The tool (R 3) reaches the middle of both gaps beside the island.
    assert!(nearest_cut(&r, Point2::new(40.0, 17.5)) < 3.0);
    assert!(nearest_cut(&r, Point2::new(40.0, 42.5)) < 3.0);
}

#[test]
fn a_floor_pass_rapids_down_over_cleared_stock() {
    // The pocket roughed to its floor (Z 12) by an earlier toolpath, then
    // finished from 0.1 above it: rapids end 1 mm above the floor's level
    // where the tool has no material within its radius, instead of above
    // the stock top, and never closer than that to material.
    let g = load(&step::pocket_block([0.0; 3]));
    let mut st = LayeredStock::planar(&stock().aabb());
    let pocket = fluut_cam::region::Region2::rect(10.0, 10.0, 70.0, 50.0)
        .subtract(&fluut_cam::region::Region2::rect(35.0, 25.0, 45.0, 35.0));
    st.remove_swept(12.0, &pocket);
    let t = tp(
        "pocket_2d",
        "f6",
        &format!(
            "{}[heights]\ntop = {{ ref = \"selection\", offset = 0.1 }}\n",
            floor(&g)
        ),
    );
    let low = |r: &fluut_cam::Generated| {
        moves(r, fluut_path::Motion::Rapid)
            .iter()
            .map(|s| end(s).z())
            .fold(f64::INFINITY, f64::min)
    };
    let plain = run(&g, &t, stock());
    assert_eq!(low(&plain), 21.0);
    let r = run_rest(&g, &t, stock(), &st);
    assert!((low(&r) - 14.1).abs() < 1e-9, "{}", low(&r));
    for s in moves(&r, fluut_path::Motion::Rapid) {
        let e = end(s);
        if let Some(top) = st.top_near(xy(e), 3.0) {
            assert!(e.z() >= top + 1.0 - 1e-9, "{e:?}");
        }
    }
    assert_eq!(cuts(&r).len(), cuts(&plain).len());
}
