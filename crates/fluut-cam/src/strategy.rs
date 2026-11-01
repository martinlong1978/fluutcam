// SPDX-License-Identifier: MIT OR Apache-2.0

//! v1 strategies (D09): the M3 types `contour_2d`, `pocket_2d` and
//! `parallel`, and the 3-D finishing types of M6 (D15b §5).

mod contour;
mod contour_2d;
mod horizontal;
mod parallel;
mod pocket_2d;

pub use contour::Contour;
pub use contour_2d::Contour2d;
pub use horizontal::Horizontal;
pub use parallel::Parallel;
pub use pocket_2d::Pocket2d;
