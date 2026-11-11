// SPDX-License-Identifier: MIT OR Apache-2.0

//! `parallel`, `spiral` and `scallop` sample on all threads (fu-9); the
//! toolpath must not depend on the thread count (D01 §6): the same bits
//! on one thread and on many, on a free-form and an analytic part.

mod support;

use fluut_cam::cutter::synth::sculpted;
use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry};
use fluut_cam::{Generated, calculate};
use fluut_model::project::Toolpath;
use support::finish::*;

fn on_threads(n: usize, g: &SetupGeometry, t: &Toolpath) -> Generated {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build()
        .expect("pool");
    let tl = tool(tool_of(t));
    pool.install(|| calculate(&input(g, t, &tl)).unwrap_or_else(|e| panic!("{e}")))
}

/// The exact bits of a result: `Debug` of an `f64` round-trips.
fn bits(r: &Generated) -> String {
    format!("{:?}", r.result)
}

fn same_on_all_threads(g: &SetupGeometry, t: &Toolpath, what: &str) {
    let one = on_threads(1, g, t);
    assert!(one.result.segments().count() > 20, "{what}: empty");
    let one_bits = bits(&one);
    for n in [3, 8] {
        let many = on_threads(n, g, t);
        assert!(one_bits == bits(&many), "{what}: {n} threads differ");
        assert_eq!(
            format!("{:?}", one.diagnostics),
            format!("{:?}", many.diagnostics),
            "{what}"
        );
    }
}

fn freeform() -> SetupGeometry {
    SetupGeometry {
        bodies: vec![SetupBody {
            id: "b-s".into(),
            geom: BodyGeom::Brep(sculpted(4, 4.0, 5)),
        }],
    }
}

const LEADS: &str = "[linking]
retract = \"stay_down\"
    lead_in = { kind = \"arc\", radius = 1, length = 0.5 }
    lead_out = { kind = \"linear\", length = 1 }
";

fn types() -> Vec<(&'static str, String)> {
    vec![
        (
            "parallel",
            "[passes]
stepover = 1.5
"
            .into(),
        ),
        (
            "parallel",
            format!(
                "[passes]
stepover = 2
pass_angle_deg = 30
direction = \"zigzag\"
{LEADS}"
            ),
        ),
        (
            "spiral",
            "[passes]
stepover = 1.5
"
            .into(),
        ),
        (
            "spiral",
            format!(
                "[passes]
stepover = 2
machine_corners = true
{LEADS}"
            ),
        ),
        (
            "scallop",
            "[passes]
stepover = 1.5
"
            .into(),
        ),
        (
            "scallop",
            format!(
                "[passes]
stepover = 2
direction = \"inside_out\"
continuous = true
{LEADS}"
            ),
        ),
    ]
}

#[test]
fn freeform_is_the_same_on_any_thread_count() {
    let g = freeform();
    for (kind, groups) in types() {
        for tool in ["b6"] {
            same_on_all_threads(&g, &tp(kind, tool, &groups), &format!("{kind} {tool}"));
        }
    }
}

#[test]
fn analytic_is_the_same_on_any_thread_count() {
    let g = dome();
    for (kind, groups) in types() {
        for tool in ["b6", "r6"] {
            same_on_all_threads(&g, &tp(kind, tool, &groups), &format!("{kind} {tool}"));
        }
    }
}
