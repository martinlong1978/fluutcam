// SPDX-License-Identifier: MIT OR Apache-2.0

//! Toolpath framework: 2-D regions, cutter queries and strategies (design
//! D07–D09).

pub mod chain;
pub mod cutter;
pub mod drive;
pub mod follow;
pub mod geometry;
pub mod holes;
pub mod layout;
pub mod link;
pub mod moves;
pub mod optimised_load;
pub mod path2;
pub mod radial;
pub mod region;
pub mod scallop;
pub mod section;
pub mod spike;
pub mod stock;
pub mod strategy;
pub mod surface;
pub mod toolpath;
pub mod waterline;

pub use region::{Inside, Loop2, Region2, Seg2, Vertex};
pub use toolpath::{
    CamError, Generated, Role, RotaryAxisInfo, RotaryInfo, StrategyInput, ToolpathType, calculate,
    find, registry,
};
