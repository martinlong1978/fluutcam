// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::mesh::{GOUGE, REST, WITHIN, tag_class, tag_toolpath};
use crate::run::tests::{block, feed, path, program, settings, xyz};
use crate::run::{Quiet, Simulation, Step};
use crate::tool::Tool;
use crate::verify::Part;
use fluut_path::{Pass, PassKind, ToolpathResult};

/// A zigzag raster of `rows` rows along X at height `z`, one pass per row:
/// a step in Y onto the row, then the row in two moves.
fn raster(rows: usize, z: f64) -> ToolpathResult {
    let passes = (0..rows)
        .map(|r| {
            let y = 1.0 + 2.0 * r as f64;
            let (a, b) = if r % 2 == 0 { (1.0, 19.0) } else { (19.0, 1.0) };
            Pass::new(
                PassKind::Cut,
                vec![
                    feed(xyz(a, y - 2.0, z), xyz(a, y, z)),
                    feed(xyz(a, y, z), xyz(10.0, y, z)),
                    feed(xyz(10.0, y, z), xyz(b, y, z)),
                ],
            )
        })
        .collect();
    ToolpathResult::new(passes).unwrap()
}

fn two_toolpaths() -> (ToolpathResult, ToolpathResult) {
    (
        raster(5, 8.0),
        path(&[xyz(-3.0, 5.0, 6.0), xyz(23.0, 5.0, 6.0)]),
    )
}

fn sim(part: Option<&dyn Part>) -> Simulation {
    let (a, b) = two_toolpaths();
    Simulation::run(
        block(20.0, 10.0, 10.0),
        &[
            program("t1", &a, Tool::flat(1.0)),
            program("t2", &b, Tool::flat(1.0)),
        ],
        &settings(0.25),
        part,
        &Quiet,
    )
    .unwrap()
}

#[test]
fn forward_cutting_equals_the_stock_after() {
    let s = sim(None);
    let n = s.chords.len();
    let mut p = Player::new(&s, 0);
    for k in [1, n / 5, n / 3, n / 3, n / 2, n - 1, n, 3, n / 2, 0, n] {
        p.seek(&s, k);
        assert_eq!(p.chords(), k);
        assert_eq!(p.stock().hash(), s.stock_after(k).hash(), "at {k}");
        // The blocks are the blocks of a new player there.
        let fresh = Player::new(&s, k);
        for (a, b) in p.mesh().blocks().iter().zip(fresh.mesh().blocks()) {
            assert!(**a == **b, "at {k}");
        }
    }
}

#[test]
fn a_small_step_rebuilds_few_blocks() {
    // A wide stock: one move of the raster changes the blocks along it.
    let r = raster(1, 8.0);
    let s = Simulation::run(
        block(60.0, 40.0, 10.0),
        &[program("t1", &r, Tool::flat(1.0))],
        &settings(0.25),
        None,
        &Quiet,
    )
    .unwrap();
    let mut p = Player::new(&s, 0);
    let total = p.mesh().blocks().len();
    let changed = p.seek(&s, s.chords_at(s.step(0.0, Step::Move, true)));
    assert!(
        !changed.is_empty() && changed.len() * 3 < total,
        "{changed:?} of {total}"
    );
}

#[test]
fn steps_by_move_pass_and_toolpath() {
    let s = sim(None);
    let end = |tp: usize, seg: usize| {
        s.chords
            .iter()
            .rev()
            .find(|c| c.toolpath as usize == tp && c.segment as usize == seg)
            .unwrap()
            .end
    };
    // t1: 5 passes of 3 moves; t2: 1 move.
    assert_eq!(s.toolpaths[0].passes, [0, 3, 6, 9, 12]);
    assert_eq!(s.step(0.0, Step::Move, true), end(0, 0));
    assert_eq!(s.step(end(0, 0), Step::Move, true), end(0, 1));
    assert_eq!(s.step(0.0, Step::Pass, true), end(0, 2));
    assert_eq!(s.step(end(0, 2), Step::Pass, true), end(0, 5));
    assert_eq!(s.step(0.0, Step::Toolpath, true), s.toolpaths[0].end);
    // Back from the middle of a move: to the end of the move before.
    let mid = 0.5 * (end(0, 3) + end(0, 4));
    assert_eq!(s.step(mid, Step::Move, false), end(0, 3));
    assert_eq!(s.step(end(0, 3), Step::Move, false), end(0, 2));
    assert_eq!(s.step(mid, Step::Pass, false), end(0, 2));
    assert_eq!(s.step(end(0, 2), Step::Pass, false), 0.0);
    assert_eq!(
        s.step(s.duration(), Step::Toolpath, false),
        s.toolpaths[0].end
    );
    // At the ends, nowhere further.
    assert_eq!(s.step(s.duration(), Step::Move, true), s.duration());
    assert_eq!(s.step(0.0, Step::Move, false), 0.0);
    assert_eq!(s.step(s.duration(), Step::Move, false), end(0, 14));
}

/// A part with a flat top at 7 over x ≤ 15.
struct Shelf;

impl Part for Shelf {
    fn top(&self, x: f64, _: f64) -> Option<f64> {
        (x <= 15.0).then_some(7.0)
    }
}

#[test]
fn rest_classes_against_the_part() {
    // t1 leaves 1 mm on the part (z 8); t2 then cuts a slot to 6 at y = 5:
    // a gouge of 1 mm.
    let s = sim(Some(&Shelf));
    let part = s.part.as_ref().unwrap();
    assert_eq!(part.top.len(), s.stock_final().grids[2].ray_count());
    let at = |s: &Simulation, n: usize, x: f64, y: f64| {
        let d = s.stock_after(n);
        let paint = SimPaint::new(s, &d, n);
        let (i, j) = ((x / 0.25) as usize, (y / 0.25) as usize);
        paint.column(i, j)
    };
    let n = s.chords.len();
    let t1 = s.toolpaths[0].chords.end;
    assert_eq!(at(&s, n, 5.0, 2.0).class, REST);
    assert_eq!(at(&s, n, 5.0, 5.0).class, GOUGE);
    assert_eq!(at(&s, n, 18.0, 2.0).class, OFF);
    // Before t2 the slot is rest.
    assert_eq!(at(&s, t1, 5.0, 5.0).class, REST);
    // The toolpath that last cut each column.
    assert_eq!(at(&s, n, 5.0, 5.0).toolpath, Some(1));
    assert_eq!(at(&s, n, 5.0, 2.0).toolpath, Some(0));
    assert_eq!(at(&s, t1, 5.0, 5.0).toolpath, Some(0));
    assert_eq!(at(&s, 0, 5.0, 5.0).toolpath, None);
    // A part whose top the stock meets is within.
    struct Flush;
    impl Part for Flush {
        fn top(&self, _: f64, _: f64) -> Option<f64> {
            Some(8.0)
        }
    }
    let s2 = sim(Some(&Flush));
    assert_eq!(at(&s2, s2.toolpaths[0].chords.end, 5.0, 3.0).class, WITHIN);
    // The tags reach the mesh.
    let p = Player::new(&s, n);
    let m = p.mesh().joined();
    for c in [REST, GOUGE] {
        assert!(m.tags.iter().any(|&t| tag_class(t) == c), "{c}");
    }
    assert!(m.tags.iter().any(|&t| tag_toolpath(t) == Some(1)));
}
