// SPDX-License-Identifier: MIT OR Apache-2.0

//! `parallel` (D09 §4.3, D15 §5): the slot for the strategy. It needs the
//! cutter drop queries of S2 (D07), which are on another branch; until they
//! merge, `generate` reports the type as unavailable. Its parameters,
//! validation and file format are complete (`fluut-model`).

use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType};
use fluut_model::params::ToolpathTypeId;

#[derive(Debug)]
pub struct Parallel;

impl ToolpathType for Parallel {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Parallel
    }

    fn role(&self) -> Role {
        Role::Finish
    }

    fn generate(&self, _input: &StrategyInput) -> Result<Generated, CamError> {
        Err(CamError::Unavailable(
            "parallel needs the cutter drop queries (S2) and is not in this build yet".into(),
        ))
    }
}
