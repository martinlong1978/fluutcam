// SPDX-License-Identifier: MIT OR Apache-2.0

//! B-rep and mesh topology, validity checks and the face bounding-volume hierarchy
//! (design D03).

pub mod brep;
pub mod bvh;
pub mod check;
pub mod mesh;
pub mod pcurve;
pub mod trim;
pub mod wire;

pub use brep::{BrepBody, Coedge, Edge, Face, Loop, Shell, Vertex};
pub use bvh::Bvh;
pub use check::{Issue, check};
pub use mesh::MeshBody;
pub use wire::{WireBody, WireCurve};
