// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::job::{Feeds, MachineInfo, Provenance, ToolInfo};
use crate::{Post, resolve_post};
use fluut_geom::Point3;
use fluut_model::{DefRef, MemStore};
use fluut_path::{FeedRef, MotionInfo, Pass, PassKind, ToolpathResult};
use std::sync::Arc;

const POST: &str = r#"
id = "t"
version = "1.0"
[arcs]
enabled = false
[words.X]
prefix = "X"
when = "changed"
[words.Y]
prefix = "Y"
when = "changed"
[words.Z]
prefix = "Z"
when = "changed"
[words.F]
decimals = 0
when = "changed"
[blocks.header]
lines = ["%", "G21 G90"]
[blocks.footer]
lines = ["M30", "%"]
[blocks.tool_change]
lines = ["T[T] M6", { when = "machine.tool_changer == 'manual'", line = "M0 ([TOOLNAME])" }]
[blocks.spindle_on]
lines = ["M3 S[S]"]
[blocks.spindle_off]
lines = ["M5"]
[blocks.toolpath_start]
lines = ["([TOOLPATH_NAME])"]
[blocks.rapid]
lines = ["G0 [X] [Y] [Z]"]
[blocks.feed]
lines = ["G1 [X] [Y] [Z] F[F]"]
"#;

fn compiled(extra: &str) -> CompiledPost {
    let mut s = MemStore::new();
    s.insert_toml(POST).unwrap();
    s.insert_toml(&format!("id='c'\nversion='1.0'\nextends='t'\n{extra}"))
        .unwrap();
    let r = resolve_post(&s, &DefRef::parse("c").unwrap(), None).unwrap();
    CompiledPost::compile(Post::load(&r).unwrap()).unwrap()
}

/// Line moves from `start` through `moves`, one pass.
fn result(start: [f64; 3], moves: &[(Motion, FeedRef, [f64; 3])]) -> Arc<ToolpathResult> {
    let p = |c: [f64; 3]| AxisPos::xyz(Point3::new(c[0], c[1], c[2]));
    let mut a = p(start);
    let segs = moves
        .iter()
        .map(|&(motion, feed, to)| {
            let b = p(to);
            let s = Segment::Line {
                a,
                b,
                m: MotionInfo::new(motion, feed, 0.01),
            };
            a = b;
            s
        })
        .collect();
    Arc::new(ToolpathResult::new(vec![Pass::new(PassKind::Cut, segs)]).unwrap())
}

fn pocket() -> Arc<ToolpathResult> {
    use FeedRef as F;
    use Motion as M;
    result(
        [0.0, 0.0, 10.0],
        &[
            (M::Rapid, F::Rapid, [0.0, 0.0, 5.0]),
            (M::Plunge, F::Plunge, [0.0, 0.0, -1.0]),
            (M::Cut, F::Cut, [10.0, 0.0, -1.0]),
            (M::Cut, F::Cut, [10.0, 5.0, -1.0]),
            (M::Retract, F::Rapid, [10.0, 5.0, 5.0]),
        ],
    )
}

fn toolpath(name: &str, tool: u32, r: Arc<ToolpathResult>) -> ToolpathJob {
    ToolpathJob {
        name: name.into(),
        kind: "pocket_2d".into(),
        tool: ToolInfo {
            number: tool,
            name: format!("flat {tool}"),
            diameter: 6.0,
            corner_radius: 0.0,
        },
        spindle_rpm: 12000.0,
        coolant: Coolant::Off,
        feeds: Feeds {
            cut: 1000.0,
            plunge: 300.0,
            ..Feeds::default()
        },
        safe_z: 5.0,
        result: r,
    }
}

fn job(tps: Vec<ToolpathJob>) -> PostJob {
    PostJob {
        provenance: Provenance {
            post_id: "c".into(),
            ..Provenance::default()
        },
        machine: MachineInfo {
            name: "router".into(),
            tool_changer: "atc".into(),
            has_rotary: false,
        },
        setups: vec![SetupJob {
            name: "Top".into(),
            toolpaths: tps,
            ..SetupJob::default()
        }],
        ..PostJob::default()
    }
}

fn lines(out: &PostOutput) -> Vec<&str> {
    out.files[0].lines().collect()
}

#[test]
fn minimal_program() {
    let out = post(&job(vec![toolpath("Pocket", 1, pocket())]), &compiled("")).unwrap();
    let v = env!("CARGO_PKG_VERSION");
    assert_eq!(
        out.files[0],
        format!(
            "(FluutCAM {v})\n(Post: c)\n%\nG21 G90\nT1 M6\nM3 S12000\n(Pocket)\n\
             G0 X0 Y0 Z5\nG1 Z-1 F300\nG1 X10 F1000\nG1 Y5\nG0 Z5\nM5\nM30\n%\n"
        )
    );
}

#[test]
fn line_with_only_suppressed_words_and_current_motion_code_dropped() {
    use FeedRef as F;
    use Motion as M;
    let r = result(
        [0.0, 0.0, 10.0],
        &[
            (M::Rapid, F::Rapid, [0.0, 0.0, 5.0]),
            (M::Rapid, F::Rapid, [10.0, 0.0, 5.0]),
        ],
    );
    let c = compiled("[blocks.rapid]\nlines = ['G0 [X] [Y]', 'G0 [Z]', 'M7 [Z]']");
    let out = post(&job(vec![toolpath("R", 1, r)]), &c).unwrap();
    let l = lines(&out);
    let i = l.iter().position(|x| *x == "G0 X0 Y0").unwrap();
    assert_eq!(&l[i..i + 5], ["G0 X0 Y0", "G0 Z5", "M7", "G0 X10", "M7"]);
}

#[test]
fn tool_change_only_on_change_and_axes_rewritten_after() {
    let j = job(vec![
        toolpath("A", 1, pocket()),
        toolpath("B", 1, pocket()),
        toolpath("C", 2, pocket()),
    ]);
    let out = post(&j, &compiled("")).unwrap();
    let l = lines(&out);
    assert_eq!(l.iter().filter(|x| x.ends_with("M6")).count(), 2);
    let t2 = l.iter().position(|x| *x == "T2 M6").unwrap();
    assert_eq!(l[t2 - 1], "M5");
    assert_eq!(l[t2 + 1], "M3 S12000");
    // After the tool change every axis is written again.
    assert_eq!(l[t2 + 3], "G0 X0 Y0 Z5");
    // Toolpath B starts at the same place as A ended, but `([TOOLPATH_NAME])`
    // is a non-motion block, so its first move is written in full too.
    let b = l.iter().position(|x| *x == "(B)").unwrap();
    assert_eq!(l[b + 1], "G0 X0 Y0 Z5");
}

#[test]
fn conditional_line() {
    let mut j = job(vec![toolpath("A", 3, pocket())]);
    j.machine.tool_changer = "manual".into();
    let out = post(&j, &compiled("")).unwrap();
    assert!(lines(&out).contains(&"M0 (flat 3)"));
}

#[test]
fn line_numbers_skip_percent() {
    let out = post(
        &job(vec![toolpath("A", 1, pocket())]),
        &compiled("[line_numbering]\nenabled=true\nstart=10\nincrement=5"),
    )
    .unwrap();
    let l = lines(&out);
    assert_eq!(l[2], "%");
    assert_eq!(l[3], "N10 G21 G90");
    assert_eq!(l[4], "N15 T1 M6");
    assert_eq!(*l.last().unwrap(), "%");
}

#[test]
fn crlf_and_inch() {
    let out = post(
        &job(vec![toolpath("A", 1, pocket())]),
        &compiled("[output]\nline_ending='crlf'\nunits='inch'"),
    )
    .unwrap();
    assert!(out.files[0].contains("\r\nG1 X0.394 F39\r\n"));
    assert!(!out.files[0].replace("\r\n", "").contains('\n'));
}

#[test]
fn line_map_covers_motion_lines() {
    let out = post(&job(vec![toolpath("A", 1, pocket())]), &compiled("")).unwrap();
    let l = lines(&out);
    assert_eq!(out.line_map.len(), 5);
    for (k, e) in out.line_map.iter().enumerate() {
        assert!(l[e.line].starts_with('G'), "{}", l[e.line]);
        assert_eq!((e.setup, e.toolpath, e.pass, e.segment), (0, 0, 0, k));
    }
}

#[test]
fn plunge_and_retract_blocks_used_when_defined() {
    let out = post(
        &job(vec![toolpath("A", 1, pocket())]),
        &compiled("[blocks.plunge]\nlines=['G1 [Z] F[F] (plunge)']\n[blocks.retract]\nlines=['G0 [Z] (up)']\n[blocks.first_feed]\nlines=['G1 [X] [Y] [Z] F[F] (first)']"),
    )
    .unwrap();
    let l = lines(&out);
    assert!(l.contains(&"G1 Z-1 F300 (plunge)"));
    assert!(l.contains(&"G1 X10 F1000 (first)"));
    assert!(l.contains(&"G1 Y5"));
    // The final move is a retract at rapid feed, so it is a rapid event.
    assert!(l.contains(&"G0 Z5"));
}

#[test]
fn unsupported_segments_error() {
    let r = Arc::new(
        ToolpathResult::new(vec![Pass::new(
            PassKind::Cut,
            vec![Segment::Marker {
                kind: fluut_path::MarkerKind::Stop,
            }],
        )])
        .unwrap(),
    );
    assert!(matches!(
        post(&job(vec![toolpath("A", 1, r)]), &compiled("")),
        Err(EmitError::Unsupported { .. })
    ));
}
