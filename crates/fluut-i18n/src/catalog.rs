// SPDX-License-Identifier: MIT OR Apache-2.0

//! Fluent message catalogs (D13 §9, spec 08 §8.10a).
//!
//! A [`Catalog`] holds a chain of bundles, the requested locales first and
//! English last. A message is taken from the first bundle that has it, so a
//! partial translation still shows every string. An id that no bundle has
//! comes back as the id, so a gap is visible and never a panic.
//!
//! English is embedded from `locales/en/*.ftl`. Other languages are added by
//! putting their files in `locales/<lang>/` and listing them in [`resources`].

use crate::locale::Locale;
use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use unic_langid::LanguageIdentifier;

type Bundle = FluentBundle<FluentResource>;

/// The shipped English resources, by file name.
pub(crate) const EN_FILES: [(&str, &str); 4] = [
    ("app.ftl", include_str!("../locales/en/app.ftl")),
    ("ops.ftl", include_str!("../locales/en/ops.ftl")),
    ("params.ftl", include_str!("../locales/en/params.ftl")),
    (
        "diagnostics.ftl",
        include_str!("../locales/en/diagnostics.ftl"),
    ),
];

/// The embedded resource sources of a language (`en`), if it ships.
fn resources(lang: &str) -> Option<Vec<&'static str>> {
    match lang {
        "en" => Some(EN_FILES.iter().map(|(_, s)| *s).collect()),
        _ => None,
    }
}

/// Parses a resource. Entries that fail to parse are dropped, the rest kept.
fn parse_resource(src: &str) -> (FluentResource, Vec<String>) {
    match FluentResource::try_new(src.to_owned()) {
        Ok(r) => (r, Vec::new()),
        Err((r, errs)) => (r, errs.iter().map(|e| format!("{e:?}")).collect()),
    }
}

fn bundle_from(lang: &str, sources: &[&str]) -> Bundle {
    let id: LanguageIdentifier = lang.parse().unwrap_or_default();
    let mut b = FluentBundle::new_concurrent(vec![id]);
    // Isolation marks would end up in the UI text.
    b.set_use_isolating(false);
    for s in sources {
        let (r, _) = parse_resource(s);
        // A duplicate id keeps the first message; nothing to do about the
        // error at run time (a test checks the shipped files).
        let _ = b.add_resource(r);
    }
    b
}

/// Ids of the messages in a resource: the text before `=` on lines that
/// start at column 0 and are not comments or terms.
pub fn message_ids(src: &str) -> Vec<String> {
    src.lines()
        .filter(|l| l.starts_with(|c: char| c.is_ascii_alphabetic()))
        .filter_map(|l| l.split_once('='))
        .map(|(id, _)| id.trim().to_owned())
        .collect()
}

/// The ids of the shipped English messages.
pub fn english_ids() -> Vec<String> {
    EN_FILES.iter().flat_map(|(_, s)| message_ids(s)).collect()
}

pub struct Catalog {
    chain: Vec<Bundle>,
    pseudo: bool,
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog")
            .field("bundles", &self.chain.len())
            .field("pseudo", &self.pseudo)
            .finish()
    }
}

impl Catalog {
    /// The messages for `locales` in order of preference, then English.
    /// A locale with no shipped messages is skipped.
    pub fn new(locales: &[Locale]) -> Catalog {
        let mut chain = Vec::new();
        let mut langs: Vec<&str> = locales.iter().map(Locale::language).collect();
        langs.push("en");
        let mut seen = Vec::new();
        for lang in langs {
            if seen.contains(&lang) {
                continue;
            }
            seen.push(lang);
            if let Some(src) = resources(lang) {
                chain.push(bundle_from(lang, &src));
            }
        }
        Catalog {
            chain,
            pseudo: false,
        }
    }

    pub fn english() -> Catalog {
        Catalog::new(&[])
    }

    /// The pseudo-locale (D13 §9): English with accented letters, wrapped in
    /// `⟦…⟧` and padded by about 30 %. Placeholders and their values are not
    /// accented, so the output still shows which text came from data.
    pub fn pseudo() -> Catalog {
        let srcs: Vec<String> = EN_FILES.iter().map(|(_, s)| accent_source(s)).collect();
        let refs: Vec<&str> = srcs.iter().map(String::as_str).collect();
        Catalog::from_sources(&Locale::english(), &refs, true)
    }

    /// A catalog over the given sources only (no fallback).
    pub(crate) fn from_sources(locale: &Locale, sources: &[&str], pseudo: bool) -> Catalog {
        let mut src: Vec<String> = sources.iter().map(|s| (*s).to_owned()).collect();
        if pseudo {
            src = src.iter().map(|s| accent_source(s)).collect();
        }
        let refs: Vec<&str> = src.iter().map(String::as_str).collect();
        Catalog {
            chain: vec![bundle_from(locale.language(), &refs)],
            pseudo,
        }
    }

    /// Whether some bundle in the chain has a message `id`.
    pub fn has(&self, id: &str) -> bool {
        self.chain.iter().any(|b| b.has_message(id))
    }

    /// The message `id`; unknown ids come back unchanged.
    pub fn get(&self, id: &str) -> String {
        self.format(id, &[])
    }

    /// The message `id`, or `fallback` when no bundle has it. In the
    /// pseudo-locale the fallback is wrapped too.
    pub fn get_or(&self, id: &str, fallback: &str) -> String {
        if self.has(id) {
            self.get(id)
        } else {
            self.finish(fallback.to_owned())
        }
    }

    /// The message `id` with `{ $name }` placeholders filled.
    pub fn format(&self, id: &str, args: &[(&str, &str)]) -> String {
        let mut fa = FluentArgs::new();
        for (k, v) in args {
            fa.set(*k, *v);
        }
        let text = self
            .chain
            .iter()
            .find_map(|b| {
                let pattern = b.get_message(id)?.value()?;
                let mut errors = Vec::new();
                Some(
                    b.format_pattern(pattern, Some(&fa), &mut errors)
                        .into_owned(),
                )
            })
            .unwrap_or_else(|| id.to_owned());
        self.finish(text)
    }

    /// Applies the catalog's decoration (the pseudo-locale's wrapping) to
    /// finished text.
    pub fn finish(&self, text: String) -> String {
        if self.pseudo { pad(&text) } else { text }
    }
}

/// Wraps a finished message in `⟦…⟧` and adds a third of its length in dots.
fn pad(s: &str) -> String {
    let dots = "·".repeat(s.chars().count().div_ceil(3));
    format!("⟦{s}{dots}⟧")
}

fn accent(c: char) -> char {
    const FROM: &str = "AEIOUYaeiouycnCN";
    const TO: &str = "ÁÉÍÓÚÝáéíóúýçñÇÑ";
    FROM.chars()
        .position(|f| f == c)
        .and_then(|i| TO.chars().nth(i))
        .unwrap_or(c)
}

/// Accents the literal text of single-line messages (`id = text`), outside
/// `{ … }`. Comments, terms and attributes pass through.
fn accent_source(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        match line.split_once('=') {
            Some((id, value)) if line.starts_with(|c: char| c.is_ascii_alphabetic()) => {
                out.push_str(id);
                out.push('=');
                let mut depth = 0u32;
                for c in value.chars() {
                    match c {
                        '{' => depth += 1,
                        '}' => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                    out.push(if depth == 0 && c != '}' { accent(c) } else { c });
                }
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests;
