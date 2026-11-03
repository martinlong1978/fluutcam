// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::jobs::{CalcTargets, Edit, Jobs, Outcome};
use fluut_model::{Num, PValue};

pub(crate) fn project(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fluut-cli/tests/projects")
        .join(name)
        .join(format!("{name}.fluut"))
}

pub(crate) fn opened(name: &str) -> Jobs {
    let mut j = Jobs::start(|| {});
    j.open(project(name));
    let out = j.wait();
    assert_eq!(
        out,
        Some(Outcome::Opened {
            file: format!("{name}.fluut")
        })
    );
    j
}

#[test]
fn open_gives_a_snapshot_with_scenes() {
    let j = opened("boss");
    let s = j.snapshot.clone().unwrap();
    assert_eq!(s.project.name, "Boss");
    assert_eq!(s.status("t1"), ToolpathStatus::NotCalculated);
    let scene = &s.scenes["s1"];
    // Every face of the boss body has triangles.
    assert!(!scene.faces.is_empty());
    for i in 0..scene.faces.len() as u32 {
        assert!(
            scene.mesh.face.contains(&i),
            "face {} has no triangles",
            scene.faces[i as usize]
        );
    }
    assert!(scene.edges.segment_count() > 0);
    assert!(scene.stock.is_some());
    assert!(s.paths.is_empty());
}

#[test]
fn open_of_a_missing_file_fails() {
    let mut j = Jobs::start(|| {});
    j.open(PathBuf::from("/no/such/project.fluut"));
    match j.wait() {
        Some(Outcome::Failed(d)) => assert_eq!(d[0].code, "model.project.load"),
        o => panic!("{o:?}"),
    }
    assert!(j.snapshot.is_none());
}

#[test]
fn calculate_reports_progress_in_order() {
    let mut j = opened("block");
    j.calculate(CalcTargets::All);
    let progress = j.running.as_ref().unwrap().cx.progress.clone();
    let mut steps = Vec::new();
    while j.running.is_some() {
        assert!(j.wait_event());
        if let Some(t) = j.running.as_ref().and_then(|r| r.toolpath.clone())
            && steps.last() != Some(&t)
        {
            steps.push(t);
        }
    }
    assert_eq!(j.last.clone().unwrap().1, Outcome::Calculated { count: 3 });
    assert_eq!(steps, ["t1", "t2", "t3"]);
    assert_eq!(progress.get(), (3, 3));
    let s = j.snapshot.clone().unwrap();
    for t in ["t1", "t2", "t3"] {
        assert!(
            matches!(s.status(t), ToolpathStatus::Done | ToolpathStatus::Warning),
            "{t}: {:?}",
            s.status(t)
        );
        assert!(s.paths[t].lines.segment_count() > 0);
    }
    // The cut-out goes below the stock: a warning on t3 (CLI golden test).
    assert!(
        s.diagnostics("t3")
            .iter()
            .any(|d| d.code == "param.heights.below_stock")
    );
}

#[test]
fn cancel_stops_between_toolpaths() {
    let mut j = opened("block");
    let done = |j: &Jobs| {
        let s = j.snapshot.clone().unwrap();
        ["t1", "t2", "t3"]
            .iter()
            .filter(|t| s.status(t) != ToolpathStatus::NotCalculated)
            .count()
    };
    // Cancelled at once: the worker stops at its first check, before the
    // first or the second toolpath.
    j.calculate(CalcTargets::All);
    j.cancel();
    assert_eq!(j.wait(), Some(Outcome::Cancelled));
    assert!(done(&j) <= 1, "{}", done(&j));
    // Cancelled during the first toolpath: the rest do not run.
    j.calculate(CalcTargets::All);
    while j.running.as_ref().is_some_and(|r| r.toolpath.is_none()) {
        assert!(j.wait_event());
    }
    j.cancel();
    assert_eq!(j.wait(), Some(Outcome::Cancelled));
    assert!(done(&j) < 3, "{}", done(&j));
}

#[test]
fn an_r_edit_makes_the_toolpath_stale() {
    let mut j = opened("boss");
    j.calculate(CalcTargets::Toolpaths(vec!["t1".into()]));
    assert_eq!(j.wait(), Some(Outcome::Calculated { count: 1 }));
    assert_eq!(
        j.snapshot.as_ref().unwrap().status("t1"),
        ToolpathStatus::Done
    );
    j.edit(Edit::SetParam {
        toolpath: "t1".into(),
        path: "passes.step_down".into(),
        value: Some(PValue::Num(Num::float(3.0))),
    });
    let s = j.wait_snapshot().unwrap();
    assert_eq!(s.edit_error, None);
    assert_eq!(s.status("t1"), ToolpathStatus::Stale);
    assert!(!s.paths.contains_key("t1"));
    // A P field (feed) keeps the result (D15 §1.8).
    j.calculate(CalcTargets::All);
    j.wait();
    j.edit(Edit::SetParam {
        toolpath: "t1".into(),
        path: "tool_params.feed.cut".into(),
        value: Some(PValue::Num(Num::float(900.0))),
    });
    let s = j.wait_snapshot().unwrap();
    assert_eq!(s.status("t1"), ToolpathStatus::Done);
}

#[test]
fn a_bad_edit_is_reported() {
    let mut j = opened("boss");
    j.edit(Edit::SetParam {
        toolpath: "t1".into(),
        path: "passes.no_such_field".into(),
        value: None,
    });
    let s = j.wait_snapshot().unwrap();
    assert!(s.edit_error.is_some());
}

#[test]
fn an_error_shows_under_its_field() {
    let mut j = opened("boss");
    j.edit(Edit::SetParam {
        toolpath: "t1".into(),
        path: "passes.step_down".into(),
        value: Some(PValue::Num(Num::float(-1.0))),
    });
    let s = j.wait_snapshot().unwrap();
    assert_eq!(s.status("t1"), ToolpathStatus::Error);
    assert!(
        s.diagnostics("t1")
            .iter()
            .any(|d| d.is_error() && d.field.as_deref() == Some("passes.step_down")),
        "{:?}",
        s.diagnostics("t1")
    );
}

#[test]
fn numbered_files() {
    assert_eq!(
        numbered(Path::new("d/out.ngc"), 2),
        PathBuf::from("d/out.2.ngc")
    );
    let (d, t) = now_utc();
    assert_eq!((d.len(), t.len()), (10, 8));
}

#[test]
fn simulate_then_scrub_through_the_worker() {
    use crate::vm::playback::{Playback, PlaybackVm, StockColour};
    let mut j = opened("boss");
    j.simulate("s1");
    assert_eq!(j.wait(), Some(Outcome::Simulated { findings: 0 }));
    let s = j.snapshot.clone().unwrap();
    // Calculated on the way, with its time estimate.
    assert_eq!(s.status("t1"), ToolpathStatus::Done);
    let v = s.sims["s1"].clone();
    assert!((v.duration - s.times["t1"]).abs() < 1e-9 && v.duration > 0.0);
    // The final stock arrives as a frame.
    while j.frame.is_none() {
        assert!(j.wait_event());
    }
    let end = j.frame.clone().unwrap();
    let full = end.volume();
    // Scrubbing to the start gives the uncut stock: more material.
    j.scrub("s1", 0.0);
    while j.scrubbing > 0 {
        assert!(j.wait_event());
    }
    let start = j.frame.clone().unwrap();
    assert_eq!(start.t, 0.0);
    assert!(start.volume() > full + 100.0);
    // The playback view model over this simulation.
    let mut p = Playback::default();
    p.reset("s1", v.duration);
    let vm = PlaybackVm::new(&v, &p, StockColour::Plain);
    assert_eq!(vm.clips.len(), 1);
    assert!(vm.clips[0].current && vm.clips[0].to == 1.0);
    assert!(vm.findings.is_empty());
    assert_eq!(vm.play_label, "Play");
    p.toggle(v.duration);
    assert_eq!(
        PlaybackVm::new(&v, &p, StockColour::Plain).play_label,
        "Pause"
    );
    // An edit of the toolpath makes the simulation stale.
    j.edit(Edit::SetParam {
        toolpath: "t1".into(),
        path: "passes.step_down".into(),
        value: Some(PValue::Num(Num::float(3.0))),
    });
    assert!(j.wait_snapshot().unwrap().sims.is_empty());
}

/// Scrubs to `t` and waits for the frame.
fn frame_at(j: &mut Jobs, setup: &str, t: f64) -> Arc<Frame> {
    j.scrub(setup, t);
    while j.scrubbing > 0 {
        assert!(j.wait_event());
    }
    j.frame.clone().unwrap()
}

#[test]
fn step_controls_and_incremental_frames() {
    use crate::vm::playback::{Playback, PlaybackVm, StockColour};
    use fluut_core::fluut_sim::Step;
    let mut j = opened("block");
    j.simulate("s1");
    assert!(matches!(j.wait(), Some(Outcome::Simulated { .. })));
    let s = j.snapshot.clone().unwrap();
    let v = s.sims["s1"].clone();
    let k = v.clips.len();
    assert!(k >= 2);
    let mut p = Playback::default();
    p.reset("s1", v.duration);
    // At the end: back only.
    let vm = PlaybackVm::new(&v, &p, StockColour::Plain);
    assert!(vm.can_back && !vm.can_forward);
    assert!(vm.legend.is_empty());
    // Back one toolpath at a time: the clips' ends, then the start.
    p.toggle(v.duration);
    p.seek(v.duration, v.duration);
    for i in (0..k - 1).rev() {
        p.step(&v, Step::Toolpath, false);
        assert!(!p.playing);
        assert_eq!(p.t, v.clips[i].end);
    }
    p.step(&v, Step::Toolpath, false);
    assert_eq!(p.t, 0.0);
    let vm = PlaybackVm::new(&v, &p, StockColour::Plain);
    assert!(!vm.can_back && vm.can_forward);
    // Moves are shorter than passes, passes than toolpaths.
    p.step(&v, Step::Move, true);
    let one_move = p.t;
    p.seek(0.0, v.duration);
    p.step(&v, Step::Pass, true);
    let one_pass = p.t;
    assert!(0.0 < one_move && one_move <= one_pass && one_pass <= v.clips[0].end);
    p.step(&v, Step::Move, false);
    assert!(p.t < one_pass);
    // The frames: the whole stock first, then a step forward by one move
    // rebuilds only some blocks and shares the rest.
    let a = frame_at(&mut j, "s1", 0.0);
    let n = a.blocks.as_ref().unwrap().len();
    let mid = 0.5 * (v.clips[1].start + v.clips[1].end);
    let b = frame_at(&mut j, "s1", mid);
    let after = v.step(mid, Step::Move, true);
    let c = frame_at(&mut j, "s1", after);
    assert!(c.rebuilt < n, "{} of {n}", c.rebuilt);
    let shared = b
        .blocks
        .as_ref()
        .unwrap()
        .iter()
        .zip(c.blocks.as_ref().unwrap())
        .filter(|(x, y)| Arc::ptr_eq(x, y))
        .count();
    assert_eq!(shared + c.rebuilt, n);
    assert!(c.volume() <= b.volume() + 1e-6 && b.volume() < a.volume());
    // The legends.
    assert!(v.has_part());
    let vm = PlaybackVm::new(&v, &p, StockColour::Rest);
    let labels: Vec<&str> = vm.legend.iter().map(|l| l.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Above tolerance",
            "Within tolerance",
            "Gouge",
            "No part below"
        ]
    );
    let vm = PlaybackVm::new(&v, &p, StockColour::Toolpath);
    assert_eq!(vm.legend.len(), k + 1);
    assert_eq!(vm.legend[k].label, "Not cut");
    assert_ne!(vm.legend[0].colour, vm.legend[1].colour);
}
