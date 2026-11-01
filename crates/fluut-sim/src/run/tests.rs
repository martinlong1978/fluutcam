// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use fluut_geom::Point3;
use fluut_path::{AxisPos, FeedRef, MotionInfo, Pass, PassKind, Segment};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(crate) fn xyz(x: f64, y: f64, z: f64) -> AxisPos {
    AxisPos::xyz(Point3::new(x, y, z))
}

pub(crate) fn feed(a: AxisPos, b: AxisPos) -> Segment {
    Segment::Line {
        a,
        b,
        m: MotionInfo::new(Motion::Cut, FeedRef::Cut, 0.01),
    }
}

pub(crate) fn rapid(a: AxisPos, b: AxisPos) -> Segment {
    Segment::Line {
        a,
        b,
        m: MotionInfo::new(Motion::Rapid, FeedRef::Rapid, 0.01),
    }
}

/// A polyline of cuts through `pts`.
pub(crate) fn path(pts: &[AxisPos]) -> ToolpathResult {
    let segs = pts.windows(2).map(|w| feed(w[0], w[1])).collect();
    ToolpathResult::new(vec![Pass::new(PassKind::Cut, segs)]).unwrap()
}

pub(crate) fn block(x: f64, y: f64, z: f64) -> Stock {
    Stock::Box {
        min: Point3::new(0.0, 0.0, 0.0),
        max: Point3::new(x, y, z),
    }
}

pub(crate) fn settings(res: f64) -> Settings {
    Settings {
        res,
        rotary: false,
        kinematics: Kinematics::default(),
        tol: 0.01,
    }
}

pub(crate) fn program<'a>(id: &str, r: &'a ToolpathResult, tool: Tool) -> Program<'a> {
    Program {
        id: id.into(),
        result: r,
        tool: Assembly::bare(tool),
        feeds: Feeds::all(600.0),
    }
}

fn close(a: f32, b: f64) -> bool {
    (f64::from(a) - b).abs() < 1e-5
}

#[test]
fn flat_slot_gives_exact_dexel_lengths() {
    // Flat Ø4 along X at y = 5, z = 7 through a 20 × 10 × 10 block.
    let r = path(&[xyz(-5.0, 5.0, 7.0), xyz(25.0, 5.0, 7.0)]);
    let s = Simulation::run(
        block(20.0, 10.0, 10.0),
        &[program("t1", &r, Tool::flat(2.0))],
        &settings(0.1),
        None,
        &Quiet,
    )
    .unwrap();
    let d = s.stock_final();
    let (gx, gy, gz) = (&d.grids[0], &d.grids[1], &d.grids[2]);
    for j in 0..gz.nv {
        let y = (j as f64 + 0.5) * 0.1;
        let want = if (y - 5.0).abs() < 2.0 { 7.0 } else { 10.0 };
        for i in [0, 99, 199] {
            let sp = gz.ray(i, j);
            assert_eq!(sp.len(), 1);
            assert!(
                close(sp[0].lo, 0.0) && close(sp[0].hi, want),
                "y={y}: {sp:?}"
            );
        }
    }
    // X rays (u = y, v = z): empty inside the slot, full below it.
    for (iy, kz, full) in [(50, 85, false), (50, 60, true), (10, 85, true)] {
        let sp = gx.ray(iy, kz);
        if full {
            assert!(sp.len() == 1 && close(sp[0].lo, 0.0) && close(sp[0].hi, 20.0));
        } else {
            assert!(sp.is_empty(), "{sp:?}");
        }
    }
    // Y rays (u = z, v = x): above z = 7, material on y < 3 and y > 7.
    let sp = gy.ray(85, 100);
    assert_eq!(sp.len(), 2, "{sp:?}");
    assert!(close(sp[0].lo, 0.0) && close(sp[0].hi, 3.0));
    assert!(close(sp[1].lo, 7.0) && close(sp[1].hi, 10.0));
    assert_eq!(gy.ray(60, 100).len(), 1);
    // Removed volume: 20 × 4 × 3.
    let v0 = 2000.0;
    for v in d.volumes() {
        assert!((v0 - v - 240.0).abs() < 1e-6 * v0, "{v}");
    }
}

#[test]
fn v_groove_follows_the_cone() {
    // A 90° V bit with its tip at z = 8: the groove's sides are at 45°.
    let r = path(&[xyz(-5.0, 5.0, 8.0), xyz(15.0, 5.0, 8.0)]);
    let tool = Tool::cone(3.0, 0.0, std::f64::consts::FRAC_PI_4);
    let s = Simulation::run(
        block(10.0, 10.0, 10.0),
        &[program("v", &r, tool)],
        &settings(0.1),
        None,
        &Quiet,
    )
    .unwrap();
    let g = &s.stock_final().grids[2];
    for j in 0..g.nv {
        let y = (j as f64 + 0.5) * 0.1;
        let want = (8.0 + (y - 5.0).abs()).min(10.0);
        let top = f64::from(g.ray(37, j).last().unwrap().hi);
        assert!((top - want).abs() < 2e-4, "y={y}: {top} vs {want}");
    }
}

fn raster() -> ToolpathResult {
    let mut pts = Vec::new();
    for row in 0..25 {
        let y = 1.0 + 0.7 * f64::from(row);
        let (a, b) = if row % 2 == 0 {
            (-2.0, 27.0)
        } else {
            (27.0, -2.0)
        };
        pts.push(xyz(a, y, 6.0 - 0.05 * f64::from(row)));
        pts.push(xyz(b, y, 6.0 - 0.05 * f64::from(row)));
    }
    path(&pts)
}

#[test]
fn same_stock_and_findings_on_one_and_many_threads() {
    let r1 = raster();
    let r2 = path(&[
        xyz(5.0, 5.0, 12.0),
        xyz(5.0, 5.0, 3.0),
        xyz(20.0, 12.0, 3.0),
    ]);
    let run_on = |n: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .unwrap();
        pool.install(|| {
            let mut a = Assembly::bare(Tool::bull(1.5, 0.5));
            a.flute_length = Some(2.0);
            let progs = [
                Program {
                    tool: a,
                    ..program("t1", &r1, Tool::flat(1.0))
                },
                program("t2", &r2, Tool::ball(1.0)),
            ];
            let s = Simulation::run(block(25.0, 18.0, 8.0), &progs, &settings(0.1), None, &Quiet)
                .unwrap();
            (s.stock_final().hash(), s.findings.clone(), s.duration())
        })
    };
    let (h1, f1, t1) = run_on(1);
    let (h4, f4, t4) = run_on(4);
    assert_eq!(h1, h4);
    assert_eq!(f1, f4);
    assert_eq!(t1.to_bits(), t4.to_bits());
    assert!(!f1.is_empty(), "the flute length is exceeded");
}

#[test]
fn checkpoints_share_tiles_and_scrubbing_restores_them() {
    let r1 = raster();
    let r2 = path(&[xyz(5.0, 5.0, 3.0), xyz(20.0, 5.0, 3.0)]);
    let s = Simulation::run(
        block(25.0, 18.0, 8.0),
        &[
            program("t1", &r1, Tool::flat(1.0)),
            program("t2", &r2, Tool::flat(1.0)),
        ],
        &settings(0.2),
        None,
        &Quiet,
    )
    .unwrap();
    let fresh = TriDexel::new(&s.stock, 0.2);
    // The first checkpoint is the uncut stock, untouched by later cuts.
    assert_eq!(s.checkpoint(0).hash(), fresh.hash());
    // The second toolpath changes only a few tiles of the Z family.
    let (a, b) = (&s.checkpoint(1).grids[2], &s.checkpoint(2).grids[2]);
    let shared = a.shared_tiles(b);
    assert!(shared > 0 && shared < a.nv.div_ceil(crate::dexel::ROWS));
    // Shared tiles count once: less than three full stocks.
    let one = fresh.memory().total();
    assert!(
        s.memory() > one && s.memory() < 3 * one,
        "{} vs {one}",
        s.memory()
    );
    // Scrubbing to each checkpoint's time gives that checkpoint.
    for (i, clip) in s.toolpaths.iter().enumerate() {
        assert_eq!(
            s.stock_after(clip.chords.start).hash(),
            s.checkpoint(i).hash()
        );
    }
    assert_eq!(s.stock_at(s.duration()).hash(), s.stock_final().hash());
    assert_eq!(s.stock_at(-1.0).hash(), fresh.hash());
    // Halfway through the first toolpath: re-run from its checkpoint.
    let half = s.toolpaths[0].chords.len() / 2;
    let mut want = fresh.clone();
    crate::cut(
        &mut want,
        &s.chords[..half].iter().map(|c| c.sweep).collect::<Vec<_>>(),
    );
    assert_eq!(s.stock_after(half).hash(), want.hash());
    // Times increase through the program and the clips abut.
    assert!(s.chords.windows(2).all(|w| w[0].end <= w[1].end));
    assert_eq!(s.toolpaths[0].end, s.toolpaths[1].start);
    let (tp, pose) = s.tool_at(s.duration()).unwrap();
    assert_eq!(tp, 1);
    assert!((pose.tip - DVec3::new(20.0, 5.0, 3.0)).length() < 1e-9);
}

struct Stopper {
    after: u64,
    seen: AtomicU64,
    stop: AtomicBool,
}

impl Monitor for Stopper {
    fn cancelled(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn progress(&self, done: u64, total: u64) {
        assert!(done <= total);
        self.seen.store(done, Ordering::Relaxed);
        if done >= self.after {
            self.stop.store(true, Ordering::Relaxed);
        }
    }
}

#[test]
fn cancel_stops_the_run() {
    let r = raster();
    let m = Stopper {
        after: 0,
        seen: AtomicU64::new(0),
        stop: AtomicBool::new(false),
    };
    let out = Simulation::run(
        block(25.0, 18.0, 8.0),
        &[program("t1", &r, Tool::flat(1.0))],
        &settings(0.2),
        None,
        &m,
    );
    assert_eq!(out.err(), Some(Cancelled));
    // Without a cancel, progress reaches the total.
    let m = Stopper {
        after: u64::MAX,
        seen: AtomicU64::new(0),
        stop: AtomicBool::new(false),
    };
    let s = Simulation::run(
        block(25.0, 18.0, 8.0),
        &[program("t1", &r, Tool::flat(1.0))],
        &settings(0.2),
        None,
        &m,
    )
    .unwrap();
    assert_eq!(m.seen.load(Ordering::Relaxed), s.chords.len() as u64);
}

#[test]
fn resolution_from_error_and_budget() {
    let s = block(200.0, 150.0, 25.0);
    // Small stock, coarse target: twice the error.
    assert_eq!(resolution(&s, 0.1, DEFAULT_BUDGET), 0.2);
    // A fine target that would not fit the budget is coarsened to fit it.
    let r = resolution(&s, 0.005, DEFAULT_BUDGET);
    let rays = (200.0 * 150.0 + 150.0 * 25.0 + 25.0 * 200.0) / (r * r);
    assert!(r > 0.01);
    assert!((rays * BYTES_PER_RAY - DEFAULT_BUDGET as f64).abs() < 1.0);
}
