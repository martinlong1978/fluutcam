// SPDX-License-Identifier: MIT OR Apache-2.0

//! STEP (ISO 10303-21) reader (design D04).

pub mod parse;

pub use parse::{KwId, Param, ParseError, Pos, RawEntity, RawModel, parse};
