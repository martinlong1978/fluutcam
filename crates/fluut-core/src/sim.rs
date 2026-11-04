// SPDX-License-Identifier: MIT OR Apache-2.0

//! Simulation of a Setup (D11): what `fluut-sim` needs, taken from the
//! project, the tools, the machine and the Setup frame.
//!
//! - **Stock:** the Setup frame's block, in the work frame W.
//! - **Tools:** flat, ball and bull as they are; V, taper and drill tools as
//!   cones (a taper's corner radius is left out). Flute length, a shank
//!   wider than the cutter and the holder become the probes of D11 §5; the
//!   holder starts at the tool's overall length.
//! - **Part:** the exact top of the Setup's bodies by a drop query (D07) of
//!   a 1 µm flat cutter; the distance to the part, for the rest colours, by
//!   a ball-nosed drop query of the tolerance as radius.
//! - **Machine:** cutting and rapid feed limits; machine definitions have no
//!   acceleration yet, so none is used.

use fluut_cam::cutter::solve::SolveStats;
use fluut_cam::cutter::{BuildOptions, Cutter, DropOptions, QueryGeometry, Seeding, drop};
use fluut_cam::geometry::{BodyGeom, SetupGeometry};
use fluut_model::Diagnostic;
use fluut_model::machine::Machine;
use fluut_model::resolve::resolve;
use fluut_model::tools::{Tool as ModelTool, ToolShape};
use fluut_sim::{Assembly, Check, Clearance, Feeds, Kinematics, Part, Simulation, Tool};
use std::f64::consts::FRAC_PI_2;
use std::sync::Arc;

/// How fine a simulation is.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct SimSettings {
    /// Error target between rays, mm (D11 §2); the ray spacing is twice it.
    pub error: f64,
    /// Memory budget of the stock, bytes.
    pub budget: usize,
}

impl Default for SimSettings {
    /// 0.1 mm error, 0.2 mm rays: the interactive default of S5.
    fn default() -> Self {
        SimSettings {
            error: 0.1,
            budget: fluut_sim::run::DEFAULT_BUDGET,
        }
    }
}

impl SimSettings {
    /// Part of the simulation's cache key.
    pub fn text(&self) -> String {
        format!("error = {:?}\nbudget = {}\n", self.error, self.budget)
    }
}

/// A finished simulation of one Setup.
#[derive(Debug)]
pub struct SimResult {
    pub setup: String,
    pub sim: Simulation,
    /// The findings as diagnostics on their toolpaths.
    pub diagnostics: Vec<Diagnostic>,
    /// Gouge tolerance used, mm.
    pub tol: f64,
}

/// The simulated shape of a tool, or `None` when it has no usable size.
pub fn assembly(t: &ModelTool) -> Option<Assembly> {
    let r = t.radius();
    if !(r > 0.0 && r.is_finite()) {
        return None;
    }
    let tool = match t.shape {
        ToolShape::Flat => Tool::flat(r),
        ToolShape::Ball => Tool::ball(r),
        ToolShape::Bull => Tool::bull(r, t.corner_radius().clamp(0.0, r)),
        ToolShape::V | ToolShape::Taper => match t.v_angle() {
            Some(a) => Tool::cone(r, t.tip_radius().min(0.999 * r), a),
            None => Tool::flat(r),
        },
        ToolShape::Drill => {
            let half = (t.point_angle() / 2.0).to_radians();
            if half > 0.0 && half < FRAC_PI_2 {
                Tool::cone(r, 0.0, half)
            } else {
                Tool::flat(r)
            }
        }
    };
    let holder = match (&t.holder, t.overall_length) {
        (Some(h), Some(l)) if h.diameter > 0.0 && h.length > 0.0 => {
            Some((0.5 * h.diameter, l, l + h.length))
        }
        _ => None,
    };
    Some(Assembly {
        tool,
        flute_length: t.flute_length.filter(|f| *f > 0.0),
        shank: t.shank_diameter.map(|d| 0.5 * d).filter(|s| *s > r),
        holder,
    })
}

/// Feeds of a toolpath in mm/min, as the post resolves them.
pub fn feeds(params: &fluut_model::Params, tool: &ModelTool, material: Option<&str>) -> Feeds {
    let f = resolve(params, tool, material).tool.feed;
    Feeds {
        cut: f.cut.unwrap_or(0.0),
        plunge: f.plunge.unwrap_or(0.0),
        ramp: f.ramp.unwrap_or(0.0),
        lead_in: f.lead_in.unwrap_or(0.0),
        lead_out: f.lead_out.unwrap_or(0.0),
        transition: f.transition.unwrap_or(0.0),
    }
}

/// What the time estimate takes from a machine definition.
pub fn kinematics(m: Option<&Machine>) -> Kinematics {
    let mut k = Kinematics::default();
    if let Some(m) = m {
        if let Some(f) = m.fields.feed.max_cut.filter(|f| *f > 0.0) {
            k.max_feed = [f; 3];
        }
        if let Some(f) = m.fields.feed.max_rapid.filter(|f| *f > 0.0) {
            k.rapid = [f; 3];
        }
    }
    k
}

/// The part's top by drop queries on its bodies.
#[derive(Debug)]
pub struct DropPart {
    bodies: Arc<[QueryGeometry<'static>]>,
    cutter: Cutter,
}

impl DropPart {
    /// Keeps its own copy of the Setup's bodies, so that the simulation can
    /// keep the part's distance query after the geometry is gone.
    pub fn new(g: &SetupGeometry) -> DropPart {
        let opts = BuildOptions::default();
        DropPart {
            bodies: g
                .bodies
                .iter()
                .filter_map(|b| match &b.geom {
                    BodyGeom::Brep(body) => Some(QueryGeometry::new(body, &opts).into_owned()),
                    BodyGeom::Wire(_) => None,
                })
                .collect(),
            cutter: Cutter::flat(1e-3).expect("a valid cutter"),
        }
    }
}

impl Part for DropPart {
    fn top(&self, x: f64, y: f64) -> Option<f64> {
        let mut st = SolveStats::default();
        self.bodies
            .iter()
            .filter_map(|q| drop(q, &self.cutter, x, y, &DropOptions::default(), &mut st))
            .map(|c| c.tau)
            .reduce(f64::max)
    }

    fn clearance(&self) -> Option<Arc<dyn Clearance>> {
        (!self.bodies.is_empty()).then(|| {
            Arc::new(BallClearance {
                bodies: self.bodies.clone(),
            }) as _
        })
    }
}

/// The part's distance to a point by a ball-nosed drop query on its exact
/// faces (D07): a ball of radius `r` rests on the part at the highest
/// position its centre can have over `(x, y)`, and a point is at least `r`
/// from the part exactly when it is at or above that centre. For a part a
/// three-axis tool reaches that is the closest-point distance; stock under
/// an overhang reads as near the part.
#[derive(Debug)]
struct BallClearance {
    bodies: Arc<[QueryGeometry<'static>]>,
}

impl Clearance for BallClearance {
    fn clear_height(&self, x: f64, y: f64, r: f64) -> Option<f64> {
        let ball = Cutter::ball(2.0 * r)?;
        let mut st = SolveStats::default();
        // Edges cover the part's boundary: the rim search is only a net for
        // model gaps, and is the slow part over holes.
        let opts = DropOptions {
            rims: false,
            seeding: Seeding {
                split_edges: true,
                ..Seeding::default()
            },
            ..DropOptions::default()
        };
        self.bodies
            .iter()
            .filter_map(|q| drop(q, &ball, x, y, &opts, &mut st))
            .map(|c| c.tau + r)
            .reduce(f64::max)
    }
}

/// A finding as a diagnostic on its toolpath.
pub fn diagnostic(sim: &Simulation, f: &fluut_sim::Finding) -> Diagnostic {
    let p = f.position;
    let mut msg = format!(
        "segment {} at ({:.3}, {:.3}, {:.3})",
        f.segment, p.x, p.y, p.z
    );
    let code = format!("sim.{}", f.check.id());
    msg = match f.check {
        Check::Gouge => format!("gouges the part by {:.3} mm: {msg}", f.depth),
        Check::RapidInStock => format!("rapid move through stock: {msg}"),
        Check::Holder => format!("holder touches the stock: {msg}"),
        Check::Shank => format!("shank touches the stock: {msg}"),
        Check::FluteLength => format!("cuts above the flute length: {msg}"),
    };
    let d = if f.check.is_error() {
        Diagnostic::error(&code, msg)
    } else {
        Diagnostic::warning(&code, msg)
    };
    match sim.toolpaths.get(f.toolpath) {
        Some(c) => d.on(&c.id),
        None => d,
    }
}
