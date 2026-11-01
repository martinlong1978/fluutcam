// SPDX-License-Identifier: MIT OR Apache-2.0

//! DXF and SVG import into wire bodies (design D05 §1, §3, §4).
//!
//! Input is untrusted (spec 06 §6.3): the readers return errors and never
//! panic, and every size and count is bounded by [`Limits`].

pub mod conic;
pub mod dxf;
pub mod report;
pub mod svg;

pub use dxf::read_dxf;
pub use report::{Import2D, ImportReport, Limits, Options, ReadError, Unit};
pub use svg::read_svg;
