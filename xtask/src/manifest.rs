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
//! Releases are newest first. There is one manifest per channel (D17 §3.2):
//! `stable` lists releases without a pre-release suffix and is published as
//! `releases.json`; `beta` lists every release, pre-releases included, and is
//! published as `releases-beta.json`. A stable manifest never holds a
//! pre-release: the app's stable channel would ignore it, but the file says
//! what it means. The file is signed separately (minisign,
//! `releases.json.minisig`); the app checks that signature and each
//! download's SHA-256 before offering an update.

use crate::util::{self, Flags};
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::path::Path;

pub const FORMAT: &str = "fluut.releases/1";

/// The channels a manifest can be for.
pub const CHANNELS: &[&str] = &["stable", "beta"];

/// A version with a pre-release suffix (`1.2.0-beta.1`).
pub fn is_prerelease(version: &str) -> bool {
    version.contains('-')
}

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
        (Some(x), Some(y)) => compare_prerelease(&x, &y),
    })
}

/// Pre-release identifiers by semver 2.0 precedence: dot-separated, numbers
/// compare as numbers and rank below text, and a shorter list ranks below a
/// longer one it starts. So `beta.9` is below `beta.10`.
fn compare_prerelease(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.split('.'), b.split('.'));
    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let o = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(m), Ok(n)) => m.cmp(&n),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
        }
    }
}

/// The manifest with `new` added to the releases of `previous` (a release
/// of the same version is replaced), newest first.
pub fn manifest(channel: &str, previous: Option<&Value>, new: Value) -> Result<Value, String> {
    if !CHANNELS.contains(&channel) {
        return Err(format!("channel {channel} is not stable or beta"));
    }
    let pre = |r: &Value| is_prerelease(r["version"].as_str().unwrap_or(""));
    if channel == "stable" && pre(&new) {
        return Err(format!(
            "version {} is a pre-release and does not belong in the stable manifest",
            new["version"].as_str().unwrap_or("")
        ));
    }
    let mut releases: Vec<Value> = Vec::new();
    if let Some(prev) = previous {
        if prev["format"] != FORMAT {
            return Err(format!("previous manifest is not {FORMAT}"));
        }
        if prev["channel"] != channel {
            return Err(format!(
                "previous manifest is for channel {}, not {channel}",
                prev["channel"]
            ));
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
        let prev = json!({ "format": "other/1", "channel": "stable", "releases": [] });
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

    #[test]
    fn prerelease_identifiers_compare_as_semver_says() {
        assert_eq!(
            compare_versions("1.0.0-beta.10", "1.0.0-beta.9"),
            Ordering::Greater
        );
        assert_eq!(
            compare_versions("1.0.0-beta.2", "1.0.0-beta.11"),
            Ordering::Less
        );
        assert_eq!(
            compare_versions("1.0.0-alpha", "1.0.0-alpha.1"),
            Ordering::Less
        );
        assert_eq!(
            compare_versions("1.0.0-alpha.1", "1.0.0-alpha.beta"),
            Ordering::Less
        );
        assert_eq!(
            compare_versions("1.0.0-rc.1", "1.0.0-beta.9"),
            Ordering::Greater
        );
        assert_eq!(compare_versions("1.1.0-beta.1", "1.0.0"), Ordering::Greater);
    }

    #[test]
    fn the_stable_manifest_refuses_a_pre_release() {
        let r = release("1.0.0", "2026-11-01", "https://x", &files()).unwrap();
        let mut pre = r.clone();
        pre["version"] = json!("1.1.0-beta.1");
        assert!(manifest("stable", None, pre).is_err());
        assert!(manifest("stable", None, r).is_ok());
    }

    #[test]
    fn the_beta_manifest_keeps_pre_releases_and_stable_releases_newest_first() {
        let rel = |v: &str| {
            let mut r = release("1.0.0", "2026-11-01", "https://x", &files()).unwrap();
            r["version"] = json!(v);
            r
        };
        let m0 = manifest("beta", None, rel("1.0.0")).unwrap();
        let m1 = manifest("beta", Some(&m0), rel("1.1.0-beta.1")).unwrap();
        let m2 = manifest("beta", Some(&m1), rel("1.1.0-beta.10")).unwrap();
        let m3 = manifest("beta", Some(&m2), rel("1.1.0-beta.2")).unwrap();
        assert_eq!(m3["channel"], "beta");
        let v: Vec<_> = m3["releases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["version"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            v,
            ["1.1.0-beta.10", "1.1.0-beta.2", "1.1.0-beta.1", "1.0.0"]
        );
        // Stable 1.1.0 outranks its own betas.
        let m4 = manifest("beta", Some(&m3), rel("1.1.0")).unwrap();
        assert_eq!(m4["releases"][0]["version"], "1.1.0");
    }

    #[test]
    fn a_manifest_of_another_channel_is_not_a_base_and_unknown_channels_are_refused() {
        let r = release("1.0.0", "2026-11-01", "https://x", &files()).unwrap();
        let stable = manifest("stable", None, r.clone()).unwrap();
        assert!(manifest("beta", Some(&stable), r.clone()).is_err());
        assert!(manifest("nightly", None, r).is_err());
    }
}
