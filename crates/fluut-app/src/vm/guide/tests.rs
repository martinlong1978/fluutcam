// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::jobs::{CalcTargets, Edit, PostRequest};
use crate::session::tests::{opened, project};
use fluut_model::project::DisplayUnit;
use fluut_model::{Num, PValue};
use std::path::PathBuf;

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fluut-app-guide-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn boss_step() -> PathBuf {
    project("boss").with_file_name("boss.step")
}

fn vm(j: &Jobs, state: &GuideState) -> GuideVm {
    GuideVm::new(j, &Selection::None, state)
}

/// Which steps are done, as a string of seven marks.
fn marks(v: &GuideVm) -> String {
    v.steps
        .iter()
        .map(|s| if s.done { 'x' } else { '.' })
        .collect()
}

#[test]
fn the_steps_are_listed_in_the_order_of_a_first_job() {
    let v = vm(&Jobs::start(|| {}), &GuideState::default());
    let ids: Vec<_> = v.steps.iter().map(|s| s.step.id()).collect();
    assert_eq!(
        ids,
        [
            "guide.new_project",
            "guide.import",
            "guide.setup",
            "guide.toolpath",
            "guide.calculate",
            "guide.simulate",
            "guide.post"
        ]
    );
    for s in &v.steps {
        assert!(!s.title.is_empty() && !s.hint.is_empty() && !s.button.is_empty());
    }
}

#[test]
fn every_step_runs_the_ribbon_command_of_the_same_job() {
    let t = ToolpathTypeId::Parallel;
    assert_eq!(GuideStep::NewProject.command(t), Command::NewProject);
    assert_eq!(GuideStep::Import.command(t), Command::ImportModel);
    assert_eq!(
        GuideStep::Setup.command(t),
        Command::NewSetup(SetupKind::ThreeAxis)
    );
    assert_eq!(GuideStep::Toolpath.command(t), Command::AddToolpath(t));
    assert_eq!(GuideStep::Calculate.command(t), Command::Calculate);
    assert_eq!(GuideStep::Simulate.command(t), Command::Simulate);
    assert_eq!(GuideStep::Post.command(t), Command::Post);
    let v = vm(&Jobs::start(|| {}), &GuideState::default());
    for s in &v.steps {
        assert_eq!(s.command, s.step.command(v.toolpath_type));
    }
}

#[test]
fn nothing_is_done_with_no_project_and_only_the_first_step_can_run() {
    let v = vm(&Jobs::start(|| {}), &GuideState::default());
    assert_eq!(marks(&v), ".......");
    assert_eq!(v.done, 0);
    assert!(!v.complete && !v.can_show_setup);
    assert!(v.step(GuideStep::NewProject).current);
    assert_eq!(v.steps.iter().filter(|s| s.current).count(), 1);
    let enabled: Vec<_> = v.steps.iter().map(|s| s.enabled).collect();
    assert_eq!(enabled, [true, false, false, false, false, false, false]);
}

#[test]
fn the_steps_tick_as_the_project_grows() {
    let dir = temp("grow");
    let mut j = Jobs::start(|| {});
    let mut state = GuideState::default();

    j.new_project(dir.join("job.fluut"), DisplayUnit::Mm);
    j.wait();
    let v = vm(&j, &state);
    assert_eq!(marks(&v), "x......");
    assert!(v.step(GuideStep::Import).current);
    // A model can come in; no toolpath step can run yet.
    let enabled: Vec<_> = v.steps.iter().map(|s| s.enabled).collect();
    assert_eq!(enabled, [true, true, true, false, false, false, false]);

    j.import_model(boss_step());
    j.wait();
    assert_eq!(marks(&vm(&j, &state)), "xx.....");

    j.add_setup(SetupKind::ThreeAxis, None);
    j.wait();
    let v = vm(&j, &state);
    assert_eq!(marks(&v), "xxx....");
    assert!(v.can_show_setup);
    assert!(v.step(GuideStep::Toolpath).enabled);
    assert!(!v.step(GuideStep::Calculate).enabled);

    j.add_toolpath("s1", ToolpathTypeId::Parallel);
    j.wait();
    let v = vm(&j, &state);
    assert_eq!(marks(&v), "xxxx...");
    assert!(v.step(GuideStep::Calculate).current);
    assert!(v.step(GuideStep::Calculate).enabled);

    j.calculate(CalcTargets::All);
    j.wait();
    assert_eq!(marks(&vm(&j, &state)), "xxxxx..");

    j.simulate("s1");
    j.wait();
    assert_eq!(marks(&vm(&j, &state)), "xxxxxx.");

    // Posting leaves no mark in the project: the guide notes the job.
    j.post(PostRequest {
        post: "fluut/linuxcnc".into(),
        output: dir.join("job.ngc"),
        date: "2026-11-11".into(),
        time: "12:00:00".into(),
    });
    j.wait();
    state.observe(j.last.as_ref());
    let v = vm(&j, &state);
    assert_eq!(marks(&v), "xxxxxxx");
    assert!(v.complete && v.done == 7);
    assert!(v.steps.iter().all(|s| !s.current));
    assert!(dir.join("job.ngc").exists());

    // Another project starts the guide again.
    j.open(project("boss"));
    j.wait();
    state.observe(j.last.as_ref());
    assert!(!state.posted);
    assert!(!vm(&j, &state).step(GuideStep::Post).done);
}

#[test]
fn an_edit_after_calculating_takes_the_tick_back() {
    let mut j = opened("boss");
    let state = GuideState::default();
    assert_eq!(marks(&vm(&j, &state)), "xxxx...");
    j.calculate(CalcTargets::All);
    j.wait();
    assert!(vm(&j, &state).step(GuideStep::Calculate).done);
    j.edit(Edit::SetParam {
        toolpath: "t1".into(),
        path: "passes.step_down".into(),
        value: Some(PValue::Num(Num::float(3.0))),
    });
    j.wait_snapshot();
    assert!(!vm(&j, &state).step(GuideStep::Calculate).done);
}

#[test]
fn nothing_can_run_while_a_job_runs() {
    let mut j = opened("boss");
    j.calculate(CalcTargets::All);
    let v = vm(&j, &GuideState::default());
    assert!(v.steps.iter().all(|s| !s.enabled));
    assert!(!v.can_show_setup);
    j.wait();
}

#[test]
fn the_toolpath_step_offers_the_types_of_the_setup_and_the_tools_of_the_store() {
    let j = opened("boss");
    let v = vm(&j, &GuideState::default());
    assert_eq!(v.toolpath_type, ToolpathTypeId::Parallel);
    assert!(v.types.iter().any(|(t, _)| *t == ToolpathTypeId::Pocket2d));
    // A three-axis Setup takes no rotary types.
    assert!(
        !v.types
            .iter()
            .any(|(t, _)| *t == ToolpathTypeId::RotaryRough)
    );
    assert_eq!(v.tools[0].0, "");
    assert!(v.tools.len() > 1, "the starter tools are listed");
    assert!(v.tools[1].0.contains('#'));
    // A type this Setup cannot take falls back to the first offered.
    let state = GuideState {
        toolpath_type: ToolpathTypeId::RotaryRough,
        ..GuideState::default()
    };
    let v = vm(&j, &state);
    assert_eq!(v.toolpath_type, v.types[0].0);
    assert_eq!(
        v.step(GuideStep::Toolpath).command,
        Command::AddToolpath(v.types[0].0)
    );
    // A tool that is not in the store falls back to the default.
    let state = GuideState {
        tool: Some("nowhere#none".into()),
        ..GuideState::default()
    };
    assert_eq!(vm(&j, &state).tool, "");
    let reference = vm(&j, &GuideState::default()).tools[1].0.clone();
    let state = GuideState {
        tool: Some(reference.clone()),
        ..GuideState::default()
    };
    assert_eq!(vm(&j, &state).tool, reference);
}

#[test]
fn a_failed_toolpath_leaves_no_tool_waiting() {
    let mut state = GuideState {
        pending_tool: Some("a#b".into()),
        ..GuideState::default()
    };
    let saved = (JobKind::Save, Outcome::Saved { file: "x".into() });
    state.observe(Some(&saved));
    assert!(state.pending_tool.is_some());
    state.observe(Some(&(JobKind::AddToolpath, Outcome::Failed(Vec::new()))));
    assert!(state.pending_tool.is_none());
}

#[test]
fn undo_takes_the_toolpath_and_the_ticks_after_it_back() {
    let mut j = opened("boss");
    let state = GuideState::default();
    j.calculate(CalcTargets::All);
    j.wait();
    assert_eq!(marks(&vm(&j, &state)), "xxxxx..");
    j.undo();
    j.wait();
    let v = vm(&j, &state);
    assert_eq!(marks(&v), "xxx....");
    assert!(v.step(GuideStep::Toolpath).current);
}
