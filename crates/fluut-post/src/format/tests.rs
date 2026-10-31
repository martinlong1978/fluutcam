// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use proptest::prelude::*;

fn fmt(v: f64, decimals: u32) -> String {
    let f = WordFmt {
        decimals,
        ..WordFmt::default()
    };
    format_number(v, &f, ".")
}

#[test]
fn half_away_from_zero_by_decimal_digits() {
    assert_eq!(fmt(0.0005, 3), "0.001");
    assert_eq!(fmt(-0.0005, 3), "-0.001");
    assert_eq!(fmt(1.0005, 3), "1.001");
    assert_eq!(fmt(2.675, 2), "2.68"); // nearest f64 is 2.67499999…
    assert_eq!(fmt(0.00049, 3), "0");
    assert_eq!(fmt(9.9995, 3), "10");
    assert_eq!(fmt(999.9996, 3), "1000");
    assert_eq!(fmt(0.5, 0), "1");
    assert_eq!(fmt(-0.5, 0), "-1");
    assert_eq!(fmt(0.4, 0), "0");
}

#[test]
fn never_minus_zero() {
    assert_eq!(fmt(-0.00001, 3), "0");
    assert_eq!(fmt(-0.0, 3), "0");
    assert_eq!(fmt(-0.0004, 3), "0");
}

#[test]
fn large_and_small() {
    assert_eq!(fmt(100000.0, 4), "100000");
    assert_eq!(fmt(123456.78915, 4), "123456.7892");
    assert_eq!(fmt(1e-12, 4), "0");
    assert_eq!(fmt(1e20, 2), "100000000000000000000");
}

#[test]
fn options() {
    let mut f = WordFmt {
        decimals: 3,
        trailing_zeros: true,
        ..WordFmt::default()
    };
    assert_eq!(format_number(1.5, &f, "."), "1.500");
    assert_eq!(format_number(2.0, &f, ","), "2,000");
    f.trailing_zeros = false;
    f.leading_zero = false;
    assert_eq!(format_number(0.5, &f, "."), ".5");
    assert_eq!(format_number(-0.5, &f, "."), "-.5");
    assert_eq!(format_number(0.0, &f, "."), "0");
    f.leading_zero = true;
    f.sign = Sign::Always;
    assert_eq!(format_number(1.25, &f, "."), "+1.25");
    assert_eq!(format_number(-1.25, &f, "."), "-1.25");
    let t = WordFmt {
        decimals: 0,
        pad_width: 2,
        ..WordFmt::default()
    };
    assert_eq!(format_number(3.0, &t, "."), "03");
    assert_eq!(format_number(12.0, &t, "."), "12");
    let s = WordFmt {
        decimals: 1,
        scale: 25.4,
        ..WordFmt::default()
    };
    assert_eq!(format_number(2.0, &s, "."), "50.8");
}

#[test]
fn round_dec_matches_format() {
    assert_eq!(round_dec(0.0005, 3), 0.001);
    assert_eq!(round_dec(-2.675, 2), -2.68);
    assert_eq!(round_dec(-0.00001, 3), 0.0);
    assert!(round_dec(-0.00001, 3).is_sign_positive());
}

proptest! {
    /// The formatted text parses back to `round_dec`, which is within half a
    /// unit of the last place of the input.
    #[test]
    fn round_trip(v in -1e6f64..1e6, d in 0u32..7) {
        let s = fmt(v, d);
        let back: f64 = s.parse().unwrap();
        prop_assert_eq!(back, round_dec(v, d));
        let half = 0.5 * libm::pow(10.0, -(d as f64));
        prop_assert!((back - v).abs() <= half * (1.0 + 1e-9) + 1e-9 * v.abs());
        prop_assert!(!s.starts_with("-0") || s.contains(|c: char| ('1'..='9').contains(&c)));
    }
}
