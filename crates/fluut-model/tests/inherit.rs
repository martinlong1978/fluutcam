// SPDX-License-Identifier: MIT OR Apache-2.0

//! Setup inheritance (D06 §2, §8): random chains against a manual walk,
//! origins, index angles and the `from_previous` rules.

use fluut_model::params::Num;
use fluut_model::project::{
    AngleMode, AxisDir, AxisPick, Corner, IndexAngle, Orientation, Project, Region, Setup,
    SetupKind, StockSpec, ZeroSpec, read,
};
use fluut_model::setup::{FieldOrigin, resolve_all, resolve_setup};
use proptest::prelude::*;
use std::sync::Arc;

/// One random Setup: its parent index (earlier) and optional local fields.
#[derive(Clone, Debug)]
struct Spec {
    parent: Option<usize>,
    machine: Option<u8>,
    material: Option<u8>,
    work_offset: Option<u8>,
    zero: Option<u8>,
    margin: Option<u8>,
    floor: Option<u8>,
}

fn spec(i: usize) -> impl Strategy<Value = Spec> {
    let parent = if i == 0 {
        Just(None).boxed()
    } else {
        prop_oneof![Just(None), (0..i).prop_map(Some)].boxed()
    };
    let f = || prop::option::of(0u8..3);
    (parent, f(), f(), f(), f(), f(), f()).prop_map(
        |(parent, machine, material, work_offset, zero, margin, floor)| Spec {
            parent,
            machine,
            material,
            work_offset,
            zero,
            margin,
            floor,
        },
    )
}

fn chain() -> impl Strategy<Value = Vec<Spec>> {
    (1usize..7).prop_flat_map(|n| (0..n).map(spec).collect::<Vec<_>>())
}

fn zero(v: u8) -> ZeroSpec {
    [
        ZeroSpec::StockTopCentre,
        ZeroSpec::StockTopCorner(Corner::BackRight),
        ZeroSpec::StockBottomCorner(Corner::FrontRight),
    ][v as usize]
        .clone()
}

fn stock(v: u8) -> StockSpec {
    StockSpec::Block {
        margin: Some([Num::int(v as i64); 3]),
        size: None,
    }
}

fn build(specs: &[Spec]) -> Project {
    let mut p = Project::new("chain");
    for (i, s) in specs.iter().enumerate() {
        let mut x = Setup::new(&format!("s{i}"), "");
        x.based_on = s.parent.map(|j| format!("s{j}"));
        x.machine = s.machine.map(|v| format!("m{v}"));
        x.material = s.material.map(|v| format!("mat{v}"));
        x.work_offset = s.work_offset.map(|v| format!("G5{}", 4 + v));
        x.zero = s.zero.map(zero);
        x.stock = s.margin.map(stock);
        x.region = s.floor.map(|v| Region {
            floor_z: Num::int(-(v as i64)),
        });
        p.setups.push(Arc::new(x));
    }
    p
}

/// The value of a field by walking the chain by hand, and its origin.
fn walk<T: Clone>(
    specs: &[Spec],
    i: usize,
    get: impl Fn(&Spec) -> Option<T>,
) -> (Option<T>, FieldOrigin) {
    if let Some(v) = get(&specs[i]) {
        return (Some(v), FieldOrigin::Local);
    }
    let mut at = specs[i].parent;
    while let Some(j) = at {
        if let Some(v) = get(&specs[j]) {
            return (Some(v), FieldOrigin::Inherited(format!("s{j}")));
        }
        at = specs[j].parent;
    }
    (None, FieldOrigin::Default)
}

proptest! {
    #[test]
    fn random_chains_match_a_manual_walk(specs in chain()) {
        let p = build(&specs);
        let all = resolve_all(&p);
        for (i, r) in all.into_iter().enumerate() {
            let r = r.unwrap();
            prop_assert_eq!(&r, &resolve_setup(&p, &format!("s{i}")).unwrap());
            let (m, o) = walk(&specs, i, |s| s.machine.map(|v| format!("m{v}")));
            prop_assert_eq!(&r.machine, &m);
            prop_assert_eq!(r.origin("machine"), &o);
            let (m, o) = walk(&specs, i, |s| s.material.map(|v| format!("mat{v}")));
            prop_assert_eq!(&r.material, &m);
            prop_assert_eq!(r.origin("material"), &o);
            let (w, o) = walk(&specs, i, |s| s.work_offset.map(|v| format!("G5{}", 4 + v)));
            prop_assert_eq!(&r.work_offset, &w.unwrap_or_else(|| "G54".into()));
            prop_assert_eq!(r.origin("work_offset"), &o);
            let (z, o) = walk(&specs, i, |s| s.zero.map(zero));
            prop_assert_eq!(&r.zero, &z.unwrap_or_default());
            prop_assert_eq!(r.origin("zero"), &o);
            let (st, o) = walk(&specs, i, |s| s.margin.map(stock));
            prop_assert_eq!(&r.stock, &Some(st.unwrap_or_default()));
            prop_assert_eq!(r.origin("stock"), &o);
            let (rg, o) = walk(&specs, i, |s| s.floor);
            prop_assert_eq!(r.region.as_ref().map(|x| x.floor_z.v), rg.map(|v| -(v as f64)));
            prop_assert_eq!(r.origin("region"), &o);
            prop_assert_eq!(r.kind, SetupKind::ThreeAxis);
            prop_assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
        }
    }

    #[test]
    fn written_files_never_copy_inherited_values(specs in chain()) {
        let p = build(&specs);
        let text = fluut_model::project::write(&p);
        let q = read(&text).unwrap().project;
        prop_assert_eq!(&q, &p);
        for (s, sp) in q.setups.iter().zip(&specs) {
            prop_assert_eq!(s.machine.is_some(), sp.machine.is_some());
            prop_assert_eq!(s.zero.is_some(), sp.zero.is_some());
        }
    }
}

const SPEC: &str = r#"format = "fluut.project/1"
name = "Shaft"
units = "mm"

[[setup]]
id = "s1"
name = "Turn profile"
kind = "rotary"
machine = "user/my-router@2"
material = "MDF"
orientation = { rotary_axis = { face = "f-12" }, a_zero_toward = "+Z" }
stock = { kind = "cylinder", radius = 26, length = 120 }
zero = { x = "stock_left" }
clearance = { kind = "cylinder", radius = 35 }

[[setup]]
id = "s2"
name = "Flat at 0"
based_on = "s1"
kind = "indexed"
index_angle = { mode = "absolute", value = 0 }
stock = { kind = "from_previous" }

[[setup]]
id = "s3"
name = "Flat at 90"
based_on = "s2"
index_angle = { mode = "relative", value = 90 }
stock = { kind = "from_previous" }

[[setup]]
id = "s4"
name = "Same angle"
based_on = "s3"
stock = { kind = "from_previous" }
"#;

#[test]
fn spec_example_resolves() {
    // Spec 03 §3.9.7: rotary, then two indexed sides.
    let p = read(SPEC).unwrap().project;
    let s3 = resolve_setup(&p, "s3").unwrap();
    assert!(s3.diagnostics.is_empty(), "{:?}", s3.diagnostics);
    assert_eq!(s3.kind, SetupKind::Indexed);
    assert_eq!(s3.origin("kind"), &FieldOrigin::Inherited("s2".into()));
    assert_eq!(s3.origin("machine"), &FieldOrigin::Inherited("s1".into()));
    assert_eq!(s3.origin("stock"), &FieldOrigin::Local);
    assert_eq!(s3.origin("region"), &FieldOrigin::Default);
    assert_eq!(s3.index_angle, 90.0);
    assert_eq!(s3.material.as_deref(), Some("MDF"));
    assert_eq!(
        s3.carried_stock,
        Some(StockSpec::Cylinder {
            radius: Num::int(26),
            length: Num::int(120)
        })
    );
    // An inherited angle is the parent's resolved angle, not added again.
    let s4 = resolve_setup(&p, "s4").unwrap();
    assert_eq!(s4.index_angle, 90.0);
    assert_eq!(
        s4.origin("index_angle"),
        &FieldOrigin::Inherited("s3".into())
    );
    // Relative to a rotary parent counts from 0.
    let mut q = p.clone();
    q.setup_mut("s2").unwrap().index_angle = Some(IndexAngle {
        mode: AngleMode::Relative,
        value: Num::int(30),
    });
    assert_eq!(resolve_setup(&q, "s2").unwrap().index_angle, 30.0);
    assert_eq!(resolve_setup(&q, "s3").unwrap().index_angle, 120.0);
}

#[test]
fn a_parent_edit_flows_to_children() {
    let mut p = read(SPEC).unwrap().project;
    p.setup_mut("s1").unwrap().material = Some("Oak".into());
    for id in ["s2", "s3", "s4"] {
        assert_eq!(
            resolve_setup(&p, id).unwrap().material.as_deref(),
            Some("Oak")
        );
    }
}

fn codes(p: &Project, id: &str) -> Vec<String> {
    match resolve_setup(p, id) {
        Ok(r) => r.diagnostics.into_iter().map(|d| d.code).collect(),
        Err(d) => vec![d.code],
    }
}

#[test]
fn chain_errors() {
    let p = read(SPEC).unwrap().project;
    assert!(codes(&p, "s4").is_empty());
    let mut q = p.clone();
    q.setup_mut("s2").unwrap().based_on = Some("s9".into());
    assert_eq!(codes(&q, "s2"), ["setup.based_on.missing"]);
    // The broken link shows up in every descendant.
    assert_eq!(codes(&q, "s4"), ["setup.based_on.missing"]);
    let mut q = p.clone();
    q.setup_mut("s2").unwrap().based_on = Some("s3".into());
    assert_eq!(codes(&q, "s2"), ["setup.based_on.order"]);
    let mut q = p.clone();
    q.setup_mut("s1").unwrap().based_on = Some("s1".into());
    assert_eq!(codes(&q, "s1"), ["setup.based_on.order"]);
    assert_eq!(codes(&q, "s1"), codes(&q, "s3"));
    assert_eq!(codes(&p, "nope"), ["setup.missing"]);
}

#[test]
fn from_previous_rules() {
    let p = read(SPEC).unwrap().project;
    // Kind, index angle, region and clearance may change.
    let mut q = p.clone();
    let s3 = q.setup_mut("s3").unwrap();
    s3.region = Some(Region {
        floor_z: Num::int(5),
    });
    s3.clearance = Some(fluut_model::project::ClearanceSpec::Cylinder {
        radius: Num::int(40),
    });
    assert!(codes(&q, "s3").is_empty());
    // Machine, geometry, rotary axis and zero may not.
    for change in [
        |s: &mut Setup| s.machine = Some("user/other@1".into()),
        |s: &mut Setup| {
            s.geometry = Some(fluut_model::project::GeometrySel {
                bodies: Some(vec!["b-1".into()]),
                ..Default::default()
            })
        },
        |s: &mut Setup| {
            s.orientation = Some(Orientation::Rotary {
                rotary_axis: AxisPick::Axis(AxisDir::PosY),
                a_zero_toward: None,
            })
        },
        |s: &mut Setup| {
            s.zero = Some(ZeroSpec::OnAxis {
                x: fluut_model::project::AxisX::StockRight,
            })
        },
    ] {
        let mut q = p.clone();
        change(q.setup_mut("s3").unwrap());
        assert_eq!(codes(&q, "s3"), ["setup.stock.from_previous.changed"]);
    }
    // Setting a value equal to the parent's is allowed.
    let mut q = p.clone();
    q.setup_mut("s3").unwrap().machine = Some("user/my-router@2".into());
    assert!(codes(&q, "s3").is_empty());
    let mut q = p.clone();
    q.setup_mut("s2").unwrap().based_on = None;
    assert!(codes(&q, "s2").contains(&"setup.stock.from_previous.parent".to_owned()));
}

#[test]
fn kind_rules() {
    let p = read(SPEC).unwrap().project;
    let mut q = p.clone();
    q.setup_mut("s1").unwrap().kind = Some(SetupKind::ThreeAxis);
    let c = codes(&q, "s1");
    assert!(c.contains(&"setup.orientation.kind".to_owned()), "{c:?}");
    assert!(c.contains(&"setup.zero.kind".to_owned()), "{c:?}");
    let mut q = p.clone();
    q.setup_mut("s1").unwrap().stock = None;
    assert_eq!(codes(&q, "s1"), ["setup.stock.missing"]);
    let mut q = p.clone();
    q.setup_mut("s1").unwrap().zero = Some(ZeroSpec::StockTopCentre);
    assert_eq!(codes(&q, "s1"), ["setup.zero.kind"]);
}
