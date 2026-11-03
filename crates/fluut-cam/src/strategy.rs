// SPDX-License-Identifier: MIT OR Apache-2.0

//! v1 strategies (D09): the M3 types `contour_2d`, `pocket_2d` and
//! `parallel`, the M6 2-D and hole types, the 3-D finishing types of
//! D15b §5, 3-D roughing, rotary types and `manual_nc`.

mod bore;
pub(crate) mod boundary;
mod chamfer_2d;
mod contour;
mod contour_2d;
mod drill;
mod engrave;
mod face;
mod horizontal;
mod manual_nc;
mod optimised_load_2d;
mod optimised_load_3d;
mod parallel;
mod pocket_2d;
mod pocket_3d;
mod rotary;
mod rough3d;
mod scallop;
mod spiral;
mod steep_and_shallow;
mod trace;

pub use bore::Bore;
pub use chamfer_2d::Chamfer2d;
pub use contour::Contour;
pub use contour_2d::Contour2d;
pub use drill::Drill;
pub use engrave::Engrave;
pub use face::Face;
pub use horizontal::Horizontal;
pub use manual_nc::{ManualNc, manual_nc};
pub use optimised_load_2d::OptimisedLoad2d;
pub use optimised_load_3d::OptimisedLoad3d;
pub use parallel::Parallel;
pub use pocket_2d::Pocket2d;
pub use pocket_3d::Pocket3d;
pub use rotary::{RotaryAlongX, RotaryAroundA, RotaryRough, RotarySpiral};
pub use scallop::Scallop;
pub use spiral::Spiral;
pub use steep_and_shallow::SteepAndShallow;
pub use trace::Trace;
