// SPDX-License-Identifier: MIT OR Apache-2.0

//! `trace` (D15b §2.3, D09 §2.5) on wire bodies and plate edges.

mod support;

use support::cam::*;
use support::step;

const W: &str = "[geometry]\ndrive_curves = [{ body = \"b-w\" }]\n";

fn stock() -> Stock {
    Stock::new([-20.0, -20.0, 0.0], [60.0, 60.0, 20.0])
}

#[test]
fn heights_mode_levels_end_at_bottom() {
    let g = wire(&[[0.0, 0.0, 20.0], [40.0, 0.0, 20.0]]);
    let t = tp(
        "trace",
        "v90",
        &format!("{W}[heights]\nbottom = {{ ref = \"stock_top\", offset = -3 }}\n"),
    );
    let r = run(&g, &t, stock());
    // Preset step-down 2 over 3 mm: two even levels.
    assert_eq!(cut_levels(&r), [18.5, 17.0]);
    // The second level starts where the first ended (direction any).
    let c = cuts(&r);
    assert_eq!(xy(end(c[0])), xy(start(c[1])));
    assert!(r.diagnostics.is_empty(), "{:?}", codes(&r));
    // `selection` fixes the direction.
    let t = tp(
        "trace",
        "v90",
        &format!(
            "{W}[heights]\nbottom = {{ ref = \"stock_top\", offset = -3 }}\n\
             [passes]\ndirection = \"selection\"\n"
        ),
    );
    let c2 = run(&g, &t, stock());
    assert!(cuts(&c2).iter().all(|s| end(s).x() > start(s).x()));
    let t = tp(
        "trace",
        "v90",
        &format!(
            "{W}[heights]\nbottom = {{ ref = \"stock_top\", offset = -3 }}\n\
             [passes]\ndirection = \"reversed\"\ndepth_count = 1\n"
        ),
    );
    let c3 = run(&g, &t, stock());
    assert!(cuts(&c3).iter().all(|s| end(s).x() < start(s).x()));
    assert_eq!(cut_levels(&c3), [17.0]);
}

#[test]
fn curve_mode_follows_the_curve_z() {
    // A line falling from Z 20 to Z 10 over 40 mm.
    let g = wire(&[[0.0, 0.0, 20.0], [40.0, 0.0, 10.0]]);
    let t = tp(
        "trace",
        "v90",
        &format!("{W}[passes]\ndepth_mode = \"curve\"\ncurve_depth = 1\nstep_down = 0.5\n"),
    );
    let r = run(&g, &t, stock());
    let c = cuts(&r);
    for s in &c {
        let p = end(s);
        let z_curve = 20.0 - 0.25 * p.x();
        let below = z_curve - p.z();
        assert!(
            (below - 0.5).abs() < 1e-9 || (below - 1.0).abs() < 1e-9,
            "{p:?}"
        );
    }
    assert_eq!(end(c[c.len() - 1]).z(), 20.0 - 1.0);
    // With a side offset the Z still comes from the nearest curve point.
    let t = tp(
        "trace",
        "v90",
        &format!("{W}[passes]\ndepth_mode = \"curve\"\nside_offset = 2\n"),
    );
    let r = run(&g, &t, stock());
    let c = cuts(&r);
    assert!(c.iter().all(|s| (end(s).y() - 2.0).abs() < 1e-9));
    assert!((end(c[c.len() - 1]).z() - 10.0).abs() < 1e-9);
    // Below the stock bottom: a warning.
    let t = tp(
        "trace",
        "v90",
        &format!("{W}[passes]\ndepth_mode = \"curve\"\ncurve_depth = 11\nstep_down = 20\n"),
    );
    assert!(codes(&run(&g, &t, stock())).contains(&"cam.trace.below_stock".into()));
}

#[test]
fn side_offsets_of_open_and_closed_chains() {
    let g = wire(&[
        [0.0, 0.0, 20.0],
        [10.0, 0.0, 20.0],
        [10.0, 10.0, 20.0],
        [0.0, 10.0, 20.0],
        [0.0, 0.0, 20.0],
    ]);
    let base = format!("{W}[heights]\nbottom = {{ ref = \"stock_top\", offset = -1 }}\n");
    // Positive is outward for closed chains: a 14 × 14 square with
    // rounded corners.
    let r = run(
        &g,
        &tp(
            "trace",
            "v90",
            &format!("{base}[passes]\nside_offset = 2\n"),
        ),
        stock(),
    );
    let len: f64 = cuts(&r).iter().map(|s| s.length()).sum();
    let want = 40.0 + std::f64::consts::TAU * 2.0;
    assert!((len - want).abs() < 1e-9, "{len}");
    // Negative shrinks it; too negative makes it vanish.
    let r = run(
        &g,
        &tp(
            "trace",
            "v90",
            &format!("{base}[passes]\nside_offset = -2\n"),
        ),
        stock(),
    );
    let len: f64 = cuts(&r).iter().map(|s| s.length()).sum();
    assert!((len - 24.0).abs() < 1e-9, "{len}");
    let r = run(
        &g,
        &tp(
            "trace",
            "v90",
            &format!("{base}[passes]\nside_offset = -6\n"),
        ),
        stock(),
    );
    assert!(codes(&r).contains(&"cam.trace.offset_empty".into()));
    // Open chains: positive is to the left of the selection direction.
    let g = wire(&[[0.0, 0.0, 20.0], [30.0, 0.0, 20.0]]);
    let r = run(
        &g,
        &tp(
            "trace",
            "v90",
            &format!("{base}[passes]\nside_offset = 1.5\n"),
        ),
        stock(),
    );
    assert!(cuts(&r).iter().all(|s| (end(s).y() - 1.5).abs() < 1e-12));
}

#[test]
fn edges_of_a_solid() {
    // The pocket's top edges of the plate, scored 0.8 deep (D15b example).
    let g = load(&step::plate([0.0; 3]));
    let b = g.bodies[0].id.clone();
    let t = tp(
        "trace",
        "v90",
        &format!(
            "[geometry]\ndrive_curves = [{{ face = \"{}\" }}]\n\
             [heights]\nbottom = {{ ref = \"stock_top\", offset = -0.8 }}\n",
            fluut_cam::geometry::face_id(&b, 5)
        ),
    );
    let s = Stock::new([-5.0, -5.0, 0.0], [105.0, 75.0, 20.0]);
    let r = run(&g, &t, s);
    assert_eq!(cut_levels(&r), [19.2]);
    // Two loops: the outer edge and the pocket edge.
    let len: f64 = cuts(&r).iter().map(|s| s.length()).sum();
    let pocket = 2.0 * (38.0 + 28.0) + std::f64::consts::TAU * 6.0;
    assert!((len - (340.0 + pocket)).abs() < 1e-6, "{len}");
}
