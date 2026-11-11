// SPDX-License-Identifier: MIT OR Apache-2.0

//! The signed update check (spec 06 §6.2 and §6.4, design D14 §5.3).
//!
//! The release workflow publishes `releases.json` and `releases.json.minisig`
//! (minisign, Ed25519). A check downloads both with two HTTPS GETs, verifies
//! the signature against the public key compiled into this binary
//! (`packaging/minisign.pub`), and only then reads the manifest. It offers the
//! newest release that is newer than the running version, for this platform;
//! the user downloads it from the release page. **Nothing is downloaded or
//! installed here**, and nothing is sent but the two requests.
//!
//! Whether a check may run unasked is the caller's business
//! ([`crate::settings::UpdateSettings`]); this module never decides that.
//!
//! The network sits behind [`Fetch`], so tests never use it.

use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// The manifest format this code reads.
pub const FORMAT: &str = "fluut.releases/1";
/// Signed list of stable releases, written by `cargo xtask manifest`. It is
/// an asset of the newest non-pre-release, which GitHub's `releases/latest`
/// finds (D17 §3.3).
pub const MANIFEST_URL: &str =
    "https://github.com/martinlong1978/fluutcam/releases/latest/download/releases.json";
/// Its minisign signature.
pub const SIGNATURE_URL: &str =
    "https://github.com/martinlong1978/fluutcam/releases/latest/download/releases.json.minisig";
/// Signed list of every release, pre-releases included (D17 §3.3). GitHub has
/// no "latest including pre-releases" address, so it is an asset of the
/// rolling pre-release `update-channels`, which a workflow refreshes after
/// each published release.
pub const BETA_MANIFEST_URL: &str = "https://github.com/martinlong1978/fluutcam/releases/download/update-channels/releases-beta.json";
/// Its minisign signature.
pub const BETA_SIGNATURE_URL: &str = "https://github.com/martinlong1978/fluutcam/releases/download/update-channels/releases-beta.json.minisig";
/// Where the user downloads a release.
pub const DOWNLOAD_PAGE: &str = "https://github.com/martinlong1978/fluutcam/releases";
/// Release notes of a version are on the page of its tag.
const NOTES_PREFIX: &str = "https://github.com/martinlong1978/fluutcam/releases/tag/v";
/// Largest manifest accepted, bytes.
pub const MAX_MANIFEST_BYTES: usize = 1 << 20;
/// Largest signature file accepted, bytes.
pub const MAX_SIGNATURE_BYTES: usize = 4 << 10;
/// Sent with both requests: fixed, with no version or machine details.
pub const USER_AGENT: &str = "FluutCAM-update-check";

/// The compiled-in public key file. Until the maintainer puts a key there,
/// the check reports [`UpdateError::NoKey`].
pub const PUBLIC_KEY_FILE: &str = include_str!("../../../packaging/minisign.pub");

/// Which releases the user wants to hear about (D17 §3).
///
/// `Stable` is the default: releases without a pre-release suffix. `Beta`
/// adds the pre-releases (`1.2.0-beta.1`); once a stable release is newer
/// than the running beta it is offered too, because the beta list holds
/// every release. Both channels are signed with the same key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Beta,
}

impl Channel {
    /// The name in the manifest, the settings file and on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        }
    }

    /// `stable` or `beta`; anything else is `None`.
    pub fn parse(s: &str) -> Option<Channel> {
        match s {
            "stable" => Some(Channel::Stable),
            "beta" => Some(Channel::Beta),
            _ => None,
        }
    }

    /// Where this channel's signed list is.
    pub fn manifest_url(self) -> &'static str {
        match self {
            Channel::Stable => MANIFEST_URL,
            Channel::Beta => BETA_MANIFEST_URL,
        }
    }

    /// Where its signature is.
    pub fn signature_url(self) -> &'static str {
        match self {
            Channel::Stable => SIGNATURE_URL,
            Channel::Beta => BETA_SIGNATURE_URL,
        }
    }
}

/// An unknown name in a settings file means the default: a damaged or newer
/// settings file must not lose the rest of the settings.
impl<'de> Deserialize<'de> for Channel {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Channel, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Channel::parse(&s).unwrap_or_default())
    }
}

/// Why a check did not give an answer.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UpdateError {
    #[error("no update key in this build")]
    NoKey,
    #[error("the update key in this build is not a minisign public key: {0}")]
    BadKey(String),
    #[error("the release list could not be downloaded: {0}")]
    Fetch(String),
    #[error("the release list is not signed by the FluutCAM key: {0}")]
    BadSignature(String),
    #[error("the release list is not valid: {0}")]
    BadManifest(String),
}

/// One downloadable file of a release.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Artefact {
    pub platform: String,
    pub kind: String,
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    /// Oldest supported operating system, e.g. `windows-10`. Shown, not
    /// checked.
    #[serde(default)]
    pub min_os: String,
}

/// One release of the manifest.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub version: String,
    pub date: String,
    pub artefacts: Vec<Artefact>,
}

/// `releases.json` (D14 §5.3). Unknown fields are ignored.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub channel: String,
    pub releases: Vec<Release>,
}

/// Reads a manifest. The signature must have been verified first.
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, UpdateError> {
    let m: Manifest =
        serde_json::from_slice(bytes).map_err(|e| UpdateError::BadManifest(e.to_string()))?;
    if m.format != FORMAT {
        return Err(UpdateError::BadManifest(format!(
            "format {} is not {FORMAT}",
            m.format
        )));
    }
    Ok(m)
}

/// The public key from the text of `packaging/minisign.pub`. Lines that
/// start with `#` or `untrusted comment:` and blank lines are skipped; the
/// first other line is the key. No such line: [`UpdateError::NoKey`].
pub fn parse_key(file: &str) -> Result<PublicKey, UpdateError> {
    let line = file
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("untrusted comment:"));
    match line {
        None => Err(UpdateError::NoKey),
        Some(l) => PublicKey::from_base64(l).map_err(|e| UpdateError::BadKey(e.to_string())),
    }
}

/// Checks that `signature` (the text of a `.minisig` file) is a valid
/// minisign signature of `manifest` by `key`. Both signature forms of
/// minisign are accepted (prehashed, and the older direct one): a release
/// workflow on a minisign of either age must not produce a list the app
/// refuses, and a manifest is small.
pub fn verify_manifest(
    manifest: &[u8],
    signature: &str,
    key: &PublicKey,
) -> Result<(), UpdateError> {
    let sig = Signature::decode(signature).map_err(|e| UpdateError::BadSignature(e.to_string()))?;
    key.verify(manifest, &sig, true)
        .map_err(|e| UpdateError::BadSignature(e.to_string()))
}

/// The platform id used in artefact names, for an operating system and
/// architecture as in `std::env::consts`.
pub fn platform_for(os: &str, arch: &str) -> String {
    match os {
        // One universal binary covers both architectures.
        "macos" => "macos-universal".to_owned(),
        _ => format!("{os}-{arch}"),
    }
}

/// The platform of this build.
pub fn platform() -> String {
    platform_for(std::env::consts::OS, std::env::consts::ARCH)
}

/// A release to offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub version: String,
    pub date: String,
    /// Release notes. Built from the version, not read from the manifest.
    pub notes_url: String,
    /// Where to download.
    pub download_page: String,
    /// The files of this release for the platform, as signed in the manifest.
    pub artefacts: Vec<Artefact>,
}

/// The newest release newer than `running` that has a file for `platform`.
/// Pre-releases are offered on the beta channel only, whatever the running
/// version is. Releases whose version is not semantic are skipped. Versions
/// compare by semver precedence (build metadata does not count), so
/// `1.0.0-beta.2` is older than `1.0.0` and a beta of 1.1 is newer than 1.0.
pub fn choose(
    manifest: &Manifest,
    running: &Version,
    platform: &str,
    channel: Channel,
) -> Option<Offer> {
    let allow_pre = channel == Channel::Beta;
    let mut best: Option<(Version, &Release)> = None;
    for r in &manifest.releases {
        let Ok(v) = Version::parse(&r.version) else {
            continue;
        };
        if (!allow_pre && !v.pre.is_empty())
            || v.cmp_precedence(running) != std::cmp::Ordering::Greater
            || !r.artefacts.iter().any(|a| a.platform == platform)
        {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(b, _)| v.cmp_precedence(b) == std::cmp::Ordering::Greater)
        {
            best = Some((v, r));
        }
    }
    best.map(|(v, r)| Offer {
        version: v.to_string(),
        date: r.date.clone(),
        notes_url: format!("{NOTES_PREFIX}{v}"),
        download_page: DOWNLOAD_PAGE.to_owned(),
        artefacts: r
            .artefacts
            .iter()
            .filter(|a| a.platform == platform && a.url.starts_with("https://"))
            .cloned()
            .collect(),
    })
}

/// What a check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    UpToDate,
    Available(Offer),
}

/// An HTTPS GET with a size limit. Behind a trait so tests use a fake.
pub trait Fetch: Send + Sync {
    /// The body of `url`, at most `max_bytes` long.
    fn get(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, String>;
}

/// The check, with the key file text, the running version and the platform
/// given (tests; [`check`] fills in this build's). The order matters: key
/// first (no key, no request), then the downloads, then the signature, and
/// only then the manifest is read.
pub fn check_with(
    fetch: &dyn Fetch,
    key_file: &str,
    running: &str,
    platform: &str,
) -> Result<Outcome, UpdateError> {
    check_channel_with(fetch, key_file, running, platform, Channel::Stable)
}

/// [`check_with`] for a channel.
pub fn check_channel_with(
    fetch: &dyn Fetch,
    key_file: &str,
    running: &str,
    platform: &str,
    channel: Channel,
) -> Result<Outcome, UpdateError> {
    let key = parse_key(key_file)?;
    let running = Version::parse(running)
        .map_err(|e| UpdateError::BadManifest(format!("running version {running}: {e}")))?;
    let manifest = fetch
        .get(channel.manifest_url(), MAX_MANIFEST_BYTES)
        .map_err(UpdateError::Fetch)?;
    let signature = fetch
        .get(channel.signature_url(), MAX_SIGNATURE_BYTES)
        .map_err(UpdateError::Fetch)?;
    let signature = String::from_utf8(signature)
        .map_err(|_| UpdateError::BadSignature("not text".to_owned()))?;
    verify_manifest(&manifest, &signature, &key)?;
    let m = parse_manifest(&manifest)?;
    if m.channel != channel.name() {
        return Err(UpdateError::BadManifest(format!(
            "channel {} is not {}",
            m.channel,
            channel.name()
        )));
    }
    Ok(match choose(&m, &running, platform, channel) {
        Some(o) => Outcome::Available(o),
        None => Outcome::UpToDate,
    })
}

/// The check for this build: the compiled-in key, version and platform, for
/// the channel the user chose.
pub fn check(fetch: &dyn Fetch, channel: Channel) -> Result<Outcome, UpdateError> {
    check_channel_with(
        fetch,
        PUBLIC_KEY_FILE,
        crate::build_info::VERSION,
        &platform(),
        channel,
    )
}

/// This build has a key, so a check can run.
pub fn has_key() -> bool {
    !matches!(parse_key(PUBLIC_KEY_FILE), Err(UpdateError::NoKey))
}

/// Checks a downloaded package against the manifest: size and SHA-256.
/// A helper for a later installer; nothing calls it yet.
pub fn verify_package(mut file: impl Read, artefact: &Artefact) -> Result<(), String> {
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        size += n as u64;
    }
    if size != artefact.size {
        return Err(format!(
            "{} is {size} bytes, the release list says {}",
            artefact.name, artefact.size
        ));
    }
    let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !hex.eq_ignore_ascii_case(&artefact.sha256) {
        return Err(format!("{} does not match its SHA-256", artefact.name));
    }
    Ok(())
}

/// The real network: HTTPS only (redirects too), the operating system's
/// trusted roots, no cookies, a fixed User-Agent, timeouts. The agent is
/// built on first use, on the calling thread, because setting up the system
/// certificate verifier takes a moment.
#[derive(Default)]
pub struct UreqFetch {
    agent: OnceLock<ureq::Agent>,
}

impl std::fmt::Debug for UreqFetch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UreqFetch").finish_non_exhaustive()
    }
}

impl UreqFetch {
    fn build() -> ureq::Agent {
        use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
        let tls = TlsConfig::builder()
            .provider(TlsProvider::Rustls)
            .root_certs(RootCerts::PlatformVerifier)
            .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .build();
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(5)
            .user_agent(USER_AGENT)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(30)))
            .tls_config(tls)
            .build();
        config.into()
    }
}

impl Fetch for UreqFetch {
    fn get(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
        let agent = self.agent.get_or_init(Self::build);
        let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
        response
            .body_mut()
            .with_config()
            .limit(max_bytes as u64)
            .read_to_vec()
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests;
