# Release Channels and Telemetry Policy Plan

**Goal:** Close the two open items of [spec 06](../spec/06-security-and-distribution.md) §6.5. Write the design ([D17](../design/D17-telemetry-and-channels.md)): a recommended telemetry policy, which **awaits the maintainer's decision**, and release channels (stable and beta). Build the channels, because the change is small and contained.

**Architecture:**
- `fluut-core::update`: `Channel` (`Stable` default, `Beta`), each with its manifest and signature URL; `choose` takes the channel (pre-releases on beta only); `check_channel_with` requires the list's `channel` to equal the one asked for.
- `fluut-core::settings`: `update.channel`, unknown values read as `stable`.
- `fluut-app`: `Updates::channel` and `set_channel`; the `Checker` receives the channel; a radio choice in the Settings window.
- `fluut-cli`: `fluut update-check [--channel stable|beta]`, default the saved setting.
- `xtask manifest`: `--channel stable|beta`; the stable list refuses a pre-release; pre-release identifiers order by SemVer.
- Workflows: `release.yml` builds and signs `releases-beta.json` for every release (and `releases.json` for stable ones only); `channels.yml` copies the signed beta list to the rolling pre-release `update-channels` when a release is published.

**Tech Stack:** as M8. No new dependencies.

**Design:** [D17](../design/D17-telemetry-and-channels.md); [D14](../design/D14-build-test-release.md) §5; spec 06 §6.2, §6.4, §6.5.

## Tasks

- [x] D17: telemetry options and recommendation (marked pending the maintainer), release channels, manifest layout, GitHub `releases/latest` interaction, cadence, Flatpak/Flathub beta branch, SignPath and minisign.
- [x] `Channel` in `fluut-core::update`; `choose` and the check take it; tests (stable never offers a pre-release, beta does, SemVer order, wrong-channel list refused, the beta check requests the beta URLs).
- [x] `update.channel` in the settings, tolerant of unknown values; tests.
- [x] App: `Updates::set_channel`, the checker gets the channel, Settings window radio buttons and note, `settings-channel*` strings; tests.
- [x] CLI `--channel`; tests.
- [x] `xtask manifest --channel`; SemVer pre-release order; stable refuses pre-releases; previous list of another channel refused; tests.
- [x] `release.yml`: both lists, previous lists read with an HTTP status check, `releases-beta.json` signed with the same key. `channels.yml`: copy to `update-channels` on `release: published`. Both parsed as YAML.
- [x] Docs: spec 06 §6.2/§6.4/§6.5, D14 §5.3, `PRIVACY.md` (the beta list is a different URL), `CODE_SIGNING.md`, wiki Settings and updates, Command line, UAT-STG-01 to 04 and UAT-CLI-20.
- [ ] **Maintainer:** decide the telemetry policy (D17 §2.5) and the cadence (D17 §3.7).
- [ ] **First beta tag as the test of the workflows** (they cannot be run locally). Steps: set the workspace version to `0.2.0-beta.1`, tag `v0.2.0-beta.1`, push. Check that the release job produced `releases-beta.json` and `.minisig` and no `releases.json`; that the draft is a pre-release; publish it and check that `channels.yml` created `update-channels` and uploaded both files; run `fluut update-check --channel beta` from a build with the update key; check that the stable address is unchanged.
- [ ] Flathub `beta` branch (D17 §3.5), when Flathub is done.
- [ ] Signed-list expiry (D17 §3.6).

## Known limits

- The two workflows are untested on GitHub; a mistake shows as a failed release job or a failed copy, never as a wrong update (the app verifies the signature and the channel of what it reads).
- Releases must be published in tag order, and a draft must be published before the next tag is pushed (D17 §3.3).
- Until the first release of a channel exists, its address answers 404 and the check reports a failure.
- Windows Installer treats `1.2.0-beta.N` and `1.2.0` as one version; going back to an older stable needs an uninstall.
