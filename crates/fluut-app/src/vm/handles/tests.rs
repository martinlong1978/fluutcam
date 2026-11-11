// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::jobs::Jobs;
use crate::session::tests::opened;
use fluut_model::project::{AxisX, ZeroSpec};
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
fn a_stock_given_by_size_has_a_handle_on_each_face() {
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let e = setup_edit(&s, "s1", "stock.kind", "size").unwrap();
    let s = run(&mut j, Edit::Model(e));
    let b0 = s.scenes["s1"].stock.unwrap();
    let h = handles(&s, "s1");
    let kinds: Vec<HandleKind> = h.iter().map(|h| h.kind).collect();
    assert_eq!(
        kinds,
        [
            HandleKind::Zero,
            HandleKind::Size(0),
            HandleKind::Size(1),
            HandleKind::Size(2)
        ]
    );
    // The size is centred on the model in X and Y: those faces move half
    // as far as the size does.
    assert_eq!((h[1].gain, h[2].gain, h[3].gain), (2.0, 2.0, 1.0));
    assert!((h[1].value_mm - f64::from(b0.max[0] - b0.min[0])).abs() < 1e-2);
    assert_eq!(h[1].at[0], b0.max[0]);
    assert_eq!(h[3].dir, [0.0, 0.0, 1.0]);
    // Dragging the +X face 3 mm out grows the size by 6 mm, in one edit.
    let v = dragged_value(&h[1], 3.0);
    assert!((v - h[1].value_mm - 6.0).abs() < 0.1 + 1e-9, "{v}");
    let e = value_edit(&s, "s1", HandleKind::Size(0), v)
        .unwrap()
        .unwrap();
    let s1 = run(&mut j, e);
    let b1 = s1.scenes["s1"].stock.unwrap();
    assert!(((b1.max[0] - b1.min[0]) - (b0.max[0] - b0.min[0]) - 6.0).abs() < 0.11);
    assert!((handles(&s1, "s1")[1].value_mm - v).abs() < 1e-9);
    let s2 = undo(&mut j);
    assert_eq!(s2.scenes["s1"].stock.unwrap(), b0);
    // A size never drags to nothing.
    assert!(dragged_value(&h[3], -1000.0) > 0.0);
}

#[test]
fn the_zero_snaps_to_corners_and_the_top_centre() {
    let mut j = opened("block");
    let s = j.snapshot.clone().unwrap();
    let t = zero_targets(&s, "s1");
    // Nine stock places come first; the model's points follow.
    assert!(t.len() > 9);
    assert!(t[..9].iter().all(|(k, _)| !k.starts_with("point:")));
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
    assert!(t.len() > 9);
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
    assert_eq!(&keys[..2], ["stock_left", "stock_right"]);
    assert!(keys[2..].iter().all(|k| k.starts_with("axis:")));
    assert_eq!(t[0].1, [0.0; 3], "the zero is at the left end");
    assert!(t[1].1[0] > 0.0);
    assert!(zero_edit(&s, &id, "stock_right").is_some());
    // The zero and the cylinder's radius and length: no margin.
    let kinds: Vec<HandleKind> = handles(&s, &id).iter().map(|h| h.kind).collect();
    assert_eq!(
        kinds,
        [HandleKind::Zero, HandleKind::Radius, HandleKind::Length]
    );
}

#[test]
fn cylinder_handles_sit_on_the_stock_and_drag_in_one_edit() {
    let mut j = opened("shaft");
    let s = j.snapshot.clone().unwrap();
    let id = s.project.setups[0].id.clone();
    let b0 = s.scenes[&id].stock.unwrap();
    let h = handles(&s, &id);
    let (radius, length) = (&h[1], &h[2]);
    assert_eq!((radius.value_mm, length.value_mm), (17.0, 82.0));
    // The radius handle is on top of the middle; the length handle on the
    // axis at the right end, and it moves the ends together.
    assert_eq!(radius.at, [0.5 * (b0.min[0] + b0.max[0]), 0.0, 17.0]);
    assert_eq!(radius.dir, [0.0, 0.0, 1.0]);
    assert_eq!(length.at, [b0.max[0], 0.0, 0.0]);
    assert_eq!(length.gain, 2.0);
    let r = dragged_value(radius, 4.26);
    assert!((r - 21.3).abs() < 1e-9, "{r}");
    let l = dragged_value(length, 3.0);
    assert!((l - 88.0).abs() < 1e-9, "{l}");
    let e = value_edit(&s, &id, HandleKind::Radius, r).unwrap().unwrap();
    let s1 = run(&mut j, e);
    let b1 = s1.scenes[&id].stock.unwrap();
    assert!((b1.max[2] - 21.3).abs() < 1e-3 && (b1.min[2] + 21.3).abs() < 1e-3);
    let e = value_edit(&s1, &id, HandleKind::Length, l)
        .unwrap()
        .unwrap();
    let s2 = run(&mut j, e);
    let b2 = s2.scenes[&id].stock.unwrap();
    assert!(((b2.max[0] - b2.min[0]) - 88.0).abs() < 1e-3);
    // Unchanged is no edit.
    assert_eq!(value_edit(&s2, &id, HandleKind::Length, 88.0), Ok(None));
    // Each drag is one undo step.
    let s3 = undo(&mut j);
    assert_eq!(s3.scenes[&id].stock.unwrap(), b1);
    let s4 = undo(&mut j);
    assert_eq!(s4.scenes[&id].stock.unwrap(), b0);
    // Nothing drags to zero or below.
    assert!(dragged_value(radius, -100.0) > 0.0 && dragged_value(length, -100.0) > 0.0);
    // The zero has no value.
    assert!(value_edit(&s4, &id, HandleKind::Zero, 1.0).is_err());
}

#[test]
fn the_zero_snaps_to_vertices_and_arc_centres_of_the_model() {
    let mut j = opened("holes");
    let s = j.snapshot.clone().unwrap();
    let t = zero_targets(&s, "s1");
    // The 9 stock places, then model points.
    assert!(t.len() > 9);
    assert!(t[..9].iter().all(|(k, _)| !k.starts_with("point:")));
    // The centre of the Ø22 blind hole (62, 25) at the top. The top of the
    // stock is Z 15 and the zero is its front left corner (0, 0).
    let key = "point:62.000000,25.000000,15.000000";
    let at = t.iter().find(|(k, _)| k == key).unwrap().1;
    assert!(
        (at[0] - 62.0).abs() < 1e-3 && (at[1] - 25.0).abs() < 1e-3 && at[2].abs() < 1e-3,
        "{at:?}"
    );
    // A corner of the part is a stock place already, not a second target.
    assert!(
        !t.iter()
            .any(|(k, p)| k.starts_with("point:") && *p == [0.0; 3])
    );
    // Dropping on it makes a point zero in the model frame, in one edit.
    let e = zero_edit(&s, "s1", key).unwrap();
    let s1 = run(&mut j, e);
    let Some(ZeroSpec::Point(p)) = s1.project.setup("s1").unwrap().zero.clone() else {
        panic!("{:?}", s1.project.setup("s1").unwrap().zero)
    };
    assert_eq!(p.map(|n| n.v), [62.0, 25.0, 15.0]);
    // The scene follows: the same place is the origin now, and dropping
    // there again is no edit.
    let t1 = zero_targets(&s1, "s1");
    let at = t1.iter().find(|(k, _)| k == key).unwrap().1;
    assert!(at.iter().all(|v| v.abs() < 1e-3), "{at:?}");
    assert_eq!(zero_edit(&s1, "s1", key), None);
    // Malformed keys are no edit.
    assert_eq!(zero_edit(&s1, "s1", "point:1,2"), None);
    assert_eq!(zero_edit(&s1, "s1", "axis:x"), None);
    let s2 = undo(&mut j);
    assert!(matches!(
        s2.project.setup("s1").unwrap().zero,
        Some(ZeroSpec::StockTopCorner(_))
    ));
}

#[test]
fn a_rotary_zero_snaps_to_model_vertices_by_their_x() {
    let mut j = opened("shaft");
    let s = j.snapshot.clone().unwrap();
    let id = s.project.setups[0].id.clone();
    let t = zero_targets(&s, &id);
    // The shaft steps at 30 and 55 along its axis; each X is one target.
    let axis: Vec<(&str, f32)> = t
        .iter()
        .filter(|(k, _)| k.starts_with("axis:"))
        .map(|(k, p)| (k.as_str(), p[0]))
        .collect();
    assert!(!axis.is_empty(), "{t:?}");
    let mut keys: Vec<&str> = axis.iter().map(|(k, _)| *k).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), axis.len());
    // The stock starts 1 mm before the shaft's end: the step at 30 mm is
    // at X 31 of W, and 30 in the aligned frame.
    let (key, _) = axis
        .iter()
        .find(|(_, x)| (x - 31.0).abs() < 1e-3)
        .unwrap_or_else(|| panic!("{axis:?}"));
    let e = zero_edit(&s, &id, key).unwrap();
    let s1 = run(&mut j, e);
    let Some(ZeroSpec::OnAxis { x: AxisX::At(x) }) = s1.project.setup(&id).unwrap().zero.clone()
    else {
        panic!("{:?}", s1.project.setup(&id).unwrap().zero)
    };
    assert!((x.v - 30.0).abs() < 1e-6, "{}", x.v);
    // The scene follows: that step is the origin now.
    let t1 = zero_targets(&s1, &id);
    assert!(
        t1.iter()
            .filter(|(k, _)| k.starts_with("axis:"))
            .any(|(_, p)| p[0].abs() < 1e-3),
        "{t1:?}"
    );
    assert_eq!(zero_edit(&s1, &id, key), None);
    let s2 = undo(&mut j);
    assert!(matches!(
        s2.project.setup(&id).unwrap().zero,
        Some(ZeroSpec::OnAxis {
            x: AxisX::StockLeft
        })
    ));
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
