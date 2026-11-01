// SPDX-License-Identifier: MIT OR Apache-2.0

//! FluutCAM project model and definition libraries (design D06).
//!
//! - `defs`: the generic layered resolver for definitions (posts, machines,
//!   templates; D06 §4).
//! - `params`: toolpath parameter metadata and values (D06 §3, D15).
//! - `project`: the project document, read and written (D06 §1, §7.2).
//! - `tools`: tool libraries (spec 02 §2.4).

pub mod defs;
pub mod params;
pub mod project;
pub mod tools;

pub use defs::{
    Def, DefError, DefRef, DefStore, DefWarning, LayerId, MAX_DEPTH, MemStore, ModeMerger, NoModes,
    Resolved, Version, VersionReq, apply_overrides, resolve,
};
pub use params::{
    Class, FieldKind, FieldMeta, Group, HeightSpec, Num, PValue, ParamMap, Params, Sel,
    ToolpathTypeId,
};
