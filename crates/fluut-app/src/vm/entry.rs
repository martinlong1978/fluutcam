// SPDX-License-Identifier: MIT OR Apache-2.0

//! Numeric entry in the inspector (D15 §1.2, §1.6; S 08 §8.5, §8.10a).
//!
//! An entry is arithmetic (`+ - * /`, parentheses, unary minus) over
//! numbers and the names `tool.diameter`, `tool.radius`,
//! `tool.corner_radius` and `tool.flute_length` (mm). It is evaluated once,
//! at entry, and the number is stored (D13 §4). Promoted from the S4 spike.
//!
//! Units, for lengths and feeds: at most one factor of a product carries a
//! unit (`mm`, `in` or `"`), and it applies to the whole product, so `1/4in`
//! is a quarter inch and `tool.diameter*0.4` is in mm. A product without a
//! unit is in the project unit. `,` is accepted as the decimal separator.

use fluut_model::FieldKind;
use fluut_model::project::DisplayUnit;
use fluut_model::tools::Tool;

/// How entries of a field kind scale to the stored unit.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct EntryUnit {
    /// Stored units per entered unit when the entry has no unit.
    pub scale: f64,
    /// Whether `mm`, `in` and `"` are allowed (lengths and feeds).
    pub units: bool,
}

impl EntryUnit {
    pub fn of(kind: FieldKind, unit: DisplayUnit) -> EntryUnit {
        match kind {
            FieldKind::Length | FieldKind::Feed => EntryUnit {
                scale: unit.mm(),
                units: true,
            },
            _ => EntryUnit {
                scale: 1.0,
                units: false,
            },
        }
    }
}

/// Evaluates `entry` to the stored value (mm, mm/min, degrees, …).
///
/// # Errors
///
/// The reason, for display under the field.
pub fn evaluate(entry: &str, unit: EntryUnit, tool: Option<&Tool>) -> Result<f64, String> {
    let src: Vec<char> = entry.trim().chars().collect();
    if src.is_empty() {
        return Err("empty".into());
    }
    let mut p = Parser {
        s: &src,
        i: 0,
        unit,
        tool,
    };
    let v = p.sum()?;
    p.ws();
    if p.i != src.len() {
        return Err(format!(
            "unexpected `{}`",
            src[p.i..].iter().collect::<String>()
        ));
    }
    if !v.is_finite() {
        return Err("not a finite number".into());
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
    unit: EntryUnit,
    tool: Option<&'a Tool>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: char) -> bool {
        self.ws();
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Result<f64, String> {
        let mut v = self.product()?;
        loop {
            if self.eat('+') {
                v += self.product()?;
            } else if self.eat('-') {
                v -= self.product()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn product(&mut self) -> Result<f64, String> {
        let (mut v, mut scale) = self.factor()?;
        loop {
            let div = if self.eat('*') {
                false
            } else if self.eat('/') {
                true
            } else {
                break;
            };
            let (w, s) = self.factor()?;
            if div {
                if w == 0.0 {
                    return Err("division by zero".into());
                }
                v /= w;
            } else {
                v *= w;
            }
            scale = scale.or(s);
        }
        Ok(v * scale.unwrap_or(self.unit.scale))
    }

    /// A factor: its number and, if it has a unit, stored units per that
    /// unit.
    fn factor(&mut self) -> Result<(f64, Option<f64>), String> {
        self.ws();
        if self.eat('-') {
            let (v, l) = self.factor()?;
            return Ok((-v, l));
        }
        if self.eat('(') {
            let v = self.sum()?;
            if !self.eat(')') {
                return Err("missing `)`".into());
            }
            return Ok((v, Some(1.0)));
        }
        if self.peek().is_some_and(char::is_alphabetic) {
            let start = self.i;
            while self
                .peek()
                .is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '_')
            {
                self.i += 1;
            }
            let name: String = self.s[start..self.i].iter().collect();
            let t = self.tool.ok_or_else(|| format!("`{name}`: no tool"))?;
            let v = match name.as_str() {
                "tool.diameter" => Some(t.diameter),
                "tool.radius" => Some(t.radius()),
                "tool.corner_radius" => Some(t.corner_radius()),
                "tool.flute_length" => t.flute_length,
                _ => return Err(format!("unknown name `{name}`")),
            };
            let v = v.ok_or_else(|| format!("`{name}` is not set for this tool"))?;
            return Ok((v, Some(1.0)));
        }
        let start = self.i;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_digit() || c == '.' || c == ',')
        {
            self.i += 1;
        }
        if start == self.i {
            return Err("expected a number".into());
        }
        let text: String = self.s[start..self.i].iter().collect();
        let v: f64 = text
            .replace(',', ".")
            .parse()
            .map_err(|_| format!("`{text}` is not a number"))?;
        if self.unit.units {
            let at = self.i;
            self.ws();
            let rest: String = self.s[self.i..].iter().collect();
            for (suffix, scale) in [("mm", 1.0), ("in", 25.4), ("\"", 25.4)] {
                if rest.starts_with(suffix) {
                    self.i += suffix.chars().count();
                    return Ok((v, Some(scale)));
                }
            }
            self.i = at;
        }
        Ok((v, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> Tool {
        let lib = fluut_model::tools::ToolLibrary::parse(
            r#"
format = "fluut.tools/1"
id = "t"
name = "t"
[[tool]]
id = "flat-6"
number = 1
shape = "flat"
diameter = 6
flute_length = 20
"#,
        )
        .unwrap()
        .0;
        lib.tool("flat-6").unwrap().clone()
    }

    fn len(e: &str, u: DisplayUnit) -> Result<f64, String> {
        evaluate(e, EntryUnit::of(FieldKind::Length, u), Some(&tool()))
    }

    #[test]
    fn numbers_and_units() {
        assert_eq!(len("2.5", DisplayUnit::Mm), Ok(2.5));
        assert_eq!(len("2,5", DisplayUnit::Mm), Ok(2.5));
        assert_eq!(len("1", DisplayUnit::Inch), Ok(25.4));
        assert_eq!(len("6mm", DisplayUnit::Inch), Ok(6.0));
        assert_eq!(len("1/4in", DisplayUnit::Mm), Ok(6.35));
        assert_eq!(len("1/4\"", DisplayUnit::Mm), Ok(6.35));
        assert_eq!(len("6/2", DisplayUnit::Mm), Ok(3.0));
        assert_eq!(len("-(1+2)*2", DisplayUnit::Mm), Ok(-6.0));
        assert_eq!(len(" 6 mm ", DisplayUnit::Inch), Ok(6.0));
        assert_eq!(len("1 / 4 in", DisplayUnit::Mm), Ok(6.35));
    }

    #[test]
    fn tool_names_are_mm_in_any_unit() {
        let v = len("tool.diameter*0.4", DisplayUnit::Inch).unwrap();
        assert!((v - 2.4).abs() < 1e-12);
        assert_eq!(len("tool.radius", DisplayUnit::Mm), Ok(3.0));
        assert_eq!(len("tool.flute_length/2", DisplayUnit::Mm), Ok(10.0));
        assert_eq!(len("tool.corner_radius", DisplayUnit::Mm), Ok(0.0));
    }

    #[test]
    fn errors() {
        for e in ["", "abc", "1/0", "(1", "1)", "tool.length", "1 2"] {
            assert!(len(e, DisplayUnit::Mm).is_err(), "{e}");
        }
        let angle = EntryUnit::of(FieldKind::Angle, DisplayUnit::Inch);
        assert_eq!(evaluate("45", angle, None), Ok(45.0));
        assert!(evaluate("45in", angle, None).is_err());
        assert!(evaluate("tool.diameter", angle, None).is_err());
    }
}
