// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Setup editor's kind, bodies, orientation and handles in the window.

use super::editing::{choose, newer};
use super::*;
use fluut_model::project::{SetupKind, StockSpec};

#[test]
fn the_kind_is_chosen_in_the_inspector_and_undone() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    stage(&mut h, "SETUP");
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    h.run_steps(3);
    let r = snapshot(&mut h).revision;
    choose(&mut h, "Kind", "Rotary");
    newer(&mut h, r);
    let s = snapshot(&mut h);
    let st = s.project.setup("s1").unwrap();
    assert_eq!(st.kind, Some(SetupKind::Rotary));
    assert!(matches!(st.stock, Some(StockSpec::Cylinder { .. })));
    h.get_all_by_label("Radius").next().unwrap();
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(
        snapshot(&mut h).project.setup("s1").unwrap().kind,
        Some(SetupKind::ThreeAxis)
    );
}
