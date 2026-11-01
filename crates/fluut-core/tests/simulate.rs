// SPDX-License-Identifier: MIT OR Apache-2.0

//! Simulation through the session (D11) on the `block` reference project.

use fluut_core::fluut_sim::{Check, Monitor};
use fluut_core::{NodeId, Session, SimSettings, Targets};
use fluut_model::params::{HeightSpec, Num, PValue};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn block() -> Session {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fluut-cli/tests/projects/block/block.fluut");
    Session::open(&p, &[]).unwrap()
}

fn set(s: &mut Session, tp: &str, path: &str, v: PValue) {
    s.project
        .toolpath_mut(tp)
        .unwrap()
        .params
        .set(path, Some(v))
        .unwrap();
}

fn coarse() -> SimSettings {
    SimSettings {
        error: 0.25,
        ..SimSettings::default()
    }
}

#[test]
fn simulation_node_follows_results_feeds_and_tools() {
    let mut s = block();
    let sim = NodeId::Simulation("s1".into());
    let g0 = s.graph();
    assert!(g0.hash(&sim).is_some());
    // Not a simulation input: the project name.
    s.project.name = "Other".into();
    assert!(!s.graph().changed(&g0).contains(&sim));
    // A result input, a feed and the tool all are.
    set(
        &mut s,
        "t1",
        "passes.stepover",
        PValue::Num(Num::float(2.0)),
    );
    let g1 = s.graph();
    assert!(g1.changed(&g0).contains(&sim));
    set(
        &mut s,
        "t2",
        "tool_params.feed.cut",
        PValue::Num(Num::int(1500)),
    );
    let g2 = s.graph();
    assert_eq!(
        g2.changed(&g1),
        [NodeId::Post("t2".into()), sim.clone()],
        "a feed changes the timeline only"
    );
    s.project.toolpath_mut("t1").unwrap().enabled = Some(false);
    assert!(s.graph().changed(&g2).contains(&sim));
}

#[test]
fn simulate_block_finds_no_errors_and_caches() {
    let mut s = block();
    let st = coarse();
    let err = s
        .simulate("s1", &st, &fluut_core::fluut_sim::Quiet)
        .unwrap_err();
    assert_eq!(err[0].code, "sim.result.missing");
    s.calculate(&Targets::All);
    let r = s
        .simulate("s1", &st, &fluut_core::fluut_sim::Quiet)
        .unwrap();
    // The pockets leave stock on the walls: no gouge. The floor raster
    // (t2) used to rapid down to 2 mm above its floor next to the walls,
    // through the 0.2 mm that t1 left on them; it now feeds below the stock
    // top.
    assert!(r.sim.findings.is_empty(), "{:?}", r.sim.findings);
    let rest = r.sim.rest.unwrap();
    assert!(rest.on_part > 0.0 && rest.max >= 0.1, "{rest:?}");
    // The timeline is the sum of the toolpaths' estimates.
    let total: f64 = ["t1", "t2", "t3"]
        .iter()
        .map(|t| s.time_estimate(t).unwrap())
        .sum();
    assert!((r.sim.duration() - total).abs() < 1e-6 * total);
    assert_eq!(r.sim.toolpaths.len(), 3);
    // Cached until an input changes.
    let again = s
        .simulate("s1", &st, &fluut_core::fluut_sim::Quiet)
        .unwrap();
    assert!(Arc::ptr_eq(&r, &again));
    assert!(s.simulation("s1", &SimSettings::default()).is_none());
    set(
        &mut s,
        "t2",
        "tool_params.feed.cut",
        PValue::Num(Num::int(1500)),
    );
    assert!(s.simulation("s1", &st).is_none());
}

#[test]
fn deliberate_gouge_is_found_on_its_toolpath() {
    let mut s = block();
    // The raster floor pass runs 0.5 mm below the pocket floor.
    set(
        &mut s,
        "t2",
        "heights.bottom",
        PValue::Height(HeightSpec::new("selection", -0.5)),
    );
    let rep = s.calculate(&Targets::All);
    assert!(
        rep.iter().all(|r| r.stats.is_some()),
        "{:?}",
        rep.iter().map(|r| &r.diagnostics).collect::<Vec<_>>()
    );
    let r = s
        .simulate("s1", &coarse(), &fluut_core::fluut_sim::Quiet)
        .unwrap();
    let g: Vec<_> = r
        .sim
        .findings
        .iter()
        .filter(|f| f.check == Check::Gouge)
        .collect();
    assert!(!g.is_empty());
    assert!(g.iter().all(|f| r.sim.toolpaths[f.toolpath].id == "t2"));
    assert!(g.iter().all(|f| (f.depth - 0.5).abs() < 0.05), "{g:?}");
    let d = r
        .diagnostics
        .iter()
        .find(|d| d.code == "sim.gouge")
        .unwrap();
    assert_eq!(d.toolpath.as_deref(), Some("t2"));
    assert!(d.is_error());
}

#[test]
fn rotary_setups_are_refused_and_manual_nc_is_skipped() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fluut-cli/tests/projects/shaft/shaft.fluut");
    let mut s = Session::open(&p, &[]).unwrap();
    s.calculate(&Targets::All);
    let err = s
        .simulate("s1", &coarse(), &fluut_core::fluut_sim::Quiet)
        .unwrap_err();
    // Not `sim.result.missing` for the `manual_nc` stop, which has no tool.
    assert_eq!(err[0].code, "sim.setup.rotary", "{err:?}");
}

struct Cancel(AtomicBool);

impl Monitor for Cancel {
    fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[test]
fn cancelled_simulation_is_not_kept() {
    let mut s = block();
    s.calculate(&Targets::All);
    let err = s
        .simulate("s1", &coarse(), &Cancel(AtomicBool::new(true)))
        .unwrap_err();
    assert_eq!(err[0].code, "sim.cancelled");
    assert!(s.simulation("s1", &coarse()).is_none());
}
