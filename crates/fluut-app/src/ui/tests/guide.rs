// SPDX-License-Identifier: MIT OR Apache-2.0

//! The "Getting started" card in the window: a first job from a new project
//! to G-code with the card's buttons only, closing the card and opening it
//! again.

use super::editing::choose;
use super::*;
use crate::updates::Updates;
use crate::vm::guide::GuideVm;
use egui::accesskit::Role;
use fluut_core::settings::Settings;

/// The card's button of a label.
fn step_button<'a>(h: &'a Harness<'static, State>, label: &'a str) -> egui_kittest::Node<'a> {
    h.get_by_role_and_label(Role::Button, label)
}

fn card_open(h: &mut Harness<'static, State>) {
    app(h).ui.guide.open = true;
    h.run_steps(3);
}

#[test]
fn the_card_is_closed_in_a_plain_window_and_opens_from_the_title_bar() {
    let mut h = harness();
    h.run_steps(2);
    assert!(h.query_by_label("Your first project").is_none());
    h.get_by_label("Getting started").click();
    h.run_steps(3);
    h.get_by_label("Your first project");
    h.get_by_label("0 of 7 steps done");
    // Nothing but the first step can run with no project.
    assert!(enabled(&h, "Start new project"));
    let others = [
        "Choose model file",
        "Add Setup",
        "Add toolpath",
        "Calculate toolpaths",
        "Run simulation",
        "Write G-code",
    ];
    for label in others {
        assert!(!enabled(&h, label), "{label}");
    }
}

#[test]
fn the_card_walks_a_first_job_to_gcode() {
    let d = temp("guide_walk");
    let mut h = harness();
    h.step();
    app(&mut h).ui.clock = fixed_clock;
    card_open(&mut h);

    // 1. A new project, and its unit.
    scripted(&mut h).answer(DialogKind::New(DisplayUnit::Mm), d.join("first"));
    step_button(&h, "Start new project").click();
    settle(&mut h);
    h.get_by_label("Millimetres").click();
    settle(&mut h);
    h.get_by_label("1 of 7 steps done");
    h.get_by_label("Done: Start a project");
    h.get_by_label("To do: Bring in your model");

    // 2. A model, from the native dialog.
    scripted(&mut h).answer(DialogKind::Import, boss_step());
    step_button(&h, "Choose model file").click();
    settle(&mut h);
    h.get_by_label("Done: Bring in your model");
    assert!(!snapshot(&mut h).project.geometry.is_empty());

    // 3. A Setup, and a look at its stock and work zero.
    step_button(&h, "Add Setup").click();
    settle(&mut h);
    h.get_by_label("Done: Add a Setup");
    assert_eq!(app(&mut h).ui.selection, Selection::Setup("s1".into()));
    step_button(&h, "Show stock").click();
    h.run_steps(2);
    assert_eq!(app(&mut h).ui.inspector_focus, Some("setup-stock"));
    step_button(&h, "Show work zero").click();
    h.run_steps(2);
    assert_eq!(app(&mut h).ui.inspector_focus, Some("setup-zero"));

    // 4. A toolpath of the offered type.
    step_button(&h, "Add toolpath").click();
    settle(&mut h);
    h.get_by_label("Done: Add a toolpath");
    let s = snapshot(&mut h);
    let (_, t) = s.project.toolpath("t1").unwrap();
    assert_eq!(t.kind(), fluut_model::ToolpathTypeId::Parallel);

    // 5. Calculate, 6. simulate.
    step_button(&h, "Calculate toolpaths").click();
    settle(&mut h);
    h.get_by_label("Done: Work out the tool moves");
    assert!(snapshot(&mut h).paths.contains_key("t1"));
    step_button(&h, "Run simulation").click();
    settle(&mut h);
    h.get_by_label("Done: Watch the cut");
    assert!(!snapshot(&mut h).sims.is_empty());
    h.get_by_label("To do: Write the G-code");

    // 7. Post, to the file the save dialog gave.
    let out = d.join("first.ngc");
    scripted(&mut h).answer(DialogKind::PostOutput, out.clone());
    step_button(&h, "Write G-code").click();
    settle(&mut h);
    h.get_by_label("Done: Write the G-code");
    h.get_by_label("All steps done. Your G-code is written.");
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(
        text.contains("G1") && text.contains("(Project: first)"),
        "{text}"
    );

    // The card used the dialogs of the ribbon, in the order of the steps.
    let kinds: Vec<_> = scripted(&mut h).asked().iter().map(|a| a.kind).collect();
    assert_eq!(
        kinds,
        [
            DialogKind::New(DisplayUnit::Mm),
            DialogKind::Import,
            DialogKind::PostOutput
        ]
    );
}

#[test]
fn the_toolpath_step_adds_the_chosen_type_with_the_chosen_tool() {
    let mut h = harness();
    h.step();
    open(&mut h, project("boss"));
    card_open(&mut h);
    let state = app(&mut h).ui.guide.clone();
    let vm = GuideVm::new(&app(&mut h).jobs, &Selection::None, &state);
    let (reference, label) = vm.tools[1].clone();
    choose(&mut h, "Toolpath type", "2D pocket");
    choose(&mut h, "Cutting tool", &label);
    assert_eq!(
        app(&mut h).ui.guide.toolpath_type,
        fluut_model::ToolpathTypeId::Pocket2d
    );
    assert_eq!(app(&mut h).ui.guide.tool.as_deref(), Some(&*reference));
    step_button(&h, "Add toolpath").click();
    settle(&mut h);
    h.run_steps(3);
    let s = snapshot(&mut h);
    let setup = s.project.setup("s1").unwrap();
    let added = setup.toolpaths.last().unwrap();
    assert_eq!(added.kind(), fluut_model::ToolpathTypeId::Pocket2d);
    assert_eq!(added.tool, reference);
    assert_eq!(app(&mut h).ui.guide.pending_tool, None);
}

#[test]
fn closing_the_card_is_remembered_and_the_title_bar_reopens_it() {
    let dir = temp("guide_settings");
    let path = dir.join("settings.toml");
    let updates = || {
        Updates::with(
            Settings::load_from(&path),
            Some(path.clone()),
            std::sync::Arc::new(|| Err(fluut_core::update::UpdateError::NoKey)),
            crate::updates::now,
            std::sync::Arc::new(|| {}),
            false,
        )
    };
    let mut h = harness();
    h.step();
    let ctx = h.ctx.clone();
    *h.state_mut() = Some(FluutApp::new(&ctx, None, None, Instant::now()).with_updates(updates()));
    h.run_steps(3);
    // Never closed: shown at start.
    assert!(app(&mut h).ui.guide.open);
    h.get_by_label("Your first project");

    // Close it with the window's close button.
    h.get_by_role_and_label(Role::Button, "Close window")
        .click();
    h.run_steps(3);
    assert!(!app(&mut h).ui.guide.open);
    assert!(h.query_by_label("Your first project").is_none());
    assert!(Settings::load_from(&path).guide.dismissed);

    // The next start keeps it closed.
    let ctx = h.ctx.clone();
    *h.state_mut() = Some(FluutApp::new(&ctx, None, None, Instant::now()).with_updates(updates()));
    h.run_steps(3);
    assert!(!app(&mut h).ui.guide.open);
    assert!(h.query_by_label("Your first project").is_none());

    // The title bar opens it again, and that is what the next start shows.
    h.get_by_label("Getting started").click();
    h.run_steps(3);
    h.get_by_label("Your first project");
    assert!(!Settings::load_from(&path).guide.dismissed);
    h.get_by_label("Getting started").click();
    h.run_steps(3);
    assert!(h.query_by_label("Your first project").is_none());
    assert!(Settings::load_from(&path).guide.dismissed);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_card_and_the_ribbon_run_the_same_command() {
    let mut h = harness();
    h.step();
    card_open(&mut h);
    scripted(&mut h).answer(
        DialogKind::New(DisplayUnit::Mm),
        temp("guide_both").join("x"),
    );
    h.get_by_label("New project").click();
    settle(&mut h);
    h.get_by_label("Millimetres").click();
    settle(&mut h);
    h.get_by_label("1 of 7 steps done");
}
