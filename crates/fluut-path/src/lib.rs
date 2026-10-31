// SPDX-License-Identifier: MIT OR Apache-2.0

//! Toolpath result types shared by calculation, simulation, post-processing and
//! persistence (design D08 §2, D01 §1).
//!
//! A [`ToolpathResult`] is a sequence of [`Pass`]es in machining order; links
//! between passes are passes too. Segment endpoints are exact `f64` values and
//! consecutive segments share endpoints bitwise.

pub mod linearize;
pub mod pass;
pub mod segment;

pub use linearize::linearize;
pub use pass::{CompSide, Pass, PassKind, PathError, PathStats, ToolpathResult};
pub use segment::{
    AxisPos, CycleKind, CycleParams, FeedRef, MarkerKind, Motion, MotionInfo, PersistId, Segment,
};
