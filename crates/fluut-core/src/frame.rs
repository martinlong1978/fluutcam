// SPDX-License-Identifier: MIT OR Apache-2.0

//! The frame of a Setup (spec 03 §3.9.3): scale, orientation, stock, work
//! zero, clearance, and the bodies moved into the work frame W.
//!
//! `p_W = R_index(θ) · (R_align · S · p_M − z)`, with `S` the scale and
//! mirror, `R_align` from the tool axis and X axis picks (or the rotary
//! axis), `z` the work zero in the aligned frame and `R_index` the turn of
//! an `indexed` Setup about X. The frame is built from the resolved Setup, so inherited
//! fields count (D06 §2).

use fluut_cam::RotaryInfo;
use fluut_cam::geometry::{EdgeRef, SetupBody, SetupGeometry};
use fluut_cam::stock::{LayeredStock, sin_cos_deg};
use fluut_geom::{Aabb3, Affine, Curve3, Point3, Surface, Vec3};
use fluut_model::Diagnostic;
use fluut_model::params::Num;
use fluut_model::project::{
    AxisPick, AxisX, ClearanceSpec, Corner, Orientation, SetupKind, StockSpec, ZeroSpec,
};
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
    /// Z of the Setup clearance plane in W; in a rotary Setup the radius
    /// of the clearance cylinder.
    pub clearance: f64,
    /// Rotary and indexed Setups: the largest distance from the rotary
    /// axis of the stock, the bodies and the fixtures that turn with A
    /// (none are modelled yet). Everything that turns with A lies within
    /// it, at any moment (spec 03 §3.9.5).
    pub turn_radius: Option<f64>,
    /// The turn radius + the clearance offset: the Z in W the tool rises
    /// to before A turns.
    pub turn_clearance: Option<f64>,
    /// Rotary data of a `rotary` Setup.
    pub rotary: Option<RotaryFrame>,
    /// Data of an `indexed` Setup.
    pub index: Option<IndexFrame>,
}

/// An `indexed` Setup (spec 03 §3.9.5): 3-axis machining in W, which is
/// the rotary frame turned by the index angle about X.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndexFrame {
    /// The resolved index angle, degrees.
    pub angle: f64,
    /// A `cylinder` stock: radius and the X of its ends in W.
    pub cylinder: Option<(f64, (f64, f64))>,
}

/// The cylinder data of a `rotary` Setup, in W (X is the rotary axis).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotaryFrame {
    pub stock_radius: f64,
    /// X of the stock ends.
    pub stock_x: (f64, f64),
    /// X extent of the bodies.
    pub model_x: (f64, f64),
    /// Radius range of the bodies' faces (D15b §1.3).
    pub model_r: (f64, f64),
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
        // A body used as stock is the stock, never machined.
        let (bodies, stock_body) = match &s.stock {
            Some(StockSpec::Body { body }) => {
                let (stock, rest): (Vec<_>, Vec<_>) =
                    bodies.into_iter().partition(|b| b.id == *body);
                (rest, stock.into_iter().next())
            }
            _ => (bodies, None),
        };
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
        if s.kind.is_rotary() {
            return rotary_frame(s, &bodies, &scaled, scale);
        }
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
            StockSpec::Block { margin, size } => block(&mb, margin, size),
            StockSpec::Body { body } => {
                let missing = || {
                    Diagnostic::error(
                        "setup.stock.body",
                        format!("Setup `{}`: stock body `{body}` is not in the model", s.id),
                    )
                    .at("stock")
                };
                let sb = stock_body.ok_or_else(missing)?;
                let moved = SetupGeometry::transformed(&[sb], &align)
                    .map_err(|e| Diagnostic::error("setup.geometry.transform", e))?;
                let b = moved.bounds(BOUNDS_CHORD);
                if b.is_empty() {
                    return Err(missing());
                }
                b
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
            turn_radius: None,
            turn_clearance: None,
            rotary: None,
            index: None,
        })
    }

    /// The Setup stock as in-process stock, before any toolpath (D08 §7):
    /// cylindrical layers in a rotary Setup, planar slices of the cylinder
    /// in an indexed Setup with cylinder stock.
    pub fn initial_stock(&self) -> LayeredStock {
        match (&self.rotary, &self.index) {
            (Some(r), _) => LayeredStock::cylindrical(r.stock_radius, r.stock_x.0, r.stock_x.1),
            (
                None,
                Some(IndexFrame {
                    cylinder: Some((r, (x0, x1))),
                    ..
                }),
            ) => LayeredStock::revolved(&[(*x0, *x1, *r)]),
            _ => LayeredStock::planar(&self.stock),
        }
    }

    /// Rotary data for rotary types (the machine axis is left unknown);
    /// `None` in a `three_axis` Setup.
    pub fn rotary_info(&self) -> Option<RotaryInfo> {
        self.rotary.map(|r| RotaryInfo {
            stock_radius: r.stock_radius,
            stock_x: r.stock_x,
            model_x: r.model_x,
            machine_axis: None,
        })
    }

    /// Height references of this Setup (selection left to the toolpath).
    /// In a rotary Setup they are radii (D15b §1.3).
    pub fn height_env(&self) -> HeightEnv {
        if let Some(r) = &self.rotary {
            return HeightEnv {
                setup_clearance: self.clearance,
                stock_top: r.stock_radius,
                stock_bottom: 0.0,
                model_top: r.model_r.1,
                model_bottom: r.model_r.0,
                selection: None,
            };
        }
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

/// The rotary axis line of a pick, in the scaled model frame: a point and
/// a direction (spec 03 §3.9.3).
fn axis_line(p: &AxisPick, g: &SetupGeometry) -> Result<(DVec3, DVec3), Diagnostic> {
    match p {
        AxisPick::Axis(a) => Ok((DVec3::ZERO, DVec3::from(a.vector()))),
        AxisPick::Face(id) => {
            let (b, i) = g
                .face(id)
                .ok_or_else(|| pick_error(id, "is not a face of the Setup's bodies"))?;
            let f = &b.faces[i as usize];
            match &b.surfaces[f.surface as usize] {
                Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } => {
                    Ok((dvec(frame.origin - Point3::ORIGIN), dvec(frame.z.get())))
                }
                _ => Err(pick_error(id, "is not cylindrical or conical")),
            }
        }
        AxisPick::Edge(id) => match g
            .edge(id)
            .ok_or_else(|| pick_error(id, "is not an edge of the Setup's bodies"))?
        {
            EdgeRef::Brep(b, i) => match b.edges[i as usize].curve.map(|c| &b.curves[c as usize]) {
                Some(Curve3::Line { origin, dir }) => {
                    Ok((dvec(*origin - Point3::ORIGIN), dvec(dir.get())))
                }
                _ => Err(pick_error(id, "is not a straight edge")),
            },
            EdgeRef::Wire(w) => match &w.curve {
                Curve3::Line { origin, dir } => {
                    Ok((dvec(*origin - Point3::ORIGIN), dvec(dir.get())))
                }
                _ => Err(pick_error(id, "is not a straight edge")),
            },
        },
    }
}

/// A block stock from the model bounds `mb` in the aligned frame: grown by
/// `margin` (both X sides, both Y sides, the top), or of `size` centred on
/// the model in X and Y with its bottom at the model bottom.
fn block(mb: &Aabb3, margin: Option<[Num; 3]>, size: Option<[Num; 3]>) -> Aabb3 {
    match size {
        None => {
            let m = margin.map_or([0.0; 3], |m| [m[0].v, m[1].v, m[2].v]);
            Aabb3 {
                min: Point3::new(mb.min.x() - m[0], mb.min.y() - m[1], mb.min.z()),
                max: Point3::new(mb.max.x() + m[0], mb.max.y() + m[1], mb.max.z() + m[2]),
            }
        }
        Some(size) => {
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
    }
}

/// The turn of an indexed Setup about X by `deg` degrees (spec 03
/// §3.9.3), exact at multiples of 90°.
fn index_turn(deg: f64) -> Affine {
    let (s, c) = sin_cos_deg(deg);
    let m = DMat3::from_cols(
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(0.0, c, s),
        DVec3::new(0.0, -s, c),
    );
    Affine::from_mat3_translation(m, Vec3::ZERO)
}

/// The box of `b` moved by `t`.
fn turned_box(b: &Aabb3, t: &Affine) -> Aabb3 {
    let mut out = Aabb3::empty();
    for x in [b.min.x(), b.max.x()] {
        for y in [b.min.y(), b.max.y()] {
            for z in [b.min.z(), b.max.z()] {
                out = out.with_point(t.apply_point(Point3::new(x, y, z)));
            }
        }
    }
    out
}

/// The largest distance from the X axis of a point of `b`.
fn box_radius(b: &Aabb3) -> f64 {
    let y = b.min.y().abs().max(b.max.y().abs());
    let z = b.min.z().abs().max(b.max.z().abs());
    (y * y + z * z).sqrt()
}

/// Hook for fixtures that turn with A (a chuck, a tailstock centre, clamps
/// on the rotary table): the largest distance of any of them from the
/// rotary axis, which the turn radius must include. The project model has
/// no fixture model yet, so this is `None`. Fixture bodies imported with
/// the model are Setup bodies and already count in the turn radius.
/// Fixtures fixed to the table outside the rotary do not turn and do not
/// belong here.
fn turning_fixture_radius(_s: &ResolvedSetup) -> Option<f64> {
    None
}

/// The Setup's clearance offset: the plane's, else the default.
fn clearance_offset(s: &ResolvedSetup) -> f64 {
    s.clearance
        .as_ref()
        .and_then(ClearanceSpec::plane_offset)
        .unwrap_or(ClearanceSpec::DEFAULT_OFFSET)
}

/// The frame of a `rotary` or `indexed` Setup (spec 03 §3.9.3): X of W
/// along the rotary axis, +Z toward `a_zero_toward` at A = 0, Y and Z of
/// the zero on the axis; an indexed Setup is then turned by its index
/// angle. A `cylinder` stock is centred on the bodies' X extent.
fn rotary_frame(
    s: &ResolvedSetup,
    bodies: &[SetupBody],
    scaled: &SetupGeometry,
    scale: Option<DMat3>,
) -> Result<SetupFrame, Diagnostic> {
    let Orientation::Rotary {
        rotary_axis,
        a_zero_toward,
    } = &s.orientation
    else {
        return Err(Diagnostic::error(
            "setup.orientation.kind",
            format!("Setup `{}`: a rotary Setup needs a rotary_axis", s.id),
        ));
    };
    let (p0, x) = axis_line(rotary_axis, scaled)?;
    let x = x.normalize();
    let z0 = match a_zero_toward {
        Some(p) => direction(p, scaled)?,
        None => DVec3::Z,
    };
    let z = z0 - x * z0.dot(x);
    if z.length() < 1e-9 {
        return Err(Diagnostic::error(
            "setup.orientation.invalid",
            "the A = 0 direction is parallel to the rotary axis",
        )
        .at("orientation"));
    }
    let z = z.normalize();
    let y = z.cross(x);
    let rot = DMat3::from_cols(x, y, z).transpose();
    let lin = scale.map_or(rot, |m| rot * m);
    // The axis line through the origin of the aligned frame.
    let t = -(rot * p0);
    let align = Affine::from_mat3_translation(lin, Vec3::new(t.x, t.y, t.z));
    let aligned = SetupGeometry::transformed(bodies, &align)
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
    if s.kind == SetupKind::Indexed {
        return indexed_frame(s, bodies, align, &mb, spec);
    }
    let (radius, x0, x1) = match spec {
        Some(StockSpec::Cylinder { radius, length }) => {
            let c = 0.5 * (mb.min.x() + mb.max.x());
            (radius.v, c - 0.5 * length.v, c + 0.5 * length.v)
        }
        other => {
            return Err(Diagnostic::error(
                "setup.stock.unsupported",
                format!(
                    "Setup `{}`: a rotary Setup needs a cylinder stock, not {}",
                    s.id,
                    other.map_or("none", |o| o.kind())
                ),
            )
            .at("stock"));
        }
    };
    let zx = match &s.zero {
        ZeroSpec::OnAxis { x } => match x {
            AxisX::StockLeft => x0,
            AxisX::StockRight => x1,
            AxisX::At(v) => v.v,
        },
        _ => {
            return Err(Diagnostic::error(
                "setup.zero.kind",
                format!("Setup `{}`: a rotary zero lies on the axis", s.id),
            ));
        }
    };
    let to_work = align.then(&Affine::translation(Vec3::new(-zx, 0.0, 0.0)));
    let geometry = SetupGeometry::transformed(bodies, &to_work)
        .map_err(|e| Diagnostic::error("setup.geometry.transform", e))?;
    let model = geometry.bounds(BOUNDS_CHORD);
    let model_r = fluut_cam::radial::radius_range(&geometry, BOUNDS_CHORD).unwrap_or((0.0, radius));
    let clearance = match &s.clearance {
        Some(ClearanceSpec::Cylinder { radius: r }) => r.v,
        Some(c) => radius + c.plane_offset().unwrap_or(ClearanceSpec::DEFAULT_OFFSET),
        None => radius + ClearanceSpec::DEFAULT_OFFSET,
    };
    // A turns on the rapids at the clearance cylinder (spec 03 §3.9.2).
    let turn = radius.max(model_r.1);
    if clearance <= turn {
        return Err(Diagnostic::error(
            "setup.clearance.index_unsafe",
            format!(
                "Setup `{}`: the clearance cylinder (radius {clearance}) must lie outside \
                 the stock and the part (radius {turn})",
                s.id
            ),
        )
        .at("clearance"));
    }
    // Fixtures turning with A sweep the same cylinder between Setups.
    let turn = turning_fixture_radius(s).map_or(turn, |f| turn.max(f));
    let (x0, x1) = (x0 - zx, x1 - zx);
    Ok(SetupFrame {
        to_work,
        geometry,
        stock: Aabb3 {
            min: Point3::new(x0, -radius, -radius),
            max: Point3::new(x1, radius, radius),
        },
        model,
        clearance,
        turn_radius: Some(turn),
        turn_clearance: Some(turn + clearance_offset(s)),
        rotary: Some(RotaryFrame {
            stock_radius: radius,
            stock_x: (x0, x1),
            model_x: (model.min.x(), model.max.x()),
            model_r,
        }),
        index: None,
    })
}

/// The frame of an `indexed` Setup from its rotary alignment `align` and
/// the bodies' bounds `mb` in the aligned frame: 3-axis machining in the
/// aligned frame turned by the index angle. A `block` stock is built at
/// A = 0 and its box in W is the box of the turned block.
fn indexed_frame(
    s: &ResolvedSetup,
    bodies: &[SetupBody],
    align: Affine,
    mb: &Aabb3,
    spec: Option<StockSpec>,
) -> Result<SetupFrame, Diagnostic> {
    let (stock, cylinder) = match spec {
        Some(StockSpec::Block { margin, size }) => (block(mb, margin, size), None),
        Some(StockSpec::Cylinder { radius, length }) => {
            let c = 0.5 * (mb.min.x() + mb.max.x());
            let (r, x0, x1) = (radius.v, c - 0.5 * length.v, c + 0.5 * length.v);
            (
                Aabb3 {
                    min: Point3::new(x0, -r, -r),
                    max: Point3::new(x1, r, r),
                },
                Some(r),
            )
        }
        other => {
            return Err(Diagnostic::error(
                "setup.stock.unsupported",
                format!(
                    "Setup `{}`: {} stock is not supported in an indexed Setup",
                    s.id,
                    other.map_or("no", |o| o.kind())
                ),
            )
            .at("stock"));
        }
    };
    let zx = match &s.zero {
        ZeroSpec::OnAxis { x } => match x {
            AxisX::StockLeft => stock.min.x(),
            AxisX::StockRight => stock.max.x(),
            AxisX::At(v) => v.v,
        },
        _ => {
            return Err(Diagnostic::error(
                "setup.zero.kind",
                format!("Setup `{}`: an indexed zero lies on the axis", s.id),
            ));
        }
    };
    let turn = index_turn(s.index_angle);
    let to_work = align
        .then(&Affine::translation(Vec3::new(-zx, 0.0, 0.0)))
        .then(&turn);
    let geometry = SetupGeometry::transformed(bodies, &to_work)
        .map_err(|e| Diagnostic::error("setup.geometry.transform", e))?;
    let model = geometry.bounds(BOUNDS_CHORD);
    let at_zero = Aabb3 {
        min: Point3::new(stock.min.x() - zx, stock.min.y(), stock.min.z()),
        max: Point3::new(stock.max.x() - zx, stock.max.y(), stock.max.z()),
    };
    // A coaxial cylinder is the same in every turn.
    let stock = if cylinder.is_some() {
        at_zero
    } else {
        turned_box(&at_zero, &turn)
    };
    let clearance = match &s.clearance {
        Some(ClearanceSpec::Cylinder { radius: r }) => r.v,
        Some(c) => stock.max.z() + c.plane_offset().unwrap_or(ClearanceSpec::DEFAULT_OFFSET),
        None => stock.max.z() + ClearanceSpec::DEFAULT_OFFSET,
    };
    // Radii about the axis do not change with the turn: the block at A = 0.
    let turn_radius = cylinder
        .unwrap_or_else(|| box_radius(&at_zero))
        .max(box_radius(mb));
    let turn_radius = turning_fixture_radius(s).map_or(turn_radius, |f| turn_radius.max(f));
    Ok(SetupFrame {
        to_work,
        geometry,
        stock,
        model,
        clearance,
        turn_radius: Some(turn_radius),
        turn_clearance: Some(turn_radius + clearance_offset(s)),
        rotary: None,
        index: Some(IndexFrame {
            angle: s.index_angle,
            cylinder: cylinder.map(|r| (r, (stock.min.x(), stock.max.x()))),
        }),
    })
}
