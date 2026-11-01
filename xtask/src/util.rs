// SPDX-License-Identifier: MIT OR Apache-2.0

//! Small helpers shared by the release tasks: flags, hashes, dates.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// `--key value` flags and positional arguments.
#[derive(Debug, Default)]
pub struct Flags {
    named: BTreeMap<String, String>,
    pub positional: Vec<String>,
}

impl Flags {
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Flags, String> {
        let mut f = Flags::default();
        let mut it = args.into_iter();
        while let Some(a) = it.next() {
            if let Some(key) = a.strip_prefix("--") {
                let v = it.next().ok_or(format!("--{key} needs a value"))?;
                f.named.insert(key.to_owned(), v);
            } else {
                f.positional.push(a);
            }
        }
        Ok(f)
    }

    pub fn opt(&self, key: &str) -> Option<&str> {
        self.named.get(key).map(String::as_str)
    }

    pub fn req(&self, key: &str) -> Result<&str, String> {
        self.opt(key).ok_or(format!("missing --{key}"))
    }
}

/// Lower-case hex SHA-256.
pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `(year, month, day)` of a day count since 1970-01-01 (proleptic
/// Gregorian; Howard Hinnant's `civil_from_days`).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `YYYY-MM-DD` of a Unix time.
pub fn iso_date(epoch: u64) -> String {
    let (y, m, d) = civil_from_days((epoch / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `SOURCE_DATE_EPOCH` (reproducible-builds.org), if set and valid.
pub fn source_date_epoch() -> Option<u64> {
    std::env::var("SOURCE_DATE_EPOCH").ok()?.trim().parse().ok()
}

/// The workspace root, from this crate's location.
pub fn workspace_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// The `[workspace.package]` version from the root `Cargo.toml`.
pub fn workspace_version(cargo_toml: &str) -> Result<String, String> {
    let mut in_section = false;
    for line in cargo_toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == "[workspace.package]";
        } else if in_section && let Some(rest) = line.strip_prefix("version") {
            let v = rest.trim().strip_prefix('=').map(str::trim);
            if let Some(v) = v.and_then(|v| v.strip_prefix('"')?.split('"').next()) {
                return Ok(v.to_owned());
            }
        }
    }
    Err("no version in [workspace.package]".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_split_named_and_positional() {
        let f = Flags::parse(["out".to_owned(), "--a".into(), "1".into()]).unwrap();
        assert_eq!(f.positional, ["out"]);
        assert_eq!(f.req("a").unwrap(), "1");
        assert!(f.req("b").is_err());
        assert!(Flags::parse(["--a".to_owned()]).is_err());
    }

    #[test]
    fn sha256_of_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn dates_from_epoch() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(951_782_400), "2000-02-29");
        assert_eq!(iso_date(1_793_491_200), "2026-11-01");
    }

    #[test]
    fn version_comes_from_the_workspace_package_table() {
        let t = "[workspace]\nversion = \"9\"\n[workspace.package]\nedition = \"2024\"\nversion = \"1.2.3-rc.1\"\n";
        assert_eq!(workspace_version(t).unwrap(), "1.2.3-rc.1");
        assert!(workspace_version("[package]\nversion = \"1\"\n").is_err());
        let real = std::fs::read_to_string(workspace_root().join("Cargo.toml")).unwrap();
        assert!(workspace_version(&real).is_ok());
    }
}
