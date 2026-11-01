// SPDX-License-Identifier: MIT OR Apache-2.0

//! v1 strategies (D09): the M3 types `contour_2d`, `pocket_2d` and
//! `parallel`, the M6 2-D and hole types, and the 3-D finishing types of
//! D15b §5.

mod adaptive_2d;
mod bore;
pub(crate) mod boundary;
mod chamfer_2d;
mod contour;
mod contour_2d;
mod drill;
mod engrave;
mod face;
mod horizontal;
mod parallel;
mod pocket_2d;
mod scallop;
mod spiral;
mod steep_and_shallow;
mod trace;

pub use adaptive_2d::Adaptive2d;
pub use bore::Bore;
pub use chamfer_2d::Chamfer2d;
pub use contour::Contour;
pub use contour_2d::Contour2d;
pub use drill::Drill;
pub use engrave::Engrave;
pub use face::Face;
pub use horizontal::Horizontal;
pub use parallel::Parallel;
pub use pocket_2d::Pocket2d;
pub use scallop::Scallop;
pub use spiral::Spiral;
pub use steep_and_shallow::SteepAndShallow;
pub use trace::Trace;
