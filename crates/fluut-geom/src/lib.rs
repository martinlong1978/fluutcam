// SPDX-License-Identifier: MIT OR Apache-2.0

//! Geometry core of FluutCAM (design D02).
//!
//! Lengths are `f64` millimetres, angles are radians. No I/O, no threads. Every
//! geometric comparison takes its tolerance as an argument; there are no global
//! epsilons (D02 §2).
//!
//! Transcendental functions come from the pure-Rust `libm` crate so that results
//! are bit-identical on every platform (D01 §6).

pub mod bounds;
pub mod bspline;
pub mod curve;
pub mod fit;
pub mod frame;
pub mod roots;
pub mod surface;
pub mod tol;
pub mod transform;
pub mod types;

pub use bounds::{Aabb3, Interval};
pub use bspline::{BSplineCurve3, BSplineError};
pub use curve::Curve3;
pub use fit::{FitSeg, biarc_fit, fit_points};
pub use frame::Frame;
pub use surface::{
    BSplineSurface, Curvatures, SectionCurve, SectionPlane, SurfClosest, SurfDerivs, Surface,
    SurfaceKind, SurfaceSection, UvBox, plane_section,
};
pub use tol::Tol;
pub use transform::{Affine, Rigid};
pub use types::{Point2, Point3, UnitVec3, Vec2, Vec3};
