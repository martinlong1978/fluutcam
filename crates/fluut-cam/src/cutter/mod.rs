// SPDX-License-Identifier: MIT OR Apache-2.0

//! Cutter-contact queries (design D07): tool profiles and the exact drop of a
//! tool along −Z onto a B-rep body.

pub mod drop;
pub mod geom;
pub mod profile;
pub mod reference;
pub mod solve;
pub mod synth;
pub mod trim;

pub use drop::{DropOptions, drop};
pub use geom::{BuildOptions, QueryGeometry};
pub use profile::Cutter;
pub use solve::{Contact, ContactOn, Seeding};

#[cfg(test)]
mod tests;
