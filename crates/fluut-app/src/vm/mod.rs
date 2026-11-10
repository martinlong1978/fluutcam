// SPDX-License-Identifier: MIT OR Apache-2.0

//! View models (D13 §1): what the widgets show, derived from a
//! [`Snapshot`](crate::session::Snapshot), the running job and the UI state.
//! They know nothing of `egui`, so they are tested without a window.

pub mod entry;
pub mod inspector;
pub mod pick;
pub mod playback;
pub mod ribbon;
pub mod setup;
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

/// Formats a number for display: no trailing zeros, at most 4 decimals, the
/// decimal separator of the UI locale.
pub fn fmt_num(v: f64) -> String {
    fluut_i18n::format_number(v, 4, &crate::text::locale())
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

    #[test]
    fn numbers_use_the_locale_separator() {
        use crate::text::{locale, set_locale};
        let before = locale();
        set_locale(fluut_i18n::Locale::parse("de").unwrap());
        let s = fmt_num(0.25);
        set_locale(before);
        assert_eq!(s, "0,25");
    }
}
