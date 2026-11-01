// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spike S4: the parts both UI prototypes share.
//!
//! These are the toolkit-independent layers of D13 §1 in miniature: a
//! generated mesh, an orbit camera, the `wgpu` viewport renderer, mock
//! Setup/Toolpath data shaped like D15, the brand colours and a small
//! measurement harness. Only the widget layer differs between the prototypes.

pub mod bench;
pub mod brand;
pub mod camera;
pub mod mesh;
pub mod mock;
pub mod render;

pub use {glam, wgpu};
