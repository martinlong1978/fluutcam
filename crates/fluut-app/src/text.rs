// SPDX-License-Identifier: MIT OR Apache-2.0

//! User-visible text (D13 §9).
//!
//! The messages live in `fluut-i18n` (`locales/en/*.ftl`, keyed by Fluent
//! id); this module picks the catalog for the current language and keeps the
//! helpers the views call. Every string the UI shows goes through [`tr`] or
//! one of its helpers.
//!
//! [`Lang::Pseudo`] is the pseudo-locale of D13 §9: every message is
//! accented, wrapped in `⟦…⟧` and padded by about 30 %, so tests catch text
//! that bypasses the catalog and layouts that truncate longer translations.

use fluut_i18n::{Catalog, Locale};
use fluut_model::{Diagnostic, FieldMeta};
use std::cell::{Cell, RefCell};
use std::sync::OnceLock;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Lang {
    English,
    Pseudo,
}

thread_local! {
    // The UI runs on one thread; per-thread state also keeps tests apart.
    static LANG: Cell<Lang> = const { Cell::new(Lang::English) };
    static LOCALE: RefCell<Locale> = RefCell::new(Locale::english());
}

pub fn set_lang(l: Lang) {
    LANG.with(|c| c.set(l));
}

pub fn lang() -> Lang {
    LANG.with(Cell::get)
}

/// Sets the locale that numbers are displayed in (decimal separator).
pub fn set_locale(l: Locale) {
    LOCALE.with(|c| *c.borrow_mut() = l);
}

pub fn locale() -> Locale {
    LOCALE.with(|c| c.borrow().clone())
}

fn catalog() -> &'static Catalog {
    static ENGLISH: OnceLock<Catalog> = OnceLock::new();
    static PSEUDO: OnceLock<Catalog> = OnceLock::new();
    match lang() {
        Lang::English => ENGLISH.get_or_init(Catalog::english),
        Lang::Pseudo => PSEUDO.get_or_init(Catalog::pseudo),
    }
}

/// The message `id`. Unknown ids come back unchanged, so they are visible.
pub fn tr(id: &str) -> String {
    catalog().get(id)
}

/// The message `id` with `{ $name }` placeholders filled.
pub fn tr_args(id: &str, args: &[(&str, &str)]) -> String {
    catalog().format(id, args)
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
    catalog().get_or(&m.label_id(), &words(m.key))
}

/// Text of a diagnostic: its Fluent message when the catalog has one,
/// otherwise the message `fluut-model` wrote (English, with values).
pub fn diagnostic(d: &Diagnostic) -> String {
    catalog().get_or(&d.fluent_id(), &d.message)
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

    /// Every string literal in the app's sources that looks like a message id
    /// of a family the catalog knows must be in the catalog.
    #[test]
    fn every_id_the_app_uses_exists_in_english() {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let known = fluut_i18n::english_ids();
        let families: std::collections::BTreeSet<&str> = known
            .iter()
            .filter(|id| !id.starts_with("param-"))
            .filter_map(|id| id.split('-').next())
            .collect();
        let mut files = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut checked = 0;
        for f in files {
            let src = std::fs::read_to_string(&f).unwrap();
            // Test modules may use made-up ids on purpose.
            let src = src.split("#[cfg(test)]").next().unwrap();
            for lit in src.split('"').skip(1).step_by(2) {
                let is_id = lit.contains('-')
                    && lit.chars().all(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'
                    })
                    && !lit.starts_with('-')
                    && !lit.ends_with('-')
                    && families.contains(lit.split('-').next().unwrap());
                if is_id {
                    checked += 1;
                    assert!(known.iter().any(|k| k == lit), "{}: `{lit}`", f.display());
                }
            }
        }
        assert!(checked > 60, "only {checked} ids found");
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
