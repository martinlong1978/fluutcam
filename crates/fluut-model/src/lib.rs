// SPDX-License-Identifier: MIT OR Apache-2.0

//! FluutCAM project model and definition libraries (design D06).
//!
//! So far only the generic layered resolver for definitions (posts, machines,
//! templates; D06 §4) is here. Project types follow in later milestones.

pub mod defs;

pub use defs::{
    Def, DefError, DefRef, DefStore, DefWarning, LayerId, MAX_DEPTH, MemStore, ModeMerger, NoModes,
    Resolved, Version, VersionReq, apply_overrides, resolve,
};
