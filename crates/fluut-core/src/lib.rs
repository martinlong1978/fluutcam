// SPDX-License-Identifier: MIT OR Apache-2.0

//! The FluutCAM session (design D01 §4–§5): the model store, Setup frames,
//! the invalidation graph, and calculation and post jobs.
//!
//! M3 runs jobs in one process, one after another. The result cache is in
//! memory; the disk sidecar (D06 §7.5) is M4.

pub mod frame;
pub mod graph;
pub mod session;
pub mod store;

pub use frame::SetupFrame;
pub use graph::{Graph, NodeId};
pub use session::{JobReport, JobState, PostOptions, Session, Targets};
pub use store::{LoadedSource, ModelStore};

/// Version of the calculation code, part of every result hash (D01 §4).
pub const ALGORITHM_VERSION: &str = concat!("fluut-cam ", env!("CARGO_PKG_VERSION"), " m3");
