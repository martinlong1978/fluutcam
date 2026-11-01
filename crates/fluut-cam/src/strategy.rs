// SPDX-License-Identifier: MIT OR Apache-2.0

//! v1 strategies (D09): the M3 types `contour_2d`, `pocket_2d` and
//! `parallel`, and the M6 2-D and hole types.

pub(crate) mod boundary;
mod contour_2d;
mod face;
mod parallel;
mod pocket_2d;
mod trace;

pub use contour_2d::Contour2d;
pub use face::Face;
pub use parallel::Parallel;
pub use pocket_2d::Pocket2d;
pub use trace::Trace;
