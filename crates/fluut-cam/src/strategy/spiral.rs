// SPDX-License-Identifier: MIT OR Apache-2.0

//! `spiral` (D09 §4.6, D15b §5.3): an Archimedean spiral from a centre
//! point, clipped to the region, following the surface in Z. Finishing
//! type.
//!
//! The spiral r = s'·φ/2π runs from the centre to r_max, the largest
//! distance from the centre to the region boundary, with s' = r_max /
//! ceil(r_max / s) so the last turn ends at r_max exactly. It is evaluated
//! exactly (the output is lines through CL points, so the biarc chain of
//! D09 §4.6 is not needed) and clipped by the region to [`CUTOFF`].
//! `machine_corners` adds a pass along the region boundary.

use super::contour::link_runs;
use crate::follow::{CUTOFF, Drive, LoopDrive, Run, Sampler, curve_sagitta, setup};
use crate::region::{Inside, Region2};
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType};
use fluut_geom::{Point2, Point3};
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::OffsetOrder;
use rayon::prelude::*;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

#[derive(Debug)]
pub struct Spiral;

impl ToolpathType for Spiral {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Spiral
    }

    fn role(&self) -> Role {
        Role::Finish
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        let mut st = setup(input, "spiral")?;
        let Some(sp) = st.params.spiral().cloned() else {
            return Err(CamError::Unavailable("not a spiral toolpath".into()));
        };
        let stepover = st.stepover(input, sp.scallop_height, sp.stepover)?;
        let region = st.region(input, 0.0)?;
        let tool = input.tool;
        let tol = st.params.passes.machining_tolerance;
        let centre = match sp.centre {
            Some(c) => input.point_xy(c),
            None => centroid(&region),
        };
        if !region.is_empty() && region.contains(centre) == Inside::Out {
            st.diags.push(
                Diagnostic::warning(
                    "cam.spiral.centre_outside",
                    "the centre is outside the region: the inner turns are linked over",
                )
                .at("passes.centre"),
            );
        }
        let h = st.heights;
        let pl = Sampler {
            q: &st.q,
            h: &h,
            min_slope: 0.0,
            max_slope: FRAC_PI_2,
            tol,
            max_step: 0.5 * tool.radius(),
        };
        let mut runs = Vec::new();
        let r_max = far(&region, centre);
        if r_max > 0.0 {
            let turns = (r_max / stepover - 1e-9).ceil().max(1.0);
            let d = SpiralDrive {
                c: centre,
                pitch: r_max / turns,
                sign: if sp.ccw { 1.0 } else { -1.0 },
                end: TAU * turns,
                reverse: sp.direction == OffsetOrder::OutsideIn,
            };
            // Independent stretches: sampled in parallel, joined in order.
            let spans = d.clip(&region, 0.5 * tool.radius());
            let per: Vec<Vec<Vec<Point3>>> = spans
                .par_iter()
                .map(|&(t0, t1)| pl.cut_runs(&d, t0, t1))
                .collect();
            for (k, rs) in per.into_iter().enumerate() {
                for pts in rs {
                    runs.push(Run {
                        pts,
                        pass: k + 1,
                        span: 0,
                        level: false,
                    });
                }
            }
        }
        if sp.machine_corners {
            let base = runs.len() + 1;
            let per: Vec<Vec<Vec<Point3>>> = region
                .loops
                .par_iter()
                .map(|l| {
                    let d = LoopDrive::new(l);
                    pl.cut_runs(&d, 0.0, d.length())
                })
                .collect();
            for (k, rs) in per.into_iter().enumerate() {
                for pts in rs {
                    runs.push(Run {
                        pts,
                        pass: base + k,
                        span: 0,
                        level: false,
                    });
                }
            }
        }
        let diags = std::mem::take(&mut st.diags);
        link_runs(&st, input, &region, &runs, diags, "spiral", None, None)
    }
}

/// The spiral `c + (pitch·φ/2π)(cos φ, sign·sin φ)` for φ in [0, end],
/// walked from the far end when `reverse`.
#[derive(Copy, Clone, Debug)]
struct SpiralDrive {
    c: Point2,
    pitch: f64,
    sign: f64,
    end: f64,
    reverse: bool,
}

impl SpiralDrive {
    fn phi(&self, t: f64) -> f64 {
        if self.reverse { self.end - t } else { t }
    }

    fn radius(&self, t: f64) -> f64 {
        self.pitch * self.phi(t) / TAU
    }

    /// Parameter intervals inside the region, ends bisected to
    /// [`CUTOFF`]; samples at most `step` apart.
    fn clip(&self, region: &Region2, step: f64) -> Vec<(f64, f64)> {
        let inside = |t: f64| region.contains(self.at(t)) != Inside::Out;
        let mut out = Vec::new();
        let mut t = 0.0;
        let mut was = inside(t);
        let mut start = was.then_some(0.0);
        while t < self.end {
            let r = self
                .radius(t)
                .max(self.radius(t + 1e-3))
                .max(self.pitch / TAU);
            let nt = (t + (step / r).min(0.25)).min(self.end);
            let now = inside(nt);
            if now != was {
                let (mut a, mut b) = (t, nt);
                while (self.at(b) - self.at(a)).length() > CUTOFF {
                    let m = 0.5 * (a + b);
                    if inside(m) == was {
                        a = m;
                    } else {
                        b = m;
                    }
                }
                if was {
                    if let Some(s) = start.take() {
                        out.push((s, a));
                    }
                } else {
                    start = Some(b);
                }
                was = now;
            }
            t = nt;
        }
        if let Some(s) = start {
            out.push((s, self.end));
        }
        out.retain(|&(a, b)| self.len(a, b) > CUTOFF);
        out
    }
}

impl Drive for SpiralDrive {
    fn at(&self, t: f64) -> Point2 {
        let phi = self.phi(t);
        let r = self.pitch * phi / TAU;
        let (s, c) = libm::sincos(phi);
        Point2::new(self.c.x() + r * c, self.c.y() + self.sign * r * s)
    }

    fn len(&self, ta: f64, tb: f64) -> f64 {
        // Arc length bound: at least the chord, about r·Δφ.
        let r = self.radius(ta).max(self.radius(tb));
        (self.at(tb) - self.at(ta)).length().max(r * (tb - ta))
    }

    fn sagitta(&self, ta: f64, tb: f64) -> f64 {
        curve_sagitta(self, ta, tb)
    }
}

/// Area centroid of a region (arcs by their chords and 8 points each).
fn centroid(r: &Region2) -> Point2 {
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for l in &r.loops {
        let pts: Vec<Point2> = l
            .segs()
            .flat_map(|s| {
                let n = if s.is_line() { 1 } else { 8 };
                (0..n).map(move |i| s.point_at(i as f64 / n as f64))
            })
            .collect();
        for i in 0..pts.len() {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            let w = p.x() * q.y() - q.x() * p.y();
            a += w;
            cx += (p.x() + q.x()) * w;
            cy += (p.y() + q.y()) * w;
        }
    }
    if a.abs() < 1e-300 {
        let (lo, hi) = r.bbox();
        return Point2::new(0.5 * (lo.x() + hi.x()), 0.5 * (lo.y() + hi.y()));
    }
    Point2::new(cx / (3.0 * a), cy / (3.0 * a))
}

/// Largest distance from `c` to the region's boundary (exact for lines
/// and arcs).
fn far(r: &Region2, c: Point2) -> f64 {
    let mut best: f64 = 0.0;
    for s in r.segs() {
        best = best.max((s.a - c).length()).max((s.b - c).length());
        if let Some(g) = s.arc() {
            let away = g.centre - c;
            let ang = if away.length() > 0.0 {
                libm::atan2(away.y(), away.x())
            } else {
                g.start
            };
            for a in [ang, ang + PI] {
                if g.contains_angle(a) {
                    best = best.max((g.point(a) - c).length());
                }
            }
        }
    }
    best
}
