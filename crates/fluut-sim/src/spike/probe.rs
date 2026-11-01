// SPDX-License-Identifier: MIT OR Apache-2.0

//! Exact material on one ray, for error measurements: the same removal as the
//! grid, applied to a single line at any position.

use crate::cyl::CylDexel;
use crate::dexel::{Span, Spans, TriDexel, subtract};
use crate::stock::Stock;
use crate::sweep::Sweep;
use glam::DVec3;

/// Spans along axis `k` of the line through `p`, after all `sweeps`. Offsets
/// are from the stock box minimum, as in the grid.
pub fn probe(stock: &Stock, sweeps: &[Sweep], p: DVec3, k: usize) -> Spans {
    let mut o = p;
    o[k] = stock.bbox().min.0[k];
    let mut spans = Spans::new();
    if let Some((lo, hi)) = stock.chord(o, k) {
        spans.push(Span {
            lo: lo as f32,
            hi: hi as f32,
        });
    }
    let mut e = DVec3::ZERO;
    e[k] = 1.0;
    for s in sweeps {
        if spans.is_empty() {
            break;
        }
        if let Some((t0, t1)) = s.ray(o, e) {
            subtract(&mut spans, t0 as f32, t1 as f32);
        }
    }
    spans
}

/// Spans of the line `o + t·e` after all `sweeps`, starting from the one
/// span `[lo, hi]`.
pub fn probe_line(sweeps: &[Sweep], o: DVec3, e: DVec3, lo: f64, hi: f64) -> Spans {
    let mut spans = Spans::new();
    spans.push(Span {
        lo: lo as f32,
        hi: hi as f32,
    });
    for s in sweeps {
        if spans.is_empty() {
            break;
        }
        if let Some((t0, t1)) = s.ray(o, e) {
            subtract(&mut spans, t0 as f32, t1 as f32);
        }
    }
    spans
}

/// Error statistics of a bilinear surface through the tops of the Z rays,
/// against exact probes at `samples` points `(x, y)`. Points where the four
/// surrounding tops differ by more than the resolution (slope over 45°, the
/// X and Y families' share) are skipped.
#[derive(Copy, Clone, Debug, Default)]
pub struct ErrorStats {
    pub samples: usize,
    pub max: f64,
    pub rms: f64,
    pub p99: f64,
}

pub fn top_error(
    d: &TriDexel,
    stock: &Stock,
    sweeps: &[Sweep],
    samples: &[(f64, f64)],
) -> ErrorStats {
    let g = &d.grids[2];
    let top = |i: usize, j: usize| -> Option<f64> {
        (i < g.nu && j < g.nv)
            .then(|| g.ray(i, j))
            .and_then(|r| r.last())
            .map(|s| f64::from(s.hi))
    };
    let mut errs = Vec::new();
    for &(x, y) in samples {
        let fx = (x - d.origin.x) / d.res - 0.5;
        let fy = (y - d.origin.y) / d.res - 0.5;
        if fx < 0.0 || fy < 0.0 {
            continue;
        }
        let (i, j) = (fx.floor() as usize, fy.floor() as usize);
        if i + 1 >= g.nu || j + 1 >= g.nv {
            continue;
        }
        let (Some(a), Some(b), Some(c), Some(e)) =
            (top(i, j), top(i + 1, j), top(i, j + 1), top(i + 1, j + 1))
        else {
            continue;
        };
        let (hi, lo) = (a.max(b).max(c).max(e), a.min(b).min(c).min(e));
        if hi - lo > d.res {
            continue;
        }
        let (u, v) = (fx - i as f64, fy - j as f64);
        let z = a * (1.0 - u) * (1.0 - v) + b * u * (1.0 - v) + c * (1.0 - u) * v + e * u * v;
        let exact = probe(stock, sweeps, DVec3::new(x, y, 0.0), 2);
        let Some(t) = exact.last() else { continue };
        errs.push((z - f64::from(t.hi)).abs());
    }
    stats(errs)
}

/// As [`top_error`], for the radial rays of a cylindrical stock: a bilinear
/// surface in `(x, θ)` through the outermost span ends, against exact probes
/// along the radial line at each sample `(x, θ)`.
pub fn radial_error(d: &CylDexel, sweeps: &[Sweep], samples: &[(f64, f64)]) -> ErrorStats {
    let top = |i: usize, j: usize| -> Option<f64> {
        d.radial[(j % d.nt) * d.nx + i]
            .last()
            .map(|s| f64::from(s.hi))
    };
    let dt = std::f64::consts::TAU / d.nt as f64;
    let mut errs = Vec::new();
    for &(x, theta) in samples {
        let fx = (x - d.x0) / d.res - 0.5;
        let ft = theta.rem_euclid(std::f64::consts::TAU) / dt;
        if fx < 0.0 {
            continue;
        }
        let (i, j) = (fx.floor() as usize, ft.floor() as usize);
        if i + 1 >= d.nx {
            continue;
        }
        let (Some(a), Some(b), Some(c), Some(e)) =
            (top(i, j), top(i + 1, j), top(i, j + 1), top(i + 1, j + 1))
        else {
            continue;
        };
        let (hi, lo) = (a.max(b).max(c).max(e), a.min(b).min(c).min(e));
        if hi - lo > d.res {
            continue;
        }
        let (u, v) = (fx - i as f64, ft - j as f64);
        let r = a * (1.0 - u) * (1.0 - v) + b * u * (1.0 - v) + c * (1.0 - u) * v + e * u * v;
        let (sn, cs) = libm::sincos(theta);
        let exact = probe_line(
            sweeps,
            DVec3::new(x, 0.0, 0.0),
            DVec3::new(0.0, sn, cs),
            0.0,
            d.radius,
        );
        let Some(t) = exact.last() else { continue };
        errs.push((r - f64::from(t.hi)).abs());
    }
    stats(errs)
}

fn stats(mut errs: Vec<f64>) -> ErrorStats {
    if errs.is_empty() {
        return ErrorStats::default();
    }
    errs.sort_by(f64::total_cmp);
    let n = errs.len();
    ErrorStats {
        samples: n,
        max: errs[n - 1],
        rms: (errs.iter().map(|e| e * e).sum::<f64>() / n as f64).sqrt(),
        p99: errs[(n * 99 / 100).min(n - 1)],
    }
}
