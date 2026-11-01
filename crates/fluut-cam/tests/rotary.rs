// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rotary types (D15b §6, D09 §5) on the test shaft turned so that its
//! axis is X: Ø30 from X 0 to 30, a 20 × 20 square to 55, Ø16 to 80.

mod support;

use fluut_cam::cutter::Cutter;
use fluut_cam::geometry::SetupGeometry;
use fluut_cam::radial::{Radial, Turned};
use fluut_cam::stock::LayeredStock;
use fluut_cam::surface::FaceSets;
use fluut_cam::{Generated, RotaryAxisInfo, RotaryInfo, StrategyInput, calculate};
use fluut_geom::{Affine, Vec3};
use fluut_model::project::{SetupKind, Toolpath};
use fluut_model::resolve::HeightEnv;
use fluut_path::{Motion, Segment};
use support::cam::{tool, tp};
use support::step;

/// The shaft in W: model Z → X, model X → Z.
fn shaft(dx: f64) -> SetupGeometry {
    let g = support::cam::load(&step::shaft([0.0; 3]));
    let m = glam::DMat3::from_cols(
        glam::DVec3::new(0.0, 0.0, 1.0),
        glam::DVec3::new(0.0, -1.0, 0.0),
        glam::DVec3::new(1.0, 0.0, 0.0),
    );
    SetupGeometry::transformed(
        &g.bodies,
        &Affine::from_mat3_translation(m, Vec3::new(dx, 0.0, 0.0)),
    )
    .unwrap()
}

fn info(dx: f64, axis: Option<Option<RotaryAxisInfo>>) -> RotaryInfo {
    RotaryInfo {
        stock_radius: 17.0,
        stock_x: (dx - 1.0, dx + 81.0),
        model_x: (dx, dx + 80.0),
        machine_axis: axis,
    }
}

/// Radii (D15b §1.3).
fn env() -> HeightEnv {
    HeightEnv {
        setup_clearance: 25.0,
        stock_top: 17.0,
        stock_bottom: 0.0,
        model_top: 15.0,
        model_bottom: 8.0,
        selection: None,
    }
}

const CONTINUOUS: Option<Option<RotaryAxisInfo>> = Some(Some(RotaryAxisInfo {
    continuous: true,
    limits_deg: None,
}));

fn try_run(
    t: &Toolpath,
    dx: f64,
    axis: Option<Option<RotaryAxisInfo>>,
    kind: SetupKind,
    stock: Option<&LayeredStock>,
) -> Result<Generated, Vec<String>> {
    let g = shaft(dx);
    let tool = tool(t.tool.split('#').nth(1).unwrap());
    let input = StrategyInput {
        toolpath: t,
        geometry: &g,
        tool: &tool,
        material: Some("MDF"),
        setup_kind: kind,
        env: env(),
        stock: None,
        to_work: Affine::identity(),
        machine: None,
        post: None,
        in_process: stock,
        rotary: Some(info(dx, axis)),
    };
    calculate(&input).map_err(|e| e.diagnostics().into_iter().map(|d| d.code).collect())
}

fn run(t: &Toolpath) -> Generated {
    try_run(t, 0.0, CONTINUOUS, SetupKind::Rotary, None).unwrap_or_else(|e| panic!("{e:?}"))
}

fn err(t: &Toolpath, axis: Option<Option<RotaryAxisInfo>>) -> Vec<String> {
    match try_run(t, 0.0, axis, SetupKind::Rotary, None) {
        Ok(r) => panic!("expected an error, got {:?}", codes(&r)),
        Err(e) => e,
    }
}

fn codes(r: &Generated) -> Vec<String> {
    r.diagnostics.iter().map(|d| d.code.clone()).collect()
}

fn cuts(r: &Generated) -> Vec<&Segment> {
    r.result
        .segments()
        .filter(|s| s.motion().is_some_and(|m| m.motion == Motion::Cut))
        .collect()
}

/// (x, r, a) of a segment end.
fn xra(p: &fluut_path::AxisPos) -> (f64, f64, f64) {
    (p.xyz.x(), p.xyz.z(), p.a.unwrap())
}

/// No cut position is below the radial drop of the tool (no gouge),
/// checked at `n` angles along each cut.
fn assert_no_gouge(r: &Generated, tool_id: &str) {
    let g = shaft(0.0);
    let t = tool(tool_id);
    let n = 360;
    let turned = Turned::grid(&g, n).unwrap();
    let q = Radial::new(
        &turned,
        &FaceSets::default(),
        Cutter::new(t.diameter, t.corner_radius()).unwrap(),
        0.0,
    )
    .unwrap();
    for s in cuts(r) {
        let (a, b) = (s.start().unwrap(), s.end().unwrap());
        let (x0, r0, a0) = xra(a);
        let (x1, r1, a1) = xra(b);
        // Points of the move at check-grid angles (or along X at fixed A).
        let mut probes = Vec::new();
        if a0 == a1 {
            for i in 0..=40 {
                let f = i as f64 / 40.0;
                probes.push((x0 + (x1 - x0) * f, r0 + (r1 - r0) * f, a0));
            }
        } else {
            let (lo, hi) = (a0.min(a1), a0.max(a1));
            let mut d = lo.ceil();
            while d <= hi {
                let f = (d - a0) / (a1 - a0);
                probes.push((x0 + (x1 - x0) * f, r0 + (r1 - r0) * f, d));
                d += 1.0;
            }
        }
        for (x, rr, aa) in probes {
            let k = ((aa / 360.0 * n as f64).round() as i64).rem_euclid(n as i64) as usize;
            if let Some(d) = q.drop(k, x) {
                assert!(rr >= d - 0.012, "gouge at x {x} A {aa}: r {rr} < {d}");
            }
        }
    }
}

const NARROW: &str = "[geometry]\nx_start = { ref = \"work_zero\", offset = 4 }\n\
                      x_end = { ref = \"work_zero\", offset = 26 }\n";

#[test]
fn around_a_on_a_coaxial_cylinder_is_one_segment_per_ring() {
    let t = tp(
        "rotary_around_a",
        "f6",
        &format!("{NARROW}[passes]\nstepover_x = 2\ndirection = \"zigzag\"\n"),
    );
    let r = run(&t);
    let rings: Vec<_> = cuts(&r)
        .into_iter()
        .filter(|s| matches!(s, Segment::Rotary { .. }))
        .collect();
    // 4 to 26 in steps ≤ 2: 12 rings, each one 360° rotary move at R 15.
    assert_eq!(rings.len(), 12);
    for s in &rings {
        let Segment::Rotary { a, b, radius, .. } = s else {
            unreachable!()
        };
        assert_eq!(*radius, 15.0);
        assert_eq!((a.a.unwrap() - b.a.unwrap()).abs(), 360.0);
    }
    assert_no_gouge(&r, "f6");
}

#[test]
fn around_a_climb_keeps_turning_and_follows_the_square() {
    let t = tp(
        "rotary_around_a",
        "f6",
        "[geometry]\nx_start = { ref = \"work_zero\", offset = 36 }\n\
         x_end = { ref = \"work_zero\", offset = 49 }\n[passes]\nstepover_x = 3\n",
    );
    let r = run(&t);
    let last = r.result.segments().filter_map(|s| s.end()).last().unwrap();
    // Six rings (36 to 49 in steps ≤ 3), A always increasing.
    assert!((last.a.unwrap() - 2160.0).abs() < 1e-9, "{:?}", last.a);
    // Over the square the radius goes from the flat (10) to the corner.
    let radii: Vec<f64> = cuts(&r).iter().map(|s| s.end().unwrap().xyz.z()).collect();
    let lo = radii.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = radii.iter().copied().fold(0.0, f64::max);
    assert!((lo - 10.0).abs() < 1e-6, "{lo}");
    assert!(hi > 14.0, "{hi}");
    assert_no_gouge(&r, "f6");
    // A helical ring link has no corner: no stay-down X moves.
    let h = tp(
        "rotary_around_a",
        "f6",
        "[geometry]\nx_start = { ref = \"work_zero\", offset = 36 }\n\
         x_end = { ref = \"work_zero\", offset = 49 }\n\
         [passes]\nstepover_x = 3\nring_link = \"helical\"\n",
    );
    let r = run(&h);
    assert!(
        !r.result
            .segments()
            .any(|s| s.motion().is_some_and(|m| m.motion == Motion::Link))
    );
    assert_no_gouge(&r, "f6");
}

#[test]
fn spiral_on_a_cylinder_is_one_segment_ending_at_x_end() {
    let t = tp(
        "rotary_spiral",
        "f6",
        "[geometry]\nx_start = { ref = \"work_zero\", offset = 59 }\n\
         x_end = { ref = \"work_zero\", offset = 77 }\n[passes]\nadvance = 2\n",
    );
    let r = run(&t);
    let c = cuts(&r);
    assert_eq!(c.len(), 1, "{c:?}");
    let Segment::Rotary { a, b, radius, .. } = c[0] else {
        panic!("{:?}", c[0])
    };
    assert_eq!(*radius, 8.0);
    assert_eq!(a.xyz.x(), 59.0);
    assert_eq!(b.xyz.x(), 77.0);
    // 18 mm at ≤ 2 per turn: 9 turns.
    assert_eq!(b.a.unwrap(), 3240.0);
    // Starting from the other end, conventional.
    let t = tp(
        "rotary_spiral",
        "f6",
        "[geometry]\nx_start = { ref = \"work_zero\", offset = 59 }\n\
         x_end = { ref = \"work_zero\", offset = 77 }\n\
         [passes]\nadvance = 2\nstart = \"x_end\"\ndirection = \"conventional\"\n",
    );
    let c = run(&t);
    let s = cuts(&c)[0];
    assert_eq!(s.start().unwrap().xyz.x(), 77.0);
    assert_eq!(s.end().unwrap().xyz.x(), 59.0);
    assert_eq!(s.end().unwrap().a.unwrap(), -3240.0);
}

#[test]
fn along_x_passes_at_fixed_a() {
    let t = tp("rotary_along_x", "f6", "[passes]\nstepover_deg = 20\n");
    let r = run(&t);
    let c = cuts(&r);
    assert!(c.iter().all(|s| matches!(s, Segment::Line { .. })));
    let mut a: Vec<f64> = c.iter().map(|s| s.start().unwrap().a.unwrap()).collect();
    a.dedup();
    assert_eq!(a.len(), 18);
    assert_eq!(&a[..4], &[0.0, 20.0, 40.0, 60.0]);
    // At A 0 on the square's flat the tip is at radius 10.
    let at0 = c
        .iter()
        .filter(|s| s.start().unwrap().a == Some(0.0))
        .find(|s| {
            let (x0, x1) = (s.start().unwrap().xyz.x(), s.end().unwrap().xyz.x());
            x0.min(x1) <= 42.0 && x0.max(x1) >= 43.0
        })
        .unwrap();
    assert!((at0.start().unwrap().xyz.z() - 10.0).abs() < 1e-6);
    assert_no_gouge(&r, "f6");
}

#[test]
fn rough_clears_layers_without_gouging() {
    let t = tp(
        "rotary_rough",
        "f6",
        "[passes]\nradial_step_down = 3\nstepover_x = 4\nstock_to_leave = 0.3\n",
    );
    let r = run(&t);
    let radii: Vec<f64> = {
        let mut v: Vec<f64> = cuts(&r).iter().map(|s| s.end().unwrap().xyz.z()).collect();
        v.sort_by(f64::total_cmp);
        v.dedup();
        v
    };
    // Layers from 17 down to 8, plus the coaxial cylinders + 0.3.
    assert!(radii.contains(&15.3), "{radii:?}");
    assert!(radii.contains(&8.3), "{radii:?}");
    assert!(*radii.first().unwrap() >= 8.0, "{radii:?}");
    assert_no_gouge(&r, "f6");
    // Full sweeps over the Ø16 part at the first layer below 17.
    assert!(cuts(&r).iter().any(|s| match s {
        Segment::Rotary { a, b, .. } => (a.a.unwrap() - b.a.unwrap()).abs() == 360.0,
        _ => false,
    }));
    // The result updates cylindrical stock: a second rough from it cuts
    // nothing.
    let mut st = LayeredStock::cylindrical(17.0, -1.0, 81.0);
    st.apply(&r.result, fluut_cam::stock::Sweep::of(&tool("f6")), 0.01);
    let again = tp(
        "rotary_rough",
        "f6",
        "[geometry]\nrest_from = \"previous\"\n\
         [passes]\nradial_step_down = 3\nstepover_x = 4\nstock_to_leave = 0.3\n",
    );
    let r2 = try_run(&again, 0.0, CONTINUOUS, SetupKind::Rotary, Some(&st)).unwrap();
    assert!(cuts(&r2).is_empty(), "{}", cuts(&r2).len());
}

#[test]
fn rough_helix_ramp_or_fallback() {
    let t = tp(
        "rotary_rough",
        "f6",
        "[passes]\nradial_step_down = 3\nstepover_x = 4\n[linking]\nramp = { kind = \"helix\", angle_deg = 3 }\n",
    );
    let r = run(&t);
    assert!(
        r.result
            .segments()
            .any(|s| s.motion().is_some_and(|m| m.motion == Motion::Ramp))
    );
    assert_no_gouge(&r, "f6");
}

#[test]
fn moving_the_part_along_x_moves_the_path() {
    let kinds = [
        (
            "rotary_rough",
            "[passes]\nradial_step_down = 4\nstepover_x = 6\n",
        ),
        ("rotary_around_a", "[passes]\nstepover_x = 3\n"),
        ("rotary_spiral", "[passes]\nadvance = 3\n"),
        ("rotary_along_x", "[passes]\nstepover_deg = 20\n"),
    ];
    let dx = 7.123457;
    for (k, g) in kinds {
        // X references from the model and stock move with the part.
        let t = tp(k, "f6", g);
        let a = try_run(&t, 0.0, CONTINUOUS, SetupKind::Rotary, None).unwrap();
        let b = try_run(&t, dx, CONTINUOUS, SetupKind::Rotary, None).unwrap();
        let (sa, sb): (Vec<_>, Vec<_>) =
            (a.result.segments().collect(), b.result.segments().collect());
        assert_eq!(sa.len(), sb.len(), "{k}");
        for (p, q) in sa.iter().zip(&sb) {
            for (u, v) in [(p.start(), q.start()), (p.end(), q.end())] {
                let (Some(u), Some(v)) = (u, v) else { continue };
                assert!((u.xyz.x() + dx - v.xyz.x()).abs() < 1e-6, "{k}");
                assert!((u.xyz.z() - v.xyz.z()).abs() < 1e-6, "{k}");
                assert_eq!(u.a, v.a, "{k}");
            }
        }
    }
}

#[test]
fn calculation_codes() {
    let t = tp("rotary_around_a", "f6", "[passes]\nstepover_x = 3\n");
    // A machine without a rotary axis.
    assert!(err(&t, Some(None)).contains(&"param.rotary.machine_axis".into()));
    // Not a rotary Setup.
    let e = try_run(&t, 0.0, CONTINUOUS, SetupKind::ThreeAxis, None).unwrap_err();
    assert!(e.contains(&"param.type.setup_kind".into()), "{e:?}");
    // Climb on a limited axis needs unwinding; zigzag does not.
    let limited = Some(Some(RotaryAxisInfo {
        continuous: false,
        limits_deg: Some((-360.0, 360.0)),
    }));
    assert!(err(&t, limited).contains(&"param.rotary_around_a.direction.unwind".into()));
    let z = tp(
        "rotary_around_a",
        "f6",
        "[passes]\nstepover_x = 3\ndirection = \"zigzag\"\n",
    );
    assert!(try_run(&z, 0.0, limited, SetupKind::Rotary, None).is_ok());
    // A full turn does not fit in ±90°.
    let narrow = Some(Some(RotaryAxisInfo {
        continuous: false,
        limits_deg: Some((-90.0, 90.0)),
    }));
    assert!(err(&z, narrow).contains(&"param.rotary.limits".into()));
    let s = tp("rotary_spiral", "f6", "[passes]\nadvance = 3\n");
    assert!(err(&s, limited).contains(&"param.rotary.limits".into()));
    // X range order and stock.
    let bad = tp(
        "rotary_spiral",
        "f6",
        "[geometry]\nx_start = { ref = \"work_zero\", offset = 30 }\n\
         x_end = { ref = \"work_zero\", offset = 30 }\n",
    );
    assert!(err(&bad, CONTINUOUS).contains(&"param.geometry.x_range.order".into()));
    let wide = tp(
        "rotary_spiral",
        "f6",
        "[geometry]\nx_start = { ref = \"stock_left\", offset = -0.1 }\n",
    );
    assert!(codes(&run(&wide)).contains(&"param.geometry.x_range.stock".into()));
    let ok = tp(
        "rotary_spiral",
        "f6",
        "[geometry]\nx_start = { ref = \"stock_left\" }\n",
    );
    assert!(!codes(&run(&ok)).contains(&"param.geometry.x_range.stock".into()));
    // Without a machine the axis rules are not checked.
    assert!(try_run(&t, 0.0, None, SetupKind::Rotary, None).is_ok());
}
