// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::{Chorder, Stock, Tool, cut};
use fluut_geom::Point3;
use fluut_path::{AxisPos, FeedRef, Motion, MotionInfo, Segment};

fn block() -> Stock {
    Stock::Box {
        min: Point3::new(0.0, 0.0, 0.0),
        max: Point3::new(10.0, 6.0, 4.0),
    }
}

fn line(a: (f64, f64, f64), b: (f64, f64, f64)) -> Segment {
    Segment::Line {
        a: AxisPos::xyz(Point3::new(a.0, a.1, a.2)),
        b: AxisPos::xyz(Point3::new(b.0, b.1, b.2)),
        m: MotionInfo::new(Motion::Cut, FeedRef::Cut, 0.01),
    }
}

fn slot(d: &mut TriDexel, s: Stock, tool: Tool, a: (f64, f64, f64), b: (f64, f64, f64)) {
    let chords = Chorder::new(tool, s, false, d.res).chords(&[line(a, b)]);
    cut(d, &chords);
}

#[test]
fn ranges_and_differences() {
    let s: Spans = [crate::Span { lo: 0.0, hi: 1.0 }].into_iter().collect();
    assert_eq!(ranges(&s, 0.25).as_slice(), &[(0, 3)]);
    let mut out = Vec::new();
    differ(&[(0, 9)], &[(0, 3), (6, 9)], &mut out);
    assert_eq!(out, [(4, 5)]);
    differ(&[], &[(2, 4)], &mut out);
    assert_eq!(out, [(2, 4)]);
    differ(&[(1, 2)], &[(1, 2)], &mut out);
    assert!(out.is_empty());
}

#[test]
fn changed_extent_of_two_rays() {
    let s =
        |v: &[(f32, f32)]| -> Spans { v.iter().map(|&(lo, hi)| crate::Span { lo, hi }).collect() };
    assert_eq!(changed(&s(&[(0.0, 9.0)]), &s(&[(0.0, 9.0)])), None);
    assert_eq!(
        changed(&s(&[(0.0, 9.0)]), &s(&[(0.0, 2.0), (3.0, 9.0)])),
        Some((2.0, 3.0))
    );
    assert_eq!(
        changed(&s(&[(0.0, 9.0)]), &s(&[(0.0, 5.0)])),
        Some((5.0, 9.0))
    );
    assert_eq!(changed(&s(&[(1.0, 9.0)]), &s(&[])), Some((1.0, 9.0)));
}

#[test]
fn block_is_closed_with_its_volume() {
    let d = TriDexel::new(&block(), 0.5);
    let m = surface(&d);
    assert!(m.triangle_count() > 0);
    let v = m.volume();
    assert!((v - 240.0).abs() / 240.0 < 0.05, "{v}");
    // Vertices on the faces lie on the faces.
    for p in &m.positions {
        let inside = (0..3).all(|k| p[k] >= -1e-6 && p[k] <= [10.0, 6.0, 4.0][k] + 1e-6);
        assert!(inside, "{p:?}");
    }
    // Shared vertices: far fewer than three per triangle.
    assert!(m.positions.len() < m.triangle_count());
    // Every edge of the joined mesh is used twice, once each way, within a
    // block; the seams between blocks share positions, not indices.
    assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
}

#[test]
fn closed_across_many_blocks() {
    // A stock wider than several blocks: the blocks' triangles still close
    // up, so the volume is right.
    let s = Stock::Box {
        min: Point3::new(0.0, 0.0, 0.0),
        max: Point3::new(30.0, 20.0, 3.0),
    };
    let d = TriDexel::new(&s, 0.25);
    let t = MeshTiles::new(&d, &NoPaint);
    assert!(t.blocks().len() > 4);
    let v = surface(&d).volume();
    assert!((v - 1800.0).abs() / 1800.0 < 0.02, "{v}");
}

#[test]
fn slot_lowers_the_volume() {
    let s = block();
    let mut d = TriDexel::new(&s, 0.25);
    let before = surface(&d).volume();
    slot(
        &mut d,
        s,
        Tool::flat(1.0),
        (-5.0, 3.0, 2.0),
        (15.0, 3.0, 2.0),
    );
    let after = surface(&d).volume();
    // The slot is 2 wide, 2 deep and 10 long.
    assert!(((before - after) - 40.0).abs() < 2.0, "{before} → {after}");
}

fn wide() -> Stock {
    Stock::Box {
        min: Point3::new(0.0, 0.0, 0.0),
        max: Point3::new(40.0, 30.0, 6.0),
    }
}

/// The blocks of `t` and of a new `MeshTiles` over `d` are the same.
fn same_as_new(t: &MeshTiles, d: &TriDexel) {
    let fresh = MeshTiles::new(d, &NoPaint);
    assert_eq!(t.blocks().len(), fresh.blocks().len());
    for (k, (a, b)) in t.blocks().iter().zip(fresh.blocks()).enumerate() {
        assert!(**a == **b, "block {k} differs");
    }
}

#[test]
fn a_cut_marks_only_the_blocks_it_reaches() {
    let s = wide();
    let d0 = TriDexel::new(&s, 0.25);
    let mut t = MeshTiles::new(&d0, &NoPaint);
    let before: Vec<_> = t.blocks().to_vec();
    // A short slot near the minimum corner.
    let mut d = d0.clone();
    slot(&mut d, s, Tool::flat(1.0), (2.0, 2.0, 5.0), (4.0, 2.0, 5.0));
    let changed = t.update(&d, &NoPaint);
    assert!(!changed.is_empty());
    assert!(changed.len() <= 4, "{changed:?}");
    for (k, (a, b)) in before.iter().zip(t.blocks()).enumerate() {
        assert_eq!(Arc::ptr_eq(a, b), !changed.contains(&k), "block {k}");
    }
    same_as_new(&t, &d);
    // Nothing changed: nothing rebuilt.
    assert!(t.update(&d.clone(), &NoPaint).is_empty());
    // Back to the uncut stock: the same blocks again.
    let back = t.update(&d0, &NoPaint);
    assert_eq!(back, changed);
    same_as_new(&t, &d0);
}

#[test]
fn incremental_equals_full_after_scrubs() {
    // Cuts in all directions, then a scrub back and forth through them.
    let s = wide();
    let tool = Tool::ball(1.5);
    let moves = [
        ((-2.0, 5.0, 4.0), (42.0, 7.0, 3.0)),
        ((10.0, -2.0, 5.0), (12.0, 32.0, 2.5)),
        ((30.0, 25.0, 1.0), (31.0, 25.5, 1.0)),
        ((0.0, 30.0, 4.5), (40.0, 0.0, 4.0)),
        ((20.0, 15.0, 7.0), (20.0, 15.0, -1.0)),
    ];
    let res = 0.3;
    let mut stocks = vec![TriDexel::new(&s, res)];
    for &(a, b) in &moves {
        let mut d = stocks.last().unwrap().clone();
        slot(&mut d, s, tool, a, b);
        stocks.push(d);
    }
    let mut t = MeshTiles::new(&stocks[0], &NoPaint);
    for &k in &[3usize, 1, 5, 5, 0, 2, 4, 1, 5] {
        t.update(&stocks[k], &NoPaint);
        same_as_new(&t, &stocks[k]);
    }
}

#[test]
fn same_blocks_on_any_thread_count() {
    let s = wide();
    let mut d = TriDexel::new(&s, 0.3);
    slot(
        &mut d,
        s,
        Tool::ball(2.0),
        (-2.0, 3.0, 4.0),
        (42.0, 27.0, 3.0),
    );
    let on = |n: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .unwrap();
        pool.install(|| MeshTiles::new(&d, &NoPaint))
    };
    let (a, b) = (on(1), on(4));
    for (x, y) in a.blocks().iter().zip(b.blocks()) {
        assert!(**x == **y);
    }
}

/// Paints every column by its index parity, to see tags reach vertices.
struct Parity;

impl Paint for Parity {
    fn column(&self, i: usize, j: usize) -> Column {
        Column {
            class: if (i + j).is_multiple_of(2) {
                REST
            } else {
                WITHIN
            },
            toolpath: Some(3),
        }
    }
}

#[test]
fn tags_combine_the_columns_around_a_vertex() {
    let d = TriDexel::new(&block(), 0.5);
    let m = MeshTiles::new(&d, &Parity).joined();
    // Every vertex has columns of both parities around it except at the
    // stock's edge, so the classes seen are rest; all name toolpath 3.
    assert!(m.tags.iter().all(|&t| tag_toolpath(t) == Some(3)));
    assert!(m.tags.iter().any(|&t| tag_class(t) == REST));
    assert!(
        surface(&d)
            .tags
            .iter()
            .all(|&t| tag_class(t) == OFF && tag_toolpath(t).is_none())
    );
    // The combination rules.
    let c = |class, toolpath| Column { class, toolpath };
    assert_eq!(combine(&[c(OFF, None); 4]), (OFF, None));
    assert_eq!(
        combine(&[
            c(GOUGE, Some(1)),
            c(GOUGE, Some(0)),
            c(OFF, None),
            c(GOUGE, None)
        ]),
        (GOUGE, Some(1))
    );
    assert_eq!(
        combine(&[c(GOUGE, None), c(WITHIN, None), c(REST, None), c(OFF, None)]),
        (REST, None)
    );
    assert_eq!(
        combine(&[
            c(GOUGE, None),
            c(WITHIN, None),
            c(WITHIN, None),
            c(OFF, None)
        ]),
        (WITHIN, None)
    );
}

#[test]
fn small_cuts_at_the_seams() {
    // Plunges of a tool narrower than two rays at every offset around the
    // seams between blocks, one at a time: each update equals a new mesh.
    // Every other plunge stays above the top lattice point (5.85), so only
    // Z rays change.
    let s = wide();
    let res = 0.3;
    let seam = |n: i64| (n as f64 + 0.5) * res;
    let mut d = TriDexel::new(&s, res);
    let mut t = MeshTiles::new(&d, &NoPaint);
    let b = BLOCK as i64;
    for (k, off) in (-4..=4).enumerate() {
        let x = seam(b - 1 + off);
        let y = seam(2 * b - 1 - off);
        let z = if k % 2 == 0 {
            5.9
        } else {
            5.0 - 0.4 * k as f64
        };
        slot(&mut d, s, Tool::flat(0.2), (x, 10.0, 7.0), (x, 10.0, z));
        slot(&mut d, s, Tool::flat(0.2), (15.0, y, 7.0), (15.0, y, z));
        slot(&mut d, s, Tool::flat(0.2), (x, y, 7.0), (x, y, z));
        t.update(&d, &NoPaint);
        same_as_new(&t, &d);
    }
}

#[test]
fn a_wall_shaved_between_rays() {
    // A slot along Y, then the same slot 0.05 further: the right wall moves
    // between two Z rays, so only X rays change. Walls at every offset
    // around a seam.
    let s = wide();
    let res = 0.3;
    let b = BLOCK as i64;
    for off in -4..=3 {
        let wall = (b - 1 + off) as f64 * res + 0.6 * res;
        let mut d = TriDexel::new(&s, res);
        let x = wall - 1.0;
        slot(&mut d, s, Tool::flat(1.0), (x, -2.0, 3.0), (x, 32.0, 3.0));
        let mut t = MeshTiles::new(&d, &NoPaint);
        let z = d.grids[2].clone();
        slot(
            &mut d,
            s,
            Tool::flat(1.0),
            (x + 0.05, -2.0, 3.0),
            (x + 0.05, 32.0, 3.0),
        );
        assert!(d.grids[2].rays().eq(z.rays()), "a Z ray changed");
        t.update(&d, &NoPaint);
        same_as_new(&t, &d);
    }
}
