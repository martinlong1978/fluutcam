// SPDX-License-Identifier: MIT OR Apache-2.0

//! The frame of a `three_axis` Setup (spec 03 §3.9.3): orientation, stock,
//! work zero, clearance, and the bodies moved into the work frame W.
//!
//! `p_W = R_align · p_M − z`, with `R_align` from the tool axis and X axis
//! picks and `z` the work zero in the aligned frame.

use fluut_cam::geometry::{SetupBody, SetupGeometry};
use fluut_geom::{Aabb3, Affine, Point3, Vec3};
use fluut_model::Diagnostic;
use fluut_model::project::{
    ClearanceSpec, Corner, Orientation, Setup, SetupKind, StockSpec, ZeroSpec,
};
use fluut_model::resolve::HeightEnv;
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

fn rotation(o: Orientation) -> Result<DMat3, Diagnostic> {
    let z = DVec3::from(o.tool_axis.vector());
    let x = DVec3::from(o.x_axis.vector());
    if z.dot(x) != 0.0 {
        return Err(Diagnostic::error(
            "setup.orientation.invalid",
            format!(
                "the X axis {} is not perpendicular to the tool axis {}",
                o.x_axis.as_str(),
                o.tool_axis.as_str()
            ),
        ));
    }
    let y = z.cross(x);
    // Rows map model directions to W axes.
    Ok(DMat3::from_cols(x, y, z).transpose())
}

impl SetupFrame {
    /// Builds the frame from the Setup's fields and its bodies (in M).
    pub fn new(s: &Setup, bodies: Vec<SetupBody>) -> Result<SetupFrame, Diagnostic> {
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
        let rot = rotation(s.orientation.unwrap_or_default())?;
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
        let StockSpec::Block { margin } = s.stock.clone().unwrap_or_default();
        let (mx, my, mz) = (margin[0].v, margin[1].v, margin[2].v);
        let stock = Aabb3 {
            min: Point3::new(mb.min.x() - mx, mb.min.y() - my, mb.min.z()),
            max: Point3::new(mb.max.x() + mx, mb.max.y() + my, mb.max.z() + mz),
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
        let zero = match s.zero.clone().unwrap_or_default() {
            ZeroSpec::StockTopCorner(c) => corner(c, stock.max.z()),
            ZeroSpec::StockBottomCorner(c) => corner(c, stock.min.z()),
            ZeroSpec::StockTopCentre => Point3::new(
                0.5 * (stock.min.x() + stock.max.x()),
                0.5 * (stock.min.y() + stock.max.y()),
                stock.max.z(),
            ),
            ZeroSpec::Point(p) => align.apply_point(Point3::new(p[0].v, p[1].v, p[2].v)),
        };
        let shift = Vec3::new(-zero.x(), -zero.y(), -zero.z());
        let to_work =
            Affine::from_mat3_translation(rot, Vec3::ZERO).then(&Affine::translation(shift));
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
            .map_or(ClearanceSpec::DEFAULT_OFFSET, |c| c.offset.v);
        Ok(SetupFrame {
            to_work,
            geometry,
            clearance: stock.max.z() + offset,
            stock,
            model,
        })
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
