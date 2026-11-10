// SPDX-License-Identifier: MIT OR Apache-2.0

//! Fixtures that turn with A (spec 03 section 3.9.6): the turn radius over
//! the X range the tool reaches, on the two sided plate (x 0 to 80, y 25,
//! z 8 about the X axis, block margin 5, 5, 1: stock x 0 to 90, corner
//! radius 31.321) and on the test shaft.

#[path = "../../fluut-cam/tests/support/step.rs"]
mod step;

use fluut_core::{Session, SetupFrame};
use fluut_model::setup::resolve_setup;
use std::path::PathBuf;

const CORNER: f64 = 31.320_919_526_731_65;

/// The frame of the indexed Setup `s1` at `angle` with `fixtures` (the file
/// text of the `fixtures` value, or empty).
fn indexed(name: &str, angle: f64, fixtures: &str) -> SetupFrame {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("p.step"), step::two_sided([0.0; 3])).unwrap();
    let fx = if fixtures.is_empty() {
        String::new()
    } else {
        format!("fixtures = {fixtures}\n")
    };
    let text = format!(
        "format = \"fluut.project/1\"
[[geometry]]
id = \"g1\"
path = \"p.step\"
kind = \"step\"
[[setup]]
id = \"s1\"
kind = \"indexed\"
orientation = {{ rotary_axis = \"+X\" }}
index_angle = {{ mode = \"absolute\", value = {angle:?} }}
stock = {{ kind = \"block\", margin = [5, 5, 1] }}
zero = {{ x = \"stock_left\" }}
{fx}"
    );
    std::fs::write(dir.join("p.fluut"), text).unwrap();
    let s = Session::open(&dir.join("p.fluut"), &[]).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    let r = resolve_setup(&s.project, "s1").unwrap();
    SetupFrame::new(&r, s.store.bodies().cloned().collect()).unwrap()
}

fn close(a: Option<f64>, b: f64) {
    let a = a.expect("a turn radius");
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

#[test]
fn without_fixtures_the_turn_radius_is_the_stocks() {
    let f = indexed("fx_none", 0.0, "");
    assert!(f.fixtures.is_empty());
    for reach in [0.0, 3.0, 500.0] {
        close(f.turn_radius_reaching(reach), CORNER);
    }
    close(f.turn_clearance, CORNER + 10.0);
}

#[test]
fn a_chuck_bigger_than_the_part_raises_the_turn_over_its_range() {
    // A chuck of radius 50 against the stock's start face (x 0): x -20 to 0.
    let chuck = "[{ kind = \"chuck\", diameter = 100, length = 20 }]";
    for angle in [0.0, 30.0, 180.0] {
        let f = indexed(&format!("fx_chuck_{angle}"), angle, chuck);
        // It only touches the stock, so a tool that stays within the
        // stock's X range does not meet it.
        close(f.turn_radius, CORNER);
        close(f.turn_clearance, CORNER + 10.0);
        // A tool of radius 3 reaches x -3: the chuck counts.
        close(f.turn_radius_reaching(3.0), 50.0);
        close(f.turn_clearance_reaching(3.0), 60.0);
        // The chuck is 20 long: a tool that stays 1 mm out of it does not.
        close(f.turn_radius_reaching(0.0), CORNER);
    }
}

#[test]
fn fixtures_smaller_than_the_part_change_nothing() {
    let small = "[{ kind = \"chuck\", diameter = 40, length = 20 }, \
                 { kind = \"tailstock\", diameter = 30, length = 20 }, \
                 { kind = \"clamp\", min = [10, -10, -5], max = [20, 10, 5] }]";
    let f = indexed("fx_small", 0.0, small);
    assert_eq!(f.fixtures.len(), 3);
    for reach in [0.0, 3.0, 25.0] {
        close(f.turn_radius_reaching(reach), CORNER);
    }
    close(f.turn_clearance, CORNER + 10.0);
}

#[test]
fn a_tailstock_cone_counts_by_its_radius_inside_the_reach() {
    // From Ø90 at the stock's end (x 90) to Ø20 at x 120: 45 down to 10.
    let t = "[{ kind = \"tailstock\", diameter = 20, length = 30, tip_diameter = 90 }]";
    let f = indexed("fx_tail", 0.0, t);
    close(f.turn_radius_reaching(0.0), CORNER);
    close(f.turn_radius_reaching(3.0), 45.0);
    // The same cone the other way round is only 45 at its far end.
    let t = "[{ kind = \"tailstock\", diameter = 90, length = 30, tip_diameter = 20 }]";
    let f = indexed("fx_tail_rev", 0.0, t);
    // At x 93 the cone is 10 + 35 * 3 / 30 = 13.5 wide at the radius.
    close(f.turn_radius_reaching(3.0), CORNER);
    close(f.turn_radius_reaching(30.0), 45.0);
    // A plain cylinder tailstock is its radius from the stock's end.
    let t = "[{ kind = \"tailstock\", diameter = 80, length = 10 }]";
    let f = indexed("fx_tail_cyl", 0.0, t);
    close(f.turn_radius_reaching(1.0), 40.0);
}

#[test]
fn a_clamp_counts_only_over_the_x_range_it_covers() {
    // Radius hypot(45, 5) = 45.277 over x 40 to 60, inside the stock.
    let inside = "[{ kind = \"clamp\", min = [40, -45, -5], max = [60, 45, 5] }]";
    let want = libm::hypot(45.0, 5.0);
    for angle in [0.0, 90.0, 30.0] {
        let f = indexed(&format!("fx_clamp_{angle}"), angle, inside);
        // A turn does not change a distance from the axis.
        close(f.turn_radius, want);
        close(f.turn_clearance, want + 10.0);
    }
    // The same clamp 110 mm further out: beyond the stock and the tool.
    let outside = "[{ kind = \"clamp\", min = [200, -45, -5], max = [220, 45, 5] }]";
    let f = indexed("fx_clamp_far", 0.0, outside);
    close(f.turn_radius_reaching(3.0), CORNER);
    close(f.turn_radius_reaching(50.0), CORNER);
    close(f.turn_radius_reaching(150.0), want);
}

#[test]
fn a_clamp_is_bounded_after_the_index_turn() {
    // The clamp is given at A = 0 and turns with the Setup: at 90 degrees a
    // box 90 wide in Y and 10 high in Z is 10 wide and 90 high in W.
    let c = "[{ kind = \"clamp\", min = [40, -45, -5], max = [60, 45, 5] }]";
    let f = indexed("fx_clamp_turned", 90.0, c);
    match &f.fixtures[0] {
        fluut_core::fixture::FixtureShape::Box { min, max, radius } => {
            assert_eq!((min[1], max[1]), (-5.0, 5.0));
            assert_eq!((min[2], max[2]), (-45.0, 45.0));
            assert!((radius - libm::hypot(45.0, 5.0)).abs() < 1e-9);
        }
        other => panic!("{other:?}"),
    }
}

/// The rotary frame of the shaft with `extra` keys in the Setup.
fn rotary(name: &str, extra: &str) -> Result<SetupFrame, fluut_model::Diagnostic> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("shaft.step"), step::shaft([2.0, -1.0, 5.0])).unwrap();
    let text = format!(
        "format = \"fluut.project/1\"\n[[geometry]]\nid = \"g1\"\npath = \"shaft.step\"\n\
         kind = \"step\"\n[[setup]]\nid = \"s1\"\nkind = \"rotary\"\n\
         orientation = {{ rotary_axis = {{ face = \"f-354.0\" }}, a_zero_toward = \"+X\" }}\n\
         stock = {{ kind = \"cylinder\", radius = 17, length = 82 }}\n\
         zero = {{ x = \"stock_left\" }}\n{extra}"
    );
    std::fs::write(dir.join("p.fluut"), text).unwrap();
    let s = Session::open(&dir.join("p.fluut"), &[]).unwrap();
    let r = resolve_setup(&s.project, "s1")?;
    SetupFrame::new(&r, s.store.bodies().cloned().collect())
}

#[test]
fn a_rotary_setup_counts_fixtures_over_the_stock() {
    // A clamp across the shaft's middle, radius 40.
    let c = "fixtures = [{ kind = \"clamp\", min = [30, -40, -2], max = [40, 40, 2] }]\n";
    let f = rotary(
        "fx_rot_clamp",
        &format!("{c}clearance = {{ kind = \"cylinder\", radius = 60 }}\n"),
    )
    .unwrap();
    close(f.turn_radius, libm::hypot(40.0, 2.0));
    // The stock is 17: the clamp's 40 holds a clearance cylinder of 25 off.
    let e = rotary(
        "fx_rot_unsafe",
        &format!("{c}clearance = {{ kind = \"cylinder\", radius = 25 }}\n"),
    )
    .unwrap_err();
    assert_eq!(e.code, "setup.clearance.index_unsafe");
    // A chuck at the stock's start is outside the stock: it does not
    // count for the cylinder the rapids go round on.
    let chuck = "fixtures = [{ kind = \"chuck\", diameter = 120, length = 30 }]\n";
    let f = rotary(
        "fx_rot_chuck",
        &format!("{chuck}clearance = {{ kind = \"cylinder\", radius = 25 }}\n"),
    )
    .unwrap();
    close(f.turn_radius, 17.0);
    close(f.turn_radius_reaching(2.0), 60.0);
}
