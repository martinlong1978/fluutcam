// SPDX-License-Identifier: MIT OR Apache-2.0

//! Template expressions (D15 §1.6): arithmetic over numbers with optional
//! `mm` or `in` units and the names `tool.diameter`, `tool.radius`,
//! `tool.corner_radius` and `tool.flute_length`. Evaluated once, when a
//! template is applied.

use crate::tools::Tool;

/// Evaluates `text`. `name` gives the value of a name, or `None` when it is
/// unknown.
pub fn eval(text: &str, name: &dyn Fn(&str) -> Option<f64>) -> Result<f64, String> {
    let mut p = P {
        s: text.as_bytes(),
        i: 0,
        name,
    };
    let v = p.sum()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(format!("`{text}`: unexpected `{}`", &text[p.i..]));
    }
    if !v.is_finite() {
        return Err(format!("`{text}` is not a finite number"));
    }
    Ok(v)
}

/// The names a template may use, for `tool`.
pub fn tool_names(tool: Option<&Tool>) -> impl Fn(&str) -> Option<f64> + '_ {
    move |n| {
        let t = tool?;
        match n {
            "tool.diameter" => Some(t.diameter),
            "tool.radius" => Some(t.radius()),
            "tool.corner_radius" => Some(t.corner_radius()),
            "tool.flute_length" => t.flute_length,
            _ => None,
        }
    }
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
    name: &'a dyn Fn(&str) -> Option<f64>,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Result<f64, String> {
        let mut v = self.product()?;
        loop {
            if self.eat(b'+') {
                v += self.product()?;
            } else if self.eat(b'-') {
                v -= self.product()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn product(&mut self) -> Result<f64, String> {
        let mut v = self.unary()?;
        loop {
            if self.eat(b'*') {
                v *= self.unary()?;
            } else if self.eat(b'/') {
                let d = self.unary()?;
                if d == 0.0 {
                    return Err("division by zero".into());
                }
                v /= d;
            } else {
                return Ok(v);
            }
        }
    }

    fn unary(&mut self) -> Result<f64, String> {
        if self.eat(b'-') {
            return Ok(-self.unary()?);
        }
        if self.eat(b'(') {
            let v = self.sum()?;
            if !self.eat(b')') {
                return Err("missing `)`".into());
            }
            return Ok(v);
        }
        self.ws();
        let start = self.i;
        let c = *self.s.get(self.i).ok_or("expression ends early")?;
        if c.is_ascii_digit() || c == b'.' {
            while self
                .s
                .get(self.i)
                .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
            {
                self.i += 1;
            }
            // An exponent: `1e-3`.
            if matches!(self.s.get(self.i), Some(b'e' | b'E'))
                && self
                    .s
                    .get(self.i + 1)
                    .is_some_and(|c| c.is_ascii_digit() || *c == b'-' || *c == b'+')
            {
                self.i += 2;
                while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                    self.i += 1;
                }
            }
            let text = std::str::from_utf8(&self.s[start..self.i]).map_err(|e| e.to_string())?;
            let v: f64 = text
                .parse()
                .map_err(|_| format!("`{text}` is not a number"))?;
            return Ok(v * self.unit());
        }
        if c.is_ascii_alphabetic() {
            while self
                .s
                .get(self.i)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'.')
            {
                self.i += 1;
            }
            let n = std::str::from_utf8(&self.s[start..self.i]).map_err(|e| e.to_string())?;
            return (self.name)(n).ok_or_else(|| format!("`{n}` has no value here"));
        }
        Err(format!("unexpected `{}`", c as char))
    }

    /// An optional unit after a number: `mm` (1) or `in` (25.4).
    fn unit(&mut self) -> f64 {
        let rest = &self.s[self.i..];
        let word_end = |n: usize| {
            rest.get(n)
                .is_none_or(|c| !c.is_ascii_alphanumeric() && *c != b'_')
        };
        if rest.starts_with(b"mm") && word_end(2) {
            self.i += 2;
            1.0
        } else if rest.starts_with(b"in") && word_end(2) {
            self.i += 2;
            25.4
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> Result<f64, String> {
        eval(s, &|n| match n {
            "tool.diameter" => Some(6.0),
            _ => None,
        })
    }

    #[test]
    fn arithmetic() {
        assert_eq!(ev("0.4*tool.diameter").unwrap(), 0.4 * 6.0);
        assert_eq!(ev(" 1 + 2 * 3 ").unwrap(), 7.0);
        assert_eq!(ev("(1 + 2) * 3").unwrap(), 9.0);
        assert_eq!(ev("-tool.diameter / 4").unwrap(), -1.5);
        assert_eq!(ev("1in + 1mm").unwrap(), 26.4);
        assert_eq!(ev("2.5e-1").unwrap(), 0.25);
        for bad in ["", "1 +", "(1", "1 / 0", "tool.radius", "2 x", "1..2", "#"] {
            assert!(ev(bad).is_err(), "{bad}");
        }
    }
}
