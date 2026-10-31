// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spike S2 cost breakdown: time per primitive solve by kind, and the cost
//! of visiting every candidate leaf without pruning, for a 6 mm ball on the
//! synthetic 5 000-face models.
//! `cargo run -p fluut-cam --example s2_cost --release`.

use fluut_cam::cutter::drop::{bound, solve_prim};
use fluut_cam::cutter::geom::Prim;
use fluut_cam::cutter::solve::{Axis, SolveStats};
use fluut_cam::cutter::synth::{Lcg, tiled};
use fluut_cam::cutter::{BuildOptions, Cutter, QueryGeometry, Seeding};
use std::time::Instant;
fn main() {
    for free in [false, true] {
        let body = tiled(25, free);
        let g = QueryGeometry::new(&body, &BuildOptions::default());
        let c = Cutter::ball(6.0).unwrap();
        let mut rng = Lcg(3);
        let mut t_kind = [0.0f64; 5];
        let mut n_kind = [0usize; 5];
        let mut t_trav = 0.0;
        for _ in 0..3000 {
            let (x, y) = (250.0 * rng.uniform(), 250.0 * rng.uniform());
            let ax = Axis { x, y };
            let t = Instant::now();
            let mut leaves = Vec::new();
            g.bvh.best_first(
                f64::NEG_INFINITY,
                |b| bound(b, ax, &c),
                |i, _| {
                    leaves.push(i);
                    None
                },
            );
            t_trav += t.elapsed().as_secs_f64();
            for i in leaves {
                let k = match g.prims[i as usize] {
                    Prim::Vertex(_) => 0,
                    Prim::Edge(e) => {
                        match body.curves[body.edges[e as usize].curve.unwrap() as usize] {
                            fluut_geom::Curve3::Line { .. } => 1,
                            _ => 4,
                        }
                    }
                    Prim::Face { face, .. } => {
                        if matches!(
                            body.surfaces[body.faces[face as usize].surface as usize],
                            fluut_geom::Surface::BSpline(_)
                        ) {
                            3
                        } else {
                            2
                        }
                    }
                };
                let mut st = SolveStats::default();
                let t = Instant::now();
                let _ = solve_prim(&g, i, ax, &c, &Seeding::default(), &mut st);
                t_kind[k] += t.elapsed().as_secs_f64();
                n_kind[k] += 1;
            }
        }
        println!(
            "{}: visiting every candidate leaf {:.1} µs/query",
            body.name,
            t_trav / 3000.0 * 1e6
        );
        for (k, name) in [
            "vertex",
            "line edge",
            "analytic face",
            "B-spline face patch",
            "curved edge",
        ]
        .iter()
        .enumerate()
        {
            println!(
                "  {name}: {} solves, {:.2} µs each",
                n_kind[k],
                t_kind[k] / n_kind[k].max(1) as f64 * 1e6
            );
        }
    }
}
