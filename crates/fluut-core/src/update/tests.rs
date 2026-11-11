// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use blake2::Blake2b512;
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// A minisign key pair made for the test; the secret half never leaves it.
struct TestKey {
    pair: Ed25519KeyPair,
    id: [u8; 8],
}

impl TestKey {
    fn new(seed: u8) -> TestKey {
        TestKey {
            pair: Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap(),
            id: [seed; 8],
        }
    }

    /// The text of a `.pub` file.
    fn public_file(&self) -> String {
        let mut raw = b"Ed".to_vec();
        raw.extend_from_slice(&self.id);
        raw.extend_from_slice(self.pair.public_key().as_ref());
        format!(
            "untrusted comment: minisign public key\n{}\n",
            STANDARD.encode(raw)
        )
    }

    /// A signature in the older direct form ("Ed"), made without hashing.
    fn sign_legacy(&self, message: &[u8]) -> String {
        let sig = self.pair.sign(message);
        let mut raw = b"Ed".to_vec();
        raw.extend_from_slice(&self.id);
        raw.extend_from_slice(sig.as_ref());
        let trusted = "timestamp:1\tfile:releases.json";
        let mut global = sig.as_ref().to_vec();
        global.extend_from_slice(trusted.as_bytes());
        let global = self.pair.sign(&global);
        format!(
            "untrusted comment: signature\n{}\ntrusted comment: {trusted}\n{}\n",
            STANDARD.encode(raw),
            STANDARD.encode(global.as_ref())
        )
    }

    /// The text of a `.minisig` file for `message` (prehashed, as minisign
    /// writes it).
    fn sign(&self, message: &[u8]) -> String {
        let hashed = Blake2b512::digest(message);
        let sig = self.pair.sign(&hashed);
        let mut raw = b"ED".to_vec();
        raw.extend_from_slice(&self.id);
        raw.extend_from_slice(sig.as_ref());
        let trusted = "timestamp:1\tfile:releases.json";
        let mut global = sig.as_ref().to_vec();
        global.extend_from_slice(trusted.as_bytes());
        let global = self.pair.sign(&global);
        format!(
            "untrusted comment: signature\n{}\ntrusted comment: {trusted}\n{}\n",
            STANDARD.encode(raw),
            STANDARD.encode(global.as_ref())
        )
    }
}

/// A manifest as `cargo xtask manifest` writes it, plus a field this code
/// does not know.
fn manifest_json(channel: &str, versions: &[&str]) -> String {
    let releases: Vec<String> = versions
        .iter()
        .map(|v| {
            let art = |platform: &str, kind: &str| {
                format!(
                    r#"{{ "platform": "{platform}", "kind": "{kind}",
                      "name": "fluutcam-{v}-{platform}.{kind}",
                      "url": "https://github.com/martinlong1978/fluutcam/releases/download/v{v}/fluutcam-{v}-{platform}.{kind}",
                      "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                      "size": 3, "min_os": "glibc-2.35", "future": true }}"#
                )
            };
            format!(
                r#"{{ "version": "{v}", "date": "2026-11-01", "artefacts": [{}, {}] }}"#,
                art("linux-x86_64", "tar.gz"),
                art("windows-x86_64", "zip")
            )
        })
        .collect();
    format!(
        r#"{{ "format": "fluut.releases/1", "channel": "{channel}", "note": 1,
            "releases": [{}] }}"#,
        releases.join(",")
    )
}

fn parsed(versions: &[&str]) -> Manifest {
    parse_manifest(manifest_json("stable", versions).as_bytes()).unwrap()
}

fn ver(v: &str) -> Version {
    Version::parse(v).unwrap()
}

/// Answers from a table and notes every request.
struct Fake {
    files: BTreeMap<&'static str, Result<Vec<u8>, String>>,
    calls: Mutex<Vec<String>>,
}

impl Fake {
    fn serving(manifest: &[u8], signature: &str) -> Fake {
        Fake {
            files: BTreeMap::from([
                (MANIFEST_URL, Ok(manifest.to_vec())),
                (SIGNATURE_URL, Ok(signature.as_bytes().to_vec())),
            ]),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Fetch for Fake {
    fn get(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
        self.calls.lock().unwrap().push(url.to_owned());
        let body = self.files.get(url).cloned().ok_or("404")??;
        if body.len() > max_bytes {
            return Err("too large".into());
        }
        Ok(body)
    }
}

#[test]
fn the_urls_are_https_and_the_signature_sits_next_to_the_list() {
    assert!(MANIFEST_URL.starts_with("https://"));
    assert_eq!(SIGNATURE_URL, format!("{MANIFEST_URL}.minisig"));
    assert!(DOWNLOAD_PAGE.starts_with("https://"));
    assert!(NOTES_PREFIX.starts_with("https://"));
    assert!(!USER_AGENT.contains(char::is_numeric));
}

// --- manifest -------------------------------------------------------------

#[test]
fn parses_the_manifest_that_xtask_writes() {
    let m = parsed(&["1.2.0", "1.1.0"]);
    assert_eq!(m.channel, "stable");
    assert_eq!(m.releases.len(), 2);
    assert_eq!(m.releases[0].version, "1.2.0");
    assert_eq!(m.releases[0].artefacts.len(), 2);
    let a = &m.releases[0].artefacts[0];
    assert_eq!(a.platform, "linux-x86_64");
    assert_eq!(a.kind, "tar.gz");
    assert_eq!(a.size, 3);
    assert_eq!(a.min_os, "glibc-2.35");
}

#[test]
fn a_foreign_format_or_broken_json_is_refused() {
    let other = r#"{ "format": "other/2", "channel": "stable", "releases": [] }"#;
    assert!(matches!(
        parse_manifest(other.as_bytes()),
        Err(UpdateError::BadManifest(_))
    ));
    assert!(matches!(
        parse_manifest(b"{ not json"),
        Err(UpdateError::BadManifest(_))
    ));
    assert!(matches!(
        parse_manifest(br#"{ "format": "fluut.releases/1" }"#),
        Err(UpdateError::BadManifest(_))
    ));
}

// --- key and signature ----------------------------------------------------

#[test]
fn a_file_with_only_comments_has_no_key() {
    assert_eq!(parse_key("").unwrap_err(), UpdateError::NoKey);
    assert_eq!(
        parse_key("# nothing here\n\n  # yet\nuntrusted comment: x\n").unwrap_err(),
        UpdateError::NoKey
    );
}

#[test]
fn a_line_that_is_not_a_key_is_a_bad_key() {
    assert!(matches!(
        parse_key("# comment\nRWQnotakey\n"),
        Err(UpdateError::BadKey(_))
    ));
}

#[test]
fn a_key_file_with_comments_parses() {
    let file = format!("# why\n{}", TestKey::new(1).public_file());
    assert!(parse_key(&file).is_ok());
}

#[test]
fn the_shipped_key_file_is_a_valid_key_or_still_empty() {
    // Catches a maintainer pasting a broken key before a release.
    assert!(matches!(
        parse_key(PUBLIC_KEY_FILE),
        Ok(_) | Err(UpdateError::NoKey)
    ));
}

#[test]
fn a_good_signature_verifies() {
    let k = TestKey::new(1);
    let key = parse_key(&k.public_file()).unwrap();
    let text = manifest_json("stable", &["1.0.0"]);
    assert_eq!(
        verify_manifest(text.as_bytes(), &k.sign(text.as_bytes()), &key),
        Ok(())
    );
}

#[test]
fn a_changed_manifest_does_not_verify() {
    let k = TestKey::new(1);
    let key = parse_key(&k.public_file()).unwrap();
    let text = manifest_json("stable", &["1.0.0"]);
    let sig = k.sign(text.as_bytes());
    let mut bad = text.into_bytes();
    bad[10] ^= 1;
    assert!(matches!(
        verify_manifest(&bad, &sig, &key),
        Err(UpdateError::BadSignature(_))
    ));
}

#[test]
fn another_key_does_not_verify() {
    let signer = TestKey::new(1);
    let other = parse_key(&TestKey::new(2).public_file()).unwrap();
    let text = manifest_json("stable", &["1.0.0"]);
    assert!(matches!(
        verify_manifest(text.as_bytes(), &signer.sign(text.as_bytes()), &other),
        Err(UpdateError::BadSignature(_))
    ));
}

#[test]
fn a_signature_made_with_the_same_id_but_another_secret_does_not_verify() {
    let real = TestKey::new(1);
    let mut forger = TestKey::new(9);
    forger.id = real.id;
    let key = parse_key(&real.public_file()).unwrap();
    let text = manifest_json("stable", &["9.0.0"]);
    assert!(matches!(
        verify_manifest(text.as_bytes(), &forger.sign(text.as_bytes()), &key),
        Err(UpdateError::BadSignature(_))
    ));
}

#[test]
fn a_truncated_or_empty_signature_does_not_verify() {
    let k = TestKey::new(1);
    let key = parse_key(&k.public_file()).unwrap();
    let sig = k.sign(b"abc");
    assert!(verify_manifest(b"abc", &sig[..sig.len() / 2], &key).is_err());
    assert!(verify_manifest(b"abc", "", &key).is_err());
}

#[test]
fn the_verifiers_own_test_vector_verifies() {
    // From the minisign-verify test suite: the key and signature of "test".
    let key = parse_key("RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3").unwrap();
    let sig = "untrusted comment: signature from minisign secret key
RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=
trusted comment: timestamp:1556193335\tfile:test
y/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";
    assert_eq!(verify_manifest(b"test", sig, &key), Ok(()));
    assert!(verify_manifest(b"Test", sig, &key).is_err());
}

// --- version choice -------------------------------------------------------

const LINUX: &str = "linux-x86_64";

fn chosen(m: &Manifest, running: &str, platform: &str) -> Option<String> {
    choose(m, &ver(running), platform, Channel::Stable).map(|o| o.version)
}

fn chosen_beta(m: &Manifest, running: &str) -> Option<String> {
    choose(m, &ver(running), LINUX, Channel::Beta).map(|o| o.version)
}

#[test]
fn offers_the_newest_release_whatever_the_order_in_the_file() {
    let m = parsed(&["1.1.0", "1.3.0", "1.2.0"]);
    assert_eq!(chosen(&m, "1.0.0", LINUX).as_deref(), Some("1.3.0"));
}

#[test]
fn compares_numbers_not_text() {
    let m = parsed(&["0.10.0", "0.9.0"]);
    assert_eq!(chosen(&m, "0.9.0", LINUX).as_deref(), Some("0.10.0"));
    assert_eq!(chosen(&m, "0.10.0", LINUX), None);
}

#[test]
fn nothing_is_offered_for_the_same_or_an_older_version() {
    let m = parsed(&["1.0.0", "0.9.0"]);
    assert_eq!(chosen(&m, "1.0.0", LINUX), None);
    assert_eq!(chosen(&m, "2.0.0", LINUX), None);
    // A development build ahead of every release.
    assert_eq!(chosen(&m, "1.0.1-dev.3", LINUX), None);
}

#[test]
fn a_stable_run_is_not_offered_a_pre_release() {
    let m = parsed(&["1.1.0-rc.1", "1.0.0"]);
    assert_eq!(chosen(&m, "0.9.0", LINUX).as_deref(), Some("1.0.0"));
    let only_pre = parsed(&["1.1.0-rc.1"]);
    assert_eq!(chosen(&only_pre, "1.0.0", LINUX), None);
}

#[test]
fn a_stable_channel_never_offers_a_pre_release_even_to_a_pre_release_run() {
    let m = parsed(&["1.0.0-rc.2", "1.0.0-rc.1"]);
    assert_eq!(chosen(&m, "1.0.0-rc.1", LINUX), None);
    // The release after the pre-releases is offered.
    let m = parsed(&["1.0.0", "1.0.0-rc.2", "1.0.0-rc.1"]);
    assert_eq!(chosen(&m, "1.0.0-rc.1", LINUX).as_deref(), Some("1.0.0"));
}

#[test]
fn the_beta_channel_offers_pre_releases_and_the_release_after_them() {
    let m = parsed(&["1.0.0", "1.0.0-rc.2", "1.0.0-rc.1"]);
    assert_eq!(chosen_beta(&m, "1.0.0-rc.1").as_deref(), Some("1.0.0"));
    let m = parsed(&["1.0.0-rc.2", "1.0.0-rc.1"]);
    assert_eq!(chosen_beta(&m, "1.0.0-rc.1").as_deref(), Some("1.0.0-rc.2"));
    assert_eq!(chosen_beta(&m, "1.0.0-rc.2"), None);
    // Numeric identifiers compare as numbers: beta.10 is after beta.9.
    let m = parsed(&["1.0.0-beta.10", "1.0.0-beta.9"]);
    assert_eq!(
        chosen_beta(&m, "1.0.0-beta.9").as_deref(),
        Some("1.0.0-beta.10")
    );
    // A stable user who switches to beta is offered the next beta of the
    // coming version, and never an older beta than the release they run.
    let m = parsed(&["1.1.0-beta.1", "1.0.0-beta.3", "1.0.0"]);
    assert_eq!(chosen_beta(&m, "1.0.0").as_deref(), Some("1.1.0-beta.1"));
    let m = parsed(&["1.0.0-beta.3", "1.0.0"]);
    assert_eq!(chosen_beta(&m, "1.0.0"), None);
}

#[test]
fn channel_names_round_trip_and_unknown_names_fall_back_to_stable() {
    assert_eq!(Channel::default(), Channel::Stable);
    for c in [Channel::Stable, Channel::Beta] {
        assert_eq!(Channel::parse(c.name()), Some(c));
    }
    assert_eq!(Channel::parse("nightly"), None);
    assert_eq!(Channel::parse("Beta"), None);
}

#[test]
fn each_channel_has_its_own_https_pair_with_the_signature_next_to_the_list() {
    for c in [Channel::Stable, Channel::Beta] {
        assert!(c.manifest_url().starts_with("https://"));
        assert_eq!(c.signature_url(), format!("{}.minisig", c.manifest_url()));
    }
    assert_ne!(Channel::Stable.manifest_url(), Channel::Beta.manifest_url());
    assert_eq!(Channel::Stable.manifest_url(), MANIFEST_URL);
}

#[test]
fn build_metadata_does_not_count() {
    let m = parsed(&["1.0.0+b2"]);
    assert_eq!(chosen(&m, "1.0.0+b1", LINUX), None);
}

#[test]
fn only_releases_with_a_file_for_the_platform_count() {
    let m = parsed(&["1.0.0"]);
    assert_eq!(
        chosen(&m, "0.9.0", "windows-x86_64").as_deref(),
        Some("1.0.0")
    );
    assert_eq!(chosen(&m, "0.9.0", "macos-universal"), None);
    let o = choose(&m, &ver("0.9.0"), "windows-x86_64", Channel::Stable).unwrap();
    assert_eq!(o.artefacts.len(), 1);
    assert_eq!(o.artefacts[0].kind, "zip");
}

#[test]
fn a_release_with_a_bad_version_is_skipped_not_fatal() {
    let m = parsed(&["not-a-version", "1.0", "1.1.0"]);
    assert_eq!(chosen(&m, "1.0.0", LINUX).as_deref(), Some("1.1.0"));
}

#[test]
fn the_offer_links_are_built_here_not_taken_from_the_manifest() {
    let o = choose(&parsed(&["1.2.3"]), &ver("1.0.0"), LINUX, Channel::Stable).unwrap();
    assert_eq!(
        o.notes_url,
        "https://github.com/martinlong1978/fluutcam/releases/tag/v1.2.3"
    );
    assert_eq!(o.download_page, DOWNLOAD_PAGE);
    assert_eq!(o.date, "2026-11-01");
}

#[test]
fn artefacts_that_are_not_https_are_left_out_of_the_offer() {
    let mut m = parsed(&["1.2.3"]);
    m.releases[0].artefacts[0].url = "http://example.org/x".into();
    let o = choose(&m, &ver("1.0.0"), LINUX, Channel::Stable);
    assert!(o.unwrap().artefacts.is_empty());
}

#[test]
fn platform_ids_match_the_package_names() {
    assert_eq!(platform_for("windows", "x86_64"), "windows-x86_64");
    assert_eq!(platform_for("linux", "x86_64"), "linux-x86_64");
    assert_eq!(platform_for("macos", "aarch64"), "macos-universal");
    assert_eq!(platform_for("macos", "x86_64"), "macos-universal");
}

// --- the whole check, with a fake network ---------------------------------

fn signed_fake(k: &TestKey, channel: &str, versions: &[&str]) -> Fake {
    let text = manifest_json(channel, versions);
    Fake::serving(text.as_bytes(), &k.sign(text.as_bytes()))
}

#[test]
fn a_signed_newer_release_is_offered() {
    let k = TestKey::new(1);
    let fake = signed_fake(&k, "stable", &["1.1.0", "1.0.0"]);
    let out = check_with(&fake, &k.public_file(), "1.0.0", LINUX).unwrap();
    let Outcome::Available(o) = out else {
        panic!("expected an offer");
    };
    assert_eq!(o.version, "1.1.0");
    // Exactly the two requests, both HTTPS.
    assert_eq!(fake.calls(), [MANIFEST_URL, SIGNATURE_URL]);
}

#[test]
fn the_latest_version_is_up_to_date() {
    let k = TestKey::new(1);
    let fake = signed_fake(&k, "stable", &["1.0.0"]);
    assert_eq!(
        check_with(&fake, &k.public_file(), "1.0.0", LINUX),
        Ok(Outcome::UpToDate)
    );
}

#[test]
fn without_a_key_nothing_is_requested() {
    let k = TestKey::new(1);
    let fake = signed_fake(&k, "stable", &["2.0.0"]);
    let placeholder = "# no key yet\n";
    assert_eq!(
        check_with(&fake, placeholder, "1.0.0", LINUX),
        Err(UpdateError::NoKey)
    );
    assert!(fake.calls().is_empty());
}

#[test]
fn a_manifest_signed_by_another_key_is_not_read() {
    let signer = TestKey::new(2);
    let fake = signed_fake(&signer, "stable", &["2.0.0"]);
    let r = check_with(&fake, &TestKey::new(1).public_file(), "1.0.0", LINUX);
    assert!(matches!(r, Err(UpdateError::BadSignature(_))));
}

#[test]
fn a_signed_but_unreadable_manifest_is_a_manifest_error() {
    let k = TestKey::new(1);
    let fake = Fake::serving(b"not json", &k.sign(b"not json"));
    let r = check_with(&fake, &k.public_file(), "1.0.0", LINUX);
    assert!(matches!(r, Err(UpdateError::BadManifest(_))));
}

#[test]
fn a_signed_manifest_for_another_channel_is_refused() {
    let k = TestKey::new(1);
    let fake = signed_fake(&k, "beta", &["2.0.0"]);
    let r = check_with(&fake, &k.public_file(), "1.0.0", LINUX);
    assert!(matches!(r, Err(UpdateError::BadManifest(_))));
}

fn beta_fake(k: &TestKey, channel: &str, versions: &[&str]) -> Fake {
    let text = manifest_json(channel, versions);
    Fake {
        files: BTreeMap::from([
            (BETA_MANIFEST_URL, Ok(text.clone().into_bytes())),
            (BETA_SIGNATURE_URL, Ok(k.sign(text.as_bytes()).into_bytes())),
        ]),
        calls: Mutex::new(Vec::new()),
    }
}

#[test]
fn the_beta_check_fetches_the_beta_list_and_offers_a_pre_release() {
    let k = TestKey::new(1);
    let fake = beta_fake(&k, "beta", &["1.1.0-beta.1", "1.0.0"]);
    let out = check_channel_with(&fake, &k.public_file(), "1.0.0", LINUX, Channel::Beta).unwrap();
    let Outcome::Available(o) = out else {
        panic!("expected an offer");
    };
    assert_eq!(o.version, "1.1.0-beta.1");
    assert_eq!(fake.calls(), [BETA_MANIFEST_URL, BETA_SIGNATURE_URL]);
}

#[test]
fn a_list_of_the_wrong_channel_is_refused() {
    let k = TestKey::new(1);
    let fake = beta_fake(&k, "stable", &["2.0.0"]);
    let r = check_channel_with(&fake, &k.public_file(), "1.0.0", LINUX, Channel::Beta);
    assert!(matches!(r, Err(UpdateError::BadManifest(_))));
}

#[test]
fn a_failed_request_is_reported_and_stops_the_check() {
    let k = TestKey::new(1);
    let mut fake = signed_fake(&k, "stable", &["2.0.0"]);
    fake.files.insert(MANIFEST_URL, Err("timed out".into()));
    assert_eq!(
        check_with(&fake, &k.public_file(), "1.0.0", LINUX),
        Err(UpdateError::Fetch("timed out".into()))
    );
    assert_eq!(fake.calls(), [MANIFEST_URL]);
}

#[test]
fn a_missing_signature_file_is_reported() {
    let k = TestKey::new(1);
    let mut fake = signed_fake(&k, "stable", &["2.0.0"]);
    fake.files.remove(SIGNATURE_URL);
    let r = check_with(&fake, &k.public_file(), "1.0.0", LINUX);
    assert!(matches!(r, Err(UpdateError::Fetch(_))));
}

#[test]
fn the_size_limits_are_passed_to_the_network() {
    let k = TestKey::new(1);
    let big = vec![b' '; MAX_MANIFEST_BYTES + 1];
    let fake = Fake::serving(&big, &k.sign(&big));
    let r = check_with(&fake, &k.public_file(), "1.0.0", LINUX);
    assert!(matches!(r, Err(UpdateError::Fetch(_))));
}

#[test]
fn this_build_has_a_version_the_check_can_compare() {
    assert!(Version::parse(crate::build_info::VERSION).is_ok());
    // The shipped build without a key reports it and does nothing.
    if !has_key() {
        let fake = Fake::serving(b"", "");
        assert_eq!(check(&fake, Channel::Stable), Err(UpdateError::NoKey));
        assert!(fake.calls().is_empty());
    }
}

// --- package hash ---------------------------------------------------------

fn artefact(size: u64, sha256: &str) -> Artefact {
    Artefact {
        platform: LINUX.into(),
        kind: "tar.gz".into(),
        name: "fluutcam-1.0.0-linux-x86_64.tar.gz".into(),
        url: "https://example.org/x".into(),
        sha256: sha256.into(),
        size,
        min_os: String::new(),
    }
}

#[test]
fn a_package_with_the_listed_size_and_hash_passes() {
    // SHA-256 of "abc".
    let a = artefact(
        3,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
    assert_eq!(verify_package(&b"abc"[..], &a), Ok(()));
    let upper = artefact(
        3,
        "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD",
    );
    assert_eq!(verify_package(&b"abc"[..], &upper), Ok(()));
}

#[test]
fn a_changed_or_cut_package_fails() {
    let a = artefact(
        3,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
    assert!(
        verify_package(&b"abd"[..], &a)
            .unwrap_err()
            .contains("SHA-256")
    );
    assert!(
        verify_package(&b"ab"[..], &a)
            .unwrap_err()
            .contains("bytes")
    );
    assert!(verify_package(&b"abcd"[..], &a).is_err());
}

#[test]
fn the_older_direct_signature_form_verifies_too() {
    let k = TestKey::new(1);
    let key = parse_key(&k.public_file()).unwrap();
    assert_eq!(
        verify_manifest(b"abc", &k.sign_legacy(b"abc"), &key),
        Ok(())
    );
    assert!(verify_manifest(b"abd", &k.sign_legacy(b"abc"), &key).is_err());
}
