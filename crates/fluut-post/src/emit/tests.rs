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
    compiled_stack(&[extra])
}

/// `extras` as a chain of child layers over the base post, each extending
/// the one before, so they may set the same tables.
fn compiled_stack(extras: &[&str]) -> CompiledPost {
    let mut s = MemStore::new();
    s.insert_toml(POST).unwrap();
    let mut parent = "t".to_owned();
    for (i, x) in extras.iter().enumerate() {
        let id = format!("c{i}");
        s.insert_toml(&format!(
            "id='{id}'\nversion='1.0'\nextends='{parent}'\n{x}"
        ))
        .unwrap();
        parent = id;
    }
    let r = resolve_post(&s, &DefRef::parse(&parent).unwrap(), None).unwrap();
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

// --- arcs and helices (Task 7) ----------------------------------------------

const ARCS: &str = r#"
[arcs]
enabled = true
[words.I]
prefix = "I"
[words.J]
prefix = "J"
[words.K]
prefix = "K"
[blocks.arc_cw]
lines = ["G2 [X] [Y] [Z] [I] [J] [K] F[F]"]
[blocks.arc_ccw]
lines = ["G3 [X] [Y] [Z] [I] [J] [K] F[F]"]
"#;

fn p3(c: [f64; 3]) -> AxisPos {
    AxisPos::xyz(Point3::new(c[0], c[1], c[2]))
}

fn unit(v: [f64; 3]) -> fluut_geom::UnitVec3 {
    fluut_geom::UnitVec3::new(fluut_geom::Vec3::new(v[0], v[1], v[2])).unwrap()
}

/// One arc (or helix) segment as the whole toolpath.
fn arc_result(
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
    n: [f64; 3],
    sweep: f64,
    helix: bool,
) -> Arc<ToolpathResult> {
    let m = MotionInfo::new(Motion::Cut, FeedRef::Cut, 0.01);
    let (a, b, centre, normal) = (p3(a), p3(b), Point3::new(c[0], c[1], c[2]), unit(n));
    let seg = if helix {
        Segment::Helix {
            a,
            b,
            centre,
            normal,
            sweep,
            m,
        }
    } else {
        Segment::Arc {
            a,
            b,
            centre,
            normal,
            sweep,
            m,
        }
    };
    Arc::new(ToolpathResult::new(vec![Pass::new(PassKind::Cut, vec![seg])]).unwrap())
}

fn motion_lines(extra: &str, r: Arc<ToolpathResult>) -> Vec<String> {
    let out = post(
        &job(vec![toolpath("A", 1, r)]),
        &compiled_stack(&[ARCS, extra]),
    )
    .unwrap();
    out.line_map
        .iter()
        .map(|e| lines(&out)[e.line].to_owned())
        .collect()
}

const Q: f64 = std::f64::consts::FRAC_PI_2;

#[test]
fn quarter_arcs_in_each_plane() {
    let o = [0.0; 3];
    let cases = [
        // (start, end, normal, sweep, expected)
        (
            [10.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [0.0, 0.0, 1.0],
            Q,
            "G3 X0 Y10 Z0 I-10 J0 F1000",
        ),
        (
            [10.0, 0.0, 0.0],
            [0.0, -10.0, 0.0],
            [0.0, 0.0, -1.0],
            Q,
            "G2 X0 Y-10 Z0 I-10 J0 F1000",
        ),
        (
            [10.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [0.0, 0.0, -1.0],
            -Q,
            "G3 X0 Y10 Z0 I-10 J0 F1000",
        ),
        (
            [10.0, 0.0, 0.0],
            [0.0, 0.0, -10.0],
            [0.0, 1.0, 0.0],
            Q,
            "G3 X0 Y0 Z-10 I-10 K0 F1000",
        ),
        (
            [0.0, 10.0, 0.0],
            [0.0, 0.0, 10.0],
            [1.0, 0.0, 0.0],
            Q,
            "G3 X0 Y0 Z10 J-10 K0 F1000",
        ),
        (
            [0.0, 10.0, 0.0],
            [0.0, 0.0, -10.0],
            [-1.0, 0.0, 0.0],
            Q,
            "G2 X0 Y0 Z-10 J-10 K0 F1000",
        ),
    ];
    for (a, b, n, sw, want) in cases {
        assert_eq!(motion_lines("", arc_result(a, b, o, n, sw, false)), [want]);
    }
}

#[test]
fn full_circle_by_max_sweep() {
    let a = [10.0, 0.0, 0.0];
    let r = || arc_result(a, a, [0.0; 3], [0.0, 0.0, 1.0], 4.0 * Q, false);
    assert_eq!(motion_lines("", r()), ["G3 X10 Y0 Z0 I-10 J0 F1000"]);
    assert_eq!(
        motion_lines("[arcs]\nmax_sweep_deg = 180", r()),
        ["G3 X-10 Y0 Z0 I-10 J0 F1000", "G3 X10 I10 J0"]
    );
}

#[test]
fn r_mode() {
    let cfg = "[arcs]
centre_mode = 'r'
max_sweep_deg = 180
[blocks.arc_ccw]
lines = ['G3 [X] [Y] R[R] F[F]']";
    let half = arc_result(
        [10.0, 0.0, 0.0],
        [-10.0, 0.0, 0.0],
        [0.0; 3],
        [0.0, 0.0, 1.0],
        2.0 * Q,
        false,
    );
    assert_eq!(motion_lines(cfg, half), ["G3 X-10 Y0 R10 F1000"]);
    // 270° is split into two 135° arcs.
    let three_q = arc_result(
        [10.0, 0.0, 0.0],
        [0.0, -10.0, 0.0],
        [0.0; 3],
        [0.0, 0.0, 1.0],
        3.0 * Q,
        false,
    );
    let l = motion_lines(cfg, three_q);
    assert_eq!(l.len(), 2);
    assert!(l.iter().all(|x| x.contains(" R10")), "{l:?}");
    assert!(l[1].starts_with("G3 X0 Y-10"), "{l:?}");
}

#[test]
fn rejected_arcs_linearized() {
    let r = || {
        arc_result(
            [10.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [0.0; 3],
            [0.0, 0.0, 1.0],
            Q,
            false,
        )
    };
    let l = motion_lines("[arcs]\nmin_radius = 20.0", r());
    assert!(
        l.len() > 5 && l.iter().all(|x| x.starts_with("G1")),
        "{l:?}"
    );
    let l = motion_lines("[arcs]\nplanes = ['XZ']", r());
    assert!(l.iter().all(|x| x.starts_with("G1")), "{l:?}");
    // A plane that is not principal is always linearized.
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let l = motion_lines(
        "",
        arc_result(
            [s * 10.0, -s * 10.0, 0.0],
            [0.0, 0.0, 10.0],
            [0.0; 3],
            [s, s, 0.0],
            Q,
            false,
        ),
    );
    assert!(l.iter().all(|x| x.starts_with("G1")), "{l:?}");
}

#[test]
fn helix_as_arc_with_axial_word() {
    let r = || {
        arc_result(
            [10.0, 0.0, 0.0],
            [0.0, 10.0, -1.0],
            [0.0; 3],
            [0.0, 0.0, 1.0],
            Q,
            true,
        )
    };
    let on = "[helix]\nenabled = true\n[blocks.helix_cw]\nlines=['G2 [X] [Y] [Z] [I] [J] F[F]']\n[blocks.helix_ccw]\nlines=['G3 [X] [Y] [Z] [I] [J] F[F]']";
    assert_eq!(motion_lines(on, r()), ["G3 X0 Y10 Z-1 I-10 J0 F1000"]);
    assert!(motion_lines("", r()).iter().all(|x| x.starts_with("G1")));
}

/// Parses `G2`/`G3` lines and checks the controller radius check against the
/// position the controller holds (spec 05 §5.8 rule 3).
fn check_arcs(text: &str, tol: f64) -> usize {
    let mut pos = [0.0f64; 2];
    let mut arcs = 0;
    for line in text.lines() {
        let mut w: BTreeMap<char, f64> = BTreeMap::new();
        for t in line.split_whitespace() {
            let mut cs = t.chars();
            if let (Some(c), Ok(v)) = (cs.next(), cs.as_str().parse::<f64>()) {
                w.insert(c, v);
            }
        }
        let g = w.get(&'G').copied();
        if matches!(g, Some(2.0) | Some(3.0)) {
            let e = [
                w.get(&'X').copied().unwrap_or(pos[0]),
                w.get(&'Y').copied().unwrap_or(pos[1]),
            ];
            let c = [
                pos[0] + w.get(&'I').copied().unwrap_or(0.0),
                pos[1] + w.get(&'J').copied().unwrap_or(0.0),
            ];
            let err = (libm::hypot(c[0] - pos[0], c[1] - pos[1])
                - libm::hypot(c[0] - e[0], c[1] - e[1]))
            .abs();
            assert!(err <= tol + 1e-9, "{line}: radius error {err}");
            arcs += 1;
        }
        if matches!(g, Some(0.0) | Some(1.0) | Some(2.0) | Some(3.0)) {
            pos = [
                w.get(&'X').copied().unwrap_or(pos[0]),
                w.get(&'Y').copied().unwrap_or(pos[1]),
            ];
        }
    }
    arcs
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]
    #[test]
    fn rounded_arcs_pass_radius_check_or_are_linearized(
        cx in -50.0f64..50.0, cy in -50.0f64..50.0, r in 0.05f64..200.0,
        t0 in 0.0f64..6.3, sweep in -6.25f64..6.25, d in 0u32..4,
    ) {
        proptest::prop_assume!(sweep.abs() > 0.01);
        let a = [cx + r * libm::cos(t0), cy + r * libm::sin(t0), 0.0];
        let b = [cx + r * libm::cos(t0 + sweep), cy + r * libm::sin(t0 + sweep), 0.0];
        let res = arc_result(a, b, [cx, cy, 0.0], [0.0, 0.0, 1.0], sweep, false);
        let extra = format!("[words.X]\ndecimals={d}\n[words.Y]\ndecimals={d}\n[words.I]\ndecimals={d}\n[words.J]\ndecimals={d}");
        let c = compiled_stack(&[ARCS, &extra]);
        // Rapid to the start first so the controller position is known.
        let lead = result([a[0], a[1], 5.0], &[(Motion::Rapid, FeedRef::Rapid, a)]);
        let out = post(&job(vec![toolpath("L", 1, lead), toolpath("A", 1, res)]), &c).unwrap();
        check_arcs(&out.files[0], 0.002);
    }
}
