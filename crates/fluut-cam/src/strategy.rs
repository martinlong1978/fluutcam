// SPDX-License-Identifier: MIT OR Apache-2.0

//! v1 strategies (D09). M3 has the 2-D types; `parallel` is registered and
//! reports that it is not available until the S2 drop queries merge.

mod contour_2d;
mod parallel;
mod pocket_2d;

pub use contour_2d::Contour2d;
pub use parallel::Parallel;
pub use pocket_2d::Pocket2d;
