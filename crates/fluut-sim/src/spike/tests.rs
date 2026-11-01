// SPDX-License-Identifier: MIT OR Apache-2.0

use super::probe::probe;
use super::{relief, rotary};
use crate::dexel::TriDexel;
use crate::motion::Chorder;
use crate::sim::cut;
use crate::stock::Stock;
use fluut_geom::Point3;
use fluut_path::{Pass, PassKind};

#[test]
fn relief_program_is_continuous_and_in_range() {
    let segs = relief::program(3.0, 2.0);
    Pass::new(PassKind::Cut, segs.clone()).check(0).unwrap();
    for s in &segs {
        let p = s.end().unwrap().xyz;
        assert!((8.5..=23.5).contains(&p.z()), "{p:?}");
        assert!((-2.0..=202.0 + 1e-9).contains(&p.x()) && (0.0..=150.0).contains(&p.y()));
    }
    assert_eq!(segs.len(), 51 * 103 - 1);
}

#[test]
fn rotary_program_is_continuous_and_in_range() {
    let segs = rotary::program(5.0, 10.0);
    Pass::new(PassKind::Cut, segs.clone()).check(0).unwrap();
    for s in &segs {
        let p = s.end().unwrap();
        assert!((24.0..=29.0).contains(&p.xyz.z()), "{p:?}");
        assert_eq!(p.xyz.y(), 0.0);
    }
    let last = segs.last().unwrap().end().unwrap();
    // Whole turns until x reaches 154: 32 turns of 5 mm.
    assert!((last.xyz.x() - 156.0).abs() < 1e-9);
    assert!((last.a.unwrap() - 360.0 * 32.0).abs() < 1e-6);
}

#[test]
fn probe_matches_grid_at_ray_positions() {
    let stock = Stock::Box {
        min: Point3::new(90.0, 60.0, 0.0),
        max: Point3::new(96.0, 64.0, 25.0),
    };
    let res = 0.2;
    let segs = relief::program(0.6, 1.0);
    let chords = Chorder::new(relief::tool(), stock, false, res).chords(&segs);
    let mut d = TriDexel::new(&stock, res);
    cut(&mut d, &chords);
    for k in 0..3 {
        let g = &d.grids[k];
        for j in (0..g.nv).step_by(7) {
            for i in (0..g.nu).step_by(5) {
                let p = g.ray_origin(&d, i, j);
                assert_eq!(probe(&stock, &chords, p, k), *g.ray(i, j));
            }
        }
    }
}

#[test]
fn roughing_program_is_continuous_and_above_relief() {
    let segs = relief::roughing(2.4, 0.5);
    Pass::new(PassKind::Cut, segs.clone()).check(0).unwrap();
    for s in &segs {
        let p = s.end().unwrap().xyz;
        if p.z() < 27.0 && (0.0..=200.0).contains(&p.x()) {
            assert!(relief::surface(p.x(), p.y()) + 0.5 <= p.z() + 1e-9, "{p:?}");
        }
    }
    assert!(segs.len() > 1000);
}
