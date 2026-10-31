// SPDX-License-Identifier: MIT OR Apache-2.0

//! Toolpath framework: 2-D regions, cutter queries and strategies (design
//! D07–D09).

pub mod cutter;
pub mod region;
pub mod spike;

pub use region::{Inside, Loop2, Region2, Seg2, Vertex};
