// SPDX-License-Identifier: MIT OR Apache-2.0

//! The FluutCAM session (design D01 §4–§5): the model store, Setup frames,
//! the invalidation graph, and calculation and post jobs.
//!
//! Jobs run in one process, one after another. Results are cached in
//! memory and, when enabled, in the disk sidecar (D06 §7.5).

pub mod cache;
pub mod entities;
pub mod frame;
pub mod graph;
pub mod reimport;
pub mod session;
pub mod store;

pub use frame::SetupFrame;
pub use graph::{Graph, NodeId};
pub use session::{
    JobReport, JobState, OpenOptions, PostOptions, Session, Targets, builtin_library,
};
pub use store::{LoadedSource, ModelStore};

/// Version of the calculation code, part of every result hash (D01 §4).
pub const ALGORITHM_VERSION: &str = concat!("fluut-cam ", env!("CARGO_PKG_VERSION"), " m3");
