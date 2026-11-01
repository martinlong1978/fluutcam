// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::run::tests::{block, feed, path, program, rapid, settings, xyz};
use crate::run::{Assembly, Program, Quiet, Settings};
use crate::tool::Tool;
use fluut_path::{Pass, PassKind, ToolpathResult};

/// A box part from the stock bottom up to `top`, over `[x0, x1] × [y0, y1]`.
struct BoxPart {
    x: (f64, f64),
    y: (f64, f64),
    top: f64,
}

impl Part for BoxPart {
    fn top(&self, x: f64, y: f64) -> Option<f64> {
        (x >= self.x.0 && x <= self.x.1 && y >= self.y.0 && y <= self.y.1).then_some(self.top)
    }
}

fn whole(top: f64) -> BoxPart {
    BoxPart {
        x: (0.0, 20.0),
        y: (0.0, 10.0),
        top,
    }
}

fn run_with(progs: &[Program], s: &Settings, part: Option<&dyn Part>) -> Simulation {
    Simulation::run(block(20.0, 10.0, 10.0), progs, s, part, &Quiet).unwrap()
}

/// A rapid down beside the stock, then a cut along X at height `z`.
fn slot(z: f64) -> ToolpathResult {
    ToolpathResult::new(vec![Pass::new(
        PassKind::Cut,
        vec![
            rapid(xyz(-5.0, 5.0, 12.0), xyz(-5.0, 5.0, z)),
            feed(xyz(-5.0, 5.0, z), xyz(25.0, 5.0, z)),
        ],
    )])
    .unwrap()
}

fn of(s: &Simulation, c: Check) -> Vec<Finding> {
    s.findings
        .iter()
        .filter(|f| f.check == c)
        .copied()
        .collect()
}

#[test]
fn gouge_found_where_the_slot_goes_into_the_part() {
    let r = slot(6.0);
    let part = whole(7.0);
    let s = run_with(
        &[program("t1", &r, Tool::flat(2.0))],
        &settings(0.1),
        Some(&part),
    );
    let g = of(&s, Check::Gouge);
    assert_eq!(g.len(), 1, "{:?}", s.findings);
    assert_eq!((g[0].toolpath, g[0].segment), (0, 1));
    assert!((g[0].depth - 1.0).abs() < 1e-5);
    assert!((g[0].position.z - 6.0).abs() < 1e-5);
    assert!((g[0].position.y - 5.0).abs() < 2.0);
    assert!(Check::Gouge.is_error());
}

#[test]
fn no_gouge_on_the_part_and_rest_measured() {
    let r = slot(7.0);
    let part = whole(7.0);
    let s = run_with(
        &[program("t1", &r, Tool::flat(2.0))],
        &settings(0.1),
        Some(&part),
    );
    assert!(of(&s, Check::Gouge).is_empty(), "{:?}", s.findings);
    let rest = s.rest.unwrap();
    // 3 mm over the part except the 4 mm wide slot: 20 × 6 × 3.
    assert!((rest.on_part - 360.0).abs() < 1e-3, "{rest:?}");
    assert!((rest.max - 3.0).abs() < 1e-5);
    assert_eq!(rest.off_part, 0.0);
}

#[test]
fn ring_keeps_a_cut_beside_a_wall_from_counting() {
    // Part x ≤ 10, top 7. A flat Ø4 runs down Y at x = 11.95, so it cuts
    // 0.05 into the wall, inside the 0.1 tolerance, down to z = 2.
    let part = BoxPart {
        x: (0.0, 10.0),
        y: (0.0, 10.0),
        top: 7.0,
    };
    let r = path(&[xyz(11.95, -5.0, 2.0), xyz(11.95, 15.0, 2.0)]);
    let s = Settings {
        tol: 0.1,
        ..settings(0.1)
    };
    let sim = run_with(&[program("t1", &r, Tool::flat(2.0))], &s, Some(&part));
    assert!(of(&sim, Check::Gouge).is_empty(), "{:?}", sim.findings);
    // 0.3 into the wall is a gouge.
    let r = path(&[xyz(11.7, -5.0, 2.0), xyz(11.7, 15.0, 2.0)]);
    let sim = run_with(&[program("t1", &r, Tool::flat(2.0))], &s, Some(&part));
    let g = of(&sim, Check::Gouge);
    assert_eq!(g.len(), 1);
    assert!((g[0].depth - 5.0).abs() < 1e-5);
    assert!(sim.rest.unwrap().off_part > 0.0);
}

#[test]
fn rapid_through_stock() {
    let tool = Tool::flat(2.0);
    // The plunge is a rapid down to z = 8, below the stock top.
    let bad = ToolpathResult::new(vec![Pass::new(
        PassKind::Cut,
        vec![
            rapid(xyz(5.0, 5.0, 12.0), xyz(5.0, 5.0, 8.0)),
            feed(xyz(5.0, 5.0, 8.0), xyz(15.0, 5.0, 8.0)),
        ],
    )])
    .unwrap();
    let s = run_with(&[program("t1", &bad, tool)], &settings(0.1), None);
    let f = of(&s, Check::RapidInStock);
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].segment, 0);
    // A point of the stock inside the plunge.
    let q = f[0].position;
    assert!(q.z >= 8.0 && q.z <= 10.0 + 1e-5, "{q}");
    assert!((q.truncate() - glam::DVec2::new(5.0, 5.0)).length() <= 2.0 + 1e-5);
    // Rapids that stay clear of the stock, or run in the cut slot, are fine.
    let good = slot(7.0);
    let s = run_with(&[program("t1", &good, tool)], &settings(0.1), None);
    assert!(of(&s, Check::RapidInStock).is_empty());
}

fn with(f: impl Fn(&mut Assembly)) -> Assembly {
    let mut a = Assembly::bare(Tool::flat(2.0));
    f(&mut a);
    a
}

#[test]
fn holder_and_shank_contact() {
    let holder = with(|a| a.holder = Some((5.0, 4.0, 20.0)));
    // 3 deep: the holder bottom stays at 11, above the stock.
    let r = slot(7.0);
    let p = Program {
        tool: holder,
        ..program("t1", &r, Tool::flat(2.0))
    };
    let s = run_with(&[p], &settings(0.1), None);
    assert!(of(&s, Check::Holder).is_empty());
    // 5 deep: the holder reaches z = 9.
    let r = slot(5.0);
    let p = Program {
        tool: holder,
        ..program("t1", &r, Tool::flat(2.0))
    };
    let s = run_with(&[p], &settings(0.1), None);
    let f = of(&s, Check::Holder);
    assert_eq!((f.len(), f[0].segment), (1, 1));
    assert!(f[0].position.z >= 9.0 - 1e-5);

    let shank = with(|a| {
        a.flute_length = Some(4.0);
        a.shank = Some(3.0);
    });
    let r = slot(7.0);
    let p = Program {
        tool: shank,
        ..program("t1", &r, Tool::flat(2.0))
    };
    assert!(run_with(&[p], &settings(0.1), None).findings.is_empty());
    let r = slot(5.0);
    let p = Program {
        tool: shank,
        ..program("t1", &r, Tool::flat(2.0))
    };
    let s = run_with(&[p], &settings(0.1), None);
    assert_eq!(of(&s, Check::Shank).len(), 1);
}

#[test]
fn flute_length_is_a_warning() {
    let r = slot(7.0);
    let short = with(|a| a.flute_length = Some(2.0));
    let p = Program {
        tool: short,
        ..program("t1", &r, Tool::flat(2.0))
    };
    let s = run_with(&[p], &settings(0.1), None);
    let f = of(&s, Check::FluteLength);
    assert_eq!(f.len(), 1);
    assert!(!Check::FluteLength.is_error());
    let long = with(|a| a.flute_length = Some(4.0));
    let p = Program {
        tool: long,
        ..program("t1", &r, Tool::flat(2.0))
    };
    assert!(run_with(&[p], &settings(0.1), None).findings.is_empty());
}
