// SPDX-License-Identifier: MIT OR Apache-2.0

//! The frame of a `three_axis` Setup (spec 03 §3.9.3): scale, orientation,
//! stock, work zero, clearance, and the bodies moved into the work frame W.
//!
//! `p_W = R_align · S · p_M − z`, with `S` the scale and mirror, `R_align`
//! from the tool axis and X axis picks and `z` the work zero in the
//! aligned frame. The frame is built from the resolved Setup, so inherited
//! fields count (D06 §2).

use fluut_cam::RotaryInfo;
use fluut_cam::geometry::{EdgeRef, SetupBody, SetupGeometry};
use fluut_cam::stock::LayeredStock;
use fluut_geom::{Aabb3, Affine, Curve3, Point3, Surface, Vec3};
use fluut_model::Diagnostic;
use fluut_model::project::{AxisPick, Corner, Orientation, SetupKind, StockSpec, ZeroSpec};
use fluut_model::resolve::HeightEnv;
use fluut_model::setup::ResolvedSetup;
use glam::{DMat3, DVec3};

/// Chord for bounds from edge samples, mm.
const BOUNDS_CHORD: f64 = 1e-3;

#[derive(Debug, Clone)]
pub struct SetupFrame {
    /// Model frame M → work frame W.
    pub to_work: Affine,
    pub geometry: SetupGeometry,
    /// Stock box in W.
    pub stock: Aabb3,
    /// Bounds of the Setup's bodies in W.
    pub model: Aabb3,
    /// Z of the Setup clearance plane in W.
    pub clearance: f64,
}

fn dvec(v: Vec3) -> DVec3 {
    DVec3::new(v.x(), v.y(), v.z())
}

fn pick_error(id: &str, what: &str) -> Diagnostic {
    Diagnostic::error(
        "setup.orientation.pick",
        format!("orientation: `{id}` {what}"),
    )
    .at("orientation")
}

/// The model direction of a pick, from the scaled bodies.
fn direction(p: &AxisPick, g: &SetupGeometry) -> Result<DVec3, Diagnostic> {
    match p {
        AxisPick::Axis(a) => Ok(DVec3::from(a.vector())),
        AxisPick::Face(id) => {
            let (b, i) = g
                .face(id)
                .ok_or_else(|| pick_error(id, "is not a face of the Setup's bodies"))?;
            let f = &b.faces[i as usize];
            let sign = if f.same_sense { 1.0 } else { -1.0 };
            match &b.surfaces[f.surface as usize] {
                Surface::Plane { frame } => Ok(dvec(frame.z.get()) * sign),
                Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } => {
                    Ok(dvec(frame.z.get()))
                }
                _ => Err(pick_error(id, "is not planar, cylindrical or conical")),
            }
        }
        AxisPick::Edge(id) => {
            let c = match g
                .edge(id)
                .ok_or_else(|| pick_error(id, "is not an edge of the Setup's bodies"))?
            {
                EdgeRef::Brep(b, i) => b.edges[i as usize]
                    .curve
                    .map(|c| b.curves[c as usize].clone()),
                EdgeRef::Wire(w) => Some(w.curve.clone()),
            };
            match c {
                Some(Curve3::Line { dir, .. }) => Ok(dvec(dir.get())),
                _ => Err(pick_error(id, "is not a straight edge")),
            }
        }
    }
}

fn rotation(
    tool_axis: &AxisPick,
    x_axis: &AxisPick,
    g: &SetupGeometry,
) -> Result<DMat3, Diagnostic> {
    let z = direction(tool_axis, g)?;
    let x0 = direction(x_axis, g)?;
    // Picked directions need not be exactly perpendicular: X is projected
    // into the plane normal to the tool axis. Signed axes are unchanged.
    let d = x0.dot(z);
    let x = if d == 0.0 { x0 } else { x0 - z * d };
    if x.length() < 1e-9 {
        return Err(Diagnostic::error(
            "setup.orientation.invalid",
            "the X axis is parallel to the tool axis",
        )
        .at("orientation"));
    }
    let x = if d == 0.0 { x } else { x.normalize() };
    let y = z.cross(x);
    // Rows map model directions to W axes.
    Ok(DMat3::from_cols(x, y, z).transpose())
}

impl SetupFrame {
    /// Builds the frame from the resolved Setup and its bodies (in M).
    pub fn new(s: &ResolvedSetup, bodies: Vec<SetupBody>) -> Result<SetupFrame, Diagnostic> {
        if let Some(d) = s.diagnostics.iter().find(|d| d.is_error()) {
            return Err(d.clone());
        }
        if s.kind != SetupKind::ThreeAxis {
            return Err(Diagnostic::error(
                "setup.kind.unsupported",
                format!("Setup `{}`: {} Setups are not supported yet", s.id, s.kind),
            ));
        }
        if bodies.is_empty() {
            return Err(Diagnostic::error(
                "setup.geometry.empty",
                format!("Setup `{}` has no bodies to machine", s.id),
            ));
        }
        let mut sc = [1.0; 3];
        if let Some(k) = &s.geometry.scale {
            sc = [k[0].v, k[1].v, k[2].v];
        }
        if let Some(m) = s.geometry.mirror {
            for (a, flip) in sc.iter_mut().zip(m) {
                if flip {
                    *a = -*a;
                }
            }
        }
        if sc.iter().any(|a| !a.is_finite() || *a == 0.0) {
            return Err(Diagnostic::error(
                "setup.geometry.scale",
                format!(
                    "Setup `{}`: every scale factor must be a non-zero number",
                    s.id
                ),
            )
            .at("geometry"));
        }
        // Identity scale skips the products, so unscaled frames stay bit for
        // bit as before (no -0.0 turned into 0.0).
        let scale = (sc != [1.0; 3]).then(|| DMat3::from_diagonal(DVec3::from(sc)));
        let scaled = match scale {
            None => SetupGeometry {
                bodies: bodies.clone(),
            },
            Some(m) => {
                SetupGeometry::transformed(&bodies, &Affine::from_mat3_translation(m, Vec3::ZERO))
                    .map_err(|e| Diagnostic::error("setup.geometry.transform", e))?
            }
        };
        let Orientation::ThreeAxis { tool_axis, x_axis } = &s.orientation else {
            return Err(Diagnostic::error(
                "setup.orientation.kind",
                format!("Setup `{}`: a three_axis Setup needs a tool_axis", s.id),
            ));
        };
        let rot = rotation(tool_axis, x_axis, &scaled)?;
        let rot = scale.map_or(rot, |m| rot * m);
        let align = Affine::from_mat3_translation(rot, Vec3::ZERO);
        let aligned = SetupGeometry::transformed(&bodies, &align)
            .map_err(|e| Diagnostic::error("setup.geometry.transform", e))?;
        let mb = aligned.bounds(BOUNDS_CHORD);
        if mb.is_empty() {
            return Err(Diagnostic::error(
                "setup.geometry.empty",
                format!("Setup `{}`: the bodies have no extent", s.id),
            ));
        }
        let spec = match &s.stock {
            Some(StockSpec::FromPrevious) => s.carried_stock.clone(),
            st => st.clone(),
        };
        let stock = match spec.unwrap_or_default() {
            StockSpec::Block { margin, size: None } => {
                let m = margin.map_or([0.0; 3], |m| [m[0].v, m[1].v, m[2].v]);
                Aabb3 {
                    min: Point3::new(mb.min.x() - m[0], mb.min.y() - m[1], mb.min.z()),
                    max: Point3::new(mb.max.x() + m[0], mb.max.y() + m[1], mb.max.z() + m[2]),
                }
            }
            StockSpec::Block {
                size: Some(size), ..
            } => {
                let (cx, cy) = (
                    0.5 * (mb.min.x() + mb.max.x()),
                    0.5 * (mb.min.y() + mb.max.y()),
                );
                let (hx, hy) = (0.5 * size[0].v, 0.5 * size[1].v);
                Aabb3 {
                    min: Point3::new(cx - hx, cy - hy, mb.min.z()),
                    max: Point3::new(cx + hx, cy + hy, mb.min.z() + size[2].v),
                }
            }
            other => {
                return Err(Diagnostic::error(
                    "setup.stock.unsupported",
                    format!(
                        "Setup `{}`: {} stock is not supported for calculation yet",
                        s.id,
                        other.kind()
                    ),
                )
                .at("stock"));
            }
        };
        let corner = |c: Corner, z: f64| {
            let x = match c {
                Corner::FrontLeft | Corner::BackLeft => stock.min.x(),
                _ => stock.max.x(),
            };
            let y = match c {
                Corner::FrontLeft | Corner::FrontRight => stock.min.y(),
                _ => stock.max.y(),
            };
            Point3::new(x, y, z)
        };
        let zero = match &s.zero {
            ZeroSpec::StockTopCorner(c) => corner(*c, stock.max.z()),
            ZeroSpec::StockBottomCorner(c) => corner(*c, stock.min.z()),
            ZeroSpec::StockTopCentre => Point3::new(
                0.5 * (stock.min.x() + stock.max.x()),
                0.5 * (stock.min.y() + stock.max.y()),
                stock.max.z(),
            ),
            ZeroSpec::Point(p) => align.apply_point(Point3::new(p[0].v, p[1].v, p[2].v)),
            ZeroSpec::OnAxis { .. } => {
                return Err(Diagnostic::error(
                    "setup.zero.kind",
                    format!("Setup `{}`: a three_axis zero needs a `method`", s.id),
                ));
            }
        };
        let shift = Vec3::new(-zero.x(), -zero.y(), -zero.z());
        let to_work = align.then(&Affine::translation(shift));
        let geometry = SetupGeometry::transformed(&bodies, &to_work)
            .map_err(|e| Diagnostic::error("setup.geometry.transform", e))?;
        let model = geometry.bounds(BOUNDS_CHORD);
        let stock = Aabb3 {
            min: stock.min + shift,
            max: stock.max + shift,
        };
        let offset = s
            .clearance
            .as_ref()
            .and_then(|c| c.plane_offset())
            .ok_or_else(|| {
                Diagnostic::error(
                    "setup.clearance.unsupported",
                    format!(
                        "Setup `{}`: a three_axis Setup needs a clearance plane",
                        s.id
                    ),
                )
                .at("clearance")
            })?;
        Ok(SetupFrame {
            to_work,
            geometry,
            clearance: stock.max.z() + offset,
            stock,
            model,
        })
    }

    /// The Setup stock as in-process stock, before any toolpath (D08 §7).
    pub fn initial_stock(&self) -> LayeredStock {
        LayeredStock::planar(&self.stock)
    }

    /// Rotary data for rotary types; `None` in a `three_axis` Setup.
    pub fn rotary_info(&self) -> Option<RotaryInfo> {
        None
    }

    /// Height references of this Setup (selection left to the toolpath).
    pub fn height_env(&self) -> HeightEnv {
        HeightEnv {
            setup_clearance: self.clearance,
            stock_top: self.stock.max.z(),
            stock_bottom: self.stock.min.z(),
            model_top: self.model.max.z(),
            model_bottom: self.model.min.z(),
            selection: None,
        }
    }
}
