// SPDX-License-Identifier: MIT OR Apache-2.0

//! FluutCAM desktop application (design D13).
//!
//! Layers, bottom up (D13 §1):
//! - `session` and `jobs`: the `fluut-core` session on a worker thread, with
//!   job progress and cancel (D01 §3). Only `session` calls `fluut-core`.
//! - `scene`: display geometry of a Setup and its toolpaths.
//! - `vm`: view models, independent of the toolkit.
//! - `viewport`: the `wgpu` renderer and camera.
//! - `ui`: the `egui` widget layer.
//! - `text`, `brand`: user-visible text and the brand.

pub mod brand;
pub mod jobs;
pub mod scene;
pub mod session;
pub mod text;
