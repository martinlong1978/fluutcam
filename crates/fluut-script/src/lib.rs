// SPDX-License-Identifier: MIT OR Apache-2.0

//! FluutCAM scripting (design D12, API D12b): `rhai` scripts that read and
//! change a project, calculate and post, inside a sandbox.
//!
//! - `engine`: limits and the sandboxed engine.
//! - `perm`: file permissions and the library module resolver.
//! - `host`: console output, dialogs and cancellation.
//! - `api`: the objects scripts use, mapped onto `fluut-model` edits.
//! - `run`: one run of a script as one undo step.

pub mod api;
pub mod engine;
pub mod host;
pub mod perm;
pub mod run;
pub mod value;

pub use engine::{Limits, Stop};
pub use host::{Console, Level, ScriptHost, UiRequest, UiResponse};
pub use perm::{Access, Permissions};
pub use run::{ErrorKind, RunOptions, RunOutcome, ScriptError, run};
