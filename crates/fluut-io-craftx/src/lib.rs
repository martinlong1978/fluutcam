// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reader for craft exchange files (`*.craftx`, `craft-exchange/1`): a
//! linked model sent from SolidCraft, with persistent face names (SolidCraft
//! RFC 0001, a draft). The format details live in [`format`].
//!
//! The file is untrusted input: the ZIP container is read with limits
//! ([`zip::Limits`]), every part is checked against its SHA-256 in the
//! manifest, and every problem is a [`Finding`] with an `import.craftx.*`
//! code, never a panic.

pub mod format;
mod read;
pub mod zip;

pub use read::{Body, Exchange, Meta, Options, Param, SourceApp, read};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}

/// A problem found while reading: an error refuses the file, a warning
/// lets it load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    /// Stable code, `import.craftx.*`.
    pub code: &'static str,
    pub message: String,
}

impl Finding {
    pub fn error(code: &'static str, message: impl Into<String>) -> Finding {
        Finding {
            severity: Severity::Error,
            code,
            message: message.into(),
        }
    }

    pub fn warning(code: &'static str, message: impl Into<String>) -> Finding {
        Finding {
            severity: Severity::Warning,
            code,
            message: message.into(),
        }
    }
}
