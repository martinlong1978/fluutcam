// SPDX-License-Identifier: MIT OR Apache-2.0

//! Simulation of material removal on a tri-dexel stock (design D11).
//!
//! The stock is three families of rays along X, Y and Z, each ray a sorted list
//! of material spans. Every move cuts the rays it crosses by the exact swept
//! volume of the tool. Results are deterministic on any thread count.
//!
//! [`Simulation`] is the product API: a Setup's stock cut by whole toolpath
//! results, with checkpoints for scrubbing, verification findings and the
//! time estimate.
//!
//! [`CylDexel`] is an alternative layout for rotary Setups, measured against
//! the tri-dexel in the S5 follow-ups: radial rays around the rotary axis plus
//! X rays.

pub mod cyl;
pub mod dexel;
pub mod gjk;
pub mod mesh;
pub mod motion;
pub mod player;
pub mod run;
pub mod sim;
pub mod spike;
pub mod stock;
pub mod sweep;
pub mod time;
pub mod tool;
pub mod verify;

pub use cyl::CylDexel;
pub use dexel::{DexelGrid, Span, TriDexel};
pub use mesh::{MeshTiles, NoPaint, Paint, StockMesh, surface};
pub use motion::Chorder;
pub use player::{Limits, Player, SimPaint};
pub use run::{
    Assembly, Cancelled, Check, Clip, Finding, Monitor, Program, Quiet, Settings, Simulation, Step,
    resolution,
};
pub use sim::{Stats, cut};
pub use stock::Stock;
pub use sweep::Sweep;
pub use time::{Feeds, Kinematics};
pub use tool::{Pose, Tool};
pub use verify::{Part, PartHeights, Rest, index_turn};
