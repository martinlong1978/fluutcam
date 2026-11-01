// SPDX-License-Identifier: MIT OR Apache-2.0

//! Units in entry and display (spec 08 §8.10a, D15 §1.2).
//!
//! The stored unit is always mm (feeds mm/min, angles degrees). A project
//! shows lengths in [`LengthUnit`]; an entry may carry its own unit, which
//! wins: `6mm`, `0.25in`, `1/4"`.

use crate::locale::Locale;
use crate::num::{NumberError, format_number, scan_number};
use thiserror::Error;

/// The display and entry unit of a project. The application maps its own
/// unit type onto this one (the crate does not depend on the model).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum LengthUnit {
    Mm,
    Inch,
}

impl LengthUnit {
    /// mm per unit.
    pub fn mm(self) -> f64 {
        match self {
            LengthUnit::Mm => 1.0,
            LengthUnit::Inch => 25.4,
        }
    }
}

/// What a field measures.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Dimension {
    /// Stored in mm.
    Length,
    /// Stored in mm/min.
    Feed,
    /// Stored in degrees.
    Angle,
    /// rev/min.
    Speed,
    /// Counts and ratios: no unit.
    Plain,
}

/// A unit written after a number in an entry.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Unit {
    Mm,
    Inch,
    MmPerMin,
    InchPerMin,
    Degree,
}

impl Unit {
    /// Stored units per this unit (mm per inch, and so on).
    pub fn factor(self) -> f64 {
        match self {
            Unit::Inch | Unit::InchPerMin => 25.4,
            Unit::Mm | Unit::MmPerMin | Unit::Degree => 1.0,
        }
    }

    pub fn dimension(self) -> Dimension {
        match self {
            Unit::Mm | Unit::Inch => Dimension::Length,
            Unit::MmPerMin | Unit::InchPerMin => Dimension::Feed,
            Unit::Degree => Dimension::Angle,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum UnitError {
    #[error("empty")]
    Empty,
    #[error(transparent)]
    Number(#[from] NumberError),
    #[error("unexpected `{0}`")]
    Trailing(String),
    #[error("division by zero")]
    DivisionByZero,
}

/// Spellings of each unit, longest first.
const SPELLINGS: &[(&str, Unit)] = &[
    ("mm/min", Unit::MmPerMin),
    ("in/min", Unit::InchPerMin),
    ("\"/min", Unit::InchPerMin),
    ("mm", Unit::Mm),
    ("in", Unit::Inch),
    ("\"", Unit::Inch),
    ("deg", Unit::Degree),
    ("°", Unit::Degree),
];

/// Reads a unit at the start of `s` that is valid for `dim`: `mm`, `in`, `"`
/// for lengths; also `mm/min`, `in/min`, `"/min` for feeds (a bare `mm` or
/// `in` means the feed unit); `°` or `deg` for angles. A unit written in
/// letters must end there, so `6index` has no unit. Returns the unit and the
/// number of chars used.
pub fn scan_unit(s: &[char], dim: Dimension) -> Option<(Unit, usize)> {
    for (text, unit) in SPELLINGS {
        let ok = matches!(
            (dim, unit.dimension()),
            (Dimension::Length, Dimension::Length)
                | (Dimension::Angle, Dimension::Angle)
                | (Dimension::Feed, Dimension::Length | Dimension::Feed)
        );
        if !ok {
            continue;
        }
        let n = text.chars().count();
        if s.len() < n || !s[..n].iter().copied().eq(text.chars()) {
            continue;
        }
        let letters = text.ends_with(|c: char| c.is_alphabetic());
        if letters && s.get(n).is_some_and(|c| c.is_alphanumeric()) {
            continue;
        }
        let unit = match (dim, *unit) {
            (Dimension::Feed, Unit::Mm) => Unit::MmPerMin,
            (Dimension::Feed, Unit::Inch) => Unit::InchPerMin,
            (_, u) => u,
        };
        return Some((unit, n));
    }
    None
}

/// The symbol shown next to a value of `dim`, in the project unit.
pub fn unit_symbol(dim: Dimension, unit: LengthUnit) -> &'static str {
    match (dim, unit) {
        (Dimension::Length, LengthUnit::Mm) => "mm",
        (Dimension::Length, LengthUnit::Inch) => "in",
        (Dimension::Feed, LengthUnit::Mm) => "mm/min",
        (Dimension::Feed, LengthUnit::Inch) => "in/min",
        (Dimension::Angle, _) => "°",
        (Dimension::Speed, _) => "rpm",
        (Dimension::Plain, _) => "",
    }
}

/// Stored units per displayed unit.
pub fn display_scale(dim: Dimension, unit: LengthUnit) -> f64 {
    match dim {
        Dimension::Length | Dimension::Feed => unit.mm(),
        _ => 1.0,
    }
}

/// The text of a stored value for display: the number in the project unit,
/// then the symbol (`6 mm`, `0.25 in`, `45°`, `1200 mm/min`).
pub fn format_quantity(stored: f64, dim: Dimension, unit: LengthUnit, locale: &Locale) -> String {
    let n = format_number(stored / display_scale(dim, unit), 4, locale);
    match unit_symbol(dim, unit) {
        "" => n,
        "°" => format!("{n}°"),
        sym => format!("{n} {sym}"),
    }
}

struct Cursor<'a> {
    s: &'a [char],
    i: usize,
}

impl Cursor<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: char) -> bool {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn number(&mut self) -> Result<f64, UnitError> {
        self.ws();
        match scan_number(&self.s[self.i..])? {
            Some((v, n)) => {
                self.i += n;
                Ok(v)
            }
            None => Err(NumberError::Invalid(self.s[self.i..].iter().collect()).into()),
        }
    }

    /// `/ number` after a numerator.
    fn denominator(&mut self) -> Result<f64, UnitError> {
        let den = self.number()?;
        if den == 0.0 {
            return Err(UnitError::DivisionByZero);
        }
        Ok(den)
    }

    /// A mixed number's fraction (`1/4` in `1 1/4`), if one follows.
    fn fraction_part(&mut self) -> Result<Option<f64>, UnitError> {
        let at = self.i;
        self.ws();
        if self.s.get(self.i).is_some_and(char::is_ascii_digit)
            && let Ok(num) = self.number()
            && self.eat('/')
        {
            return Ok(Some(num / self.denominator()?));
        }
        self.i = at;
        Ok(None)
    }
}

/// Parses one entry that is a number or a fraction with an optional unit,
/// and returns the stored value: `2.5`, `1/4in`, `1 1/4"`, `-6 mm`,
/// `1200 in/min`, `45°`. A value without a unit is in the project unit for
/// lengths and feeds. Expressions with `+ - * ()` are the application's entry
/// parser, which builds on [`scan_number`] and [`scan_unit`].
///
/// # Errors
///
/// The entry is empty, not a number, has a unit that does not fit `dim`, or
/// divides by zero.
pub fn parse_quantity(text: &str, dim: Dimension, unit: LengthUnit) -> Result<f64, UnitError> {
    let chars: Vec<char> = text.trim().chars().collect();
    if chars.is_empty() {
        return Err(UnitError::Empty);
    }
    let mut c = Cursor { s: &chars, i: 0 };
    let neg = if c.eat('-') || c.eat('\u{2212}') {
        true
    } else {
        c.eat('+');
        false
    };
    let mut v = c.number()?;
    if c.eat('/') {
        v /= c.denominator()?;
    } else if v.fract() == 0.0
        && let Some(f) = c.fraction_part()?
    {
        v += f;
    }
    c.ws();
    let mut scale = display_scale(dim, unit);
    if let Some((u, n)) = scan_unit(&chars[c.i..], dim) {
        c.i += n;
        scale = u.factor();
    }
    c.ws();
    if c.i != chars.len() {
        return Err(UnitError::Trailing(chars[c.i..].iter().collect()));
    }
    let out = v * scale;
    if !out.is_finite() {
        return Err(NumberError::NotFinite.into());
    }
    Ok(if neg { -out } else { out })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn q(t: &str, d: Dimension, u: LengthUnit) -> Result<f64, UnitError> {
        parse_quantity(t, d, u)
    }

    const MM: LengthUnit = LengthUnit::Mm;
    const IN: LengthUnit = LengthUnit::Inch;

    #[test]
    fn plain_numbers_are_in_the_project_unit() {
        assert_eq!(q("2.5", Dimension::Length, MM), Ok(2.5));
        assert_eq!(q("2,5", Dimension::Length, MM), Ok(2.5));
        assert_eq!(q("1", Dimension::Length, IN), Ok(25.4));
        assert_eq!(q("100", Dimension::Feed, IN), Ok(2540.0));
        assert_eq!(q("100", Dimension::Feed, MM), Ok(100.0));
        assert_eq!(q("45", Dimension::Angle, IN), Ok(45.0));
        assert_eq!(q("8000", Dimension::Speed, IN), Ok(8000.0));
        assert_eq!(q("-3", Dimension::Length, MM), Ok(-3.0));
    }

    #[test]
    fn explicit_units_win() {
        assert_eq!(q("6mm", Dimension::Length, IN), Ok(6.0));
        assert_eq!(q("0.25in", Dimension::Length, MM), Ok(6.35));
        assert_eq!(q("1in", Dimension::Length, MM), Ok(25.4));
        assert_eq!(q(" 6 mm ", Dimension::Length, IN), Ok(6.0));
        assert_eq!(q("-0.5 in", Dimension::Length, MM), Ok(-12.7));
    }

    #[test]
    fn fractions_and_inch_marks() {
        assert_eq!(q("1/4\"", Dimension::Length, MM), Ok(6.35));
        assert_eq!(q("1/4in", Dimension::Length, MM), Ok(6.35));
        assert_eq!(q("1 / 4 in", Dimension::Length, MM), Ok(6.35));
        assert_eq!(q("1 1/4\"", Dimension::Length, MM), Ok(31.75));
        let v = q("3/8", Dimension::Length, IN).unwrap();
        assert!((v - 9.525).abs() < 1e-12, "{v}");
        assert_eq!(q("6/2", Dimension::Length, MM), Ok(3.0));
        assert_eq!(q("-1/2\"", Dimension::Length, MM), Ok(-12.7));
    }

    #[test]
    fn feeds_in_mm_or_inches_per_minute() {
        assert_eq!(q("1200mm/min", Dimension::Feed, IN), Ok(1200.0));
        assert_eq!(q("40in/min", Dimension::Feed, MM), Ok(1016.0));
        assert_eq!(q("40 \"/min", Dimension::Feed, MM), Ok(1016.0));
        assert_eq!(q("40in", Dimension::Feed, MM), Ok(1016.0));
        assert_eq!(q("500 mm", Dimension::Feed, IN), Ok(500.0));
    }

    #[test]
    fn angles_in_degrees() {
        assert_eq!(q("45°", Dimension::Angle, MM), Ok(45.0));
        assert_eq!(q("45 deg", Dimension::Angle, IN), Ok(45.0));
        assert_eq!(q("-7.5°", Dimension::Angle, MM), Ok(-7.5));
    }

    #[test]
    fn errors() {
        assert_eq!(q("", Dimension::Length, MM), Err(UnitError::Empty));
        assert_eq!(
            q("1/0", Dimension::Length, MM),
            Err(UnitError::DivisionByZero)
        );
        assert_eq!(
            q("1 1/0", Dimension::Length, MM),
            Err(UnitError::DivisionByZero)
        );
        for (t, d) in [
            ("abc", Dimension::Length),
            ("6 furlongs", Dimension::Length),
            ("6index", Dimension::Length),
            ("6mmx", Dimension::Length),
            ("45in", Dimension::Angle),
            ("45mm", Dimension::Angle),
            ("6deg", Dimension::Length),
            ("6mm/min", Dimension::Length),
            ("6mm", Dimension::Speed),
            ("6mm", Dimension::Plain),
            ("1 2", Dimension::Length),
            ("1/2/3", Dimension::Length),
            ("1.2.3mm", Dimension::Length),
            ("mm", Dimension::Length),
            ("--1", Dimension::Length),
            ("1/", Dimension::Length),
        ] {
            assert!(q(t, d, MM).is_err(), "{t}");
        }
    }

    #[test]
    fn scan_unit_by_dimension() {
        let su = |s: &str, d| scan_unit(&chars(s), d);
        assert_eq!(su("mm+1", Dimension::Length), Some((Unit::Mm, 2)));
        assert_eq!(su("\"*2", Dimension::Length), Some((Unit::Inch, 1)));
        assert_eq!(su("in/2", Dimension::Length), Some((Unit::Inch, 2)));
        assert_eq!(su("in/min", Dimension::Feed), Some((Unit::InchPerMin, 6)));
        assert_eq!(su("in/2", Dimension::Feed), Some((Unit::InchPerMin, 2)));
        assert_eq!(su("°", Dimension::Angle), Some((Unit::Degree, 1)));
        assert_eq!(su("mm", Dimension::Angle), None);
        assert_eq!(su("mm", Dimension::Plain), None);
        assert_eq!(su("min", Dimension::Length), None);
        assert_eq!(su("", Dimension::Length), None);
    }

    #[test]
    fn symbols_and_display() {
        let en = Locale::english();
        let de = Locale::parse("de").unwrap();
        assert_eq!(format_quantity(6.0, Dimension::Length, MM, &en), "6 mm");
        assert_eq!(format_quantity(6.35, Dimension::Length, IN, &en), "0.25 in");
        assert_eq!(format_quantity(6.35, Dimension::Length, IN, &de), "0,25 in");
        assert_eq!(
            format_quantity(1016.0, Dimension::Feed, IN, &en),
            "40 in/min"
        );
        assert_eq!(
            format_quantity(1200.0, Dimension::Feed, MM, &en),
            "1200 mm/min"
        );
        assert_eq!(format_quantity(45.0, Dimension::Angle, IN, &en), "45°");
        assert_eq!(
            format_quantity(8000.0, Dimension::Speed, MM, &en),
            "8000 rpm"
        );
        assert_eq!(format_quantity(3.0, Dimension::Plain, MM, &en), "3");
        assert_eq!(unit_symbol(Dimension::Feed, IN), "in/min");
    }

    #[test]
    fn display_then_entry_round_trips() {
        for loc in [Locale::english(), Locale::parse("fr").unwrap()] {
            for unit in [MM, IN] {
                for dim in [Dimension::Length, Dimension::Feed, Dimension::Angle] {
                    for v in [0.0, 6.35, 25.4, 12.7, 1200.0, -3.0] {
                        let text = format_quantity(v, dim, unit, &loc);
                        let back = q(&text, dim, unit).unwrap();
                        assert!((back - v).abs() < 1e-3, "{v}: {text} -> {back}");
                    }
                }
            }
        }
    }

    #[test]
    fn factors() {
        assert_eq!(Unit::Inch.factor(), 25.4);
        assert_eq!(Unit::MmPerMin.factor(), 1.0);
        assert_eq!(Unit::Degree.dimension(), Dimension::Angle);
        assert_eq!(LengthUnit::Inch.mm(), 25.4);
    }
}
