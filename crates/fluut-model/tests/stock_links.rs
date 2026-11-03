// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which toolpath types link over the in-process stock (D08 §8.2): their
//! results depend on the earlier stock-updating toolpaths.

use fluut_model::params::ToolpathTypeId;

#[test]
fn finishing_types_link_over_the_stock() {
    for t in ToolpathTypeId::ALL {
        if t.follows_surface() {
            assert!(t.links_over_stock(), "{}", t.as_str());
            assert!(!t.updates_stock(), "{}", t.as_str());
        }
    }
    // Rotary types and `manual_nc` read no planar stock.
    for t in ToolpathTypeId::ALL {
        if t.is_rotary() || t == ToolpathTypeId::ManualNc {
            assert!(!t.links_over_stock(), "{}", t.as_str());
        }
    }
}
