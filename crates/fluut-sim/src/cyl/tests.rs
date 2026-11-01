// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::motion::Chorder;
use crate::spike::probe::probe_line;
use crate::spike::rotary;
use crate::tool::Tool;
use fluut_geom::Point3;
use fluut_path::{AxisPos, FeedRef, Motion, MotionInfo, Segment};

fn run(stock: Stock, tool: Tool, res: f64, segs: &[Segment]) -> (CylDexel, Vec<Sweep>) {
    let mut d = CylDexel::new(&stock, res);
    let chords = Chorder::new(tool, stock, true, res).chords(segs);
    d.cut(&chords);
    (d, chords)
}

fn turn(x: f64, z: f64) -> Segment {
    let p = |a: f64| AxisPos {
        xyz: Point3::new(x, 0.0, z),
        a: Some(a),
        b: None,
    };
    Segment::Rotary {
        a: p(0.0),
        b: p(360.0),
        radius: z,
        m: MotionInfo::new(Motion::Cut, FeedRef::Cut, 0.01),
    }
}

#[test]
fn fresh_volume_is_the_cylinder() {
    let stock = Stock::Cylinder {
        x0: 0.0,
        x1: 20.0,
        radius: 10.0,
    };
    let d = CylDexel::new(&stock, 0.1);
    let exact = PI * 100.0 * 20.0;
    for v in d.volumes() {
        assert!((v - exact).abs() / exact < 2e-3, "{v} vs {exact}");
    }
    // Only X rays inside the circle are stored: about π/4 of the square.
    let m = d.memory();
    assert_eq!(m.rays, d.nx * d.nt + d.xrays.len());
    let square = d.n * d.n;
    assert!((d.xrays.len() as f64 / square as f64 - PI / 4.0).abs() < 0.01);
    assert!(d.x_ray(0, 0).is_none());
    assert!(d.x_ray(d.n / 2, d.n / 2).is_some());
}

#[test]
fn full_rotary_turn_cuts_a_ring() {
    let stock = Stock::Cylinder {
        x0: 0.0,
        x1: 20.0,
        radius: 10.0,
    };
    let res = 0.1;
    let (d, _) = run(stock, Tool::ball(1.0), res, &[turn(10.0, 8.0)]);
    // Radial rays at x = 10.05, 0.05 from the ball centre: material up to
    // the ball's bottom there, within the sagitta (chords lie inside the
    // circle).
    let r = 8.0 + 1.0 - (1.0 - 0.05 * 0.05_f64).sqrt();
    let tol = res / 4.0 + 1e-4;
    for j in (0..d.nt).step_by(7) {
        let spans = &d.radial[j * d.nx + 100];
        assert_eq!(spans.len(), 1, "θ row {j}: {spans:?}");
        assert_eq!(spans[0].lo, 0.0);
        let hi = f64::from(spans[0].hi);
        assert!(hi <= r + 1e-5 && hi >= r - tol, "θ row {j}: {hi} vs {r}");
        // Far from the cut the ray is whole.
        let far = &d.radial[j * d.nx + 20];
        assert_eq!(far[0].hi, 10.0);
    }
    // X rays: through the ring they are cut in two around x = 10, inside it
    // they are whole.
    let (ring, _) = d.x_ray(d.n / 2, d.n / 2 + 88).unwrap(); // z = 8.85
    assert_eq!(ring.len(), 2, "{ring:?}");
    // 0.15 below the ball centres' circle: the ball's half chord there.
    let half = (1.0 - 0.15 * 0.15_f64).sqrt();
    assert!(
        (f64::from(ring[0].hi) - (10.0 - half)).abs() < tol,
        "{ring:?}"
    );
    assert!(
        (f64::from(ring[1].lo) - (10.0 + half)).abs() < tol,
        "{ring:?}"
    );
    let (inner, _) = d.x_ray(d.n / 2, d.n / 2 + 70).unwrap(); // z = 7.05
    assert_eq!(inner.len(), 1);
    assert_eq!(inner[0].hi, 20.0);
}

#[test]
fn probe_matches_grid() {
    let stock = Stock::Cylinder {
        x0: 60.0,
        x1: 72.0,
        radius: 30.0,
    };
    let res = 0.4;
    let (d, chords) = run(stock, rotary::tool(), res, &rotary::program(5.0, 10.0));
    let mut cut = 0;
    for j in (0..d.nt).step_by(13) {
        for i in (0..d.nx).step_by(3) {
            let (o, e) = d.radial_ray(i, j);
            let want = &d.radial[j * d.nx + i];
            assert_eq!(&probe_line(&chords, o, e, 0.0, 30.0), want);
            cut += usize::from(want[0].hi < 30.0);
        }
    }
    assert!(cut > 100, "{cut}");
    for j in (0..d.n).step_by(5) {
        for i in (0..d.n).step_by(5) {
            if let Some((spans, o)) = d.x_ray(i, j) {
                assert_eq!(&probe_line(&chords, o, DVec3::X, 0.0, 12.0), spans);
            }
        }
    }
}

#[test]
fn same_hash_on_one_and_many_threads() {
    let stock = Stock::Cylinder {
        x0: 60.0,
        x1: 75.0,
        radius: 30.0,
    };
    let segs = rotary::program(2.0, 4.0);
    let run_on = |n: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .unwrap();
        pool.install(|| run(stock, rotary::tool(), 0.4, &segs).0.hash())
    };
    assert_eq!(run_on(1), run_on(4));
}
