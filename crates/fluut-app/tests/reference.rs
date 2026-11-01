// SPDX-License-Identifier: MIT OR Apache-2.0

//! The M5 exit criterion: open, calculate and post a reference project from
//! the UI, and get the G-code `fluut post` writes.
//!
//! The reference projects and their golden G-code are the CLI's
//! (`crates/fluut-cli/tests/`); the CLI test checks that `fluut post` gives
//! exactly those files, with the same date and time as here.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use fluut_app::FluutApp;
use fluut_app::jobs::{CalcTargets, Jobs, Outcome, PostRequest};
use fluut_app::session::ToolpathStatus;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DATE: &str = "2026-11-01";
const TIME: &str = "12:00:00";

fn cli(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fluut-cli/tests")
        .join(p)
}

fn out_dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn same_as_golden(out: &Path, golden: &str) {
    let got = std::fs::read_to_string(out).unwrap();
    let want = std::fs::read_to_string(cli(&format!("golden/{golden}"))).unwrap();
    if got != want {
        let line = got
            .lines()
            .zip(want.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        panic!(
            "{golden} differs at line {}:\n  got:  {:?}\n  want: {:?}",
            line + 1,
            got.lines().nth(line),
            want.lines().nth(line)
        );
    }
}

/// Through the job layer: open, calculate all with progress, post.
fn through_jobs(project: &str, post: &str, golden: &str) {
    let mut j = Jobs::start(|| {});
    j.open(cli(&format!("projects/{project}/{project}.fluut")));
    assert!(matches!(j.wait(), Some(Outcome::Opened { .. })));

    j.calculate(CalcTargets::All);
    let progress = j.running.as_ref().unwrap().cx.progress.clone();
    match j.wait() {
        Some(Outcome::Calculated { .. }) => {}
        o => panic!("{project}: {o:?}"),
    }
    let (done, total) = progress.get();
    assert_eq!(done, total);
    let s = j.snapshot.clone().unwrap();
    for st in &s.project.setups {
        for t in &st.toolpaths {
            assert!(
                matches!(
                    s.status(&t.id),
                    ToolpathStatus::Done | ToolpathStatus::Warning
                ),
                "{project} {}: {:?}",
                t.id,
                s.status(&t.id)
            );
        }
    }

    let out = out_dir(&format!("jobs-{golden}")).join("out.nc");
    j.post(PostRequest {
        post: post.into(),
        output: out.clone(),
        date: DATE.into(),
        time: TIME.into(),
    });
    assert_eq!(
        j.wait(),
        Some(Outcome::Posted {
            files: vec![out.clone()]
        })
    );
    same_as_golden(&out, golden);
}

#[test]
fn block_posts_like_the_cli() {
    through_jobs("block", "fluut/linuxcnc", "block-linuxcnc.ngc");
    through_jobs("block", "fluut/grbl", "block-grbl.gcode");
}

#[test]
fn boss_posts_like_the_cli() {
    through_jobs("boss", "fluut/linuxcnc", "boss-linuxcnc.ngc");
}

#[test]
fn ridge_posts_like_the_cli() {
    through_jobs("ridge", "fluut/linuxcnc", "ridge-linuxcnc.ngc");
}

#[test]
fn plate_posts_like_the_cli() {
    through_jobs("plate", "fluut/linuxcnc", "plate-linuxcnc.ngc");
}

/// Post without calculating first: the post job calculates what is stale.
#[test]
fn post_calculates_what_is_stale() {
    let mut j = Jobs::start(|| {});
    j.open(cli("projects/boss/boss.fluut"));
    j.wait();
    let out = out_dir("stale").join("out.nc");
    j.post(PostRequest {
        post: "fluut/linuxcnc".into(),
        output: out.clone(),
        date: DATE.into(),
        time: TIME.into(),
    });
    assert!(matches!(j.wait(), Some(Outcome::Posted { .. })));
    same_as_golden(&out, "boss-linuxcnc.ngc");
}

type State = Option<FluutApp>;

fn app<'a>(h: &'a mut Harness<'static, State>) -> &'a mut FluutApp {
    h.state_mut().as_mut().expect("built on the first frame")
}

/// Steps the window until no job runs; the worker has its own thread.
fn settle(h: &mut Harness<'static, State>) {
    for _ in 0..4000 {
        h.step();
        if !app(h).jobs.is_busy() {
            h.step();
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("job did not finish");
}

fn fixed_clock() -> (String, String) {
    (DATE.into(), TIME.into())
}

/// The same through the window: the ribbon's Open, Calculate all and Post
/// buttons, found by their accessible labels.
#[test]
fn window_opens_calculates_and_posts() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_ui_state(
            |ui, app: &mut State| {
                app.get_or_insert_with(|| FluutApp::new(ui.ctx(), None, None, Instant::now()))
                    .frame(ui);
            },
            None,
        );
    h.step();
    let out = out_dir("window").join("block.ngc");
    {
        let a = app(&mut h);
        a.ui.open_path = cli("projects/block/block.fluut").display().to_string();
        a.ui.clock = fixed_clock;
    }
    h.get_by_label("Open").click();
    settle(&mut h);
    h.get_by_label("Opened block.fluut");

    h.get_by_label("Calculate all").click();
    // One frame handles the click, the next shows the running job.
    h.run_steps(2);
    // While it runs, Cancel (in the ribbon and the job strip) is enabled
    // and Calculate is not.
    if app(&mut h).jobs.is_busy() {
        let cancels: Vec<_> = h.get_all_by_label("Cancel").collect();
        assert_eq!(cancels.len(), 2);
        assert!(cancels.iter().all(|n| !n.accesskit_node().is_disabled()));
        assert!(
            h.get_by_label("Calculate all")
                .accesskit_node()
                .is_disabled()
        );
    }
    settle(&mut h);
    h.get_by_label("Calculated 3 toolpaths");
    h.get_by_label("Calculated with warnings");

    app(&mut h).ui.post_output = out.display().to_string();
    h.get_by_label("Post").click();
    settle(&mut h);
    h.get_by_label(&format!("Wrote {}", out.display()));
    same_as_golden(&out, "block-linuxcnc.ngc");
}
