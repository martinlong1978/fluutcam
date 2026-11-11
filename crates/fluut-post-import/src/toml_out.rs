// SPDX-License-Identifier: MIT OR Apache-2.0

//! Writes a tool library as `fluut.tools/1` TOML text.

use std::fmt::Write;

/// A TOML basic string: quotes, backslashes and control characters escaped.
pub(crate) fn string(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// A float with at most six decimals, always with a `.`.
pub(crate) fn num(x: f64) -> String {
    let r = (x * 1e6).round() / 1e6;
    let r = if r == 0.0 { 0.0 } else { r };
    let mut s = format!("{r}");
    if !s.contains('.') {
        s.push_str(".0");
    }
    s
}
