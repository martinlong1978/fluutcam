// SPDX-License-Identifier: MIT OR Apache-2.0

//! The frame of a `rotary` Setup (spec 03 §3.9.3, D15b §1.3) on the test
//! shaft, an axis along model Z.

#[path = "../../fluut-cam/tests/support/step.rs"]
mod step;

use fluut_core::{Session, SetupFrame};
use fluut_model::setup::resolve_setup;
use std::path::PathBuf;

fn frame(name: &str, setup: &str) -> Result<SetupFrame, fluut_model::Diagnostic> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("shaft.step"), step::shaft([2.0, -1.0, 5.0])).unwrap();
    let text = format!(
        "format = \"fluut.project/1\"\n[[geometry]]\nid = \"g1\"\npath = \"shaft.step\"\n\
         kind = \"step\"\n[[setup]]\nid = \"s1\"\nkind = \"rotary\"\n{setup}"
    );
    std::fs::write(dir.join("p.fluut"), text).unwrap();
    let s = Session::open(&dir.join("p.fluut"), &[]).unwrap();
    let r = resolve_setup(&s.project, "s1")?;
    SetupFrame::new(&r, s.store.bodies().cloned().collect())
}

#[test]
fn axis_from_a_cylinder_face_and_zero_on_the_stock_end() {
    let f = frame(
        "rotary-frame-ok",
        "orientation = { rotary_axis = { face = \"f-354.0\" }, a_zero_toward = \"+X\" }\n\
         stock = { kind = \"cylinder\", radius = 17, length = 82 }\n\
         zero = { x = \"stock_left\" }\nclearance = { kind = \"cylinder\", radius = 25 }\n",
    )
    .unwrap();
    let r = f.rotary.unwrap();
    // The stock is centred on the 80 mm shaft; X starts at its left end.
    assert!((r.stock_x.0).abs() < 1e-9 && (r.stock_x.1 - 82.0).abs() < 1e-9);
    assert!((r.model_x.0 - 1.0).abs() < 1e-9 && (r.model_x.1 - 81.0).abs() < 1e-9);
    // Radii: the Ø16 end (8) to the Ø30 part (15); the square's flats (10)
    // lie between.
    assert!((r.model_r.0 - 8.0).abs() < 1e-9, "{:?}", r.model_r);
    assert!((r.model_r.1 - 15.0).abs() < 1e-9, "{:?}", r.model_r);
    let env = f.height_env();
    assert_eq!(env.stock_top, 17.0);
    assert_eq!(env.stock_bottom, 0.0);
    assert_eq!(env.setup_clearance, 25.0);
    // The model's +X points to +Z of W at A = 0.
    let p = f
        .to_work
        .apply_point(fluut_geom::Point3::new(2.0 + 15.0, -1.0, 5.0 + 10.0));
    assert!((p.z() - 15.0).abs() < 1e-9 && p.y().abs() < 1e-9, "{p:?}");
    assert_eq!(f.initial_stock().top(), 17.0);
}

#[test]
fn rotary_frame_errors() {
    // A block stock is not a rotary stock.
    let e = frame(
        "rotary-frame-block",
        "orientation = { rotary_axis = \"+Z\", a_zero_toward = \"+X\" }\n\
         stock = { kind = \"block\" }\nzero = { x = \"stock_left\" }\n",
    )
    .unwrap_err();
    assert_eq!(e.code, "setup.stock.unsupported");
    // A = 0 along the axis.
    let e = frame(
        "rotary-frame-a0",
        "orientation = { rotary_axis = \"+Z\", a_zero_toward = \"+Z\" }\n\
         stock = { kind = \"cylinder\", radius = 17, length = 82 }\nzero = { x = \"stock_left\" }\n",
    )
    .unwrap_err();
    assert_eq!(e.code, "setup.orientation.invalid");
}

/// The frame of an indexed Setup on the two-sided plate (x 0…80, y ±25,
/// z ±8 about the X axis) with a 5, 5, 1 mm block margin.
fn indexed(name: &str, angle: f64, stock: &str) -> SetupFrame {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("p.step"), step::two_sided([0.0; 3])).unwrap();
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
         stock = {stock}
zero = {{ x = \"stock_left\" }}
"
    );
    std::fs::write(dir.join("p.fluut"), text).unwrap();
    let s = Session::open(&dir.join("p.fluut"), &[]).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    let r = resolve_setup(&s.project, "s1").unwrap();
    SetupFrame::new(&r, s.store.bodies().cloned().collect()).unwrap()
}

const BLOCK: &str = "{ kind = \"block\", margin = [5, 5, 1] }";

#[test]
fn indexed_frames_turn_the_rotary_frame() {
    use fluut_geom::Point3;
    let p = Point3::new(40.0, 20.0, 8.0);
    for (angle, lo, hi, q) in [
        (
            0.0,
            [0.0, -30.0, -8.0],
            [90.0, 30.0, 9.0],
            [45.0, 20.0, 8.0],
        ),
        (
            180.0,
            [0.0, -30.0, -9.0],
            [90.0, 30.0, 8.0],
            [45.0, -20.0, -8.0],
        ),
        (
            90.0,
            [0.0, -9.0, -30.0],
            [90.0, 8.0, 30.0],
            [45.0, -8.0, 20.0],
        ),
    ] {
        let f = indexed(&format!("indexed-{angle}"), angle, BLOCK);
        assert_eq!(f.stock.min, Point3::new(lo[0], lo[1], lo[2]), "{angle}");
        assert_eq!(f.stock.max, Point3::new(hi[0], hi[1], hi[2]), "{angle}");
        assert_eq!(
            f.to_work.apply_point(p),
            Point3::new(q[0], q[1], q[2]),
            "{angle}"
        );
        let env = f.height_env();
        assert_eq!(env.stock_top, hi[2]);
        assert_eq!(env.setup_clearance, hi[2] + 10.0);
        assert_eq!(f.initial_stock().top(), hi[2]);
        assert!(f.rotary.is_none() && f.rotary_info().is_none());
        assert_eq!(f.index.unwrap().angle, angle);
    }
    // A cylinder stock: planar slices of the cylinder.
    let f = indexed(
        "indexed-cyl",
        30.0,
        "{ kind = \"cylinder\", radius = 30, length = 90 }",
    );
    assert_eq!(f.stock.max.z(), 30.0);
    let st = f.initial_stock();
    assert_eq!((st.bottom, st.top()), (-30.0, 30.0));
    assert_eq!(f.index.unwrap().cylinder, Some((30.0, (0.0, 90.0))));
}

#[test]
fn turn_radius_holds_everything_that_turns() {
    // The block's farthest edge from the axis: y ±30, z 9 at A = 0, at any
    // index angle.
    let block = (30.0f64 * 30.0 + 9.0 * 9.0).sqrt();
    for angle in [0.0, 30.0, 180.0] {
        let f = indexed(&format!("turn-{angle}"), angle, BLOCK);
        assert_eq!(f.turn_radius, Some(block), "{angle}");
        assert_eq!(f.turn_clearance, Some(block + 10.0), "{angle}");
    }
    // A cylinder stock: its radius, which holds the plate (y ±25, z ±8).
    let f = indexed(
        "turn-cyl",
        90.0,
        "{ kind = \"cylinder\", radius = 30, length = 90 }",
    );
    assert_eq!(f.turn_radius, Some(30.0));
    assert_eq!(f.turn_clearance, Some(40.0));
    // A rotary Setup: the stock cylinder; the clearance offset is the
    // default for a clearance cylinder.
    let f = frame(
        "turn-rotary",
        "orientation = { rotary_axis = { face = \"f-354.0\" }, a_zero_toward = \"+X\" }\n\
         stock = { kind = \"cylinder\", radius = 17, length = 82 }\n\
         zero = { x = \"stock_left\" }\nclearance = { kind = \"cylinder\", radius = 25 }\n",
    )
    .unwrap();
    assert_eq!(f.turn_radius, Some(17.0));
    assert_eq!(f.turn_clearance, Some(27.0));
}

#[test]
fn rotary_clearance_inside_the_stock_is_refused() {
    for r in [12, 17] {
        let e = frame(
            &format!("turn-unsafe-{r}"),
            &format!(
                "orientation = {{ rotary_axis = {{ face = \"f-354.0\" }}, a_zero_toward = \"+X\" }}\n\
                 stock = {{ kind = \"cylinder\", radius = 17, length = 82 }}\n\
                 zero = {{ x = \"stock_left\" }}\nclearance = {{ kind = \"cylinder\", radius = {r} }}\n"
            ),
        )
        .unwrap_err();
        assert_eq!(e.code, "setup.clearance.index_unsafe", "{r}");
    }
}
