// SPDX-License-Identifier: MIT OR Apache-2.0

//! Number formatting (spec 05 §5.3, §5.8; D10 §4.3).
//!
//! Rounding works on decimal digits, not on binary `f64` arithmetic: the
//! value is first written as its shortest round-trip decimal (`{:e}`), then
//! rounded half away from zero as a digit string. So `0.0005` at 3 decimals
//! is `0.001`, although the nearest `f64` to 0.0005 is slightly below it.

use crate::post::{Sign, WordFmt};

#[cfg(test)]
mod tests;

/// A value rounded to a number of decimals, as digit strings.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Rounded {
    neg: bool,
    /// Integer digits without leading zeros ("" for zero).
    int: String,
    /// Exactly `decimals` fraction digits.
    frac: String,
}

impl Rounded {
    fn is_zero(&self) -> bool {
        self.int.is_empty() && self.frac.bytes().all(|b| b == b'0')
    }
}

fn round_decimal(x: f64, decimals: u32) -> Rounded {
    let dec = decimals as i64;
    if !x.is_finite() || x == 0.0 {
        return Rounded {
            neg: false,
            int: String::new(),
            frac: "0".repeat(decimals as usize),
        };
    }
    // "d.ddde-N": mantissa digits and the exponent of the first digit.
    let e = format!("{:e}", x.abs());
    let (mant, exp) = e.split_once('e').expect("{:e} always has an exponent");
    let exp: i64 = exp.parse().expect("{:e} exponent is an integer");
    let mut digits: Vec<u8> = mant
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    // Number of digits before the decimal point.
    let mut point = exp + 1;
    let keep = point + dec;
    if keep < 0 {
        digits.clear();
    } else if (keep as usize) < digits.len() {
        let up = digits[keep as usize] >= 5;
        digits.truncate(keep as usize);
        if up {
            let mut i = digits.len();
            loop {
                if i == 0 {
                    digits.insert(0, 1);
                    point += 1;
                    break;
                }
                i -= 1;
                if digits[i] == 9 {
                    digits[i] = 0;
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
    }
    // Lay the digits out around the point.
    let digit_at = |k: i64| -> u8 {
        if k < 0 {
            0
        } else {
            digits.get(k as usize).copied().unwrap_or(0)
        }
    };
    // When everything rounded away, `digits` is empty and `point` is stale.
    let point = if digits.is_empty() { 0 } else { point };
    let int: String = (0..point.max(0))
        .map(|k| (b'0' + digit_at(k)) as char)
        .collect::<String>()
        .trim_start_matches('0')
        .to_owned();
    let frac: String = (point..point + dec)
        .map(|k| (b'0' + digit_at(k)) as char)
        .collect();
    let mut r = Rounded {
        neg: x < 0.0,
        int,
        frac,
    };
    if r.is_zero() {
        r.neg = false;
    }
    r
}

/// `x` rounded half away from zero to `decimals` decimal places, by decimal
/// digits (see module docs).
pub fn round_dec(x: f64, decimals: u32) -> f64 {
    let r = round_decimal(x, decimals);
    let s = format!(
        "{}{}.{}0",
        if r.neg { "-" } else { "" },
        if r.int.is_empty() { "0" } else { &r.int },
        r.frac
    );
    s.parse().expect("digits form a valid float")
}

/// Formats `v` (already in output units) per the word format. `v` is
/// multiplied by `f.scale` first.
pub fn format_number(v: f64, f: &WordFmt, decimal_char: &str) -> String {
    let r = round_decimal(v * f.scale, f.decimals);
    let frac = if f.trailing_zeros {
        r.frac.as_str()
    } else {
        r.frac.trim_end_matches('0')
    };
    let mut int = r.int.clone();
    if int.is_empty() && (frac.is_empty() || f.leading_zero) {
        int.push('0');
    }
    while int.len() < f.pad_width as usize {
        int.insert(0, '0');
    }
    let mut out = String::new();
    if r.neg {
        out.push('-');
    } else if f.sign == Sign::Always {
        out.push('+');
    }
    out.push_str(&int);
    if !frac.is_empty() {
        out.push_str(decimal_char);
        out.push_str(frac);
    }
    out
}
