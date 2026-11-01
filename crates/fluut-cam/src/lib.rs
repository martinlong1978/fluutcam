// SPDX-License-Identifier: MIT OR Apache-2.0

//! Toolpath framework: 2-D regions, cutter queries and strategies (design
//! D07–D09).

pub mod chain;
pub mod cutter;
pub mod drive;
pub mod geometry;
pub mod layout;
pub mod link;
pub mod moves;
pub mod path2;
pub mod region;
pub mod scallop;
pub mod spike;
pub mod strategy;
pub mod surface;
pub mod toolpath;

pub use region::{Inside, Loop2, Region2, Seg2, Vertex};
pub use toolpath::{
    CamError, Generated, Role, StrategyInput, ToolpathType, calculate, find, registry,
};
