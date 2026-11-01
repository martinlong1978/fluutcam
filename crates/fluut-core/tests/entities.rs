// SPDX-License-Identifier: MIT OR Apache-2.0

//! Body and face descriptions on the reference projects.

use fluut_core::Session;
use fluut_core::entities::{Param, bodies};
use std::path::PathBuf;

fn reference(name: &str) -> Session {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fluut-cli/tests/projects")
        .join(name)
        .join(format!("{name}.fluut"));
    Session::open(&p, &[]).unwrap()
}

fn num(p: &Param) -> f64 {
    match p {
        Param::Num(x) => *x,
        _ => panic!("{p:?}"),
    }
}

#[test]
fn holes_are_inside_vertical_cylinders() {
    let s = reference("holes");
    let b = bodies(&s.store);
    assert_eq!(b.len(), 1);
    assert_eq!((b[0].id.as_str(), b[0].kind), ("b-612", "solid"));
    assert_eq!(b[0].source, "g1");
    let mut radii: Vec<f64> = b[0]
        .faces
        .iter()
        .filter(|f| f.kind == "cylinder")
        .filter(|f| f.params["inside"] == Param::Bool(true))
        .filter(|f| f.params["axis_is_vertical"] == Param::Bool(true))
        .map(|f| num(&f.params["radius"]))
        .collect();
    radii.sort_by(f64::total_cmp);
    radii.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert!(radii.contains(&11.0) && radii.contains(&2.5), "{radii:?}");
    for f in &b[0].faces {
        assert!(f.id.starts_with("f-612."));
        assert!(f.bounds[0] <= f.bounds[3] && f.bounds[2] <= f.bounds[5]);
    }
    let bb = b[0].bounds;
    for f in &b[0].faces {
        for i in 0..3 {
            assert!(f.bounds[i] >= bb[i] && f.bounds[i + 3] <= bb[i + 3]);
        }
    }
}

#[test]
fn block_planes_point_out() {
    let s = reference("block");
    let b = &bodies(&s.store)[0];
    let top_z = b.bounds[5];
    let tops: Vec<_> = b
        .faces
        .iter()
        .filter(|f| f.kind == "plane" && f.bounds[2] == top_z && f.bounds[5] == top_z)
        .collect();
    assert!(!tops.is_empty());
    for f in tops {
        assert_eq!(f.params["normal"], Param::Vec([0.0, 0.0, 1.0]), "{}", f.id);
        assert_eq!(f.params["normal_is_vertical"], Param::Bool(true));
        assert!((num(&f.params["offset"]) - top_z).abs() < 1e-9);
    }
}

#[test]
fn wire_bodies_have_no_faces() {
    let s = reference("plate");
    let b = bodies(&s.store);
    assert!(!b.is_empty());
    for x in b {
        assert_eq!(x.kind, "wire");
        assert!(x.faces.is_empty());
        assert!(x.bounds[0] < x.bounds[3]);
    }
}
