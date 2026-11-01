// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use fluut_topo::mesh::NONE;

type Tri = ([f64; 3], [[f64; 3]; 3]);

/// A binary STL with `header` (padded to 80 bytes).
fn binary(header: &[u8], tris: &[Tri]) -> Vec<u8> {
    let mut b = header.to_vec();
    b.resize(80, b' ');
    b.extend((tris.len() as u32).to_le_bytes());
    for (n, p) in tris {
        for v in std::iter::once(n).chain(p.iter()) {
            for x in v {
                b.extend((*x as f32).to_le_bytes());
            }
        }
        b.extend([0u8, 0]);
    }
    b
}

fn ascii(name: &str, tris: &[Tri]) -> String {
    let mut s = format!("solid {name}\n");
    for (n, p) in tris {
        s.push_str(&format!(
            "  facet normal {} {} {}\n    outer loop\n",
            n[0], n[1], n[2]
        ));
        for v in p {
            s.push_str(&format!("      vertex {} {} {}\n", v[0], v[1], v[2]));
        }
        s.push_str("    endloop\n  endfacet\n");
    }
    s.push_str(&format!("endsolid {name}\n"));
    s
}

/// A tetrahedron with outward normals, corners counter-clockwise from
/// outside.
fn tetra() -> Vec<Tri> {
    let p = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, 0.0, 10.0],
    ];
    let s = 1.0 / 3f64.sqrt();
    vec![
        ([0.0, 0.0, -1.0], [p[0], p[2], p[1]]),
        ([0.0, -1.0, 0.0], [p[0], p[1], p[3]]),
        ([s, s, s], [p[1], p[2], p[3]]),
        ([-1.0, 0.0, 0.0], [p[0], p[3], p[2]]),
    ]
}

/// A unit cube without its top: 10 triangles.
fn open_box() -> Vec<Tri> {
    let v = |x: f64, y: f64, z: f64| [x, y, z];
    let quad = |n: [f64; 3], a, b, c, d| [(n, [a, b, c]), (n, [a, c, d])];
    [
        quad(
            [0.0, 0.0, -1.0],
            v(0., 0., 0.),
            v(0., 1., 0.),
            v(1., 1., 0.),
            v(1., 0., 0.),
        ),
        quad(
            [0.0, -1.0, 0.0],
            v(0., 0., 0.),
            v(1., 0., 0.),
            v(1., 0., 1.),
            v(0., 0., 1.),
        ),
        quad(
            [1.0, 0.0, 0.0],
            v(1., 0., 0.),
            v(1., 1., 0.),
            v(1., 1., 1.),
            v(1., 0., 1.),
        ),
        quad(
            [0.0, 1.0, 0.0],
            v(1., 1., 0.),
            v(0., 1., 0.),
            v(0., 1., 1.),
            v(1., 1., 1.),
        ),
        quad(
            [-1.0, 0.0, 0.0],
            v(0., 1., 0.),
            v(0., 0., 0.),
            v(0., 0., 1.),
            v(0., 1., 1.),
        ),
    ]
    .concat()
}

fn opts() -> Options {
    Options {
        scale: Some(1.0),
        tol_model: 1e-3,
        limits: Limits::default(),
    }
}

#[test]
fn binary_with_solid_header() {
    let b = binary(b"solid looks like ascii", &tetra());
    let m = read_stl(&b, &opts()).unwrap();
    assert!(m.report.binary);
    assert_eq!(m.body.name, "stl");
    assert_eq!(m.body.positions.len(), 4);
    assert_eq!(m.body.tris.len(), 4);
    assert!(m.report.is_closed(), "{:?}", m.report);
    assert!(m.body.neighbours.iter().flatten().all(|&t| t != NONE));
    assert_eq!((m.report.normals_agree, m.report.normals_disagree), (4, 0));
}

#[test]
fn ascii_with_mixed_case_and_spacing() {
    let text = ascii("part one", &tetra())
        .replace("facet normal", "FACET\tNormal ")
        .replace("outer loop", "Outer   LOOP")
        .replace("endloop", "ENDLOOP")
        .replace('\n', "\r\n");
    let m = read_stl(text.as_bytes(), &opts()).unwrap();
    assert!(!m.report.binary);
    assert_eq!(m.report.solids, ["part one"]);
    assert_eq!(m.body.name, "part one");
    assert!(m.report.is_closed());
    // Two solids in one file make one body.
    let two = format!("{}{}", ascii("a", &tetra()[..2]), ascii("b", &tetra()[2..]));
    let m = read_stl(two.as_bytes(), &opts()).unwrap();
    assert_eq!(m.report.solids, ["a", "b"]);
    assert!(m.report.is_closed());
}

#[test]
fn welding_at_tol_model() {
    let tol = opts().tol_model;
    let shift = |t: &[Tri], d: f64| -> Vec<Tri> {
        // Move the second copy of each shared corner by `d`.
        let mut t = t.to_vec();
        for v in t[1].1.iter_mut() {
            v[0] += d;
        }
        t
    };
    let near = shift(&tetra(), 0.4 * tol);
    let m = read_stl(&binary(b"", &near), &opts()).unwrap();
    assert_eq!(m.body.positions.len(), 4);
    assert!(m.report.is_closed());
    // The first occurrence is kept.
    assert_eq!(m.body.positions[2], Point3::new(10.0, 0.0, 0.0));
    let far = shift(&tetra(), 3.0 * tol);
    let m = read_stl(&binary(b"", &far), &opts()).unwrap();
    assert_eq!(m.body.positions.len(), 7);
    assert_eq!(m.report.open_edges, 6);
    assert_eq!(m.report.boundary_loops, 2);
}

#[test]
fn open_box_and_fin() {
    let m = read_stl(&binary(b"", &open_box()), &opts()).unwrap();
    assert_eq!(m.body.tris.len(), 10);
    assert_eq!((m.report.open_edges, m.report.boundary_loops), (4, 1));
    assert_eq!(m.report.non_manifold_edges, 0);
    let mut fin = tetra();
    fin.push((
        [0.0, 0.0, 1.0],
        [[10.0, 0.0, 0.0], [0.0, 0.0, 0.0], [5.0, -5.0, 0.0]],
    ));
    let m = read_stl(&binary(b"", &fin), &opts()).unwrap();
    assert_eq!(m.report.non_manifold_edges, 1);
    assert!(!m.report.is_closed());
}

#[test]
fn flipped_files_are_turned_back() {
    let mut t = tetra();
    for (_, p) in &mut t {
        p.swap(1, 2);
    }
    let m = read_stl(&binary(b"", &t), &opts()).unwrap();
    assert!(m.report.flipped);
    assert_eq!((m.report.normals_agree, m.report.normals_disagree), (0, 4));
    let reference = read_stl(&binary(b"", &tetra()), &opts()).unwrap();
    let corners = |m: &ImportMesh| -> Vec<[Point3; 3]> {
        m.body
            .tris
            .iter()
            .map(|t| t.map(|i| m.body.positions[i as usize]))
            .collect()
    };
    assert_eq!(corners(&m), corners(&reference));
    assert!(m.report.is_closed());
    // Zero normals do not vote.
    let zero: Vec<Tri> = tetra().into_iter().map(|(_, p)| ([0.0; 3], p)).collect();
    let m = read_stl(&binary(b"", &zero), &opts()).unwrap();
    assert!(!m.report.flipped);
}

#[test]
fn degenerate_triangles_are_dropped() {
    let mut t = tetra();
    t.push(([0.0; 3], [[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]));
    t.push(([0.0; 3], [[0.0; 3], [0.0004, 0.0, 0.0], [5.0, 5.0, 5.0]]));
    t.push((
        [0.0; 3],
        [[f64::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [5.0, 5.0, 5.0]],
    ));
    t.push((
        [0.0; 3],
        [[2e9, 0.0, 0.0], [1.0, 0.0, 0.0], [5.0, 5.0, 5.0]],
    ));
    let m = read_stl(&binary(b"", &t), &opts()).unwrap();
    assert_eq!(m.report.triangles_read, 8);
    assert_eq!(m.report.dropped, 4);
    assert_eq!(m.body.tris.len(), 4);
    assert!(m.report.is_closed());
    // NaN in ASCII parses and is dropped the same way.
    let text = ascii("x", &tetra()).replacen("vertex 0 0 0", "vertex nan 0 0", 1);
    let m = read_stl(text.as_bytes(), &opts()).unwrap();
    assert_eq!(m.report.dropped, 1);
}

#[test]
fn scale_and_many_coincident_points() {
    let mut o = opts();
    o.scale = Some(25.4);
    let m = read_stl(&binary(b"", &tetra()), &o).unwrap();
    assert!(
        m.body
            .positions
            .iter()
            .any(|p| *p == Point3::new(254.0, 0.0, 0.0))
    );
    // A fan of 20 000 triangles around one vertex welds without a quadratic
    // search.
    let fan: Vec<Tri> = (0..20_000)
        .map(|i| {
            let a = i as f64 * 1e-3;
            (
                [0.0, 0.0, 1.0],
                [[0.0; 3], [a, 1.0, 0.0], [a + 1e-3, 1.0, 0.0]],
            )
        })
        .collect();
    let m = read_stl(&binary(b"", &fan), &o).unwrap();
    assert_eq!(m.body.positions.len(), 20_002);
}

#[test]
fn malformed_files_are_errors() {
    let o = opts();
    assert!(read_stl(b"", &o).is_err());
    assert!(
        read_stl(b"hello world", &o)
            .unwrap_err()
            .msg
            .contains("not an STL")
    );
    // A huge count in a short file is not binary, and not ASCII either.
    let mut b = binary(b"", &tetra());
    b[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(read_stl(&b, &o).is_err());
    // Truncated binary: one byte short.
    let b = binary(b"", &tetra());
    assert!(read_stl(&b[..b.len() - 1], &o).is_err());
    // Truncated ASCII.
    let text = ascii("x", &tetra());
    let cut = &text[..text.find("endloop").unwrap()];
    let e = read_stl(cut.as_bytes(), &o).unwrap_err();
    assert!(e.line.is_some(), "{e}");
    let no_end = text.replace("endsolid x\n", "");
    let m = read_stl(no_end.as_bytes(), &o).unwrap();
    assert!(m.report.warnings[0].contains("endsolid"));
    let bad = text.replacen("vertex 0 0 0", "vertex zero 0 0", 1);
    assert_eq!(read_stl(bad.as_bytes(), &o).unwrap_err().line, Some(4));
    let quad = text.replacen("endloop", "vertex 1 1 1\nendloop", 1);
    assert!(read_stl(quad.as_bytes(), &o).is_err());
    let mut o2 = o;
    o2.scale = Some(-1.0);
    assert!(read_stl(text.as_bytes(), &o2).is_err());
}

#[test]
fn limits() {
    let mut o = opts();
    o.limits.max_triangles = 3;
    assert!(
        read_stl(&binary(b"", &tetra()), &o)
            .unwrap_err()
            .msg
            .contains("limit")
    );
    assert!(read_stl(ascii("x", &tetra()).as_bytes(), &o).is_err());
    let mut o = opts();
    o.limits.max_token = 8;
    let long = format!("solid x\nfacet normal 0 0 {}\n", "1".repeat(9));
    assert!(
        read_stl(long.as_bytes(), &o)
            .unwrap_err()
            .msg
            .contains("token")
    );
    let mut o = opts();
    o.limits.max_bytes = 100;
    assert!(read_stl(&binary(b"", &tetra()), &o).is_err());
}
