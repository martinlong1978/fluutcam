// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use fluut_geom::Tol;

/// One entity as group-code pairs.
fn ent(kind: &str, codes: &[(i32, &str)]) -> String {
    let mut s = format!("0\n{kind}\n");
    for (c, v) in codes {
        s.push_str(&format!("{c}\n{v}\n"));
    }
    s
}

/// A file with `entities` (and `blocks`, if any) and `$INSUNITS` `units`.
fn file(units: Option<i64>, blocks: &str, entities: &str) -> String {
    let mut s = String::new();
    if let Some(u) = units {
        s.push_str(&format!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n{u}\n0\nENDSEC\n"
        ));
    }
    if !blocks.is_empty() {
        s.push_str(&format!("0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n"));
    }
    s.push_str(&format!(
        "0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
    ));
    s
}

fn block(name: &str, base: (f64, f64), entities: &str) -> String {
    format!(
        "{}{entities}{}",
        ent(
            "BLOCK",
            &[
                (8, "0"),
                (2, name),
                (70, "0"),
                (10, &base.0.to_string()),
                (20, &base.1.to_string()),
                (30, "0")
            ]
        ),
        ent("ENDBLK", &[(8, "0")])
    )
}

fn read(text: &str) -> Import2D {
    read_dxf(text.as_bytes(), &opts()).unwrap()
}

fn opts() -> Options {
    Options {
        scale: Some(1.0),
        tol_model: 1e-6,
        limits: Limits::default(),
    }
}

fn all_curves(r: &Import2D) -> Vec<&WireCurve> {
    r.wires.iter().flat_map(|w| w.curves.iter()).collect()
}

fn at(c: &WireCurve, f: f64) -> Point3 {
    c.curve.eval(c.range.at(f))
}

#[track_caller]
fn near(a: Point3, b: (f64, f64, f64)) {
    let b = Point3::new(b.0, b.1, b.2);
    assert!(a.distance(b) < 1e-9, "{a:?} vs {b:?}");
}

fn line_ent(layer: &str, a: (f64, f64), b: (f64, f64)) -> String {
    ent(
        "LINE",
        &[
            (8, layer),
            (10, &a.0.to_string()),
            (20, &a.1.to_string()),
            (30, "0"),
            (11, &b.0.to_string()),
            (21, &b.1.to_string()),
            (31, "0"),
        ],
    )
}

#[test]
fn lines_by_layer_in_first_seen_order() {
    let text = file(
        None,
        "",
        &[
            line_ent("B", (0.0, 0.0), (1.0, 0.0)),
            line_ent("A", (0.0, 0.0), (0.0, 2.0)),
            line_ent("B", (1.0, 0.0), (1.0, 1.0)),
        ]
        .concat(),
    );
    let r = read(&text);
    let names: Vec<&str> = r.wires.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, ["B", "A"]);
    assert_eq!(r.wires[0].curves.len(), 2);
    near(at(&r.wires[1].curves[0], 1.0), (0.0, 2.0, 0.0));
    assert_eq!(r.wires[1].tol_model, 1e-6);
}

#[test]
fn circle_and_arc() {
    let text = file(
        None,
        "",
        &[
            ent(
                "CIRCLE",
                &[(8, "0"), (10, "10"), (20, "1"), (30, "0"), (40, "2")],
            ),
            ent(
                "ARC",
                &[
                    (8, "0"),
                    (10, "1"),
                    (20, "1"),
                    (30, "0"),
                    (40, "2"),
                    (50, "0"),
                    (51, "90"),
                ],
            ),
            // Crosses 0°: from 270° to 45°.
            ent(
                "ARC",
                &[
                    (8, "0"),
                    (10, "0"),
                    (20, "0"),
                    (30, "0"),
                    (40, "1"),
                    (50, "270"),
                    (51, "45"),
                ],
            ),
        ]
        .concat(),
    );
    let r = read(&text);
    let c = all_curves(&r);
    assert!(matches!(c[0].curve, Curve3::Circle { radius, .. } if radius == 2.0));
    near(at(c[1], 0.0), (3.0, 1.0, 0.0));
    near(at(c[1], 0.5), (1.0 + 2f64.sqrt(), 1.0 + 2f64.sqrt(), 0.0));
    near(at(c[1], 1.0), (1.0, 3.0, 0.0));
    near(at(c[2], 0.0), (0.0, -1.0, 0.0));
    near(at(c[2], 1.0), (0.5f64.sqrt(), 0.5f64.sqrt(), 0.0));
    assert!((c[2].range.length() - 135f64.to_radians()).abs() < 1e-12);
    assert_eq!(r.report.closed_chains, 1);
}

#[test]
fn arc_with_negative_extrusion_is_mirrored() {
    // OCS for (0, 0, −1): x = (−1, 0, 0), y = (0, 1, 0). The arc turns
    // clockwise seen from +Z.
    let text = file(
        None,
        "",
        &ent(
            "ARC",
            &[
                (8, "0"),
                (10, "5"),
                (20, "0"),
                (30, "0"),
                (40, "2"),
                (50, "0"),
                (51, "90"),
                (210, "0"),
                (220, "0"),
                (230, "-1"),
            ],
        ),
    );
    let r = read(&text);
    let c = all_curves(&r)[0];
    near(at(c, 0.0), (-7.0, 0.0, 0.0));
    let h = 2.0 * 0.5f64.sqrt();
    near(at(c, 0.5), (-5.0 - h, h, 0.0));
    near(at(c, 1.0), (-5.0, 2.0, 0.0));
}

/// The arc midpoint of a bulge segment: the sagitta `b·c/2` to the right of
/// the chord for positive (counter-clockwise) bulges.
#[test]
fn bulge_segments() {
    let (p0, p1) = (Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 6.0, 3.0));
    let c = p0.distance(p1);
    let right = Vec3::new(4.0, -3.0, 0.0) * (1.0 / 5.0);
    for b in [
        1.0,
        -1.0,
        0.41421356237309503,
        -0.41421356237309503,
        2.5,
        -0.1,
    ] {
        let s = bulge_segment(p0, p1, b).unwrap();
        near(at(&s, 0.0), (1.0, 2.0, 3.0));
        near(at(&s, 1.0), (4.0, 6.0, 3.0));
        let m = p0.lerp(p1, 0.5) + right * (b * c / 2.0);
        near(at(&s, 0.5), (m.x(), m.y(), m.z()));
        assert!((s.range.length() - 4.0 * libm::atan(b.abs())).abs() < 1e-12);
    }
    assert!(matches!(
        bulge_segment(p0, p1, 0.0).unwrap().curve,
        Curve3::Line { .. }
    ));
    assert!(bulge_segment(p0, p0, 1.0).is_none());
}

fn lwpolyline(extrusion: &str) -> String {
    ent(
        "LWPOLYLINE",
        &[
            (8, "P"),
            (90, "4"),
            (70, "1"),
            (38, "2"),
            (10, "0"),
            (20, "0"),
            (10, "10"),
            (20, "0"),
            (42, "1"),
            (10, "10"),
            (20, "6"),
            (10, "0"),
            (20, "6"),
            (42, "-0.5"),
            (210, "0"),
            (220, "0"),
            (230, extrusion),
        ],
    )
}

#[test]
fn lwpolyline_with_bulges_and_elevation() {
    let r = read(&file(None, "", &lwpolyline("1")));
    let c = &r.wires[0].curves;
    assert_eq!(c.len(), 4);
    assert!(matches!(c[0].curve, Curve3::Line { .. }));
    // Semicircle on the right of (10,0)→(10,6): through (13, 3).
    near(at(&c[1], 0.5), (13.0, 3.0, 2.0));
    near(at(&c[3], 1.0), (0.0, 0.0, 2.0));
    assert_eq!((r.report.chains, r.report.closed_chains), (1, 1));
}

#[test]
fn lwpolyline_with_negative_extrusion_mirrors_x() {
    let up = read(&file(None, "", &lwpolyline("1")));
    let down = read(&file(None, "", &lwpolyline("-1")));
    for (a, b) in up.wires[0].curves.iter().zip(&down.wires[0].curves) {
        for f in [0.0, 0.3, 0.5, 1.0] {
            let p = at(a, f);
            // OCS z is along −Z, so the elevation goes to z = −2 as well.
            near(at(b, f), (-p.x(), p.y(), -p.z()));
        }
    }
}

#[test]
fn old_polylines_2d_and_3d() {
    let vertex = |x: &str, y: &str, z: &str, bulge: &str| {
        ent(
            "VERTEX",
            &[(8, "0"), (10, x), (20, y), (30, z), (42, bulge)],
        )
    };
    let two_d = [
        ent(
            "POLYLINE",
            &[
                (8, "0"),
                (66, "1"),
                (10, "0"),
                (20, "0"),
                (30, "1.5"),
                (70, "0"),
            ],
        ),
        vertex("0", "0", "0", "1"),
        vertex("2", "0", "0", "0"),
        vertex("2", "5", "0", "0"),
        ent("SEQEND", &[(8, "0")]),
    ]
    .concat();
    let three_d = [
        ent("POLYLINE", &[(8, "0"), (66, "1"), (70, "9")]),
        vertex("0", "0", "0", "0"),
        vertex("1", "0", "1", "0"),
        vertex("1", "1", "2", "0"),
        ent("SEQEND", &[(8, "0")]),
    ]
    .concat();
    let mesh = [
        ent("POLYLINE", &[(8, "0"), (66, "1"), (70, "64")]),
        vertex("0", "0", "0", "0"),
        ent("SEQEND", &[(8, "0")]),
    ]
    .concat();
    let r = read(&file(None, "", &[two_d, three_d, mesh].concat()));
    let c = all_curves(&r);
    assert_eq!(c.len(), 2 + 3);
    near(at(c[0], 0.5), (1.0, -1.0, 1.5));
    near(at(c[1], 1.0), (2.0, 5.0, 1.5));
    near(at(c[3], 1.0), (1.0, 1.0, 2.0));
    near(at(c[4], 1.0), (0.0, 0.0, 0.0));
    assert_eq!(r.report.unsupported.get("POLYLINE mesh"), Some(&1));
}

#[test]
fn ellipses() {
    let e = |lo: &str, hi: &str, ratio: &str| {
        ent(
            "ELLIPSE",
            &[
                (8, "0"),
                (10, "1"),
                (20, "0"),
                (30, "0"),
                (11, "4"),
                (21, "0"),
                (31, "0"),
                (40, ratio),
                (41, lo),
                (42, hi),
            ],
        )
    };
    let pi = std::f64::consts::PI.to_string();
    let r = read(&file(
        None,
        "",
        &[e("0", &pi, "0.5"), e("0", "6.283185307179586", "1")].concat(),
    ));
    let c = all_curves(&r);
    assert!(matches!(c[0].curve, Curve3::Ellipse { .. }));
    near(at(c[0], 0.0), (5.0, 0.0, 0.0));
    near(at(c[0], 0.5), (1.0, 2.0, 0.0));
    near(at(c[0], 1.0), (-3.0, 0.0, 0.0));
    assert!(matches!(c[1].curve, Curve3::Circle { radius, .. } if (radius - 4.0).abs() < 1e-12));
}

#[test]
fn nested_mirrored_inserts_and_layer_zero() {
    let a = block(
        "A",
        (0.0, 0.0),
        &[
            line_ent("0", (0.0, 0.0), (1.0, 0.0)),
            ent(
                "CIRCLE",
                &[(8, "X"), (10, "2"), (20, "0"), (30, "0"), (40, "1")],
            ),
        ]
        .concat(),
    );
    let b = block(
        "b", // block names are case-insensitive
        (5.0, 0.0),
        &ent(
            "INSERT",
            &[
                (8, "0"),
                (2, "A"),
                (10, "10"),
                (20, "0"),
                (30, "0"),
                (50, "90"),
            ],
        ),
    );
    let ins = ent(
        "INSERT",
        &[
            (8, "L"),
            (2, "B"),
            (10, "0"),
            (20, "0"),
            (30, "0"),
            (41, "-1"),
            (42, "1"),
            (43, "1"),
        ],
    );
    let r = read(&file(None, &[a, b].concat(), &ins));
    let names: Vec<&str> = r.wires.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, ["L", "X"]);
    let l = &r.wires[0].curves[0];
    near(at(l, 0.0), (-5.0, 0.0, 0.0));
    near(at(l, 1.0), (-5.0, 1.0, 0.0));
    let c = &r.wires[1].curves[0];
    let Curve3::Circle { frame, radius } = &c.curve else {
        panic!("{c:?}")
    };
    near(frame.origin, (-5.0, 2.0, 0.0));
    assert!((radius - 1.0).abs() < 1e-12);
}

#[test]
fn insert_arrays() {
    let a = block("A", (0.0, 0.0), &line_ent("0", (0.0, 0.0), (1.0, 0.0)));
    let ins = ent(
        "INSERT",
        &[
            (8, "0"),
            (2, "A"),
            (10, "100"),
            (20, "0"),
            (70, "3"),
            (71, "2"),
            (44, "10"),
            (45, "5"),
        ],
    );
    let r = read(&file(None, &a, &ins));
    let starts: Vec<(f64, f64)> = all_curves(&r)
        .iter()
        .map(|c| {
            let p = at(c, 0.0);
            (p.x(), p.y())
        })
        .collect();
    assert_eq!(
        starts,
        [
            (100.0, 0.0),
            (110.0, 0.0),
            (120.0, 0.0),
            (100.0, 5.0),
            (110.0, 5.0),
            (120.0, 5.0)
        ]
    );
}

#[test]
fn splines_from_control_and_fit_points() {
    let ctrl = ent(
        "SPLINE",
        &[
            (8, "0"),
            (70, "8"),
            (71, "3"),
            (72, "8"),
            (73, "4"),
            (40, "0"),
            (40, "0"),
            (40, "0"),
            (40, "0"),
            (40, "1"),
            (40, "1"),
            (40, "1"),
            (40, "1"),
            (10, "0"),
            (20, "0"),
            (30, "0"),
            (10, "1"),
            (20, "2"),
            (30, "0"),
            (10, "3"),
            (20, "2"),
            (30, "0"),
            (10, "4"),
            (20, "0"),
            (30, "0"),
        ],
    );
    let fit_pts = [(0.0, 0.0), (1.0, 1.0), (3.0, 1.5), (4.0, -1.0), (7.0, 0.0)];
    let mut codes: Vec<(i32, String)> = vec![
        (8, "0".into()),
        (70, "8".into()),
        (71, "3".into()),
        (74, "5".into()),
    ];
    for (x, y) in fit_pts {
        codes.extend([(11, x.to_string()), (21, y.to_string()), (31, "0".into())]);
    }
    let codes: Vec<(i32, &str)> = codes.iter().map(|(c, v)| (*c, v.as_str())).collect();
    let fit = ent("SPLINE", &codes);
    let r = read(&file(None, "", &[ctrl, fit].concat()));
    let c = all_curves(&r);
    near(at(c[0], 0.5), (2.0, 1.5, 0.0));
    near(at(c[0], 1.0), (4.0, 0.0, 0.0));
    let tol = Tol::for_machining(1e-6, 1e-3);
    for (x, y) in fit_pts {
        let (_, d) = c[1].curve.closest(Point3::new(x, y, 0.0), c[1].range, &tol);
        assert!(d < 1e-9, "({x}, {y}): {d}");
    }
    // Two fit points give a line-like spline of degree 1.
    let two = interpolate(&[Point3::ORIGIN, Point3::new(2.0, 0.0, 0.0)]).unwrap();
    assert_eq!(two.degree(), 1);
    assert!(interpolate(&[Point3::ORIGIN, Point3::ORIGIN]).is_none());
}

#[test]
fn units_are_suggested_and_scale_applied() {
    let body = line_ent("0", (0.0, 0.0), (1.0, 0.0));
    for (code, unit) in [
        (4, Some(Unit::Mm)),
        (1, Some(Unit::In)),
        (6, Some(Unit::M)),
        (0, None),
    ] {
        assert_eq!(read(&file(Some(code), "", &body)).report.unit, unit);
    }
    let odd = read(&file(Some(99), "", &body));
    assert_eq!(odd.report.unit, None);
    assert!(
        odd.report.warnings[0].contains("99"),
        "{:?}",
        odd.report.warnings
    );
    let mut o = opts();
    o.scale = Some(25.4);
    let r = read_dxf(file(Some(1), "", &body).as_bytes(), &o).unwrap();
    near(at(all_curves(&r)[0], 1.0), (25.4, 0.0, 0.0));
}

#[test]
fn chain_report() {
    let open = [
        line_ent("0", (0.0, 0.0), (1.0, 0.0)),
        line_ent("0", (1.0, 0.0), (1.0, 1.0)),
        line_ent("0", (1.0, 1.0), (0.0, 1.0)),
    ]
    .concat();
    let r = read(&file(None, "", &open));
    assert_eq!(
        (r.report.chains, r.report.closed_chains, r.report.open_ends),
        (1, 0, 2)
    );
    let closed = [open.clone(), line_ent("0", (0.0, 1.0), (0.0, 0.0))].concat();
    let r = read(&file(None, "", &closed));
    assert_eq!(
        (r.report.chains, r.report.closed_chains, r.report.open_ends),
        (1, 1, 0)
    );
}

#[test]
fn ignored_and_unsupported_entities() {
    let text = file(
        None,
        "",
        &[
            ent("TEXT", &[(8, "0"), (1, "hello")]),
            ent("MTEXT", &[(8, "0"), (1, "x")]),
            ent("3DSOLID", &[(8, "0"), (1, "acis")]),
            ent("FROBNICATOR", &[(8, "0")]),
            ent("LINE", &[(8, "0"), (67, "1"), (11, "1")]),
        ]
        .concat(),
    );
    let r = read(&text);
    assert!(r.wires.is_empty());
    assert_eq!(r.report.ignored.get("TEXT"), Some(&1));
    assert_eq!(r.report.ignored.get("paper space"), Some(&1));
    assert_eq!(r.report.unsupported.get("3DSOLID"), Some(&1));
    assert_eq!(r.report.unsupported.get("FROBNICATOR"), Some(&1));
}

#[test]
fn crlf_and_padding() {
    let text = file(None, "", &line_ent("0", (0.0, 0.0), (3.0, 0.0)))
        .replace('\n', "\r\n")
        .replace("\r\n10\r\n", "\r\n 10\r\n");
    let r = read(&text);
    near(at(all_curves(&r)[0], 1.0), (3.0, 0.0, 0.0));
}

// --- Untrusted input --------------------------------------------------------

#[test]
fn malformed_files_are_errors() {
    let e = read_dxf(b"0\nSECTION\n2", &opts()).unwrap_err();
    assert_eq!(e.line, Some(3));
    let e = read_dxf(b"0\nSECTION\nX\nfoo\n", &opts()).unwrap_err();
    assert_eq!(e.line, Some(3));
    assert!(
        read_dxf(b"AutoCAD Binary DXF\r\n\x1a\x00", &opts())
            .unwrap_err()
            .msg
            .contains("binary")
    );
    let mut o = opts();
    o.limits.max_bytes = 10;
    assert!(read_dxf(&[b'0'; 11], &o).is_err());
    let mut o = opts();
    o.scale = Some(0.0);
    assert!(read_dxf(file(None, "", "").as_bytes(), &o).is_err());
}

#[test]
fn missing_eof_is_a_warning() {
    let text = file(None, "", &line_ent("0", (0.0, 0.0), (1.0, 0.0)));
    let cut = text.trim_end_matches("0\nEOF\n");
    let r = read(cut);
    assert_eq!(all_curves(&r).len(), 1);
    assert!(r.report.warnings.iter().any(|w| w.contains("EOF")));
    let cut = &text[..text.find("0\nENDSEC").unwrap()];
    let r = read(cut);
    assert_eq!(all_curves(&r).len(), 1);
}

#[test]
fn bad_values_drop_the_entity() {
    let text = file(
        None,
        "",
        &[
            ent("LINE", &[(8, "0"), (10, "nan"), (11, "1")]),
            ent("LINE", &[(8, "0"), (10, "1e999"), (11, "1")]),
            ent("LINE", &[(8, "0"), (10, "2e9"), (11, "1")]),
            ent("CIRCLE", &[(8, "0"), (40, "0")]),
            ent("ARC", &[(8, "0"), (40, "1"), (50, "10"), (51, "10")]),
            ent("LWPOLYLINE", &[(8, "0"), (70, "x"), (10, "0"), (20, "0")]),
            ent(
                "SPLINE",
                &[(8, "0"), (71, "3"), (40, "0"), (10, "0"), (10, "1")],
            ),
            ent("SPLINE", &[(8, "0"), (71, "300"), (10, "0"), (10, "1")]),
            ent("ELLIPSE", &[(8, "0"), (11, "1"), (40, "1"), (230, "0")]),
            ent("INSERT", &[(8, "0"), (2, "NOPE")]),
            line_ent("0", (0.0, 0.0), (1.0, 0.0)),
        ]
        .concat(),
    );
    let r = read(&text);
    assert_eq!(all_curves(&r).len(), 1);
    assert_eq!(r.report.dropped, 9, "{:?}", r.report.warnings);
    assert!(r.report.warnings.iter().any(|w| w.contains("NOPE")));
}

#[test]
fn self_inserting_and_deep_blocks_stop_at_the_depth_limit() {
    let selfish = block(
        "S",
        (0.0, 0.0),
        &[
            line_ent("0", (0.0, 0.0), (1.0, 0.0)),
            ent("INSERT", &[(8, "0"), (2, "S"), (10, "1")]),
        ]
        .concat(),
    );
    let r = read(&file(None, &selfish, &ent("INSERT", &[(8, "0"), (2, "S")])));
    assert_eq!(all_curves(&r).len(), 16);
    assert!(
        r.report
            .warnings
            .iter()
            .any(|w| w.contains("deeper than 16"))
    );

    let mut blocks = String::new();
    for i in 0..18 {
        let inner = if i == 17 {
            line_ent("0", (0.0, 0.0), (1.0, 0.0))
        } else {
            ent("INSERT", &[(8, "0"), (2, &format!("B{}", i + 1))])
        };
        blocks.push_str(&block(&format!("B{i}"), (0.0, 0.0), &inner));
    }
    // B2 … B17 is 16 levels: expanded. B1 … B17 is 17: not.
    let r = read(&file(None, &blocks, &ent("INSERT", &[(8, "0"), (2, "B2")])));
    assert_eq!(all_curves(&r).len(), 1);
    let r = read(&file(None, &blocks, &ent("INSERT", &[(8, "0"), (2, "B1")])));
    assert!(all_curves(&r).is_empty());
    assert!(r.report.warnings.iter().any(|w| w.contains("deeper")));
}

#[test]
fn exponential_fan_out_hits_a_limit() {
    // Ten levels of ten inserts each: 10^10 lines if expanded.
    let mut blocks = block("F0", (0.0, 0.0), &line_ent("0", (0.0, 0.0), (1.0, 0.0)));
    for i in 1..=10 {
        let ten: String = (0..10)
            .map(|_| ent("INSERT", &[(8, "0"), (2, &format!("F{}", i - 1))]))
            .collect();
        blocks.push_str(&block(&format!("F{i}"), (0.0, 0.0), &ten));
    }
    let text = file(None, &blocks, &ent("INSERT", &[(8, "0"), (2, "F10")]));
    let mut o = opts();
    o.limits.max_items = 100_000;
    assert!(
        read_dxf(text.as_bytes(), &o)
            .unwrap_err()
            .msg
            .contains("entities")
    );
    let mut o = opts();
    o.limits.max_curves = 1000;
    assert!(
        read_dxf(text.as_bytes(), &o)
            .unwrap_err()
            .msg
            .contains("curves")
    );
    // Empty blocks fanned out produce no curves but still hit the item limit.
    let mut empty = block("E0", (0.0, 0.0), "");
    for i in 1..=10 {
        let ten: String = (0..10)
            .map(|_| ent("INSERT", &[(8, "0"), (2, &format!("E{}", i - 1))]))
            .collect();
        empty.push_str(&block(&format!("E{i}"), (0.0, 0.0), &ten));
    }
    let text = file(None, &empty, &ent("INSERT", &[(8, "0"), (2, "E10")]));
    o.limits.max_items = 100_000;
    assert!(read_dxf(text.as_bytes(), &o).is_err());
    let huge_array = ent(
        "INSERT",
        &[(8, "0"), (2, "F0"), (70, "100000"), (71, "100000")],
    );
    assert!(read_dxf(file(None, &blocks, &huge_array).as_bytes(), &opts()).is_err());
}

#[test]
fn point_limit() {
    let mut o = opts();
    o.limits.max_points = 3;
    let pts: String = (0..4).map(|i| format!("10\n{i}\n20\n0\n")).collect();
    let text = file(None, "", &format!("0\nLWPOLYLINE\n8\n0\n{pts}"));
    let r = read_dxf(text.as_bytes(), &o).unwrap();
    assert_eq!(r.report.dropped, 1);
}
