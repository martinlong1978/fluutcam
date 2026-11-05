// SPDX-License-Identifier: MIT OR Apache-2.0

//! Editing in the window: the Setup editor, tools, heights, picking and the
//! browser's actions.

use super::*;
use egui::accesskit::Role;
use fluut_model::Num;
use fluut_model::project::{StockSpec, ZeroSpec};

/// Types into the text field labelled `label` and commits with Enter.
pub(crate) fn enter(h: &mut Harness<'static, State>, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get_by_role_and_label(Role::TextInput, label)
        .type_text(text);
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
}

/// Chooses `option` in the dropdown labelled `label`.
pub(crate) fn choose(h: &mut Harness<'static, State>, label: &str, option: &str) {
    h.get_by_role_and_label(Role::ComboBox, label).click();
    h.run_steps(2);
    h.get_by_label(option).click();
    h.run_steps(2);
}

/// Steps until a snapshot newer than `after` has arrived.
pub(crate) fn newer(h: &mut Harness<'static, State>, after: u64) {
    for _ in 0..2000 {
        h.step();
        if snapshot(h).revision > after {
            h.run_steps(2);
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("no new snapshot");
}

#[test]
fn the_setup_editor_edits_stock_and_zero_in_the_window() {
    let mut h = harness();
    h.step();
    open(&mut h, project("block"));
    stage(&mut h, "SETUP");
    app(&mut h).ui.selection = Selection::Setup("s1".into());
    button(&h, "Stock").click();
    h.run_steps(2);
    let r = snapshot(&mut h).revision;
    enter(&mut h, "Margin X, each side", "9");
    newer(&mut h, r);
    let s = snapshot(&mut h);
    assert_eq!(
        s.project.setup("s1").unwrap().stock,
        Some(StockSpec::Block {
            margin: Some([Num::float(9.0), Num::int(5), Num::int(0)]),
            size: None
        })
    );
    let r = s.revision;
    choose(&mut h, "Location", "Stock top, centre");
    newer(&mut h, r);
    let zero =
        |h: &mut Harness<'static, State>| snapshot(h).project.setup("s1").unwrap().zero.clone();
    assert_eq!(zero(&mut h), Some(ZeroSpec::StockTopCentre));
    // Undo goes back one edit at a time.
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_ne!(zero(&mut h), Some(ZeroSpec::StockTopCentre));
}
