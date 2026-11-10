// SPDX-License-Identifier: MIT OR Apache-2.0

//! Fixtures that turn with A (spec 03 section 3.9.8) reduced to the shapes
//! the turn clearance and the simulation need: solids of revolution about
//! the rotary axis (a chuck, a tailstock) and boxes (clamps), in the work
//! frame W, whose X is the rotary axis.

use fluut_cam::stock::sin_cos_deg;
use fluut_model::project::Fixture;

/// A fixture in W.
#[derive(Debug, Clone, PartialEq)]
pub enum FixtureShape {
    /// A cylinder or cone about the axis between `x.0` and `x.1`, of radius
    /// `r.0` at `x.0` and `r.1` at `x.1`.
    Revolved {
        kind: &'static str,
        x: (f64, f64),
        r: (f64, f64),
    },
    /// A box. `min` and `max` are the box in W (the bounding box of the
    /// box as turned by the index angle); `radius` is the largest distance
    /// of the box from the axis, which a turn does not change.
    Box {
        min: [f64; 3],
        max: [f64; 3],
        radius: f64,
    },
}

impl FixtureShape {
    /// The shape of `f` for a stock that spans `stock_x` in W. A clamp is
    /// given at A = 0; in an indexed Setup it is bounded after the index
    /// turn of `index_deg` degrees about X.
    pub fn of(f: &Fixture, stock_x: (f64, f64), index_deg: f64) -> FixtureShape {
        match f {
            Fixture::Chuck { diameter, length } => {
                let r = 0.5 * diameter.v;
                FixtureShape::Revolved {
                    kind: "chuck",
                    x: (stock_x.0 - length.v, stock_x.0),
                    r: (r, r),
                }
            }
            Fixture::Tailstock {
                diameter,
                length,
                tip_diameter,
            } => {
                let r = 0.5 * diameter.v;
                let tip = tip_diameter.map_or(r, |t| 0.5 * t.v);
                FixtureShape::Revolved {
                    kind: "tailstock",
                    x: (stock_x.1, stock_x.1 + length.v),
                    r: (tip, r),
                }
            }
            Fixture::Clamp { min, max } => {
                let min = [min[0].v, min[1].v, min[2].v];
                let max = [max[0].v, max[1].v, max[2].v];
                let y = min[1].abs().max(max[1].abs());
                let z = min[2].abs().max(max[2].abs());
                let (sn, cs) = sin_cos_deg(index_deg);
                let mut tmin = [min[0], f64::INFINITY, f64::INFINITY];
                let mut tmax = [max[0], f64::NEG_INFINITY, f64::NEG_INFINITY];
                for py in [min[1], max[1]] {
                    for pz in [min[2], max[2]] {
                        let (ty, tz) = (cs * py - sn * pz, sn * py + cs * pz);
                        tmin[1] = tmin[1].min(ty);
                        tmax[1] = tmax[1].max(ty);
                        tmin[2] = tmin[2].min(tz);
                        tmax[2] = tmax[2].max(tz);
                    }
                }
                FixtureShape::Box {
                    min: tmin,
                    max: tmax,
                    radius: libm::hypot(y, z),
                }
            }
        }
    }

    /// The kind of fixture, as in the project file.
    pub fn kind(&self) -> &'static str {
        match self {
            FixtureShape::Revolved { kind, .. } => kind,
            FixtureShape::Box { .. } => "clamp",
        }
    }

    /// The X range the fixture covers.
    pub fn x_range(&self) -> (f64, f64) {
        match self {
            FixtureShape::Revolved { x, .. } => *x,
            FixtureShape::Box { min, max, .. } => (min[0], max[0]),
        }
    }

    /// The largest distance from the axis of the part of the fixture whose
    /// X lies in `lo..=hi`; `None` when it does not reach into that range
    /// (a fixture that only touches it does not count).
    pub fn radius_within(&self, lo: f64, hi: f64) -> Option<f64> {
        let (a, b) = self.x_range();
        let (from, to) = (a.max(lo), b.min(hi));
        if from >= to {
            return None;
        }
        Some(match self {
            FixtureShape::Revolved { x, r, .. } => {
                let at = |p: f64| r.0 + (r.1 - r.0) * (p - x.0) / (x.1 - x.0);
                at(from).max(at(to))
            }
            FixtureShape::Box { radius, .. } => *radius,
        })
    }

    /// Whether a tool whose cutter is a vertical cylinder of radius `tool_r`
    /// with its tip at `tip` (W), pointing up without end, meets the
    /// fixture. Conservative: a box is taken as its bounding box, and the
    /// tool as a cylinder of its cutter radius from the tip up.
    pub fn hit_by(&self, tip: [f64; 3], tool_r: f64) -> bool {
        const EPS: f64 = 1e-9;
        match self {
            FixtureShape::Revolved { x, r, .. } => {
                let (from, to) = ((tip[0] - tool_r).max(x.0), (tip[0] + tool_r).min(x.1));
                if from >= to {
                    return false;
                }
                let at = |p: f64| r.0 + (r.1 - r.0) * (p - x.0) / (x.1 - x.0);
                let fixture_r = at(from).max(at(to));
                // The tool's nearest point to the axis: sideways by the
                // part of its width beyond the axis line, up by its tip's
                // height when that is above the axis.
                let dy = (tip[1].abs() - tool_r).max(0.0);
                let dz = tip[2].max(0.0);
                libm::hypot(dy, dz) < fixture_r - EPS
            }
            FixtureShape::Box { min, max, .. } => {
                tip[0] + tool_r > min[0] + EPS
                    && tip[0] - tool_r < max[0] - EPS
                    && tip[1] + tool_r > min[1] + EPS
                    && tip[1] - tool_r < max[1] - EPS
                    && tip[2] < max[2] - EPS
            }
        }
    }
}

/// The largest distance from the axis of any of `fixtures` within the X
/// range `lo..=hi`.
pub fn radius_within(fixtures: &[FixtureShape], lo: f64, hi: f64) -> Option<f64> {
    fixtures
        .iter()
        .filter_map(|f| f.radius_within(lo, hi))
        .reduce(f64::max)
}
