// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::jobs::CalcTargets;
use crate::session::tests::opened;
use std::collections::BTreeSet;

fn vm_with(j: &Jobs, stage: Stage, selection: &Selection) -> RibbonVm {
    RibbonVm::new(
        j,
        RibbonCx {
            stage,
            selection,
            playback: &Playback::default(),
        },
    )
}

fn vm(j: &Jobs, stage: Stage) -> RibbonVm {
    vm_with(j, stage, &Selection::None)
}

fn ids(r: &RibbonVm) -> Vec<&'static str> {
    r.commands().map(|c| c.command.id()).collect()
}

#[test]
fn enabling_follows_project_and_job() {
    let j = Jobs::start(|| {});
    let r = vm(&j, Stage::Model);
    assert!(r.enabled(Command::Open));
    assert!(!r.enabled(Command::Calculate) && !r.enabled(Command::Post));
    assert!(!r.enabled(Command::Cancel));

    let mut j = opened("boss");
    let r = vm(&j, Stage::Model);
    assert!(r.enabled(Command::Calculate) && r.enabled(Command::Post));
    assert!(r.enabled(Command::Simulate));
    j.calculate(CalcTargets::All);
    let r = vm(&j, Stage::Model);
    assert!(r.enabled(Command::Cancel));
    assert!(!r.enabled(Command::Open) && !r.enabled(Command::Calculate));
    assert!(!r.enabled(Command::Simulate));
    j.wait();
    assert!(vm(&j, Stage::Model).enabled(Command::Post));
}

#[test]
fn ids_are_pinned_and_unique() {
    let all = Command::all();
    let unique: BTreeSet<_> = all.iter().map(|c| c.id()).collect();
    assert_eq!(unique.len(), all.len());
    let pinned = [
        (Command::NewProject, "project.new"),
        (Command::Open, "project.open"),
        (Command::Save, "project.save"),
        (Command::SaveAs, "project.save_as"),
        (Command::ImportModel, "model.import"),
        (Command::Reimport, "model.reimport"),
        (Command::Undo, "edit.undo"),
        (Command::Redo, "edit.redo"),
        (Command::NewSetup(SetupKind::ThreeAxis), "setup.new_3axis"),
        (Command::NewSetup(SetupKind::Rotary), "setup.new_rotary"),
        (Command::NewSetup(SetupKind::Indexed), "setup.new_indexed"),
        (Command::NewSetupBasedOn, "setup.new_based_on"),
        (Command::Stock, "setup.stock"),
        (Command::WorkZero, "setup.work_zero"),
        (Command::CalculateSelected, "toolpath.calculate_selected"),
        (Command::Calculate, "toolpath.calculate_all"),
        (Command::Simulate, "simulation.run"),
        (Command::StepBack, "simulation.step_back"),
        (Command::PlayPause, "simulation.play_pause"),
        (Command::StepForward, "simulation.step_forward"),
        (Command::Post, "post.all"),
        (Command::Cancel, "job.cancel"),
        (Command::ImportTools, "library.import_tools"),
        (Command::AddToolLibrary, "library.add_tool_library"),
        (Command::ImportPost, "library.import_post"),
        (Command::LibraryTools, "library.tools"),
        (Command::LibraryMachines, "library.machines"),
        (Command::LibraryPosts, "library.posts"),
    ];
    assert_eq!(all.len(), pinned.len() + ToolpathTypeId::ALL.len());
    for (c, id) in pinned {
        assert_eq!(c.id(), id);
    }
    for t in ToolpathTypeId::ALL {
        assert_eq!(
            Command::AddToolpath(t).id(),
            format!("toolpath.add:{}", t.as_str())
        );
    }
}

#[test]
fn every_command_has_a_message() {
    for c in Command::all() {
        let l = c.label();
        assert!(!l.is_empty() && !l.contains("ribbon-"), "{l}");
        assert!(!l.contains("toolpath-type-"), "{l}");
    }
    for s in Stage::ALL {
        assert!(!s.label().contains("ribbon-"));
    }
}

#[test]
fn stages_hold_their_commands() {
    let j = Jobs::start(|| {});
    assert_eq!(
        ids(&vm(&j, Stage::Model)),
        [
            "project.new",
            "project.open",
            "project.save",
            "project.save_as",
            "model.import",
            "model.reimport",
            "edit.undo",
            "edit.redo",
            "job.cancel"
        ]
    );
    assert_eq!(
        ids(&vm(&j, Stage::Setup)),
        [
            "setup.new_3axis",
            "setup.new_rotary",
            "setup.new_indexed",
            "setup.new_based_on",
            "setup.stock",
            "setup.work_zero",
            "edit.undo",
            "edit.redo",
            "job.cancel"
        ]
    );
    assert_eq!(
        ids(&vm(&j, Stage::Simulate)),
        [
            "simulation.run",
            "simulation.play_pause",
            "simulation.step_back",
            "simulation.step_forward",
            "edit.undo",
            "edit.redo",
            "job.cancel"
        ]
    );
    assert_eq!(
        ids(&vm(&j, Stage::Post)),
        ["post.all", "edit.undo", "edit.redo", "job.cancel"]
    );
    assert_eq!(
        ids(&vm(&j, Stage::Library)),
        [
            "library.import_tools",
            "library.add_tool_library",
            "library.import_post",
            "library.tools",
            "library.machines",
            "library.posts",
            "edit.undo",
            "edit.redo",
            "job.cancel"
        ]
    );
    let t = vm(&j, Stage::Toolpaths);
    let groups: Vec<_> = t.groups.iter().map(|g| g.id).collect();
    assert_eq!(
        groups,
        [
            GroupId::Gallery(Category::TwoD),
            GroupId::Gallery(Category::Holes),
            GroupId::Gallery(Category::ThreeD),
            GroupId::Gallery(Category::Rotary),
            GroupId::Gallery(Category::Utility),
            GroupId::Calculate,
            GroupId::Edit,
            GroupId::Job
        ]
    );
}

#[test]
fn every_command_is_on_a_stage() {
    let j = Jobs::start(|| {});
    let shown: BTreeSet<_> = Stage::ALL
        .into_iter()
        .flat_map(|s| ids(&vm(&j, s)))
        .collect();
    for c in Command::all() {
        assert!(shown.contains(c.id()), "{}", c.id());
    }
}

#[test]
fn only_the_library_is_still_to_come() {
    let j = opened("block");
    let coming = crate::text::tr("ribbon-coming");
    let mut library = 0;
    for s in Stage::ALL {
        for c in vm_with(&j, s, &Selection::Setup("s1".into())).commands() {
            assert_eq!(c.tooltip.contains(&coming), !c.command.wired());
            if !c.command.wired() {
                assert!(!c.enabled, "{}", c.command.id());
                assert_eq!(s, Stage::Library);
                library += 1;
            } else if matches!(c.command, Command::Undo | Command::Redo) {
                // Nothing to undo in a freshly opened project.
                assert!(!c.enabled);
            }
        }
    }
    assert_eq!(library, 3);
}

#[test]
fn project_commands_follow_the_project_and_the_job() {
    let none = Jobs::start(|| {});
    let r = vm(&none, Stage::Model);
    assert!(r.enabled(Command::NewProject) && r.enabled(Command::Open));
    for c in [Command::Save, Command::SaveAs, Command::ImportModel] {
        assert!(!r.enabled(c), "{}", c.id());
    }
    let mut j = opened("block");
    let r = vm(&j, Stage::Model);
    for c in [
        Command::NewProject,
        Command::Save,
        Command::SaveAs,
        Command::ImportModel,
        Command::Reimport,
        Command::NewSetup(SetupKind::Rotary),
        Command::NewSetupBasedOn,
        Command::Stock,
        Command::WorkZero,
        Command::AddToolpath(ToolpathTypeId::Parallel),
    ] {
        assert!(r.enabled(c), "{}", c.id());
    }
    // A running job disables everything but Cancel.
    j.calculate(CalcTargets::All);
    let r = vm(&j, Stage::Model);
    for c in [
        Command::NewProject,
        Command::Save,
        Command::ImportModel,
        Command::Undo,
        Command::NewSetup(SetupKind::ThreeAxis),
        Command::AddToolpath(ToolpathTypeId::Parallel),
    ] {
        assert!(!r.enabled(c), "{}", c.id());
    }
    j.wait();
}

#[test]
fn undo_and_redo_follow_the_history() {
    let mut j = opened("block");
    j.add_setup(SetupKind::ThreeAxis, None);
    j.wait();
    let r = vm(&j, Stage::Model);
    assert!(r.enabled(Command::Undo) && !r.enabled(Command::Redo));
    j.undo();
    j.wait();
    let r = vm(&j, Stage::Model);
    assert!(!r.enabled(Command::Undo) && r.enabled(Command::Redo));
}

#[test]
fn reimport_needs_geometry_and_a_setup_needs_a_selection_to_be_based_on() {
    let mut j = Jobs::start(|| {});
    j.new_project(
        std::env::temp_dir().join(format!("fluut-vm-ribbon-{}", std::process::id())),
        fluut_model::project::DisplayUnit::Mm,
    );
    j.wait();
    let r = vm(&j, Stage::Model);
    assert!(!r.enabled(Command::Reimport) && !r.enabled(Command::NewSetupBasedOn));
    assert!(r.enabled(Command::NewSetup(SetupKind::ThreeAxis)));
    assert!(!r.enabled(Command::Stock));
}

#[test]
fn shortcuts_are_pinned_and_say_cmd_on_macos() {
    let key = |c: Command| c.shortcut().map(|s| s.text_for(false));
    assert_eq!(key(Command::NewProject).as_deref(), Some("Ctrl+N"));
    assert_eq!(key(Command::SaveAs).as_deref(), Some("Ctrl+Shift+S"));
    assert_eq!(key(Command::ImportModel).as_deref(), Some("Ctrl+I"));
    assert_eq!(key(Command::Undo).as_deref(), Some("Ctrl+Z"));
    assert_eq!(key(Command::Redo).as_deref(), Some("Ctrl+Y"));
    assert_eq!(
        Command::Redo.alt_shortcut().map(|s| s.text_for(false)),
        Some("Ctrl+Shift+Z".to_owned())
    );
    assert_eq!(
        Command::SaveAs.shortcut().unwrap().text_for(true),
        "Cmd+Shift+S"
    );
    let unique: BTreeSet<_> = Command::all()
        .into_iter()
        .flat_map(|c| [c.shortcut(), c.alt_shortcut()])
        .flatten()
        .map(|s| s.text_for(false))
        .collect();
    assert_eq!(unique.len(), 8);

    let j = Jobs::start(|| {});
    let r = vm(&j, Stage::Model);
    let save = r.commands().find(|c| c.command == Command::Save).unwrap();
    let keys = if cfg!(target_os = "macos") {
        "Cmd+S"
    } else {
        "Ctrl+S"
    };
    assert!(
        save.tooltip.starts_with(&format!("Save ({keys})")),
        "{}",
        save.tooltip
    );
}

#[test]
fn gallery_without_a_project_shows_every_type_once() {
    let j = Jobs::start(|| {});
    let t = vm(&j, Stage::Toolpaths).gallery_types();
    assert_eq!(t.len(), ToolpathTypeId::ALL.len());
    let unique: BTreeSet<_> = t.iter().map(|t| t.as_str()).collect();
    assert_eq!(unique.len(), t.len());
}

#[test]
fn galleries_show_the_types_of_the_setup_kind() {
    let names = |t: Vec<ToolpathTypeId>| -> BTreeSet<&'static str> {
        t.into_iter().map(ToolpathTypeId::as_str).collect()
    };
    let not_rotary = names(
        ToolpathTypeId::ALL
            .into_iter()
            .filter(|t| !t.is_rotary())
            .collect(),
    );
    let block = opened("block");
    let r = vm(&block, Stage::Toolpaths);
    assert_eq!(r.setup_kind, Some(SetupKind::ThreeAxis));
    assert_eq!(names(r.gallery_types()), not_rotary);

    let shaft = opened("shaft");
    let r = vm(&shaft, Stage::Toolpaths);
    assert_eq!(r.setup_kind, Some(SetupKind::Rotary));
    let groups: Vec<_> = r.groups.iter().map(|g| g.id).collect();
    assert_eq!(
        groups,
        [
            GroupId::Gallery(Category::Rotary),
            GroupId::Gallery(Category::Utility),
            GroupId::Calculate,
            GroupId::Edit,
            GroupId::Job
        ]
    );
    let t = r.gallery_types();
    assert!(
        t.iter()
            .all(|t| t.is_rotary() || *t == ToolpathTypeId::ManualNc)
    );
    assert_eq!(t.len(), 5);

    // `s2` inherits `indexed` through `based_on`.
    let flip = opened("flipside");
    let r = vm_with(&flip, Stage::Toolpaths, &Selection::Setup("s2".to_owned()));
    assert_eq!(r.setup_kind, Some(SetupKind::Indexed));
    assert_eq!(names(r.gallery_types()), not_rotary);
}

#[test]
fn calculate_selected_follows_the_selection() {
    let j = opened("block");
    let r = vm(&j, Stage::Toolpaths);
    assert!(!r.enabled(Command::CalculateSelected));
    let r = vm_with(&j, Stage::Toolpaths, &Selection::Toolpath("t2".into()));
    assert_eq!(r.selected_toolpaths, ["t2"]);
    assert!(r.enabled(Command::CalculateSelected));
    let r = vm_with(&j, Stage::Toolpaths, &Selection::Setup("s1".into()));
    assert!(r.selected_toolpaths.len() > 1);
    assert!(r.enabled(Command::CalculateSelected));
}

#[test]
fn playback_commands_need_a_simulation() {
    let j = opened("block");
    let r = vm(&j, Stage::Simulate);
    assert!(!r.enabled(Command::PlayPause));
    assert!(!r.enabled(Command::StepBack) && !r.enabled(Command::StepForward));
}
