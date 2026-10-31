// SPDX-License-Identifier: MIT OR Apache-2.0

//! Simulation of material removal on a tri-dexel stock (design D11).
//!
//! The stock is three families of rays along X, Y and Z, each ray a sorted list
//! of material spans. Every move cuts the rays it crosses by the exact swept
//! volume of the tool. Results are deterministic on any thread count.

pub mod dexel;
pub mod stock;

pub use dexel::{DexelGrid, Span, TriDexel};
pub use stock::Stock;
