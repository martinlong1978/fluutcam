// SPDX-License-Identifier: MIT OR Apache-2.0

//! Exact material on one ray, for error measurements: the same removal as the
//! grid, applied to a single line at any position.

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
        g.rays
            .get(j * g.nu + i)
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
