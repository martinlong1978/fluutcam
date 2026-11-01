// SPDX-License-Identifier: MIT OR Apache-2.0

//! Localisation for FluutCAM (design D01 §2, D13 §9; spec 08 §8.10a).
//!
//! - [`Catalog`]: Fluent messages with a locale chain that ends in English.
//!   A missing id gives the id back, never a panic. [`Catalog::pseudo`] is the
//!   pseudo-locale for tests.
//! - [`Locale`] and [`num`]: number display with the locale's decimal
//!   separator, and entry that accepts `.` and `,`.
//! - [`units`]: `mm` and `inch`, explicit units in entries (`6mm`, `0.25in`,
//!   `1/4"`), feeds in mm/min or in/min, angles in degrees.
//!
//! The crate knows nothing of the model: the application maps its own unit
//! type onto [`LengthUnit`].

pub mod catalog;
pub mod locale;
pub mod num;
pub mod units;

pub use catalog::{Catalog, english_ids, message_ids};
pub use locale::{Locale, LocaleError};
pub use num::{NumberError, format_number, parse_number, scan_number};
pub use units::{
    Dimension, LengthUnit, Unit, UnitError, display_scale, format_quantity, parse_quantity,
    scan_unit, unit_symbol,
};
