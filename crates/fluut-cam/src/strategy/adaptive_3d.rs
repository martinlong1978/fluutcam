// SPDX-License-Identifier: MIT OR Apache-2.0

//! `adaptive_3d` (D15b §4).

use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType};
use fluut_model::params::ToolpathTypeId;

#[derive(Debug)]
pub struct Adaptive3d;

impl ToolpathType for Adaptive3d {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Adaptive3d
    }

    fn role(&self) -> Role {
        Role::Rough
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        super::rough3d::generate(input)
    }
}
