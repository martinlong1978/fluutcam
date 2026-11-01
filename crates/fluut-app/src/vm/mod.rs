// SPDX-License-Identifier: MIT OR Apache-2.0

//! View models (D13 §1): what the widgets show, derived from a
//! [`Snapshot`](crate::session::Snapshot), the running job and the UI state.
//! They know nothing of `egui`, so they are tested without a window.

pub mod entry;
pub mod inspector;
pub mod ribbon;
pub mod status;
pub mod tree;
pub mod visible;

/// What is selected in the browser or the viewport.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Selection {
    #[default]
    None,
    Setup(String),
    Toolpath(String),
    /// A face picked in the viewport, by id (`f-<stem>.<i>`).
    Face {
        setup: String,
        face: String,
    },
}

/// Formats a number for display: no trailing zeros, at most 4 decimals.
pub fn fmt_num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_drop_trailing_zeros() {
        assert_eq!(fmt_num(6.0), "6");
        assert_eq!(fmt_num(0.25), "0.25");
        assert_eq!(fmt_num(1.0 / 3.0), "0.3333");
        assert_eq!(fmt_num(-0.00001), "0");
    }
}
