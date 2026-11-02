// SPDX-License-Identifier: MIT OR Apache-2.0

//! `optimised_load_3d` (D15b §4).

use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType};
use fluut_model::params::ToolpathTypeId;

#[derive(Debug)]
pub struct OptimisedLoad3d;

impl ToolpathType for OptimisedLoad3d {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::OptimisedLoad3d
    }

    fn role(&self) -> Role {
        Role::Rough
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        super::rough3d::generate(input)
    }
}
