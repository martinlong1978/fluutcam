// SPDX-License-Identifier: MIT OR Apache-2.0

//! Locales: a language tag and the number conventions that go with it
//! (spec 08 §8.10a).

use thiserror::Error;
use unic_langid::LanguageIdentifier;

#[derive(Debug, Error, PartialEq, Eq)]
#[error("`{0}` is not a language tag")]
pub struct LocaleError(String);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Locale {
    id: LanguageIdentifier,
}

/// Languages that write the decimal separator as a comma.
const COMMA_LANGUAGES: &[&str] = &[
    "bg", "cs", "da", "de", "el", "es", "et", "fi", "fr", "hr", "hu", "it", "lt", "lv", "nb", "nl",
    "nn", "pl", "pt", "ro", "ru", "sk", "sl", "sr", "sv", "tr", "uk",
];

/// Regions that use a point although their language mostly uses a comma.
const POINT_REGIONS: &[(&str, &str)] = &[("de", "CH"), ("es", "MX"), ("es", "US")];

impl Locale {
    pub fn english() -> Locale {
        Locale {
            id: "en".parse().unwrap_or_default(),
        }
    }

    /// Parses a BCP 47 tag such as `en-GB` or `de` (`_` is accepted for `-`).
    ///
    /// # Errors
    ///
    /// The tag is not well formed.
    pub fn parse(tag: &str) -> Result<Locale, LocaleError> {
        let id = tag
            .trim()
            .replace('_', "-")
            .parse::<LanguageIdentifier>()
            .map_err(|_| LocaleError(tag.to_owned()))?;
        Ok(Locale { id })
    }

    /// The language subtag (`en`, `de`).
    pub fn language(&self) -> &str {
        self.id.language.as_str()
    }

    pub fn region(&self) -> Option<&str> {
        self.id.region.as_ref().map(|r| r.as_str())
    }

    /// The decimal separator used to display numbers.
    pub fn decimal_separator(&self) -> char {
        let lang = self.language();
        let region = self.region();
        if POINT_REGIONS
            .iter()
            .any(|(l, r)| *l == lang && Some(*r) == region)
        {
            return '.';
        }
        if COMMA_LANGUAGES.contains(&lang) {
            ','
        } else {
            '.'
        }
    }
}

impl Default for Locale {
    fn default() -> Locale {
        Locale::english()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_parse() {
        let l = Locale::parse("en-GB").unwrap();
        assert_eq!((l.language(), l.region()), ("en", Some("GB")));
        assert_eq!(Locale::parse("de_DE").unwrap().language(), "de");
        assert!(Locale::parse("not a tag!").is_err());
        assert!(Locale::parse("").is_err());
    }

    #[test]
    fn decimal_separator_follows_language_and_region() {
        let sep = |t: &str| Locale::parse(t).unwrap().decimal_separator();
        assert_eq!(sep("en"), '.');
        assert_eq!(sep("en-GB"), '.');
        assert_eq!(sep("de"), ',');
        assert_eq!(sep("de-DE"), ',');
        assert_eq!(sep("de-CH"), '.');
        assert_eq!(sep("fr-FR"), ',');
        assert_eq!(sep("ja"), '.');
        assert_eq!(Locale::default().decimal_separator(), '.');
    }
}
