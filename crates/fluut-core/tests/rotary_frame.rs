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

/// A closed STL box, as triangles.
fn stl_box(lo: [f64; 3], hi: [f64; 3]) -> String {
    let v = |i: usize| {
        [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ]
    };
    // Each face as two triangles, counter-clockwise from outside.
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let mut s = String::from("solid t\n");
    for q in quads {
        for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            s.push_str("facet normal 0 0 0\nouter loop\n");
            for i in t {
                let p = v(i);
                s.push_str(&format!("vertex {} {} {}\n", p[0], p[1], p[2]));
            }
            s.push_str("endloop\nendfacet\n");
        }
    }
    s.push_str("endsolid t\n");
    s
}

/// The shaft (b-?) with a mesh blank beside it as the stock body.
fn with_blank(
    name: &str,
    kind: &str,
    orientation: &str,
    zero: &str,
) -> Result<SetupFrame, fluut_model::Diagnostic> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("shaft.step"), step::shaft([0.0; 3])).unwrap();
    // The blank: 30 × 24 across, Z -2 to 84; its far corner is 15 from the
    // axis in X and 12 in Y.
    std::fs::write(
        dir.join("blank.stl"),
        stl_box([-15.0, -12.0, -2.0], [15.0, 12.0, 84.0]),
    )
    .unwrap();
    let text = format!(
        "format = \"fluut.project/1\"\n[[geometry]]\nid = \"g1\"\npath = \"shaft.step\"\n\
         kind = \"step\"\n[[geometry]]\nid = \"g2\"\npath = \"blank.stl\"\nkind = \"stl\"\nunit = \"mm\"\n\
         [[setup]]\nid = \"s1\"\nkind = \"{kind}\"\n\
         geometry = {{ bodies = [\"STEPBODY\"] }}\n{orientation}\n\
         stock = {{ kind = \"body\", body = \"BLANK\" }}\n{zero}\n"
    );
    let probe = dir.join("probe.fluut");
    std::fs::write(
        &probe,
        text.replace("geometry = { bodies = [\"STEPBODY\"] }\n", "")
            .replace("stock = { kind = \"body\", body = \"BLANK\" }\n", ""),
    )
    .unwrap();
    let s = Session::open(&probe, &[]).unwrap();
    let ids: Vec<String> = s.store.bodies().map(|b| b.id.clone()).collect();
    assert_eq!(ids.len(), 2, "{ids:?}");
    let text = text.replace("STEPBODY", &ids[0]).replace("BLANK", &ids[1]);
    std::fs::write(dir.join("p.fluut"), text).unwrap();
    let s = Session::open(&dir.join("p.fluut"), &[]).unwrap();
    let r = resolve_setup(&s.project, "s1")?;
    SetupFrame::new(&r, s.store.bodies().cloned().collect())
}

#[test]
fn a_rotary_body_stock_is_the_bounding_cylinder_of_the_body() {
    let f = with_blank(
        "rotary-body-stock",
        "rotary",
        "orientation = { rotary_axis = \"+Z\", a_zero_toward = \"+X\" }",
        "zero = { x = \"stock_left\" }\nclearance = { kind = \"cylinder\", radius = 25 }",
    )
    .unwrap();
    let r = f.rotary.unwrap();
    // Reach: the blank's corner, sqrt(15² + 12²); along the axis it spans
    // 86 mm from the zero at its left end.
    let reach = (15.0f64 * 15.0 + 12.0 * 12.0).sqrt();
    assert!((r.stock_radius - reach).abs() < 1e-9, "{}", r.stock_radius);
    assert!(r.stock_x.0.abs() < 1e-9 && (r.stock_x.1 - 86.0).abs() < 1e-9);
    // The shaft is machined, not the blank: its Ø30 end is the model's reach.
    assert!((r.model_r.1 - 15.0).abs() < 1e-9, "{:?}", r.model_r);
    assert!((f.stock.max.y() - reach).abs() < 1e-9);
}

#[test]
fn an_indexed_body_stock_is_the_box_of_the_body() {
    let f = with_blank(
        "indexed-body-stock",
        "indexed",
        "orientation = { rotary_axis = \"+Z\", a_zero_toward = \"+X\" }",
        "zero = { x = \"stock_left\" }\nclearance = { kind = \"cylinder\", radius = 25 }",
    );
    let f = f.unwrap();
    // The box of the blank: 86 along the axis, 24 in Y, 30 in Z (the
    // model's X, turned to +Z of W).
    assert!((f.stock.max.x() - f.stock.min.x() - 86.0).abs() < 1e-9);
    assert!((f.stock.max.y() - f.stock.min.y() - 24.0).abs() < 1e-9);
    assert!((f.stock.max.z() - f.stock.min.z() - 30.0).abs() < 1e-9);
    assert!(f.index.is_some() && f.rotary.is_none());
}
