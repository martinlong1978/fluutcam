// SPDX-License-Identifier: MIT OR Apache-2.0

//! Cutter-contact queries (design D07): tool profiles and the exact drop of a
//! tool along −Z onto a B-rep body.

pub mod geom;
pub mod profile;
pub mod synth;
pub mod trim;

pub use profile::Cutter;
