// SPDX-License-Identifier: MIT OR Apache-2.0

//! `cargo xtask manifest`: the update manifest `releases.json` (D14 §5.3).
//!
//! ```json
//! {
//!   "format": "fluut.releases/1",
//!   "channel": "stable",
//!   "releases": [
//!     {
//!       "version": "0.1.0",
//!       "date": "2026-11-01",
//!       "artefacts": [
//!         { "platform": "linux-x86_64", "kind": "tar.gz",
//!           "name": "fluutcam-0.1.0-linux-x86_64.tar.gz",
//!           "url": "https://…", "sha256": "…", "size": 123,
//!           "min_os": "glibc-2.35" }
//!       ]
//!     }
//!   ]
//! }
//! ```
//!
//! Releases are newest first. The file is signed separately (minisign,
//! `releases.json.minisig`); the app checks that signature and each
//! download's SHA-256 before offering an update.

use crate::util::{self, Flags};
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::path::Path;

pub const FORMAT: &str = "fluut.releases/1";

/// Artefact name suffixes, longest first so `tar.gz` wins over `gz`.
const KINDS: &[&str] = &["tar.gz", "zip", "dmg", "msi", "AppImage"];

/// Oldest operating system each platform's package supports.
fn min_os(platform: &str) -> &'static str {
    match platform {
        p if p.starts_with("windows") => "windows-10",
        p if p.starts_with("macos") => "macos-11.0",
        _ => "glibc-2.35",
    }
}

/// Splits `fluutcam-<version>-<platform>[-cli].<kind>`.
pub fn parse_name<'a>(name: &'a str, version: &str) -> Option<(&'a str, &'a str)> {
    let rest = name.strip_prefix("fluutcam-")?.strip_prefix(version)?;
    let rest = rest.strip_prefix('-')?;
    let kind = KINDS.iter().find(|k| rest.ends_with(&format!(".{k}")))?;
    let platform = &rest[..rest.len() - kind.len() - 1];
    (!platform.is_empty()).then_some((platform, kind))
}

/// One release entry from the files `(name, bytes)` of a dist directory.
pub fn release(
    version: &str,
    date: &str,
    base_url: &str,
    files: &[(String, Vec<u8>)],
) -> Result<Value, String> {
    let mut artefacts: Vec<Value> = files
        .iter()
        .filter_map(|(name, data)| {
            let (platform, kind) = parse_name(name, version)?;
            Some(json!({
                "platform": platform,
                "kind": kind,
                "name": name,
                "url": format!("{}/{name}", base_url.trim_end_matches('/')),
                "sha256": util::sha256_hex(data),
                "size": data.len(),
                "min_os": min_os(platform),
            }))
        })
        .collect();
    if artefacts.is_empty() {
        return Err(format!("no artefacts for version {version}"));
    }
    artefacts.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(json!({ "version": version, "date": date, "artefacts": artefacts }))
}

/// Semantic-version order: numbers first, then a release outranks its
/// pre-releases.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let split = |v: &str| -> (Vec<u64>, Option<String>) {
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p.to_owned())),
            None => (v, None),
        };
        (
            core.split('.').map(|n| n.parse().unwrap_or(0)).collect(),
            pre,
        )
    };
    let ((na, pa), (nb, pb)) = (split(a), split(b));
    na.cmp(&nb).then_with(|| match (pa, pb) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => x.cmp(&y),
    })
}

/// The manifest with `new` added to the releases of `previous` (a release
/// of the same version is replaced), newest first.
pub fn manifest(channel: &str, previous: Option<&Value>, new: Value) -> Result<Value, String> {
    let mut releases: Vec<Value> = Vec::new();
    if let Some(prev) = previous {
        if prev["format"] != FORMAT {
            return Err(format!("previous manifest is not {FORMAT}"));
        }
        releases = prev["releases"].as_array().cloned().unwrap_or_default();
        releases.retain(|r| r["version"] != new["version"]);
    }
    releases.push(new);
    releases.sort_by(|a, b| {
        compare_versions(
            b["version"].as_str().unwrap_or(""),
            a["version"].as_str().unwrap_or(""),
        )
    });
    Ok(json!({ "format": FORMAT, "channel": channel, "releases": releases }))
}

/// `cargo xtask manifest --dist D --base-url U --out F [--version V]
/// [--date YYYY-MM-DD] [--channel stable] [--previous releases.json]`.
pub fn run(args: Vec<String>) -> Result<(), String> {
    let f = Flags::parse(args)?;
    let version = match f.opt("version") {
        Some(v) => v.to_owned(),
        None => {
            let t = std::fs::read_to_string(util::workspace_root().join("Cargo.toml"))
                .map_err(|e| e.to_string())?;
            util::workspace_version(&t)?
        }
    };
    let date = match (f.opt("date"), util::source_date_epoch()) {
        (Some(d), _) => d.to_owned(),
        (None, Some(e)) => util::iso_date(e),
        (None, None) => return Err("give --date or set SOURCE_DATE_EPOCH".into()),
    };
    let dist = Path::new(f.req("dist")?);
    let mut files = Vec::new();
    for e in std::fs::read_dir(dist).map_err(|e| format!("{}: {e}", dist.display()))? {
        let p = e.map_err(|e| e.to_string())?.path();
        if p.is_file() {
            let name = p
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            files.push((name, std::fs::read(&p).map_err(|e| e.to_string())?));
        }
    }
    let previous = match f.opt("previous") {
        Some(p) => Some(
            serde_json::from_slice::<Value>(&std::fs::read(p).map_err(|e| format!("{p}: {e}"))?)
                .map_err(|e| format!("{p}: {e}"))?,
        ),
        None => None,
    };
    let rel = release(&version, &date, f.req("base-url")?, &files)?;
    let m = manifest(f.opt("channel").unwrap_or("stable"), previous.as_ref(), rel)?;
    let text = serde_json::to_string_pretty(&m).map_err(|e| e.to_string())? + "\n";
    std::fs::write(f.req("out")?, text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<(String, Vec<u8>)> {
        vec![
            ("fluutcam-1.0.0-windows-x86_64.zip".into(), b"abc".to_vec()),
            ("fluutcam-1.0.0-linux-x86_64.tar.gz".into(), b"".to_vec()),
            ("fluutcam-1.0.0-macos-universal.dmg".into(), b"dmg".to_vec()),
            ("SHA256SUMS".into(), b"x".to_vec()),
            ("fluutcam-0.9.0-linux-x86_64.tar.gz".into(), b"old".to_vec()),
        ]
    }

    #[test]
    fn names_split_into_platform_and_kind() {
        assert_eq!(
            parse_name("fluutcam-1.0.0-linux-x86_64.tar.gz", "1.0.0"),
            Some(("linux-x86_64", "tar.gz"))
        );
        assert_eq!(
            parse_name("fluutcam-1.0.0-rc.1-windows-x86_64.zip", "1.0.0-rc.1"),
            Some(("windows-x86_64", "zip"))
        );
        assert_eq!(parse_name("fluutcam-1.0.0-linux.txt", "1.0.0"), None);
        assert_eq!(parse_name("fluutcam-1.0.01-linux.zip", "1.0.0"), None);
        assert_eq!(parse_name("SHA256SUMS", "1.0.0"), None);
    }

    #[test]
    fn release_lists_the_artefacts_of_the_version_only() {
        let r = release("1.0.0", "2026-11-01", "https://example.org/dl/", &files()).unwrap();
        assert_eq!(r["version"], "1.0.0");
        assert_eq!(r["date"], "2026-11-01");
        let a = r["artefacts"].as_array().unwrap();
        assert_eq!(a.len(), 3);
        // Sorted by name.
        assert_eq!(a[0]["name"], "fluutcam-1.0.0-linux-x86_64.tar.gz");
        assert_eq!(a[0]["platform"], "linux-x86_64");
        assert_eq!(a[0]["kind"], "tar.gz");
        assert_eq!(
            a[0]["url"],
            "https://example.org/dl/fluutcam-1.0.0-linux-x86_64.tar.gz"
        );
        assert_eq!(a[0]["size"], 0);
        assert_eq!(a[0]["min_os"], "glibc-2.35");
        assert_eq!(a[2]["kind"], "zip");
        assert_eq!(
            a[2]["sha256"],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(a[2]["min_os"], "windows-10");
        assert_eq!(a[1]["min_os"], "macos-11.0");
    }

    #[test]
    fn release_without_artefacts_is_an_error() {
        assert!(release("2.0.0", "2026-11-01", "https://x", &files()).is_err());
    }

    #[test]
    fn manifest_has_the_documented_shape() {
        let r = release("1.0.0", "2026-11-01", "https://x", &files()).unwrap();
        let m = manifest("stable", None, r).unwrap();
        assert_eq!(m["format"], FORMAT);
        assert_eq!(m["channel"], "stable");
        assert_eq!(m["releases"].as_array().unwrap().len(), 1);
        // Round trip through text keeps it valid JSON of the same value.
        let text = serde_json::to_string_pretty(&m).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), m);
    }

    #[test]
    fn new_release_goes_first_and_replaces_its_own_version() {
        let old = release("0.9.0", "2026-10-01", "https://x", &files()).unwrap();
        let m0 = manifest("stable", None, old).unwrap();
        let new = release("1.0.0", "2026-11-01", "https://x", &files()).unwrap();
        let m1 = manifest("stable", Some(&m0), new.clone()).unwrap();
        let v: Vec<_> = m1["releases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["version"].clone())
            .collect();
        assert_eq!(v, ["1.0.0", "0.9.0"]);
        let m2 = manifest("stable", Some(&m1), new).unwrap();
        assert_eq!(m2, m1);
    }

    #[test]
    fn foreign_manifest_is_refused() {
        let prev = json!({ "format": "other/1", "releases": [] });
        let r = release("1.0.0", "d", "https://x", &files()).unwrap();
        assert!(manifest("stable", Some(&prev), r).is_err());
    }

    #[test]
    fn versions_order_numerically_with_prereleases_below() {
        assert_eq!(compare_versions("0.10.0", "0.9.0"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.0", "1.0.0-rc.1"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.0-rc.1", "1.0.0-rc.2"), Ordering::Less);
        assert_eq!(compare_versions("1.2.3", "1.2.3"), Ordering::Equal);
    }
}
