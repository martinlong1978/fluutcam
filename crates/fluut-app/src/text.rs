// SPDX-License-Identifier: MIT OR Apache-2.0

//! User-visible text (D13 §9).
//!
//! There is no `fluut-i18n` crate yet, so this module holds the English
//! messages in a table keyed by Fluent ids. Every string the UI shows goes
//! through [`tr`] or one of its helpers; swapping the table for `fluut-i18n`
//! changes this file only.
//!
//! [`Lang::Pseudo`] is the pseudo-locale of D13 §9: every message is wrapped
//! in `⟦…⟧` and padded by about 30 %, so tests catch text that bypasses the
//! table and layouts that truncate longer translations.

use fluut_model::{Diagnostic, FieldMeta};
use std::cell::Cell;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Lang {
    English,
    Pseudo,
}

thread_local! {
    // The UI runs on one thread; per-thread state also keeps tests apart.
    static LANG: Cell<Lang> = const { Cell::new(Lang::English) };
}

pub fn set_lang(l: Lang) {
    LANG.with(|c| c.set(l));
}

pub fn lang() -> Lang {
    LANG.with(Cell::get)
}

/// English messages by Fluent id.
const EN: &[(&str, &str)] = &[
    ("app-title", "FluutCAM"),
    ("app-untitled", "No project"),
    // Ribbon
    ("ribbon-stage-project", "Project"),
    ("ribbon-open", "Open"),
    ("ribbon-open-path", "Project file"),
    ("ribbon-open-hint", "path/to/project.fluut"),
    ("ribbon-calculate", "Calculate"),
    ("ribbon-calculate-all", "Calculate all"),
    ("ribbon-post", "Post"),
    ("ribbon-post-processor", "Post processor"),
    ("ribbon-post-output", "Output file"),
    ("ribbon-cancel", "Cancel"),
    ("ribbon-group-file", "File"),
    ("ribbon-group-toolpaths", "Toolpaths"),
    ("ribbon-group-output", "Output"),
    // Title bar
    ("title-units", "Project units"),
    ("title-theme-dark", "Dark theme"),
    ("title-theme-light", "Light theme"),
    ("title-about", "About FluutCAM"),
    // Dock tabs
    ("tab-browser", "Browser"),
    ("tab-viewport", "Viewport"),
    ("tab-inspector", "Inspector"),
    ("tab-messages", "Messages"),
    // Browser
    ("browser-model", "Model"),
    ("browser-body", "Body { $id }"),
    (
        "browser-empty",
        "Open a project to see its Setups and toolpaths.",
    ),
    ("status-not-calculated", "Not calculated"),
    ("status-calculating", "Calculating { $percent } %"),
    ("status-done", "Calculated"),
    ("status-stale", "Inputs changed: recalculate"),
    ("status-warning", "Calculated with warnings"),
    ("status-error", "Cannot be calculated"),
    ("status-disabled", "Disabled"),
    // Inspector
    ("inspector-empty", "Select a Setup or a toolpath."),
    ("inspector-tool", "Tool"),
    ("inspector-type", "Type"),
    ("inspector-default", "Default: { $value }"),
    ("inspector-default-none", "Default: not set"),
    ("inspector-default-preset", "tool preset { $key }"),
    ("inspector-reset", "Reset to default"),
    ("inspector-invalid-entry", "Not a value: { $text }"),
    (
        "inspector-entry-help",
        "Type a value or an expression, e.g. 6/2, 1/4in, tool.diameter*0.4",
    ),
    ("inspector-read-only", "Edit in the project file for now"),
    ("inspector-yes", "Yes"),
    ("inspector-no", "No"),
    ("inspector-none", "None"),
    ("inspector-items", "{ $count } selected"),
    ("group-tool_params", "Tool"),
    ("group-geometry", "Geometry"),
    ("group-heights", "Heights"),
    ("group-passes", "Passes"),
    ("group-linking", "Linking"),
    ("setup-kind", "Kind"),
    ("setup-material", "Material"),
    ("setup-machine", "Machine"),
    ("setup-work-offset", "Work offset"),
    ("setup-stock", "Stock"),
    ("setup-zero", "Work zero"),
    ("setup-bodies", "Bodies"),
    ("setup-bodies-all", "All bodies"),
    ("setup-toolpaths", "Toolpaths"),
    ("setup-default", "Default"),
    // Viewport
    ("viewport-label", "3-D viewport"),
    (
        "viewport-help",
        "Drag: orbit · right or middle drag: pan · wheel: zoom · click: pick",
    ),
    ("viewport-no-gpu", "No GPU device: the viewport is off"),
    ("viewport-picked-face", "Face { $id }"),
    ("viewport-picked-toolpath", "Toolpath { $name }"),
    ("viewport-fit", "Zoom to fit"),
    // Jobs
    ("job-opening", "Opening { $file }"),
    (
        "job-calculating",
        "Calculating { $name } ({ $done } of { $total })",
    ),
    ("job-posting", "Posting with { $post }"),
    ("job-idle", "Ready"),
    ("job-opened", "Opened { $file }"),
    ("job-calculated", "Calculated { $count } toolpaths"),
    ("job-cancelled", "Cancelled"),
    ("job-posted", "Wrote { $files }"),
    ("job-failed", "Failed: { $reason }"),
    ("job-open-failed", "Cannot open the project"),
    ("job-post-failed", "Cannot post the project"),
    // Messages tab
    ("messages-empty", "No messages."),
    ("severity-error", "Error"),
    ("severity-warning", "Warning"),
    // About
    ("about-version", "Version { $version }"),
    ("about-tagline", "Open-source CAM for CNC milling"),
    (
        "about-licence",
        "Licensed under the MIT or Apache-2.0 licence, at your option.",
    ),
    (
        "about-font",
        "UI font: Noto Sans, SIL Open Font License 1.1.",
    ),
    ("about-close", "Close"),
    // Field labels with wording that differs from the key (D15 §1.7)
    ("param-pass_angle_deg-label", "Pass angle"),
    ("param-angle_deg-label", "Angle"),
    ("param-ramp_angle_deg-label", "Ramp angle"),
    ("param-min_slope_deg-label", "Minimum slope"),
    ("param-max_slope_deg-label", "Maximum slope"),
    ("param-spindle_rpm-label", "Spindle speed"),
    ("param-cut-label", "Cut feed"),
    ("param-plunge-label", "Plunge feed"),
    ("param-ramp-label", "Ramp"),
    ("param-lead_in-label", "Lead in"),
    ("param-lead_out-label", "Lead out"),
];

/// The message for `id`, or `id` itself when the table has none.
fn lookup(id: &str) -> Option<&'static str> {
    EN.iter().find(|(k, _)| *k == id).map(|(_, v)| *v)
}

/// Applies the current language's transformation to a finished message.
fn finish(s: String) -> String {
    match lang() {
        Lang::English => s,
        Lang::Pseudo => {
            let pad = "·".repeat(s.chars().count().div_ceil(3));
            format!("⟦{s}{pad}⟧")
        }
    }
}

/// The message `id`. Unknown ids come back unchanged, so they are visible.
pub fn tr(id: &str) -> String {
    finish(lookup(id).unwrap_or(id).to_owned())
}

/// The message `id` with `{ $name }` placeholders filled.
pub fn tr_args(id: &str, args: &[(&str, &str)]) -> String {
    let mut s = lookup(id).unwrap_or(id).to_owned();
    for (k, v) in args {
        s = s.replace(&format!("{{ ${k} }}"), v);
    }
    finish(s)
}

/// `step_down` → `Step down`; a trailing `_deg` is dropped (the unit shows).
fn words(key: &str) -> String {
    let key = key.strip_suffix("_deg").unwrap_or(key);
    let mut s = key.replace('_', " ");
    if let Some(c) = s.get(..1) {
        let up = c.to_uppercase();
        s.replace_range(..1, &up);
    }
    s
}

/// Label of a D15 field: its `param-<key>-label` message, or the key in
/// words.
pub fn field_label(m: &FieldMeta) -> String {
    match lookup(&m.label_id()) {
        Some(s) => finish(s.to_owned()),
        None => finish(words(m.key)),
    }
}

/// Text of a diagnostic: its Fluent message when the table has one,
/// otherwise the message `fluut-model` wrote (English, with values).
pub fn diagnostic(d: &Diagnostic) -> String {
    finish(lookup(&d.fluent_id()).unwrap_or(&d.message).to_owned())
}

/// A value that is data, not a message (names, numbers, ids): shown as is.
/// Kept separate so the pseudo-locale test can tell data from text.
pub fn data(s: impl Into<String>) -> String {
    s.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluut_model::{Group, ToolpathTypeId};

    #[test]
    fn ids_are_unique_and_messages_not_empty() {
        let mut ids: Vec<&str> = EN.iter().map(|(k, _)| *k).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate ids");
        assert!(EN.iter().all(|(_, v)| !v.trim().is_empty()));
    }

    #[test]
    fn unknown_id_is_shown_as_is() {
        assert_eq!(tr("no-such-message"), "no-such-message");
    }

    #[test]
    fn placeholders_fill() {
        assert_eq!(
            tr_args(
                "job-calculating",
                &[("name", "Rough"), ("done", "1"), ("total", "3")]
            ),
            "Calculating Rough (1 of 3)"
        );
    }

    #[test]
    fn pseudo_locale_wraps_and_lengthens() {
        set_lang(Lang::Pseudo);
        let s = tr("ribbon-open");
        set_lang(Lang::English);
        assert!(s.starts_with('⟦') && s.ends_with('⟧'), "{s}");
        assert!(s.chars().count() >= "Open".len() * 13 / 10 + 2, "{s}");
    }

    #[test]
    fn every_field_has_a_label() {
        for t in ToolpathTypeId::ALL {
            for g in Group::ALL {
                for m in t.group_fields(g) {
                    let l = field_label(m);
                    assert!(!l.is_empty() && !l.contains('_'), "{}: {l}", m.key);
                }
            }
        }
        let m = fluut_model::Params::meta(ToolpathTypeId::Pocket2d, "passes.pass_angle_deg");
        assert_eq!(field_label(m.unwrap()), "Pass angle");
        let m = fluut_model::Params::meta(ToolpathTypeId::Pocket2d, "passes.step_down");
        assert_eq!(field_label(m.unwrap()), "Step down");
    }

    #[test]
    fn diagnostic_falls_back_to_its_message() {
        let d = Diagnostic::error("param.passes.stepover.max", "stepover 7 mm > 6 mm");
        assert_eq!(diagnostic(&d), "stepover 7 mm > 6 mm");
    }
}
