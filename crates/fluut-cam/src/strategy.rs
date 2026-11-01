// SPDX-License-Identifier: MIT OR Apache-2.0

//! v1 strategies (D09): the M3 types `contour_2d`, `pocket_2d` and
//! `parallel`, the M6 2-D and hole types, 3-D roughing, rotary types and
//! `manual_nc`.

mod adaptive_2d;
mod adaptive_3d;
mod bore;
pub(crate) mod boundary;
mod chamfer_2d;
mod contour_2d;
mod drill;
mod engrave;
mod face;
mod parallel;
mod pocket_2d;
mod pocket_3d;
mod rough3d;
mod trace;

pub use adaptive_2d::Adaptive2d;
pub use adaptive_3d::Adaptive3d;
pub use bore::Bore;
pub use chamfer_2d::Chamfer2d;
pub use contour_2d::Contour2d;
pub use drill::Drill;
pub use engrave::Engrave;
pub use face::Face;
pub use parallel::Parallel;
pub use pocket_2d::Pocket2d;
pub use pocket_3d::Pocket3d;
pub use trace::Trace;
