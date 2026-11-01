// SPDX-License-Identifier: MIT OR Apache-2.0

//! Number display and entry (spec 08 §8.10a).
//!
//! Display uses the locale's decimal separator. Entry accepts both `.` and
//! `,`; files and G-code never go through this module (they always use `.`).

use crate::locale::Locale;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum NumberError {
    #[error("empty")]
    Empty,
    #[error("`{0}` is not a number")]
    Invalid(String),
    #[error("unexpected `{0}`")]
    Trailing(String),
    #[error("not a finite number")]
    NotFinite,
}

/// Formats `v` with at most `decimals` decimals and no trailing zeros, using
/// the locale's decimal separator. `-0` shows as `0`.
pub fn format_number(v: f64, decimals: usize, locale: &Locale) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s.as_str()
    };
    if s == "-0" {
        return "0".into();
    }
    match locale.decimal_separator() {
        '.' => s.to_owned(),
        sep => s.replace('.', &sep.to_string()),
    }
}

fn is_number_char(c: char) -> bool {
    c.is_ascii_digit() || c == '.' || c == ','
}

/// Reads an unsigned number at the start of `s`: digits with at most one `.`
/// or `,` as the decimal separator. Returns the value and the number of chars
/// used, or `None` when `s` does not start with a digit or separator.
///
/// # Errors
///
/// The text is number-like but not a number (`1.2.3`, `1,2,3`, `.`).
pub fn scan_number(s: &[char]) -> Result<Option<(f64, usize)>, NumberError> {
    let n = s.iter().take_while(|c| is_number_char(**c)).count();
    if n == 0 {
        return Ok(None);
    }
    let text: String = s[..n].iter().collect();
    let seps = text.chars().filter(|c| matches!(c, '.' | ',')).count();
    let digits = text.chars().filter(char::is_ascii_digit).count();
    if seps > 1 || digits == 0 {
        return Err(NumberError::Invalid(text));
    }
    let v: f64 = text
        .replace(',', ".")
        .parse()
        .map_err(|_| NumberError::Invalid(text.clone()))?;
    Ok(Some((v, n)))
}

/// Parses a plain number with an optional sign and either decimal separator.
///
/// # Errors
///
/// Empty text, text that is not a number, or text after the number.
pub fn parse_number(text: &str) -> Result<f64, NumberError> {
    let chars: Vec<char> = text.trim().chars().collect();
    let (neg, body) = match chars.first() {
        None => return Err(NumberError::Empty),
        Some('-' | '\u{2212}') => (true, &chars[1..]),
        Some('+') => (false, &chars[1..]),
        Some(_) => (false, &chars[..]),
    };
    let Some((v, used)) = scan_number(body)? else {
        return Err(NumberError::Invalid(chars.iter().collect()));
    };
    if used != body.len() {
        return Err(NumberError::Trailing(body[used..].iter().collect()));
    }
    if !v.is_finite() {
        return Err(NumberError::NotFinite);
    }
    Ok(if neg { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn de() -> Locale {
        Locale::parse("de").unwrap()
    }

    #[test]
    fn display_drops_trailing_zeros() {
        let en = Locale::english();
        assert_eq!(format_number(6.0, 4, &en), "6");
        assert_eq!(format_number(0.25, 4, &en), "0.25");
        assert_eq!(format_number(1.0 / 3.0, 4, &en), "0.3333");
        assert_eq!(format_number(-0.00001, 4, &en), "0");
        assert_eq!(format_number(100.0, 4, &en), "100");
        assert_eq!(format_number(1200.0, 0, &en), "1200");
    }

    #[test]
    fn display_uses_the_locale_separator() {
        assert_eq!(format_number(0.25, 4, &de()), "0,25");
        assert_eq!(format_number(12.0, 4, &de()), "12");
        assert_eq!(format_number(-1.5, 4, &de()), "-1,5");
    }

    #[test]
    fn entry_accepts_both_separators() {
        assert_eq!(parse_number("2.5"), Ok(2.5));
        assert_eq!(parse_number("2,5"), Ok(2.5));
        assert_eq!(parse_number("-2,5"), Ok(-2.5));
        assert_eq!(parse_number("+.5"), Ok(0.5));
        assert_eq!(parse_number("\u{2212}3"), Ok(-3.0));
        assert_eq!(parse_number("  7 "), Ok(7.0));
    }

    #[test]
    fn display_and_entry_round_trip_with_either_separator() {
        for loc in [Locale::english(), de()] {
            for v in [0.0, 1.0, -1.0, 0.25, 6.35, 1234.5678, -0.0625, 25.4, 1e-4] {
                let text = format_number(v, 4, &loc);
                let back = parse_number(&text).unwrap();
                assert!((back - v).abs() < 5e-5, "{v} -> {text} -> {back}");
            }
        }
        // The other separator reads back to the same value.
        let t = format_number(2.5, 4, &de());
        assert_eq!(parse_number(&t), parse_number(&t.replace(',', ".")));
    }

    #[test]
    fn errors() {
        assert_eq!(parse_number(""), Err(NumberError::Empty));
        assert_eq!(parse_number("   "), Err(NumberError::Empty));
        for t in ["abc", "1.2.3", "1,2,3", "1,234.5", ".", ",", "-", "--1"] {
            assert!(parse_number(t).is_err(), "{t}");
        }
        assert_eq!(parse_number("1 2"), Err(NumberError::Trailing(" 2".into())));
        assert_eq!(parse_number("12x"), Err(NumberError::Trailing("x".into())));
    }

    #[test]
    fn scan_reports_chars_used() {
        let s: Vec<char> = "12,5mm".chars().collect();
        assert_eq!(scan_number(&s), Ok(Some((12.5, 4))));
        let s: Vec<char> = "mm".chars().collect();
        assert_eq!(scan_number(&s), Ok(None));
        assert_eq!(scan_number(&[]), Ok(None));
    }
}
