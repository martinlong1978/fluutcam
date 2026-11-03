// SPDX-License-Identifier: MIT OR Apache-2.0

//! The 3-D finishing linker over the in-process stock (D08 §8.2): lifts and
//! descents stop `safety_distance` above the highest material within the
//! tool's reach, never lower than any material the tool passes over, and
//! nothing changes without the stock.

mod support;

use fluut_cam::geometry::SetupGeometry;
use fluut_cam::region::Region2;
use fluut_cam::stock::{LayeredStock, Sweep};
use fluut_cam::{Generated, calculate};
use fluut_geom::{Aabb3, Point2, Point3};
use fluut_model::tools::Tool;
use fluut_path::Motion;
use support::finish::*;

/// The dome block is 80 × 60 and 10 high, its boss reaches Z 28; the
/// stock top is 29. The tool has radius 3 and the safety distance is 1.
const R: f64 = 3.0;
const SAFETY: f64 = 1.0;

fn full_stock() -> LayeredStock {
    LayeredStock::planar(&Aabb3 {
        min: Point3::new(0.0, 0.0, 0.0),
        max: Point3::new(80.0, 60.0, 29.0),
    })
}

/// The block roughed down to Z 10 round the boss (radius 12 + 1 left),
/// with `islands` of the stock left standing.
fn roughed(islands: &[Region2]) -> LayeredStock {
    let mut cleared =
        Region2::rect(-10.0, -10.0, 90.0, 70.0).subtract(&Region2::circle(40.0, 30.0, 13.0));
    for i in islands {
        cleared = cleared.subtract(i);
    }
    let mut s = full_stock();
    s.remove_swept(10.0, &cleared);
    s
}

fn finishing(g: &SetupGeometry, tl: &Tool, stock: Option<&LayeredStock>) -> Generated {
    // One way, so each link crosses the part from the end of a pass back to
    // the start of the next and lifts.
    let t = tp(
        "parallel",
        "b6",
        "[passes]\nstepover = 4\ndirection = \"climb\"\n",
    );
    let mut i = input(g, &t, tl);
    i.in_process = stock;
    calculate(&i).unwrap_or_else(|e| panic!("{e}"))
}

/// Horizontal rapids: both ends and the Z.
fn flat_rapids(r: &Generated) -> Vec<(Point3, Point3)> {
    r.result
        .segments()
        .filter(|s| s.motion().is_some_and(|m| m.motion == Motion::Rapid))
        .filter_map(|s| Some((s.start()?.xyz, s.end()?.xyz)))
        .filter(|(a, b)| (a.z() - b.z()).abs() < 1e-9 && (a.x() != b.x() || a.y() != b.y()))
        .collect()
}

fn cuts(r: &Generated) -> Vec<(Point3, Point3)> {
    r.result
        .segments()
        .filter(|s| s.motion().is_some_and(|m| m.motion == Motion::Cut))
        .filter_map(|s| Some((s.start()?.xyz, s.end()?.xyz)))
        .collect()
}

/// The lowest Z the tool needs above the material along `a`–`b`, by
/// dense point queries (the reference for the conservative answer).
fn needed(stock: &LayeredStock, a: Point3, b: Point3) -> Option<f64> {
    let n = 400;
    (0..=n)
        .filter_map(|i| {
            let p = a.lerp(b, i as f64 / n as f64);
            stock.top_near(Point2::new(p.x(), p.y()), R)
        })
        .reduce(f64::max)
        .map(|t| t + SAFETY)
}

#[test]
fn without_the_stock_rapids_stay_above_the_stock_top() {
    let g = dome();
    let tl = tool("b6");
    let r = finishing(&g, &tl, None);
    let rapids = flat_rapids(&r);
    assert!(rapids.len() > 5);
    for (a, _) in rapids {
        // Stock top 29 + safety 1.
        assert!(a.z() >= 30.0 - 1e-9, "{a:?}");
    }
}

#[test]
fn the_full_stock_changes_no_cut_and_rapids_stay_above_the_stock() {
    let g = dome();
    let tl = tool("b6");
    let plain = finishing(&g, &tl, None);
    let stock = full_stock();
    let with = finishing(&g, &tl, Some(&stock));
    assert_eq!(cuts(&plain), cuts(&with));
    for (a, b) in flat_rapids(&with) {
        if let Some(need) = needed(&stock, a, b) {
            assert!(a.z() >= need - 1e-9, "{a:?} {b:?} need {need}");
        }
    }
}

#[test]
fn after_roughing_lifts_over_the_cleared_block_are_lower() {
    let g = dome();
    let tl = tool("b6");
    let plain = finishing(&g, &tl, None);
    let stock = roughed(&[]);
    let with = finishing(&g, &tl, Some(&stock));
    // Finishing cuts the same surface.
    assert_eq!(cuts(&plain), cuts(&with));
    let top = |r: &Generated| {
        flat_rapids(r)
            .iter()
            .map(|(a, _)| a.z())
            .fold(f64::MIN, f64::max)
    };
    let low = |r: &Generated| {
        flat_rapids(r)
            .iter()
            .map(|(a, _)| a.z())
            .fold(f64::MAX, f64::min)
    };
    assert!(low(&plain) >= 30.0 - 1e-9);
    // Over the roughed block the lift is the block (Z 10) + safety, or the
    // model bound above it: well below the stock top.
    assert!(low(&with) < 20.0, "lowest lift {}", low(&with));
    assert!(top(&with) <= top(&plain) + 1e-9);
    // The air travel of the lifts shortens.
    let air =
        |r: &Generated| -> f64 { flat_rapids(r).iter().map(|(a, b)| (*b - *a).length()).sum() };
    assert!((air(&with) - air(&plain)).abs() < 1e-6, "same moves, lower");
    // Never lower than the material under the rapid.
    for (a, b) in flat_rapids(&with) {
        if let Some(need) = needed(&stock, a, b) {
            assert!(a.z() >= need - 1e-9, "{a:?} {b:?} need {need}");
        }
    }
}

#[test]
fn a_link_crossing_an_unroughed_island_clears_it() {
    let g = dome();
    let tl = tool("b6");
    // Two thin posts of stock left standing at the side of the block, which
    // the links between passes at the part's edge cross.
    let islands = [
        Region2::rect(-0.4, 9.0, 0.4, 15.0),
        Region2::rect(79.6, 30.0, 80.4, 38.0),
    ];
    let stock = roughed(&islands);
    let with = finishing(&g, &tl, Some(&stock));
    let rapids = flat_rapids(&with);
    // Within the tool radius of the first post, the rapid is above its top.
    let near_post = |p: Point3| p.x() < R + 0.4 && p.y() > 9.0 - R && p.y() < 15.0 + R;
    let mut over_island = 0;
    for (a, b) in &rapids {
        let need = needed(&stock, *a, *b);
        if let Some(need) = need {
            assert!(a.z() >= need - 1e-9, "{a:?} {b:?} need {need}");
        }
        if (0..=100).any(|i| near_post(a.lerp(*b, i as f64 / 100.0))) {
            assert!(a.z() >= 29.0 + SAFETY - 1e-9, "{a:?} {b:?}");
            over_island += 1;
        }
    }
    assert!(over_island > 0, "no link crossed an island: {rapids:?}");
    let lower = rapids.iter().filter(|(a, _)| a.z() < 20.0).count();
    assert!(lower > 0, "links away from the islands stay low");
}

#[test]
fn descents_stop_above_the_material_within_reach() {
    let g = dome();
    let tl = tool("b6");
    // Only the block and the foot of the boss are cut (top 12, feed 14), with
    // full retracts: the rapids down to an entry go to the feed height where
    // the roughed block allows, not to the stock top + 1.
    let t = tp(
        "parallel",
        "b6",
        "[passes]\nstepover = 4\ndirection = \"climb\"\n\
         [heights]\ntop = { ref = \"model_top\", offset = -16 }\n\
         [linking]\nretract = \"full\"\n",
    );
    let stock = roughed(&[Region2::rect(-0.4, 9.0, 0.4, 15.0)]);
    let mut i = input(&g, &t, &tl);
    let plain = calculate(&i).unwrap();
    i.in_process = Some(&stock);
    let with = calculate(&i).unwrap();
    assert_eq!(cuts(&plain), cuts(&with));
    let lowest_descent = |r: &Generated| {
        r.result
            .segments()
            .filter(|s| s.motion().is_some_and(|m| m.motion == Motion::Rapid))
            .filter_map(|s| Some((s.start()?.xyz, s.end()?.xyz)))
            .filter(|(a, b)| a.x() == b.x() && a.y() == b.y() && b.z() < a.z())
            .map(|(_, b)| b.z())
            .fold(f64::MAX, f64::min)
    };
    // The plain floor is the stock top + safety.
    assert!(lowest_descent(&plain) >= 30.0 - 1e-9);
    assert!(lowest_descent(&with) < lowest_descent(&plain) - 5.0);
    for s in with.result.segments() {
        let (Some(m), Some(a), Some(b)) = (s.motion(), s.start(), s.end()) else {
            continue;
        };
        if m.motion == Motion::Rapid && a.xyz.x() == b.xyz.x() && a.xyz.y() == b.xyz.y() {
            let p = Point2::new(b.xyz.x(), b.xyz.y());
            if let Some(t) = stock.top_near(p, R) {
                assert!(b.xyz.z() >= t + SAFETY - 1e-9, "{b:?} over {t}");
            }
        }
    }
}

#[test]
fn finishing_after_a_real_roughing_lifts_lower() {
    // The dome roughed with a 3-D pocket (Ø6 flat, 0.5 mm
    // left), then finished with `parallel` over the stock it leaves.
    let g = dome();
    let flat = tool("f6");
    let ball = tool("b6");
    let rough = tp(
        "pocket_3d",
        "f6",
        "[passes]\nstep_down = 4\nstock_to_leave = 0.5\n",
    );
    let block = Aabb3 {
        min: Point3::new(-5.0, -5.0, 0.0),
        max: Point3::new(85.0, 65.0, 29.0),
    };
    let mut stock = LayeredStock::planar(&block);
    let mut i = input(&g, &rough, &flat);
    i.stock = Some(block);
    i.in_process = Some(&stock);
    let r = calculate(&i).unwrap_or_else(|e| panic!("{e}"));
    stock.apply(&r.result, Sweep::of(&flat), 0.01);
    let fin = tp(
        "parallel",
        "b6",
        "[passes]\nstepover = 4\ndirection = \"climb\"\n",
    );
    let mut i = input(&g, &fin, &ball);
    let plain = calculate(&i).unwrap();
    i.in_process = Some(&stock);
    let with = calculate(&i).unwrap();
    assert_eq!(cuts(&plain), cuts(&with));
    let height = |r: &Generated| -> f64 {
        flat_rapids(r)
            .iter()
            .map(|(a, b)| a.z() * (*b - *a).length())
            .sum::<f64>()
    };
    assert!(
        height(&with) < 0.8 * height(&plain),
        "rapid height x length: {} vs {}",
        height(&with),
        height(&plain)
    );
    for (a, b) in flat_rapids(&with) {
        if let Some(need) = needed(&stock, a, b) {
            assert!(a.z() >= need - 1e-9, "{a:?} {b:?} need {need}");
        }
    }
}

#[test]
fn a_2d_toolpath_after_a_roughing_lifts_lower() {
    // The dome roughed with a 3-D pocket (Ø6 flat, 0.5 mm left), then its
    // flats cleaned with `horizontal`, which links with the 2-D linker.
    let g = dome();
    let flat = tool("f6");
    let rough = tp(
        "pocket_3d",
        "f6",
        "[passes]\nstep_down = 4\nstock_to_leave = 0.5\n",
    );
    let block = Aabb3 {
        min: Point3::new(-5.0, -5.0, 0.0),
        max: Point3::new(85.0, 65.0, 29.0),
    };
    let mut stock = LayeredStock::planar(&block);
    let mut i = input(&g, &rough, &flat);
    i.stock = Some(block);
    i.in_process = Some(&stock);
    let r = calculate(&i).unwrap_or_else(|e| panic!("{e}"));
    stock.apply(&r.result, Sweep::of(&flat), 0.01);
    let flats = tp(
        "horizontal",
        "f6",
        "[passes]
stepover = 3
[heights]
top = { ref = \"work_zero\", offset = 10 }
",
    );
    let mut i = input(&g, &flats, &flat);
    let plain = calculate(&i).unwrap();
    i.in_process = Some(&stock);
    let with = calculate(&i).unwrap();
    assert_eq!(cuts(&plain), cuts(&with));
    let height = |r: &Generated| -> f64 {
        flat_rapids(r)
            .iter()
            .map(|(a, b)| a.z() * (*b - *a).length())
            .sum::<f64>()
    };
    assert!(
        height(&with) < 0.8 * height(&plain),
        "rapid height x length: {} vs {}",
        height(&with),
        height(&plain)
    );
    for (a, b) in flat_rapids(&with) {
        if let Some(need) = needed(&stock, a, b) {
            assert!(a.z() >= need - 1e-9, "{a:?} {b:?} need {need}");
        }
    }
}
