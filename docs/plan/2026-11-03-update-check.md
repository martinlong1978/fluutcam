# Update Check Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Write the opt-in, signed update check that [spec 06 §6.2 and §6.4](../spec/06-security-and-distribution.md) and [D14 §5.3](../design/D14-build-test-release.md) describe. M8 part 1 already writes and signs `releases.json`; nothing reads it yet. The check must keep the privacy statement in [`CODE_SIGNING.md`](../CODE_SIGNING.md) true: off until the user says yes, one HTTPS request pair, nothing else sent, nothing downloaded or installed.

**Architecture:** `fluut-core::update` (no UI) holds the manifest types, the signature check, the version choice, the `Fetch` trait with a `ureq` implementation, and the SHA-256 check for a downloaded package. `fluut-core::settings` stores the user's answer and the time of the last check in `<data dir>/settings.toml` (the directory of `fluut_core::crash`). The public key is `packaging/minisign.pub`, compiled in with `include_str!`; while the file holds no key, the check stops before any network use with "no update key in this build". The app runs each check on its own short-lived thread (never the UI thread, and not the calculation worker, which a slow network must not delay) and shows the result in the status bar and a Settings window. The CLI gets `fluut update-check`, which only ever runs when typed.

**Tech Stack:** `minisign-verify` (MIT, no dependencies), `semver` (MIT OR Apache-2.0, already in the lock file), `ureq` 3 with `rustls` (ring) and `rustls-platform-verifier`, `serde_json`, `toml`.

**Design:** [`../design/D14-build-test-release.md`](../design/D14-build-test-release.md) §5.3; [`../design/D13-ui-architecture.md`](../design/D13-ui-architecture.md) §1, §8.

## Global Constraints

All earlier Global Constraints apply. In addition:

- Opt-in: the answer defaults to "no", also when the first-start dialog is dismissed. Nothing touches the network before a yes, except a check the user asks for ("Check now", `fluut update-check`).
- The request is two HTTPS GETs (`releases.json` and `releases.json.minisig`) with a timeout, a size limit, a fixed User-Agent without version or machine details, no cookies, and redirects only to HTTPS. Nothing is sent but the request itself.
- Never download or install an update. An offer is a version, a notes link and a download page; the user downloads it. The links shown are built from the validated version, not copied from the manifest.
- The manifest is trusted only after the minisign signature verifies against the compiled-in key. No key, no check.
- No invented key. No secret key in the repository; tests make their own key pair.
- `HashMap` iteration never reaches an output.

## Dependencies and licences

| Crate | Licence | Why |
|---|---|---|
| `minisign-verify` 0.3 | MIT | minisign (Ed25519) verification; no dependencies |
| `semver` 1 | MIT OR Apache-2.0 | version precedence (already in the lock file through a build dependency) |
| `ureq` 3 (`rustls-no-provider`) | MIT OR Apache-2.0 | HTTPS client, no native TLS; no cookies, no gzip |
| `rustls` 0.23 (`ring`) | Apache-2.0 OR ISC OR MIT | TLS |
| `rustls-platform-verifier` | MIT OR Apache-2.0 | certificate checks by the operating system (Windows and macOS build the chain with the system, which also fetches missing roots) |
| `ring`, `rustls-webpki`, `rustls-pki-types`, `untrusted`, `subtle`, `zeroize`, `base64`, `http`, `httparse`, `ureq-proto`, `utf8-zero`, `rustls-native-certs`, `openssl-probe`, `schannel`, `security-framework(-sys)`, `core-foundation` | MIT, Apache-2.0, ISC or a mix of them | below the above |

`deny.toml` needs no new licence. `ureq` with its default `rustls` feature would add `webpki-roots` (the Mozilla root list), whose licence, CDLA-Permissive-2.0, is not in the list of [spec 06 §6.1](../spec/06-security-and-distribution.md); `rustls-platform-verifier` lists the same data crate for `wasm32`. Therefore the certificates are checked by the operating system through `rustls-platform-verifier`, which is also what users expect behind a company proxy. The lock file names `webpki-root-certs` for that `wasm32` case, but no shipped binary contains it. `deny.toml` now sets `[graph] targets` to the three shipped platforms, so `cargo deny` does not check dependencies of platforms we never build. `rustls-native-certs` alone was tried first and rejected: on Windows it misses roots the system has not fetched yet (`example.com` failed with `UnknownIssuer`).

## Review Focus

1. **Order of checks:** key present, then fetch, then signature, then parse. Nothing in an unverified manifest is read before the signature verifies.
2. **Version choice:** semver precedence (build metadata ignored), pre-releases only for a pre-release run, the platform must have an artefact.
3. **Privacy:** every path to `Fetch::get` is behind either "auto-check enabled and due" or an explicit user action.

---

### Task 1: `fluut-core::update`

**Files:** `crates/fluut-core/src/update.rs`, `packaging/minisign.pub`, `crates/fluut-core/Cargo.toml`, `Cargo.lock`.

- `Manifest`, `Release`, `Artefact` (serde, unknown fields ignored); `parse_manifest` refuses any format but `fluut.releases/1`.
- `verify_manifest(bytes, signature, key)` with `minisign-verify`; the key is read from `packaging/minisign.pub`: comment lines (`#`, `untrusted comment:`) are skipped; no other line means "no update key in this build".
- `choose(manifest, running, platform, channel)`: the newest release with a higher version, a stable version unless the running one is a pre-release, and an artefact for the platform. Releases with an invalid version are skipped.
- `Fetch` trait (`get(url, max_bytes)`), `UreqFetch`, and `check(fetch, key, running, platform)`.
- `verify_package(reader, artefact)`: size and SHA-256 against the manifest. A helper only; there is no installer.
- [x] **Step 1:** Tests: parsing (the output shape of `cargo xtask manifest`, unknown fields, a foreign format); a good signature, a bad one (changed byte, other key, truncated); no key stops before any request; version choice with pre-releases, platforms and bad versions; the package hash helper; `check` against a fake `Fetch`, including a failed request and a manifest that does not verify.
- [x] **Step 2:** Fail, implement, pass.

### Task 2: Settings and throttle

**Files:** `crates/fluut-core/src/settings.rs`.

- `Settings` with an `update` part (`asked`, `auto_check`, `last_check`) in `<data dir>/settings.toml`, written atomically; a missing or damaged file gives the defaults (not asked, off).
- `needs_prompt()`, `answer(bool)`, `auto_check_due(now)`: enabled and at least 24 h since `last_check` (a `last_check` in the future counts as due); `record_check(now)`.
- [x] **Step 1:** Tests: defaults are off and not asked; the prompt state; the answer persists; a damaged file; due only when on; once a day; a clock set back.
- [x] **Step 2:** Fail, implement, pass.

### Task 3: App and CLI

**Files:** `crates/fluut-app/src/{updates.rs, ui/settings.rs, ui/mod.rs, main.rs, lib.rs}`, `crates/fluut-cli/src/main.rs`, `crates/fluut-i18n/locales/en/app.ftl`.

- `Updates` (app layer): settings, state (idle, checking, up to date, available, failed, no key), a thread per check with a channel back, `tick` for the automatic check. `FluutApp::new` starts with updates off (tests never touch the network or the data directory); `main` turns them on.
- The first-start dialog "Check for updates automatically?" (Yes, No; Escape is No), a Settings window with the toggle, "Check now" and the result, and a status-bar notice with the notes link and download page as text with a copy button (the build has no browser launcher).
- `fluut update-check`: one manual check, prints the result; exit code 0 for up to date or an update, 1 for a failed check, 2 for arguments, 3 when the build has no key.
- [x] **Step 1:** Tests: opt-in default off; first-start prompt state through `egui_kittest` (No on dismiss); no check when off; a manual check with the fake `Fetch` when off; at most one a day; the check runs off the UI thread (the UI polls); CLI output with the fake.
- [x] **Step 2:** Fail, implement, pass.

### Task 4: Documents

**Files:** spec 06, D14 §5.3, `docs/CODE_SIGNING.md`, `docs/plan/README.md`, this plan.

- [x] **Step 1:** Update the documents; how the maintainer adds the key; workspace checks; commit.

**Done.** `fluut-core::update`, `fluut-core::settings`, the app (`updates.rs`, first-start question, Settings window, status-bar notice) and `fluut update-check` are in, with about 70 new tests and no real network in any of them. The only real request made while building was by hand: TLS to github.com worked, an expired certificate and a body over the limit were refused, and `http://` was refused. Not tested end to end: a real signed `releases.json` (there is no key and no release yet). Deviations:
- The check thread is one `std::thread` per check, not a request on the session worker: that worker runs long calculations, and a 30 s network timeout must not wait behind them (or hold them up).
- There was no settings storage; `fluut-core::settings` is new (`<data dir>/settings.toml`, the directory of the crash reports).
- Certificates are checked by the operating system (`rustls-platform-verifier`), not by a bundled root list; see the licence table. `deny.toml` got `[graph] targets`. No licence was added.
- `Cargo.lock` also moved `async-executor` 1.14.0 → 1.14.1, because 1.14.0 was yanked and `cargo deny` failed on it.
- Both minisign signature forms are accepted (the prehashed one and the older direct one).
- The links of an offer are shown as text with a "Copy link" button, because the build has no browser launcher. The release-notes link is the tag page of the version, built in the app; the manifest has no notes field.
- The detail of a failed check is the English text of `UpdateError`, not a Fluent message.
- `min_os` is shown nowhere and checked nowhere yet.
- The first-start question is not asked by a build without a key (it could not keep its promise).
- Manifest types are not shared with `xtask` (which builds `serde_json::Value`s and is outside the layers); a test parses the shape it writes.
