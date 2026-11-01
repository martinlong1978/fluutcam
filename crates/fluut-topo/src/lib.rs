// SPDX-License-Identifier: MIT OR Apache-2.0

//! B-rep topology, validity checks and the face bounding-volume hierarchy
//! (design D03).

pub mod brep;
pub mod bvh;
pub mod check;
pub mod pcurve;
pub mod wire;

pub use brep::{BrepBody, Coedge, Edge, Face, Loop, Shell, Vertex};
pub use bvh::Bvh;
pub use check::{Issue, check};
pub use wire::{WireBody, WireCurve};
