// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::jobs::Jobs;
use crate::session::tests::opened;
use fluut_model::project::ZeroSpec;
use std::sync::Arc;

fn run(j: &mut Jobs, e: Edit) -> Arc<Snapshot> {
    j.edit(e);
    j.wait_snapshot().unwrap()
}

fn undo(j: &mut Jobs) -> Arc<Snapshot> {
    j.undo();
    j.wait();
    j.snapshot.clone().unwrap()
}

#[test]
fn block_stock_has_a_zero_and_three_margin_handles_on_its_faces() {
    let j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let b = s.scenes["s1"].stock.unwrap();
    let h = handles(&s, "s1");
    let kinds: Vec<HandleKind> = h.iter().map(|h| h.kind).collect();
    assert_eq!(
        kinds,
        [
            HandleKind::Zero,
            HandleKind::Margin(0),
            HandleKind::Margin(1),
            HandleKind::Margin(2)
        ]
    );
    assert_eq!(h[0].at, [0.0; 3]);
    assert_eq!(h[1].at[0], b.max[0]);
    assert_eq!(h[2].at[1], b.max[1]);
    assert_eq!(h[3].at[2], b.max[2]);
    assert_eq!(h[1].value_mm, 5.0);
    assert_eq!(h[3].value_mm, 0.0);
    assert_eq!(h[3].dir, [0.0, 0.0, 1.0]);
}

#[test]
fn a_stock_given_by_size_has_only_the_zero_handle() {
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let e = setup_edit(&s, "s1", "stock.kind", "size").unwrap();
    let s = run(&mut j, Edit::Model(e));
    let kinds: Vec<HandleKind> = handles(&s, "s1").iter().map(|h| h.kind).collect();
    assert_eq!(kinds, [HandleKind::Zero]);
}

#[test]
fn the_zero_snaps_to_corners_and_the_top_centre() {
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let t = zero_targets(&s, "s1");
    assert_eq!(t.len(), 9);
    let b = s.scenes["s1"].stock.unwrap();
    let pos = |t: &[(String, [f32; 3])], k: &str| t.iter().find(|(n, _)| n == k).unwrap().1;
    // The zero is at the front left of the top: that corner is the origin.
    assert_eq!(pos(&t, "top.front_left"), [0.0, 0.0, 0.0]);
    assert_eq!(pos(&t, "top_centre")[2], 0.0);
    assert_eq!(pos(&t, "bottom.back_right"), [b.max[0], b.max[1], b.min[2]]);
    // The nearest target on the screen, within a radius.
    let on_screen = [[10.0, 10.0], [100.0, 10.0], [100.0, 100.0]];
    assert_eq!(nearest_target(&on_screen, [96.0, 14.0], 20.0), Some(1));
    assert_eq!(nearest_target(&on_screen, [50.0, 50.0], 20.0), None);
    // Dropping on the current place is no edit; elsewhere it is one.
    assert_eq!(zero_edit(&s, "s1", "top.front_left"), None);
    let e = zero_edit(&s, "s1", "top_centre").unwrap();
    let s = run(&mut j, e);
    assert_eq!(
        s.project.setup("s1").unwrap().zero,
        Some(ZeroSpec::StockTopCentre)
    );
    let b = s.scenes["s1"].stock.unwrap();
    assert!((b.max[0] + b.min[0]).abs() < 1e-3);
    // The targets follow the new zero: top centre is the origin now.
    let t = zero_targets(&s, "s1");
    assert_eq!(pos(&t, "top_centre"), [0.0, 0.0, 0.0]);
    let s = undo(&mut j);
    assert!(matches!(
        s.project.setup("s1").unwrap().zero,
        Some(ZeroSpec::StockTopCorner(_))
    ));
}

#[test]
fn a_rotary_zero_snaps_to_the_ends_of_the_stock() {
    let j = opened("shaft");
    let s = j.snapshot.clone().unwrap();
    let id = s.project.setups[0].id.clone();
    let t = zero_targets(&s, &id);
    let keys: Vec<&str> = t.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["stock_left", "stock_right"]);
    assert_eq!(t[0].1, [0.0; 3], "the zero is at the left end");
    assert!(t[1].1[0] > 0.0);
    assert!(zero_edit(&s, &id, "stock_right").is_some());
    // Only the zero handle: the cylinder has no margin.
    let kinds: Vec<HandleKind> = handles(&s, &id).iter().map(|h| h.kind).collect();
    assert_eq!(kinds, [HandleKind::Zero]);
}

#[test]
fn a_margin_drag_is_one_edit_in_steps_of_a_tenth() {
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let b0 = s.scenes["s1"].stock.unwrap();
    assert!((dragged_margin(5.0, 3.21) - 8.2).abs() < 1e-9);
    assert_eq!(dragged_margin(5.0, -9.0), 0.0);
    assert_eq!(margin_edit(&s, "s1", 0, 5.0), Ok(None));
    let e = margin_edit(&s, "s1", 0, dragged_margin(5.0, 3.21))
        .unwrap()
        .unwrap();
    let s = run(&mut j, e);
    let b1 = s.scenes["s1"].stock.unwrap();
    // The margin is on both sides in X.
    assert!(((b1.max[0] - b1.min[0]) - (b0.max[0] - b0.min[0]) - 6.4).abs() < 1e-2);
    assert!((handles(&s, "s1")[1].value_mm - 8.2).abs() < 1e-9);
    // One undo step undoes the drag.
    let s = undo(&mut j);
    assert_eq!(handles(&s, "s1")[1].value_mm, 5.0);
    assert_eq!(s.scenes["s1"].stock.unwrap(), b0);
}

#[test]
fn a_margin_in_inches_is_entered_in_the_display_unit_and_kept_in_mm() {
    let mut j = opened("block");
    let e = fluut_model::edit::Edit::SetUnits("inch".into());
    let s = run(&mut j, Edit::Model(e));
    let e = margin_edit(&s, "s1", 1, 12.7).unwrap().unwrap();
    let s = run(&mut j, e);
    assert!((handles(&s, "s1")[2].value_mm - 12.7).abs() < 1e-6);
}
