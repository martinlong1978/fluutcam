// SPDX-License-Identifier: MIT OR Apache-2.0

use super::path::{Builder, parse_path};
use super::*;
use fluut_geom::Curve3;

fn opts() -> Options {
    Options {
        scale: None,
        tol_model: 1e-6,
        limits: Limits::default(),
    }
}

/// A 100 × 100 mm document with one user unit per mm, Y flipped:
/// user `(x, y)` is `(x, 100 − y)` mm.
fn doc(body: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"100mm\" height=\"100mm\" viewBox=\"0 0 100 100\">{body}</svg>"
    )
}

fn read(text: &str) -> Import2D {
    read_svg(text.as_bytes(), &opts()).unwrap()
}

fn curves(r: &Import2D) -> Vec<&WireCurve> {
    r.wires.iter().flat_map(|w| w.curves.iter()).collect()
}

fn at(c: &WireCurve, f: f64) -> Point3 {
    c.curve.eval(c.range.at(f))
}

#[track_caller]
fn near(a: Point3, b: (f64, f64)) {
    let b = Point3::new(b.0, b.1, 0.0);
    assert!(a.distance(b) < 1e-9, "{a:?} vs {b:?}");
}

fn path(d: &str) -> Builder {
    let mut b = Builder::new(1000);
    parse_path(d, &mut b).unwrap();
    b
}

#[test]
fn lines_absolute_and_relative() {
    let b = path("M10 10 L20 10 H30 V20 l-5 5 h-5 v-5 Z");
    let ends: Vec<(f64, f64)> = b
        .curves
        .iter()
        .map(|c| {
            let p = at(c, 1.0);
            (p.x(), p.y())
        })
        .collect();
    assert_eq!(
        ends,
        [
            (20.0, 10.0),
            (30.0, 10.0),
            (30.0, 20.0),
            (25.0, 25.0),
            (20.0, 25.0),
            (20.0, 20.0),
            (10.0, 10.0)
        ]
    );
    // Implicit linetos after a moveto, and a new subpath after Z.
    let b = path("m 1 1 2 0 0 2 z m 5 0 1 1");
    assert_eq!(b.curves.len(), 4);
    near(at(&b.curves[3], 0.0), (6.0, 1.0));
    near(at(&b.curves[3], 1.0), (7.0, 2.0));
}

#[test]
fn compact_numbers_and_flags() {
    let b = path("M1.5.5-1-2L1e1,2E0");
    near(at(&b.curves[0], 0.0), (1.5, 0.5));
    near(at(&b.curves[0], 1.0), (-1.0, -2.0));
    near(at(&b.curves[1], 1.0), (10.0, 2.0));
    let b = path("M0 0a5 5 0 00 10 0");
    near(at(&b.curves[0], 1.0), (10.0, 0.0));
    let b = path("M0 0a5,5,0,1,1,10,0");
    near(at(&b.curves[0], 1.0), (10.0, 0.0));
}

#[test]
fn arc_flags_choose_the_arc() {
    let h = 10.0 - 75f64.sqrt();
    for (large, sweep, mid) in [
        (0, 1, (5.0, -h)),
        (1, 1, (5.0, -20.0 + h)),
        (0, 0, (5.0, h)),
        (1, 0, (5.0, 20.0 - h)),
    ] {
        let b = path(&format!("M0 0 A10 10 0 {large} {sweep} 10 0"));
        let c = &b.curves[0];
        assert!(matches!(c.curve, Curve3::Circle { radius, .. } if (radius - 10.0).abs() < 1e-12));
        near(at(c, 0.0), (0.0, 0.0));
        near(at(c, 0.5), mid);
        near(at(c, 1.0), (10.0, 0.0));
    }
    // Radii too small are scaled up: a half circle of radius 5.
    let b = path("M0 0 A1 1 0 0 1 10 0");
    near(at(&b.curves[0], 0.5), (5.0, -5.0));
    // Zero radius is a line; same end point is nothing.
    assert!(matches!(
        path("M0 0 A0 5 0 0 1 10 0").curves[0].curve,
        Curve3::Line { .. }
    ));
    assert!(path("M0 0 A5 5 0 0 1 0 0").curves.is_empty());
}

#[test]
fn rotated_elliptical_arc() {
    let b = path("M0 0 A20 10 30 0 1 15 5");
    let c = &b.curves[0];
    near(at(c, 0.0), (0.0, 0.0));
    near(at(c, 1.0), (15.0, 5.0));
    let Curve3::Ellipse { a, b: minor, .. } = c.curve else {
        panic!("{c:?}")
    };
    assert!((a - 20.0).abs() < 1e-9 && (minor - 10.0).abs() < 1e-9);
}

#[test]
fn beziers_and_smooth_reflection() {
    let b = path("M0 0 C0 10 10 10 10 0 S20 -10 20 0 Q25 10 30 0 T40 0");
    let c: Vec<&WireCurve> = b.curves.iter().collect();
    assert_eq!(c.len(), 4);
    near(at(c[0], 0.5), (5.0, 7.5));
    // S reflects (10, 10) in (10, 0): first control (10, −10).
    let Curve3::BSpline(s) = &c[1].curve else {
        panic!()
    };
    assert_eq!(s.poles()[1], Point3::new(10.0, -10.0, 0.0));
    let Curve3::BSpline(t) = &c[3].curve else {
        panic!()
    };
    assert_eq!(t.degree(), 2);
    assert_eq!(t.poles()[1], Point3::new(35.0, -10.0, 0.0));
}

#[test]
fn bad_path_data_keeps_the_good_part() {
    let mut b = Builder::new(100);
    let e = parse_path("M0 0 L10 0 L x", &mut b).unwrap_err();
    assert_eq!(b.curves.len(), 1);
    assert_eq!(e.at, 13);
    let r = read(&doc("<path d=\"M0 0 L10 0 L x\"/><path d=\"L 1 1\"/>"));
    assert_eq!(curves(&r).len(), 1);
    assert_eq!(r.report.warnings.len(), 2, "{:?}", r.report.warnings);
}

#[test]
fn shapes() {
    let r = read(&doc("<rect x=\"10\" y=\"10\" width=\"30\" height=\"20\"/>\
         <rect x=\"0\" y=\"0\" width=\"20\" height=\"10\" rx=\"2\"/>\
         <circle cx=\"50\" cy=\"50\" r=\"5\"/>\
         <ellipse cx=\"50\" cy=\"50\" rx=\"8\" ry=\"4\"/>\
         <line x1=\"0\" y1=\"0\" x2=\"10\" y2=\"10\"/>\
         <polyline points=\"0,0 10,0 10,10\"/>\
         <polygon points=\"60 60 70 60 70 70\"/>\
         <rect width=\"0\" height=\"5\"/>"));
    let c = curves(&r);
    assert_eq!(c.len(), 4 + 8 + 1 + 1 + 1 + 2 + 3);
    near(at(c[0], 0.0), (10.0, 90.0));
    near(at(c[0], 1.0), (40.0, 90.0));
    // Rounded rect: top edge from x = 2 to 18, then a quarter arc.
    near(at(c[4], 0.0), (2.0, 100.0));
    near(at(c[5], 1.0), (20.0, 98.0));
    let s = std::f64::consts::FRAC_1_SQRT_2;
    near(at(c[5], 0.5), (18.0 + 2.0 * s, 98.0 + 2.0 * s));
    assert!(matches!(c[12].curve, Curve3::Circle { .. }));
    assert!(matches!(c[13].curve, Curve3::Ellipse { .. }));
    // The rounded rect, circle, ellipse and polygon. The first rect meets the
    // line and the polyline at (10, 10), so that chain branches.
    assert_eq!(r.report.closed_chains, 4);
}

#[test]
fn transforms_compose_and_keep_arcs() {
    // rotate(30) scale(2 1): the circle becomes a rotated ellipse.
    let r = read(&doc(
        "<g transform=\"translate(50 50)\"><circle r=\"5\" transform=\"rotate(30) scale(2 1)\"/></g>",
    ));
    let c = curves(&r)[0];
    let Curve3::Ellipse { a, b, .. } = &c.curve else {
        panic!("{c:?}")
    };
    assert!((a - 10.0).abs() < 1e-9 && (b - 5.0).abs() < 1e-9);
    let (sn, cs) = libm::sincos(30f64.to_radians());
    for i in 0..8 {
        let t = i as f64 * std::f64::consts::FRAC_PI_4;
        let (u, v) = (10.0 * libm::cos(t), 5.0 * libm::sin(t));
        let p = (50.0 + cs * u - sn * v, 50.0 + sn * u + cs * v);
        let want = Point3::new(p.0, 100.0 - p.1, 0.0);
        let (_, d) = c
            .curve
            .closest(want, c.range, &fluut_geom::Tol::for_machining(1e-6, 1e-3));
        assert!(d < 1e-9, "{i}: {d}");
    }
    // An arc in a rotated, unevenly scaled group: same points as mapping the
    // user-space arc.
    let r = read(&doc(
        "<path transform=\"matrix(1 0.5 -0.3 2 10 20) skewX(10)\" d=\"M0 0 A10 10 0 0 1 10 10\"/>",
    ));
    let got = curves(&r)[0];
    assert!(matches!(got.curve, Curve3::Ellipse { .. }));
    let user = path("M0 0 A10 10 0 0 1 10 10");
    let m = parse_transform("matrix(1 0.5 -0.3 2 10 20) skewX(10)").unwrap();
    for f in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let p = m.apply_point(at(&user.curves[0], f));
        near(at(got, f), (p.x(), 100.0 - p.y()));
    }
    assert!(parse_transform("rotate(10").is_none());
    assert!(parse_transform("frob(1)").is_none());
    let rc = parse_transform("rotate(90 10 0)").unwrap();
    let p = rc.apply_point(Point3::new(10.0, 0.0, 0.0));
    near(p, (10.0, 0.0));
    let p = rc.apply_point(Point3::ORIGIN);
    near(p, (10.0, -10.0));
}

#[test]
fn document_units() {
    let line = "<line x1=\"0\" y1=\"0\" x2=\"200\" y2=\"0\"/>";
    // mm document with a viewBox: 0.5 mm per user unit.
    let r = read(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100mm\" height=\"50mm\" \
         viewBox=\"0 0 200 100\">{line}</svg>"
    ));
    assert_eq!(r.report.unit, Some(Unit::Mm));
    near(at(curves(&r)[0], 0.0), (0.0, 50.0));
    near(at(curves(&r)[0], 1.0), (100.0, 50.0));
    // Inch document without a viewBox: user units are px.
    let r = read(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4in\" height=\"2in\">\
         <line x1=\"0\" y1=\"0\" x2=\"96\" y2=\"0\"/></svg>",
    );
    assert_eq!(r.report.unit, Some(Unit::In));
    near(at(curves(&r)[0], 1.0), (25.4, 50.8));
    // Plain px document: flipped about y = 0.
    let r = read(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><line x1=\"0\" y1=\"96\" x2=\"96\" y2=\"96\"/></svg>",
    );
    assert_eq!(r.report.unit, Some(Unit::Px));
    near(at(curves(&r)[0], 1.0), (25.4, -25.4));
    // Aspect ratio: meet centres, none stretches.
    let sq = "<line x1=\"0\" y1=\"0\" x2=\"100\" y2=\"0\"/>";
    let svg = |par: &str| {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200mm\" height=\"100mm\" \
             viewBox=\"0 0 100 100\" preserveAspectRatio=\"{par}\">{sq}</svg>"
        )
    };
    let r = read(&svg("xMidYMid meet"));
    near(at(curves(&r)[0], 0.0), (50.0, 100.0));
    near(at(curves(&r)[0], 1.0), (150.0, 100.0));
    let r = read(&svg("xMaxYMax"));
    near(at(curves(&r)[0], 0.0), (100.0, 100.0));
    let r = read(&svg("none"));
    near(at(curves(&r)[0], 1.0), (200.0, 100.0));
    let r = read(&svg("xMinYMin slice"));
    near(at(curves(&r)[0], 1.0), (200.0, 100.0));
    // A project unit overrides the document: 1 user unit = 2 mm, flipped
    // about the viewBox bottom.
    let mut o = opts();
    o.scale = Some(2.0);
    let r = read_svg(svg("none").as_bytes(), &o).unwrap();
    near(at(curves(&r)[0], 1.0), (200.0, 200.0));
    // Lengths with units on attributes.
    let r = read(&doc("<line x1=\"0\" y1=\"0\" x2=\"1in\" y2=\"0\"/>"));
    near(at(curves(&r)[0], 1.0), (96.0, 100.0));
}

#[test]
fn use_and_symbol() {
    let r = read(&doc("<defs><path id=\"p\" d=\"M0 0 L1 0\"/>\
         <symbol id=\"s\"><path d=\"M0 0 L0 1\"/></symbol></defs>\
         <use xlink:href=\"#p\" x=\"10\" y=\"0\"/>\
         <use href=\"#s\" transform=\"translate(20 0)\"/>\
         <use href=\"#missing\"/>"));
    let c = curves(&r);
    assert_eq!(c.len(), 2);
    near(at(c[0], 0.0), (10.0, 100.0));
    near(at(c[1], 1.0), (20.0, 99.0));
    assert!(r.report.warnings.iter().any(|w| w.contains("missing")));
}

#[test]
fn self_reference_and_fan_out() {
    let r = read(&doc(
        "<g id=\"a\"><path d=\"M0 0 L1 0\"/><use href=\"#a\"/></g><use id=\"u\" href=\"#u\"/>",
    ));
    assert_eq!(curves(&r).len(), 1);
    assert_eq!(
        r.report
            .warnings
            .iter()
            .filter(|w| w.contains("itself"))
            .count(),
        2,
        "{:?}",
        r.report.warnings
    );
    // Ten levels of ten uses: 10^10 paths if expanded.
    let mut defs = String::from("<path id=\"l0\" d=\"M0 0 L1 0\"/>");
    for i in 1..=10 {
        defs.push_str(&format!("<g id=\"l{i}\">"));
        for _ in 0..10 {
            defs.push_str(&format!("<use href=\"#l{}\"/>", i - 1));
        }
        defs.push_str("</g>");
    }
    let text = doc(&format!("<defs>{defs}</defs><use href=\"#l10\"/>"));
    let mut o = opts();
    o.limits.max_items = 100_000;
    assert!(
        read_svg(text.as_bytes(), &o)
            .unwrap_err()
            .msg
            .contains("elements")
    );
    let mut o = opts();
    o.limits.max_curves = 1000;
    assert!(
        read_svg(text.as_bytes(), &o)
            .unwrap_err()
            .msg
            .contains("curves")
    );
}

#[test]
fn ignored_and_unsupported_elements() {
    let r = read(&doc(
        "<text>hi</text><image href=\"x.png\"/><style>path{}</style><frobnicate/>\
         <title>t</title><foo:bar xmlns:foo=\"urn:x\"/>",
    ));
    assert!(r.wires.is_empty());
    assert_eq!(r.report.ignored.get("text"), Some(&1));
    assert_eq!(r.report.ignored.get("image"), Some(&1));
    assert_eq!(r.report.ignored.get("style"), Some(&1));
    assert_eq!(r.report.unsupported.get("frobnicate"), Some(&1));
    assert_eq!(r.report.unsupported.len(), 1);
}

#[test]
fn bad_documents_are_errors() {
    assert!(
        read_svg(b"<html/>", &opts())
            .unwrap_err()
            .msg
            .contains("not an SVG")
    );
    assert!(
        read_svg(b"\xff\xfe<", &opts())
            .unwrap_err()
            .msg
            .contains("UTF-8")
    );
    let e = read_svg(
        b"<svg xmlns=\"http://www.w3.org/2000/svg\">\n<g>\n</svg>",
        &opts(),
    )
    .unwrap_err();
    assert_eq!(e.line, Some(3));
    let mut o = opts();
    o.limits.max_bytes = 5;
    assert!(read_svg(doc("").as_bytes(), &o).is_err());
    o = opts();
    o.limits.max_items = 3;
    assert!(read_svg(doc("<g/><g/><g/><g/>").as_bytes(), &o).is_err());
    // A BOM is fine; NaN and huge numbers are not numbers.
    let r = read(&format!(
        "\u{feff}{}",
        doc("<line x2=\"nan\" y2=\"1e999\"/>")
    ));
    assert!(r.wires.is_empty());
}

#[test]
fn path_segment_limit() {
    let mut o = opts();
    o.limits.max_points = 3;
    let r = read_svg(doc("<path d=\"M0 0 L1 0 L2 0 L3 0 L4 0\"/>").as_bytes(), &o);
    assert!(r.is_err());
}

#[test]
fn doctypes_are_read_and_entity_bombs_stopped() {
    // Illustrator writes a DOCTYPE with an external DTD that is never fetched.
    let t = "<?xml version=\"1.0\"?><!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \
             \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\
             <svg xmlns=\"http://www.w3.org/2000/svg\"><line x2=\"1\"/></svg>";
    assert_eq!(read_svg(t.as_bytes(), &opts()).unwrap().wires.len(), 1);
    let mut bomb = String::from("<?xml version=\"1.0\"?><!DOCTYPE svg [<!ENTITY e0 \"lol\">");
    for i in 1..10 {
        let refs = format!("&e{};", i - 1).repeat(10);
        bomb.push_str(&format!("<!ENTITY e{i} \"{refs}\">"));
    }
    bomb.push_str("]><svg xmlns=\"http://www.w3.org/2000/svg\"><text>&e9;</text></svg>");
    assert!(read_svg(bomb.as_bytes(), &opts()).is_err());
    let external = "<?xml version=\"1.0\"?><!DOCTYPE svg [<!ENTITY x SYSTEM \"file:///etc/passwd\">]>\
                    <svg xmlns=\"http://www.w3.org/2000/svg\"><text>&x;</text></svg>";
    assert!(read_svg(external.as_bytes(), &opts()).is_err());
}
