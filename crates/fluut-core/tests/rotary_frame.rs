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
