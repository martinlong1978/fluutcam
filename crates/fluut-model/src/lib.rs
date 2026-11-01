// SPDX-License-Identifier: MIT OR Apache-2.0

//! FluutCAM project model and definition libraries (design D06).
//!
//! - `defs`: the generic layered resolver for definitions (posts, machines,
//!   templates; D06 §4).
//! - `library`: the layered definition library (built-in, user, project).
//! - `machine`: machine definitions (spec 02 §2.3).
//! - `edit`, `document`: edits, undo and redo (D06 §6).
//! - `persist`, `journal`: atomic save and the edit journal (D06 §7).
//! - `params`: toolpath parameter metadata and values (D06 §3, D15).
//! - `project`: the project document, read and written (D06 §1, §7.2).
//! - `setup`: Setup inheritance (`based_on`, D06 §2).
//! - `tools`: tool libraries (spec 02 §2.4).
//! - `resolve`, `validate`, `hash`: defaults, validation codes and the
//!   result-hash inputs (D15).

pub mod defs;
pub mod diag;
pub mod document;
pub mod edit;
pub mod hash;
pub mod journal;
pub mod library;
pub mod machine;
pub mod params;
pub mod persist;
pub mod project;
pub mod resolve;
pub mod setup;
pub mod tools;
pub mod validate;

pub use defs::{
    Def, DefError, DefRef, DefStore, DefWarning, LayerId, MAX_DEPTH, MemStore, ModeMerger, NoModes,
    Resolved, Version, VersionReq, apply_overrides, resolve,
};
pub use diag::{Diagnostic, Severity};
pub use params::{
    Class, FieldKind, FieldMeta, Group, HeightSpec, Num, PValue, ParamMap, Params, Sel,
    ToolpathTypeId,
};
