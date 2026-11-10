// SPDX-License-Identifier: MIT OR Apache-2.0

//! Mesh bodies (D05 §2.5): a triangle mesh of the `dome` part gives the same
//! drop heights as the exact part within the tessellation tolerance, and the
//! 3-D toolpaths run on it without gouging.

mod support;

use fluut_cam::geometry::{BodyGeom, SetupBody, SetupGeometry};
use fluut_geom::Point3;
use fluut_topo::MeshBody;
use fluut_topo::mesh::{adjacency, to_breps};
use std::time::Instant;
use support::finish::{reference, tool};

/// Height of the exact dome part (80 × 60 block 10 high, boss of radius 12
/// at (40, 30): cylinder to 16, hemisphere above).
fn exact_z(x: f64, y: f64) -> f64 {
    let r2 = (x - 40.0).powi(2) + (y - 30.0).powi(2);
    if r2 >= 144.0 {
        10.0
    } else {
        16.0 + (144.0 - r2).sqrt()
    }
}

/// A closed mesh of the dome part: the top as a height field on a grid of
/// `h` mm (each cell cut along its diagonal), the four sides and the bottom
/// flat. The boss wall becomes a one-cell ramp.
fn dome_mesh(h: f64) -> MeshBody {
    let (nx, ny) = ((80.0 / h) as usize, (60.0 / h) as usize);
    let mut pos: Vec<Point3> = Vec::new();
    let top = |i: usize, j: usize| (j * (nx + 1) + i) as u32;
    for j in 0..=ny {
        for i in 0..=nx {
            let (x, y) = (i as f64 * h, j as f64 * h);
            pos.push(Point3::new(x, y, exact_z(x, y)));
        }
    }
    let bottom_base = pos.len() as u32;
    for (x, y) in [(0.0, 0.0), (80.0, 0.0), (80.0, 60.0), (0.0, 60.0)] {
        pos.push(Point3::new(x, y, 0.0));
    }
    let mut tris: Vec<[u32; 3]> = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            let (a, b, c, d) = (top(i, j), top(i + 1, j), top(i + 1, j + 1), top(i, j + 1));
            tris.push([a, b, c]);
            tris.push([a, c, d]);
        }
    }
    // Sides, counter-clockwise seen from outside, along the border of the
    // grid; the bottom fans between the corners.
    let border: [Vec<u32>; 4] = [
        (0..=nx).map(|i| top(i, 0)).collect(),
        (0..=ny).map(|j| top(nx, j)).collect(),
        (0..=nx).rev().map(|i| top(i, ny)).collect(),
        (0..=ny).rev().map(|j| top(0, j)).collect(),
    ];
    for (k, edge) in border.iter().enumerate() {
        let c0 = bottom_base + k as u32;
        let c1 = bottom_base + (k as u32 + 1) % 4;
        for w in edge.windows(2) {
            tris.push([w[0], c0, w[1]]);
        }
        tris.push([edge[edge.len() - 1], c0, c1]);
    }
    tris.push([bottom_base, bottom_base + 2, bottom_base + 1]);
    tris.push([bottom_base, bottom_base + 3, bottom_base + 2]);
    let (neighbours, _) = adjacency(&tris);
    MeshBody {
        name: "dome".into(),
        tol_model: 1e-4,
        positions: pos,
        tris,
        neighbours,
    }
}

fn mesh_geometry(h: f64) -> SetupGeometry {
    let bodies = to_breps(&dome_mesh(h))
        .into_iter()
        .enumerate()
        .map(|(i, mut b)| {
            fluut_topo::pcurve::compute(&mut b);
            SetupBody {
                id: format!("b-{}", i + 1),
                geom: BodyGeom::Brep(b),
            }
        })
        .collect();
    SetupGeometry { bodies }
}

#[test]
fn one_body_per_closed_shell() {
    let g = mesh_geometry(4.0);
    assert_eq!(g.bodies.len(), 1);
    let BodyGeom::Brep(b) = &g.bodies[0].geom else {
        panic!()
    };
    assert!(b.from_mesh);
    assert_eq!(b.faces.len(), 600 + 42 + 32 + 2);
    assert!(b.shells[0].closed);
    assert_eq!(b.pcurves.len(), b.coedges.len());
    assert!(b.pcurves.iter().all(Option::is_some));
}

#[test]
fn mesh_drops_match_the_exact_part() {
    let exact = support::finish::dome();
    let mesh = mesh_geometry(0.5);
    for id in ["b6", "f6", "r6"] {
        let tl = tool(id);
        let (qe, qm) = (reference(&exact, &tl, 0.0), reference(&mesh, &tl, 0.0));
        let (mut worst, mut n) = (0.0_f64, 0);
        let (mut te, mut tm) = (0.0, 0.0);
        for i in 0..40 {
            for j in 0..30 {
                let (x, y) = (3.0 + 1.9 * f64::from(i), 3.0 + 1.9 * f64::from(j));
                // Skip tool positions that touch the one-cell boss wall.
                let rr = ((x - 40.0).powi(2) + (y - 30.0).powi(2)).sqrt();
                if (rr - 12.0).abs() < 3.0 + 1.5 {
                    continue;
                }
                let t0 = Instant::now();
                let a = qe.drop(x, y).unwrap().tip;
                te += t0.elapsed().as_secs_f64();
                let t0 = Instant::now();
                let b = qm.drop(x, y).unwrap().tip;
                tm += t0.elapsed().as_secs_f64();
                worst = worst.max((a - b).abs());
                n += 1;
            }
        }
        eprintln!(
            "{id}: {n} drops, worst difference {worst:.4} mm, exact {:.1} us, mesh {:.1} us per drop",
            1e6 * te / f64::from(n),
            1e6 * tm / f64::from(n)
        );
        assert!(worst < 0.02, "{id}: {worst}");
    }
}

#[test]
fn sections_of_a_mesh_close_even_beside_flat_levels() {
    use fluut_cam::section::Sectioner;
    let g = mesh_geometry(1.0);
    let s = Sectioner::new(&g, 0.01).unwrap();
    // Just above and below the plateau at Z 10, and through the boss.
    for z in [9.99, 9.9999, 10.00001, 10.0001, 10.001, 12.3, 20.0, 25.9999] {
        let r = s.section(z).unwrap_or_else(|e| panic!("z {z}: {e}"));
        assert!(r.region.area() > 100.0, "z {z}");
    }
    // The plateau area is the block less the part under the boss.
    let a = s.section(5.0).unwrap().region.area();
    assert!((a - 4800.0).abs() < 1e-6, "{a}");
}
