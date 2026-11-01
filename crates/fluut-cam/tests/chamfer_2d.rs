// SPDX-License-Identifier: MIT OR Apache-2.0

//! `chamfer_2d` (D15b §2.5, D09 §2.7) on the boss and the holes block.

mod support;

use fluut_cam::geometry::face_id;
use std::f64::consts::TAU;
use support::cam::*;
use support::step;

fn boss_walls(g: &fluut_cam::geometry::SetupGeometry) -> String {
    let b = g.bodies[0].id.clone();
    let ids: Vec<String> = (0..8)
        .map(|i| format!("{{ face = \"{}\" }}", face_id(&b, i)))
        .collect();
    format!("[geometry]\ndrive_faces = [{}]\n", ids.join(", "))
}

fn boss_stock() -> Stock {
    Stock::new([-5.0, -5.0, 0.0], [45.0, 35.0, 10.0])
}

#[test]
fn tip_depth_and_axis_offset() {
    let g = load(&step::boss([0.0; 3]));
    let t = tp(
        "chamfer_2d",
        "v90",
        &format!(
            "{}[passes]\nchamfer_width = 1.5\ntip_offset = 0.3\n",
            boss_walls(&g)
        ),
    );
    let r = run(&g, &t, boss_stock());
    // α = 45°, Rt = 0.1: d = 1.5 / tan α + 0.3 = 1.8, o = 0.1 + 0.3.
    assert_eq!(cut_levels(&r), [10.0 - 1.8]);
    let len: f64 = cuts(&r).iter().map(|s| s.length()).sum();
    let want = 100.0 + TAU * (5.0 + 0.4);
    assert!((len - want).abs() < 1e-9, "{len} vs {want}");
    assert!(r.diagnostics.is_empty(), "{:?}", codes(&r));
    // The top face's edge loop gives the same path.
    let b = g.bodies[0].id.clone();
    let t2 = tp(
        "chamfer_2d",
        "v90",
        &format!(
            "[geometry]\ndrive_curves = [{{ face = \"{}\" }}]\n\
             [passes]\nchamfer_width = 1.5\ntip_offset = 0.3\n",
            face_id(&b, 9)
        ),
    );
    let r2 = run(&g, &t2, boss_stock());
    let len2: f64 = cuts(&r2).iter().map(|s| s.length()).sum();
    assert!((len2 - want).abs() < 1e-9);
    // Climb outside a boss runs clockwise (material on the right).
    let c = cuts(&r);
    let area: f64 = c
        .iter()
        .map(|s| {
            let (a, b) = (start(s), end(s));
            0.5 * (a.x() * b.y() - b.x() * a.y())
        })
        .sum();
    assert!(area < 0.0, "{area}");
    let t3 = tp(
        "chamfer_2d",
        "v90",
        &format!("{}[passes]\ndirection = \"conventional\"\n", boss_walls(&g)),
    );
    let c3 = run(&g, &t3, boss_stock());
    let area3: f64 = cuts(&c3)
        .iter()
        .map(|s| {
            let (a, b) = (start(s), end(s));
            0.5 * (a.x() * b.y() - b.x() * a.y())
        })
        .sum();
    assert!(area3 > 0.0);
}

#[test]
fn errors() {
    let g = load(&step::boss([0.0; 3]));
    let b = g.bodies[0].id.clone();
    // A face that is not a wall.
    let t = tp(
        "chamfer_2d",
        "v90",
        &format!(
            "[geometry]\ndrive_faces = [{{ face = \"{}\" }}]\n",
            face_id(&b, 9)
        ),
    );
    assert!(run_err(&g, &t, boss_stock()).contains(&"param.geometry.drive_faces.kind".into()));
    // Inside or outside need closed chains.
    let w = wire(&[[0.0, 0.0, 10.0], [30.0, 0.0, 10.0]]);
    let t = tp(
        "chamfer_2d",
        "v90",
        "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n[passes]\nside = \"outside\"\n",
    );
    assert!(run_err(&w, &t, boss_stock()).contains(&"param.chamfer_2d.side.closure".into()));
    let t = tp(
        "chamfer_2d",
        "v90",
        "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n[passes]\nside = \"left\"\n",
    );
    let r = run(&w, &t, boss_stock());
    assert!(cuts(&r).iter().all(|s| (end(s).y() - 0.6).abs() < 1e-9));
    // An edge that is not at one Z.
    let s = wire(&[[0.0, 0.0, 10.0], [30.0, 0.0, 8.0]]);
    let t = tp(
        "chamfer_2d",
        "v90",
        "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n",
    );
    assert!(run_err(&s, &t, boss_stock()).contains(&"cam.chamfer_2d.edge.not_horizontal".into()));
}

#[test]
fn tip_gouge_on_a_shallow_step() {
    // The Ø10 counterbore is 3 mm deep: its floor is at Z 12. A tip
    // 3.5 mm below the edge touches it; 1.8 mm does not.
    let g = load(&step::holes([0.0; 3]));
    let b = g.bodies[0].id.clone();
    let geo = format!(
        "[geometry]\ndrive_faces = [{{ face = \"{}\" }}, {{ face = \"{}\" }}]\n",
        face_id(&b, 10),
        face_id(&b, 11)
    );
    let s = Stock::new([0.0, 0.0, 0.0], [80.0, 50.0, 15.0]);
    let ok = tp(
        "chamfer_2d",
        "v90",
        &format!("{geo}[passes]\nside = \"inside\"\nchamfer_width = 1.5\ntip_offset = 0.3\n"),
    );
    let r = run(&g, &ok, s);
    assert_eq!(cut_levels(&r), [15.0 - 1.8]);
    let len: f64 = cuts(&r).iter().map(|s| s.length()).sum();
    assert!((len - TAU * (5.0 - 0.4)).abs() < 1e-9, "{len}");
    let deep = tp(
        "chamfer_2d",
        "v90",
        &format!("{geo}[passes]\nside = \"inside\"\nchamfer_width = 1.5\ntip_offset = 2\n"),
    );
    assert!(run_err(&g, &deep, s).contains(&"cam.chamfer_2d.tip_gouge".into()));
}
