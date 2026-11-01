// SPDX-License-Identifier: MIT OR Apache-2.0

//! Brute-force drop reference (D07 §9): dense samples of every face, edge
//! and vertex near the axis, then local refinement of the best samples by
//! compass search (faces) and Brent's method (edges). Independent of the
//! solvers' contact conditions; slow.

use super::geom::{Prim, QueryGeometry};
use super::profile::Cutter;
use super::solve::{Axis, maximize};
use fluut_geom::Aabb3;

/// Sample counts of the reference.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Density {
    /// Samples per parameter direction of each face.
    pub face: usize,
    /// Samples per edge.
    pub edge: usize,
}

fn near(b: &Aabb3, ax: Axis, r: f64) -> bool {
    let dx = (b.min.x() - ax.x).max(ax.x - b.max.x()).max(0.0);
    let dy = (b.min.y() - ax.y).max(ax.y - b.max.y()).max(0.0);
    libm::hypot(dx, dy) <= r
}

/// The highest `τ` found, or `None` if nothing is under the tool.
pub fn reference(g: &QueryGeometry, c: &Cutter, ax: Axis, n: Density) -> Option<f64> {
    let mut best = f64::NEG_INFINITY;
    let rad = c.radius();
    // Primitives whose boxes reach the footprint (a box query, no bounds).
    let reach = Aabb3::from_points(&[
        fluut_geom::Point3::new(ax.x - rad, ax.y - rad, -1e300),
        fluut_geom::Point3::new(ax.x + rad, ax.y + rad, 1e300),
    ]);
    let mut faces = std::collections::BTreeMap::new();
    let mut edges = std::collections::BTreeSet::new();
    for i in g.bvh.query(&reach) {
        match g.prims[i as usize] {
            Prim::Face { face, .. } if near(g.prim_box(i), ax, rad) => {
                let b = g.prim_box(i);
                let size: &mut f64 = faces.entry(face).or_insert(0.0);
                *size = size.max((b.max - b.min).length());
            }
            Prim::Edge(e) => {
                edges.insert(e);
            }
            _ => {}
        }
    }
    for v in &g.body.vertices {
        if let Some(t) = ax.tau(c, v.p) {
            best = best.max(t);
        }
    }
    for &ei in &edges {
        let e = &g.body.edges[ei as usize];
        let Some(ci) = e.curve else { continue };
        let curve = &g.body.curves[ci as usize];
        let f = |t: f64| ax.tau(c, curve.eval(t));
        let m = n.edge.max(2);
        let ts: Vec<f64> = (0..=m).map(|i| e.range.at(i as f64 / m as f64)).collect();
        let vals: Vec<Option<f64>> = ts.iter().map(|&t| f(t)).collect();
        for k in 0..ts.len() {
            let Some(vk) = vals[k] else { continue };
            best = best.max(vk);
            let lo = ts[k.saturating_sub(1)];
            let hi = ts[(k + 1).min(ts.len() - 1)];
            let ok = |j: usize| vals[j].is_none_or(|x| x <= vk);
            if (k == 0 || ok(k - 1)) && (k + 1 == ts.len() || ok(k + 1)) {
                best = best.max(maximize(f, lo, hi, 1e-13).1);
            }
        }
    }
    for (&fi, size) in &faces {
        let Some(q) = &g.faces[fi as usize] else {
            continue;
        };
        let s = &g.body.surfaces[g.body.faces[fi as usize].surface as usize];
        let uv = q.uv;
        // At least one sample per tenth of the tool radius on large faces.
        let m = ((size / (0.1 * rad)).ceil() as usize).clamp(n.face.max(2), 300);
        let val = |u: f64, v: f64| {
            let t = ax.tau(c, s.eval(u, v))?;
            q.trim.contains(u, v).then_some(t)
        };
        let mut samples: Vec<(f64, f64, f64)> = Vec::new();
        for i in 0..=m {
            for j in 0..=m {
                let (u, v) = (uv.u.at(i as f64 / m as f64), uv.v.at(j as f64 / m as f64));
                if let Some(t) = val(u, v) {
                    samples.push((t, u, v));
                }
            }
        }
        samples.sort_by(|a, b| b.0.total_cmp(&a.0));
        for &(t0, u0, v0) in samples.iter().take(8) {
            best = best.max(t0);
            // Compass search from the sample.
            let (mut u, mut v, mut t) = (u0, v0, t0);
            let (mut hu, mut hv) = (uv.u.length() / m as f64, uv.v.length() / m as f64);
            for _ in 0..300 {
                if hu <= 1e-13 * (1.0 + uv.u.length()) && hv <= 1e-13 * (1.0 + uv.v.length()) {
                    break;
                }
                let mut moved = false;
                for (du, dv) in [
                    (1.0, 0.0),
                    (-1.0, 0.0),
                    (0.0, 1.0),
                    (0.0, -1.0),
                    (1.0, 1.0),
                    (-1.0, -1.0),
                    (1.0, -1.0),
                    (-1.0, 1.0),
                ] {
                    let (mut nu, mut nv) = (u + du * hu, v + dv * hv);
                    let mut trial = val(nu, nv);
                    if trial.is_none() {
                        // Slide up to the footprint or trim boundary: flat
                        // tools touch on the rim.
                        let (mut a, mut b) = (0.0, 1.0);
                        for _ in 0..60 {
                            let m = 0.5 * (a + b);
                            if val(u + du * hu * m, v + dv * hv * m).is_some() {
                                a = m;
                            } else {
                                b = m;
                            }
                        }
                        (nu, nv) = (u + du * hu * a, v + dv * hv * a);
                        trial = if a > 0.0 { val(nu, nv) } else { None };
                    }
                    if let Some(nt) = trial
                        && nt > t
                    {
                        (u, v, t) = (nu, nv, nt);
                        moved = true;
                    }
                }
                if !moved {
                    hu *= 0.5;
                    hv *= 0.5;
                }
            }
            best = best.max(t);
        }
        // The rim d = R: from the best samples, project onto the rim and
        // run a compass search that re-projects after every step.
        let rim = |u: f64, v: f64| -> Option<(f64, f64, f64)> {
            let (mut u, mut v) = (u, v);
            for _ in 0..30 {
                let d = s.derivs(u, v);
                let w = [d.p.x() - ax.x, d.p.y() - ax.y];
                let dist = libm::hypot(w[0], w[1]);
                if dist < 1e-300 {
                    return None;
                }
                let gu = (w[0] * d.su.x() + w[1] * d.su.y()) / dist;
                let gv = (w[0] * d.sv.x() + w[1] * d.sv.y()) / dist;
                let g2 = gu * gu + gv * gv;
                if g2 < 1e-300 {
                    return None;
                }
                let k = (rad - dist) / g2;
                u += k * gu;
                v += k * gv;
                if (rad - dist).abs() < 1e-13 * (1.0 + rad) {
                    break;
                }
            }
            let p = s.eval(u, v);
            let dist = ax.lateral(p);
            ((dist - rad).abs() < 1e-9 && q.trim.contains(u, v)).then(|| (p.z() - c.h(rad), u, v))
        };
        for &(_, u0, v0) in samples.iter().take(8) {
            let Some((mut t, mut u, mut v)) = rim(u0, v0) else {
                continue;
            };
            let (mut hu, mut hv) = (uv.u.length() / m as f64, uv.v.length() / m as f64);
            for _ in 0..300 {
                if hu <= 1e-13 * (1.0 + uv.u.length()) && hv <= 1e-13 * (1.0 + uv.v.length()) {
                    break;
                }
                let mut moved = false;
                for (du, dv) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    if let Some((nt, nu, nv)) = rim(u + du * hu, v + dv * hv)
                        && nt > t
                    {
                        (t, u, v) = (nt, nu, nv);
                        moved = true;
                    }
                }
                if !moved {
                    hu *= 0.5;
                    hv *= 0.5;
                }
            }
            best = best.max(t);
        }
    }
    best.is_finite().then_some(best)
}
