// SPDX-License-Identifier: MIT OR Apache-2.0

//! Diagnostics with stable codes (D15 §1.7).

use std::fmt;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    /// The toolpath cannot be calculated.
    Error,
    /// Calculation runs; the message is shown.
    Warning,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    /// `param.…` for parameter rules, `cam.…` for calculation findings.
    pub code: String,
    pub severity: Severity,
    pub message: String,
    /// Toolpath id, when the diagnostic belongs to one.
    pub toolpath: Option<String>,
    /// Field path (`passes.step_down`), when it belongs to one.
    pub field: Option<String>,
}

impl Diagnostic {
    pub fn error(code: &str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code: code.to_owned(),
            severity: Severity::Error,
            message: message.into(),
            toolpath: None,
            field: None,
        }
    }

    pub fn warning(code: &str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            ..Diagnostic::error(code, message)
        }
    }

    pub fn at(mut self, field: &str) -> Diagnostic {
        self.field = Some(field.to_owned());
        self
    }

    pub fn on(mut self, toolpath: &str) -> Diagnostic {
        self.toolpath = Some(toolpath.to_owned());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// Fluent message id: the code with `.` replaced by `-` (D15 §1.7).
    pub fn fluent_id(&self) -> String {
        self.code.replace('.', "-")
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{sev}")?;
        if let Some(t) = &self.toolpath {
            write!(f, " [{t}]")?;
        }
        write!(f, " {}: {}", self.code, self.message)
    }
}
