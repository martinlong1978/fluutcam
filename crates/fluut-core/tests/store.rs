// SPDX-License-Identifier: MIT OR Apache-2.0

//! The model store reads DXF, SVG and STL sources (M7).

use fluut_cam::geometry::BodyGeom;
use fluut_core::ModelStore;
use fluut_model::Diagnostic;
use fluut_model::diag::Severity;
use fluut_model::project::{GeometrySource, SourceKind};
use std::path::{Path, PathBuf};

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("store-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn source(path: &str, kind: SourceKind, unit: Option<&str>) -> GeometrySource {
    GeometrySource {
        id: "g1".into(),
        path: path.into(),
        kind,
        hash: None,
        unit: unit.map(str::to_owned),
        bodies: Vec::new(),
    }
}

fn load(
    d: &Path,
    file: &str,
    text: &str,
    kind: SourceKind,
    unit: Option<&str>,
) -> (ModelStore, Vec<Diagnostic>) {
    std::fs::write(d.join(file), text).unwrap();
    let mut s = ModelStore::new();
    let diags = s.load(&source(file, kind, unit), d);
    (s, diags)
}

fn codes(d: &[Diagnostic]) -> Vec<&str> {
    d.iter().map(|d| d.code.as_str()).collect()
}

const DXF: &str = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n\
                   0\nSECTION\n2\nENTITIES\n\
                   0\nLINE\n8\nA\n10\n0\n20\n0\n11\n1\n21\n0\n\
                   0\nCIRCLE\n8\nB\n10\n5\n20\n5\n40\n1\n\
                   0\nTEXT\n8\nA\n1\nhello\n\
                   0\nENDSEC\n0\nEOF\n";

#[test]
fn dxf_layers_become_wire_bodies() {
    let d = dir("dxf");
    let (s, diags) = load(&d, "a.dxf", DXF, SourceKind::Dxf, Some("in"));
    assert_eq!(codes(&diags), ["model.geometry.import"], "{diags:?}");
    assert!(diags[0].message.contains("1 TEXT"));
    let src = s.source("g1").unwrap();
    let ids: Vec<&str> = src.bodies.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["b-1", "b-2"]);
    let BodyGeom::Wire(w) = &src.bodies[0].geom else {
        panic!()
    };
    assert_eq!(w.name, "A");
    let end = w.curves[0].curve.eval(w.curves[0].range.hi);
    assert!((end.x() - 25.4).abs() < 1e-12);
}

#[test]
fn dxf_unit_is_required_and_checked() {
    let d = dir("units");
    let (s, diags) = load(&d, "a.dxf", DXF, SourceKind::Dxf, None);
    assert_eq!(codes(&diags), ["model.geometry.unit.missing"]);
    assert!(
        diags[0].message.contains("suggests `mm`"),
        "{}",
        diags[0].message
    );
    assert_eq!(diags[0].severity, Severity::Error);
    assert!(s.source("g1").is_none());
    let (_, diags) = load(&d, "a.dxf", DXF, SourceKind::Dxf, Some("furlong"));
    assert_eq!(codes(&diags), ["model.geometry.unit.unknown"]);
    let (_, diags) = load(&d, "b.dxf", "X\n", SourceKind::Dxf, Some("mm"));
    assert_eq!(codes(&diags), ["model.geometry.dxf"]);
}

#[test]
fn svg_takes_the_document_unit() {
    let d = dir("svg");
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10mm\" height=\"10mm\" \
               viewBox=\"0 0 10 10\"><line x1=\"0\" y1=\"0\" x2=\"10\" y2=\"0\"/></svg>";
    let (s, diags) = load(&d, "a.svg", svg, SourceKind::Svg, None);
    assert!(diags.is_empty(), "{diags:?}");
    let BodyGeom::Wire(w) = &s.source("g1").unwrap().bodies[0].geom else {
        panic!()
    };
    let end = w.curves[0].curve.eval(w.curves[0].range.hi);
    assert!((end.x() - 10.0).abs() < 1e-12 && (end.y() - 10.0).abs() < 1e-12);
    let (_, diags) = load(&d, "b.svg", "<html/>", SourceKind::Svg, None);
    assert_eq!(codes(&diags), ["model.geometry.svg"]);
}

#[test]
fn stl_is_checked_but_not_offered() {
    let d = dir("stl");
    let stl = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n\
               endloop\nendfacet\nendsolid t\n";
    let (s, diags) = load(&d, "a.stl", stl, SourceKind::Stl, Some("mm"));
    assert_eq!(
        codes(&diags),
        ["model.geometry.import", "model.geometry.mesh.unused"]
    );
    assert!(
        diags[0].message.contains("3 open edges in 1 holes"),
        "{}",
        diags[0].message
    );
    assert!(s.source("g1").unwrap().bodies.is_empty());
    let (_, diags) = load(&d, "a.stl", stl, SourceKind::Stl, None);
    assert_eq!(codes(&diags), ["model.geometry.unit.missing"]);
    let (_, diags) = load(&d, "b.stl", "garbage", SourceKind::Stl, Some("mm"));
    assert_eq!(codes(&diags), ["model.geometry.stl"]);
}
