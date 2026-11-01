// SPDX-License-Identifier: MIT OR Apache-2.0

//! D15 defaults (§1.5) and height resolution (§2.4).

use fluut_model::params::{HeightSpec, Num, PValue, Params, ToolpathTypeId};
use fluut_model::resolve::*;
use fluut_model::tools::{Tool, ToolLibrary};

fn tool() -> Tool {
    let src = "format = \"fluut.tools/1\"\nid = \"lib\"\n[[tool]]\nid = \"f\"\nshape = \"flat\"\n\
               diameter = 6\n[[tool.preset]]\nmaterial = \"MDF\"\nrpm = 18000\nfeed = 2000\n\
               plunge = 500\nstepover = 2.4\nstep_down = 3\n";
    ToolLibrary::parse(src).unwrap().0.tools.remove(0)
}

fn n(v: f64) -> Option<PValue> {
    Some(PValue::Num(Num::float(v)))
}

#[test]
fn contour_defaults() {
    let p = Params::new(ToolpathTypeId::Contour2d);
    let r = resolve(&p, &tool(), Some("MDF"));
    assert_eq!(r.tool.spindle_rpm, Some(18000.0));
    assert_eq!(r.tool.feed.cut, Some(2000.0));
    assert_eq!(r.tool.feed.plunge, Some(500.0));
    assert_eq!(r.tool.feed.ramp, Some(2000.0));
    assert_eq!(r.tool.feed.lead_out, Some(2000.0));
    assert_eq!(r.tool.coolant, Coolant::Off);
    assert_eq!(r.heights.top, HeightSpec::new("stock_top", 0.0));
    assert_eq!(r.heights.bottom, HeightSpec::new("selection", 0.0));
    assert_eq!(r.heights.retract, HeightSpec::new("stock_top", 5.0));
    assert_eq!(r.heights.feed, HeightSpec::new("top", 2.0));
    assert_eq!(r.passes.machining_tolerance, 0.01);
    assert!(r.passes.output_arcs);
    assert_eq!(r.linking.retract, RetractPolicy::Minimum);
    assert_eq!(
        r.linking.lead_in,
        Lead {
            kind: LeadKind::Arc,
            radius: 3.0,
            length: 0.0
        }
    );
    assert_eq!(r.linking.lead_out, r.linking.lead_in);
    assert_eq!(r.linking.ramp.kind, RampKind::Plunge);
    let c = r.contour().unwrap();
    assert_eq!(c.side, None);
    assert_eq!(c.direction, CutDirection::Climb);
    assert_eq!(c.step_down, Some(3.0));
    assert!((c.finishing_stepover - 0.6).abs() < 1e-15);
    assert_eq!(c.tabs.width, 6.0);
    assert_eq!(c.tabs.count, 4);
    // Without a preset, FromTool fields have no value.
    let r = resolve(&p, &tool(), Some("Oak"));
    assert_eq!(r.tool.feed.cut, None);
    assert_eq!(r.contour().unwrap().step_down, None);
}

#[test]
fn pocket_defaults_and_overrides() {
    let mut p = Params::new(ToolpathTypeId::Pocket2d);
    p.set("tool_params.feed.cut", n(1000.0)).unwrap();
    p.set("tool_params.feed.lead_in", n(700.0)).unwrap();
    p.set("passes.stock_to_leave", n(0.2)).unwrap();
    p.set("linking.lead_in.kind", Some(PValue::Enum("linear".into())))
        .unwrap();
    p.set("linking.lead_out.radius", n(1.0)).unwrap();
    let r = resolve(&p, &tool(), Some("MDF"));
    assert_eq!(r.tool.feed.ramp, Some(1000.0));
    assert_eq!(r.tool.feed.transition, Some(1000.0));
    assert_eq!(r.tool.feed.lead_out, Some(700.0));
    assert_eq!(r.passes.stock_to_leave_axial, 0.2);
    assert_eq!(
        r.linking.lead_in,
        Lead {
            kind: LeadKind::Linear,
            radius: 3.0,
            length: 3.0
        }
    );
    // lead_out takes lead_in's values for the keys it does not set.
    assert_eq!(
        r.linking.lead_out,
        Lead {
            kind: LeadKind::Linear,
            radius: 1.0,
            length: 3.0
        }
    );
    assert_eq!(
        r.linking.ramp,
        Ramp {
            kind: RampKind::Helix,
            angle_deg: 3.0,
            radius: 3.0
        }
    );
    let pk = r.pocket().unwrap();
    assert_eq!((pk.stepover, pk.step_down), (Some(2.4), Some(3.0)));
    assert_eq!(pk.pattern, Pattern::Offset);
    assert_eq!(pk.offset_order, OffsetOrder::InsideOut);
    assert_eq!(r.linking.lead_in.kind, LeadKind::Linear);
    let none = resolve(&Params::new(ToolpathTypeId::Pocket2d), &tool(), Some("MDF"));
    assert_eq!(none.linking.lead_in.kind, LeadKind::None);
}

#[test]
fn heights_resolve() {
    let env = HeightEnv {
        setup_clearance: 30.0,
        stock_top: 21.0,
        stock_bottom: 0.0,
        model_top: 20.0,
        model_bottom: 0.0,
        selection: Some((4.0, 12.0)),
    };
    let r = resolve(&Params::new(ToolpathTypeId::Pocket2d), &tool(), Some("MDF"));
    let z = r.heights.resolve(&env).unwrap();
    assert_eq!(
        z,
        ResolvedHeights {
            clearance: 30.0,
            retract: 26.0,
            feed: 23.0,
            top: 21.0,
            bottom: 4.0
        }
    );
    let mut h = r.heights.clone();
    h.top = HeightSpec::new("selection", -1.0);
    h.bottom = HeightSpec::new("work_zero", -0.5);
    let z = h.resolve(&env).unwrap();
    assert_eq!((z.top, z.bottom, z.feed), (11.0, -0.5, 13.0));
    let e = h
        .resolve(&HeightEnv {
            selection: None,
            ..env
        })
        .unwrap_err();
    assert_eq!(e.code, "param.heights.reference.empty");
    let par = resolve(&Params::new(ToolpathTypeId::Parallel), &tool(), Some("MDF"));
    let z = par.heights.resolve(&env).unwrap();
    assert_eq!((z.top, z.bottom), (20.0, 0.0));
}
