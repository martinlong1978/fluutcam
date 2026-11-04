// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::mesh::{GOUGE, REST, WITHIN, tag_class, tag_toolpath};
use crate::run::tests::{block, feed, path, program, settings, xyz};
use crate::run::{Quiet, Settings, Simulation, Step};
use crate::tool::Tool;
use crate::verify::Part;
use fluut_path::{Pass, PassKind, ToolpathResult};
use glam::DVec3;

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

/// A fixed pseudo-random sequence (no `rand`: the tests stay reproducible).
fn lcg(state: &mut u64) -> usize {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*state >> 33) as usize
}

/// The stock after `n` chords cut forward from the start, with no
/// checkpoint and no snapshot.
fn from_the_start(s: &Simulation, n: usize) -> TriDexel {
    let mut d = s.stock_initial().clone();
    s.cut(&mut d, 0..n);
    d
}

/// Many short moves: 40 rows of three moves, then a pass across.
fn long_sim() -> Simulation {
    let a = raster(40, 8.0);
    let b = path(&[xyz(-3.0, 5.0, 6.0), xyz(23.0, 5.0, 6.0)]);
    Simulation::run(
        block(20.0, 90.0, 10.0),
        &[
            program("t1", &a, Tool::flat(1.0)),
            program("t2", &b, Tool::flat(1.0)),
        ],
        &settings(0.5),
        Some(&Shelf),
        &Quiet,
    )
    .unwrap()
}

#[test]
fn seeks_equal_a_fresh_forward_cut() {
    let s = long_sim();
    let n = s.chords.len();
    let limits = [
        Limits::default(),
        Limits {
            stride: 7,
            budget: usize::MAX,
        },
        Limits {
            stride: 64,
            budget: 1 << 20,
        },
        // So small that snapshots are evicted all the time.
        Limits {
            stride: 5,
            budget: 40 << 10,
        },
        // Too small for any.
        Limits {
            stride: 5,
            budget: 16,
        },
    ];
    for (k, lim) in limits.into_iter().enumerate() {
        let mut state = 17 + k as u64;
        let mut p = Player::with_limits(&s, n / 2, lim);
        let mut at = n / 2;
        for i in 0..60 {
            let r = lcg(&mut state);
            at = match i % 4 {
                // Anywhere.
                0 => r % (n + 1),
                // A few chords, forward or back.
                1 => (at + r % 40).saturating_sub(20).min(n),
                // A run of back steps.
                2 => at.saturating_sub(1 + r % 30),
                _ => (at + 1 + r % 200).min(n),
            };
            p.seek(&s, at);
            assert_eq!(p.chords(), at);
            assert_eq!(p.stock().hash(), from_the_start(&s, at).hash(), "{k}: {at}");
            let fresh = Player::new(&s, at);
            for (a, b) in p.mesh().blocks().iter().zip(fresh.mesh().blocks()) {
                assert!(**a == **b, "{k}: blocks at {at}");
            }
            assert!(p.snapshot_bytes() <= lim.budget, "{k}: {at}");
        }
    }
}

#[test]
fn a_step_back_cuts_at_most_a_stride() {
    let s = long_sim();
    let n = s.chords.len();
    let stride = 16;
    assert!(n > 4 * stride, "{n}");
    let mut p = Player::with_limits(
        &s,
        0,
        Limits {
            stride,
            budget: usize::MAX,
        },
    );
    // Jump to the end (the final checkpoint: nothing is cut, no snapshots),
    // then step back one chord at a time. The first step back cuts from the
    // checkpoint and leaves snapshots in the last FINE strides; the steps
    // back inside them cut a stride at most.
    p.seek(&s, n);
    let toolpath = s.toolpaths[1].chords.start;
    p.seek(&s, toolpath - 1);
    assert_eq!(p.last_cut(), toolpath - 1);
    let zone = (toolpath - 1 - FINE * stride).div_ceil(stride) * stride;
    assert!(zone > 0);
    for k in (zone..toolpath - 1).rev() {
        p.seek(&s, k);
        assert!(p.last_cut() < stride, "at {k}: {}", p.last_cut());
    }
    // Below them it cuts from the checkpoint (and leaves snapshots again).
    p.seek(&s, zone - 1);
    assert_eq!(p.last_cut(), zone - 1);
    p.seek(&s, zone - 2);
    assert!(p.last_cut() < stride);
    // A forward step of one cuts one.
    p.seek(&s, 1);
    assert_eq!(p.last_cut(), 1);
}

#[test]
fn snapshots_share_tiles_and_are_bounded() {
    let s = long_sim();
    let n = s.toolpaths[1].chords.start;
    let mut p = Player::with_limits(
        &s,
        0,
        Limits {
            stride: 4,
            budget: usize::MAX,
        },
    );
    for t in (0..n).step_by(12) {
        p.seek(&s, t);
    }
    let all = p.snapshot_bytes();
    assert!(all > 0);
    // The stock's own size, for scale: every snapshot would hold a whole
    // copy without sharing.
    let one = s.stock_initial().memory();
    let one = one.rays * std::mem::size_of::<crate::dexel::Spans>();
    let count = n / 4;
    assert!(count > 20);
    assert!(
        all < count * one / 2,
        "{all} bytes for {count} snapshots of {one}"
    );
    // A budget evicts down to it.
    let mut q = Player::with_limits(
        &s,
        0,
        Limits {
            stride: 4,
            budget: all / 4,
        },
    );
    for t in (0..n).step_by(12) {
        q.seek(&s, t);
    }
    assert!(q.snapshot_bytes() <= all / 4);
    assert!(q.snapshot_bytes() > 0);
}

// ---- Rest classes by the true distance to the part (D11 §10 "Paint") ----

/// A part with a wall: solid where `x ≤ 10` and `z ≤ 8`, and below `z = 3`.
/// With `exact`, it answers distance queries.
#[derive(Debug)]
struct Wall {
    exact: bool,
}

impl Part for Wall {
    fn top(&self, x: f64, _: f64) -> Option<f64> {
        Some(if x <= 10.0 { 8.0 } else { 3.0 })
    }

    fn clearance(&self) -> Option<std::sync::Arc<dyn crate::verify::Clearance>> {
        self.exact
            .then(|| std::sync::Arc::new(Wall { exact: true }) as _)
    }
}

impl crate::verify::Clearance for Wall {
    fn clear_height(&self, x: f64, _: f64, r: f64) -> Option<f64> {
        let beside = x - 10.0;
        Some(if beside <= 0.0 {
            8.0 + r
        } else if beside < r {
            // The ball rests on the wall's top edge.
            8.0 + (r * r - beside * beside).sqrt()
        } else {
            3.0 + r
        })
    }
}

/// A block cut down to the wall's floor (z = 3) beyond a skin of stock
/// `skin` thick against the wall, tolerance 0.2 mm.
fn walled(skin: f64, part: &Wall) -> Simulation {
    let r = path(&[xyz(15.0 + skin, -6.0, 3.0), xyz(15.0 + skin, 16.0, 3.0)]);
    Simulation::run(
        block(20.0, 10.0, 8.0),
        &[program("t1", &r, Tool::flat(5.0))],
        &Settings {
            tol: 0.2,
            ..settings(0.25)
        },
        Some(part),
        &Quiet,
    )
    .unwrap()
}

/// The classes of the vertices on the outer face of the skin, away from its
/// ends.
fn skin_classes(s: &Simulation, skin: f64) -> Vec<u8> {
    let m = Player::new(s, s.chords.len()).mesh().joined();
    m.positions
        .iter()
        .zip(&m.tags)
        .filter(|(p, _)| {
            (f64::from(p[0]) - 10.0 - skin).abs() < 1e-3
                && (3.3..7.7).contains(&p[2])
                && (2.0..8.0).contains(&p[1])
        })
        .map(|(_, &t)| tag_class(t))
        .collect()
}

#[test]
fn a_thin_skin_against_a_wall_is_within() {
    // 0.14 mm of stock covers the Z ray beside the wall, which the vertical
    // distance reads as 5 mm of rest; by distance it is under the 0.2 mm
    // tolerance.
    let by_distance = skin_classes(&walled(0.14, &Wall { exact: true }), 0.14);
    assert!(by_distance.len() > 20);
    assert!(by_distance.iter().all(|&c| c == WITHIN), "{by_distance:?}");
    // Without a distance query the part keeps the vertical method.
    let vertical = skin_classes(&walled(0.14, &Wall { exact: false }), 0.14);
    assert!(vertical.iter().all(|&c| c == REST), "{vertical:?}");
}

#[test]
fn a_thick_skin_against_a_wall_is_rest() {
    let by_distance = skin_classes(&walled(0.3, &Wall { exact: true }), 0.3);
    assert!(by_distance.len() > 20);
    assert!(by_distance.iter().all(|&c| c == REST), "{by_distance:?}");
}

/// A part with a flat top at 7 over x ≤ 15 that answers distance queries.
#[derive(Debug)]
struct ShelfExact;

impl Part for ShelfExact {
    fn top(&self, x: f64, y: f64) -> Option<f64> {
        Shelf.top(x, y)
    }

    fn clearance(&self) -> Option<std::sync::Arc<dyn crate::verify::Clearance>> {
        Some(std::sync::Arc::new(ShelfExact))
    }
}

impl crate::verify::Clearance for ShelfExact {
    fn clear_height(&self, x: f64, _: f64, r: f64) -> Option<f64> {
        let beside = x - 15.0;
        if beside <= 0.0 {
            Some(7.0 + r)
        } else if beside < r {
            Some(7.0 + (r * r - beside * beside).sqrt())
        } else {
            None
        }
    }
}

#[test]
fn a_flat_floor_keeps_its_class() {
    // t1 leaves 1 mm of rest over the shelf at z = 8.
    let floor = |part: &dyn Part| -> Vec<u8> {
        let s = sim(Some(part));
        let m = Player::new(&s, s.toolpaths[0].chords.end).mesh().joined();
        m.positions
            .iter()
            .zip(&m.tags)
            .filter(|(p, _)| {
                (f64::from(p[2]) - 8.0).abs() < 1e-3
                    && (3.0..14.0).contains(&p[0])
                    && (2.0..8.0).contains(&p[1])
            })
            .map(|(_, &t)| tag_class(t))
            .collect()
    };
    let (vertical, exact) = (floor(&Shelf), floor(&ShelfExact));
    assert!(vertical.len() > 100);
    assert_eq!(vertical, exact);
    assert!(exact.iter().all(|&c| c == REST));
}

/// A part whose top rises 1:1, solid below it, with exact distances.
#[derive(Debug)]
struct Slope;

impl Part for Slope {
    fn top(&self, x: f64, _: f64) -> Option<f64> {
        Some(x)
    }

    fn clearance(&self) -> Option<std::sync::Arc<dyn crate::verify::Clearance>> {
        Some(std::sync::Arc::new(Slope))
    }
}

impl crate::verify::Clearance for Slope {
    fn clear_height(&self, x: f64, _: f64, r: f64) -> Option<f64> {
        Some(x + r * std::f64::consts::SQRT_2)
    }
}

#[test]
fn a_wall_at_45_degrees_is_judged_by_its_perpendicular_distance() {
    // Tolerance 0.2: a vertex 0.25 above the slope is rest vertically, but
    // 0.25 / √2 = 0.177 from it; 0.3 above is 0.212 from it.
    let r = path(&[xyz(1.0, 5.0, 9.0), xyz(2.0, 5.0, 9.0)]);
    let s = Simulation::run(
        block(20.0, 10.0, 10.0),
        &[program("t1", &r, Tool::flat(1.0))],
        &Settings {
            tol: 0.2,
            ..settings(0.25)
        },
        Some(&Slope),
        &Quiet,
    )
    .unwrap();
    let d = s.stock_final().clone();
    let paint = SimPaint::new(&s, &d, s.chords.len());
    let at = |gap: f64| paint.vertex(DVec3::new(5.0, 5.0, 5.0 + gap), REST);
    assert_eq!(at(0.25), WITHIN);
    assert_eq!(at(0.3), REST);
    assert_eq!(at(2.0), REST);
    // Under the slope (inside the part) is never rest: the stock's bottom.
    assert_eq!(paint.vertex(DVec3::new(5.0, 5.0, 2.0), REST), WITHIN);
    // Gouge and within are kept as they are.
    assert_eq!(paint.vertex(DVec3::new(5.0, 5.0, 5.0), WITHIN), WITHIN);
    assert_eq!(paint.vertex(DVec3::new(5.0, 5.0, 4.0), GOUGE), GOUGE);
}

#[test]
fn rest_colours_do_not_depend_on_the_thread_count() {
    let on = |threads: usize| {
        // A simulation each: its cache of answers starts empty.
        let s = walled(0.14, &Wall { exact: true });
        let n = s.chords.len();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| Player::new(&s, n).mesh().joined())
    };
    let (a, b, c) = (on(1), on(3), on(8));
    assert!(a.tags.iter().any(|&t| tag_class(t) == WITHIN));
    assert!(a == b && a == c);
}
