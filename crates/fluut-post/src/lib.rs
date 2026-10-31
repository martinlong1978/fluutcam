// SPDX-License-Identifier: MIT OR Apache-2.0

//! FluutCAM post-processor engine (spec 05, design D10).
//!
//! Posts are layered TOML definitions resolved by `fluut-model`; this crate
//! adds the block merger, the typed post, templates, expressions, number
//! formatting and the G-code emitter.

pub mod expr;
pub mod merge;
pub mod post;

pub use merge::{POST_FORMAT, PostMerger, flatten, resolve_post};
pub use post::{LoadedPost, Post, PostError};
