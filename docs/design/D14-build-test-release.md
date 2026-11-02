# D14 — Build, Test and Release

Implements: [S 01 §1.8, §1.8.1], [S 06 §6.1–6.3], OQ-10 (Windows, macOS, Linux).

## 1. Repository layout

```
(repository root)
├─ Cargo.toml                 workspace; shared lints, profiles, dependency versions
├─ rust-toolchain.toml        pinned stable toolchain
├─ deny.toml                  cargo-deny: licences (S 06 §6.1), advisories, bans, sources
├─ crates/                    one folder per crate (D01 §1)
├─ xtask/                     dev tasks: check-layers, golden update, release packaging
├─ defs/                      built-in posts, machines, tools, templates (embedded at build)
├─ testdata/                  small fixtures in git; large STEP corpus via Git LFS
├─ fuzz/                      cargo-fuzz targets
├─ docs/                      spec, design, plans; user and API docs (generated parts from D12 §5)
├─ assets/                    brand: logos and app icons as SVG masters (assets/README.md)
└─ packaging/                 WiX, AppImage and Flatpak manifests (not yet written; §5.2)
```

Every source file carries `SPDX-License-Identifier: MIT OR Apache-2.0`.

## 2. Build configuration

- Workspace lints: `unsafe_code = "forbid"` (exceptions per crate with a comment, S 01 §1.8.1),
  `clippy::all` and selected pedantic lints as warnings, CI with `-D warnings`.
- Profiles: `release` with `lto = "thin"`, `codegen-units = 1`, `panic = "unwind"` (crash
  reporter needs unwinding to capture state); `bench` inherits release with debug info.
- Windows: `-C control-flow-guard` [S 06 §6.3]. All platforms: PIE/ASLR, NX defaults.
  The release flags (CFG, path remapping) come from one place, `cargo xtask rustflags`; the
  release workflow and `cargo xtask repro-check` both use it. Ordinary development builds do
  not set them.
- Determinism: no `-C target-cpu=native` in release builds (CH-01).

## 3. CI pipeline (GitHub Actions)

| Job | Platforms | Runs |
|---|---|---|
| lint | Linux | `cargo fmt --check`, `clippy -D warnings`, `xtask check-layers`, `cargo deny check` |
| docs | Linux | `cargo doc --workspace --no-deps` (warnings do not fail it yet) |
| repro | Linux | `xtask repro-check`: two clean release builds of `fluut`, equal hashes (§5.4); on main, tags and request |
| test | Windows, macOS (arm64), Linux | `cargo nextest run` (unit, property, integration), golden posts, D08 endpoint/stepover/invariance tests |
| determinism | all three | calculate reference projects with 1 and N threads; result hashes must be identical across all platforms (CH-01, required) |
| bench | Linux (dedicated runner) | `criterion` targets of D02 §8, D07 §8, D11 §7; fail on > 10 % regression |
| fuzz | Linux, nightly toolchain | each fuzz target 10 min nightly; corpus cached |
| audit | Linux, daily | `cargo audit` (`.github/workflows/audit.yml`; the ignore list in `.cargo/audit.toml` mirrors `deny.toml`) |
| ui | Linux (headless GPU via lavapipe) | view-model tests, screenshot tests (D13 §10) |

Pull requests run lint + test + determinism; nightly runs everything. Tests run with
`cargo test`, not `nextest`. The determinism, bench, fuzz and ui jobs are not set up yet.
There is no MSRV job: `rust-version` equals the pinned toolchain.

## 4. Test data

| Set | Content | Storage |
|---|---|---|
| Geometry | STEP corpus (CAx-IF, CAD exports), STL, DXF, SVG, images | Git LFS; reference fixtures per CH-07 |
| Reference projects | prismatic block, pocketed plate, mould cavity, relief, rotary shaft, rotary-then-indexed (spec example) | git |
| Post reference suite | synthetic results (D10 §8) | generated in code |
| Golden outputs | G-code per built-in post × suite; toolpath geometry summaries | git; updated only via `xtask golden --accept` with review |

## 5. Release

### 5.1 Versioning

Semantic versioning for the application. Format versions (`fluut.project/1` etc.) and the
scripting API version follow [S 02 §2.0] and [S 09 §9.6] independently.

There is one version for the whole workspace: `[workspace.package] version` in the root
`Cargo.toml`, which every crate inherits. `fluut --version` prints it with the short git commit
(`fluut 0.1.0 (d4af3c037012)`), and so does the About dialog. `fluut_core::build_info` holds both;
its build script reads the commit from git, or from `FLUUT_GIT_COMMIT` when building from a
source archive. A release is a tag `vX.Y.Z`; the release workflow stops if the tag and the
workspace version differ.

### 5.2 Packaging and signing [S 06 §6.2]

| Platform | Artefact | Tooling |
|---|---|---|
| Windows | MSI + portable zip | `cargo-wix`; Authenticode signing through SignPath |
| macOS | universal DMG | `lipo`; ad-hoc `codesign` |
| Linux | AppImage, Flatpak | `appimagetool` (signed); Flathub manifest |

Built so far (`.github/workflows/release.yml`, `cargo xtask package`):

| Platform | Artefact | Notes |
|---|---|---|
| Windows x86-64 | `fluutcam-<v>-windows-x86_64.zip` | `fluutcam.exe`, `fluut.exe`, icon, licences. Executables signed through SignPath when its settings exist. No MSI yet |
| Linux x86-64 | `fluutcam-<v>-linux-x86_64.tar.gz` | `bin/`, `.desktop` file, hicolor icons, licences, built on Ubuntu 22.04 (glibc 2.35). No AppImage or Flatpak yet |
| macOS | `fluutcam-<v>-macos-universal.dmg`, `…-macos-universal-cli.tar.gz` | `lipo` of the arm64 and x86-64 builds in `FluutCAM.app`; ad-hoc signing (no Developer ID, by decision) |

The archives are written by `cargo xtask package` (sorted entries, fixed time stamp, no
owners), so they are reproducible and need no platform tools. Every signing and
verification step is skipped when its secrets are missing, so unsigned builds still work. The
workflow makes a draft release; a person publishes it.

App and installer icons (ICO, ICNS, PNG at the sizes each platform needs) are generated from the
SVG masters in `assets/` during packaging; generated rasters are not committed.
`cargo xtask icons <out-dir>` renders `fluut-D-icon-stock.svg` with `resvg` and writes
`windows/fluutcam.ico` (16–256 px), `macos/fluutcam.icns` (16–1024 px) and
`linux/hicolor/<N>x<N>/apps/fluutcam.png` (16–512 px). The window icon and the About logos are
rendered from the same SVGs by `fluut-app`'s build script. The `.exe` file icon is not set
yet.

**Windows version resource.** The build scripts of `fluut-cli` and `fluut-app` give both
executables a version resource through `winresource` (Windows hosts only): product name
`FluutCAM`, product and file version the workspace version, a file description and the
original file name. SignPath checks the product name and version before it signs, and the
release workflow checks them on the built files first.

**Signing** (decided 2026-11-01; public policy in [`docs/CODE_SIGNING.md`](../CODE_SIGNING.md)):

| What | How | Cost |
|---|---|---|
| Windows `.exe` | Authenticode through the SignPath Foundation open-source programme. `release.yml` uploads the unsigned executables as a GitHub artifact, submits it with `signpath/github-action-submit-signing-request@v3`, waits for an approver (up to 2 h) and takes the signed files back. The certificate's publisher is SignPath Foundation | free |
| macOS app and DMG | Ad hoc only. Apple has no free programme for open-source projects, so there is no Developer ID or notarisation. Users open the app once with right-click → Open ([`docs/install.md`](../install.md)). Homebrew's main cask repository will not take it | free |
| Every release file | GitHub build provenance attestation (`actions/attest-build-provenance@v2`, Sigstore keyless) in the `release` job | free |
| `releases.json`, `SHA256SUMS` | minisign | free |

The artifact configuration that SignPath applies is kept in
[`packaging/signpath/artifact-configuration.xml`](../../packaging/signpath/artifact-configuration.xml)
and pasted into the SignPath project, so changes to it are reviewed like code.

Repository settings for the release workflow (each group is optional; without it that step is
skipped):
- SignPath: secret `SIGNPATH_API_TOKEN` (a CI user with submitter rights); variables
  `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`, `SIGNPATH_SIGNING_POLICY_SLUG`
  (`test-signing` until the Foundation approves the project, then `release-signing`) and
  `SIGNPATH_ARTIFACT_CONFIGURATION_SLUG`.
- minisign: secrets `MINISIGN_SECRET_KEY`, `MINISIGN_PASSWORD`, `MINISIGN_PUBLIC_KEY`.

SignPath Foundation terms that shape the project (listed in full on signpath.org): an OSI
licence and no proprietary components; the project already released in the form to be
signed; builds from this repository on GitHub-hosted runners; build scripts reviewed as
source; every release approved by a person; MFA for every team member; a code signing policy
page linked from the home page; uninstall instructions; and no network transfer unless the
user asks, which is why the update check is opt-in [S 06 §6.4].

CI verifies every signature after signing; a failed check stops the release [S 06 §6.2].

### 5.3 Update manifest

`releases.json` with versions, URLs, SHA-256 and minimum OS, signed with minisign (Ed25519). The
public key is compiled into the app. Published with each release alongside checksums and a
CycloneDX SBOM (`cargo-cyclonedx`).

Format `fluut.releases/1`, written by `cargo xtask manifest`:

```json
{
  "format": "fluut.releases/1",
  "channel": "stable",
  "releases": [
    {
      "version": "0.1.0",
      "date": "2026-11-01",
      "artefacts": [
        { "platform": "linux-x86_64", "kind": "tar.gz",
          "name": "fluutcam-0.1.0-linux-x86_64.tar.gz",
          "url": "https://…/fluutcam-0.1.0-linux-x86_64.tar.gz",
          "sha256": "…", "size": 123456, "min_os": "glibc-2.35" }
      ]
    }
  ]
}
```

Releases are newest first; a new manifest is the old one (downloaded from the latest release)
plus the new release. `min_os` is `windows-10`, `macos-11.0` or `glibc-2.35`. The signature is
`releases.json.minisig`. The release workflow also signs `SHA256SUMS`. The update check itself
(HTTPS, signature and hash verification) is not written yet.

### 5.4 Reproducibility

Builds use pinned toolchain, `--locked`, `SOURCE_DATE_EPOCH`, and path remapping
(`--remap-path-prefix`). A second CI job rebuilds Linux artefacts and compares hashes.

`cargo xtask repro-check` builds `fluut` twice from scratch in the same directory with
`--locked`, `SOURCE_DATE_EPOCH` and the flags of `cargo xtask rustflags`, and compares SHA-256.
The release workflow sets `SOURCE_DATE_EPOCH` to the commit time and builds on a fixed runner
image. Archives made by `cargo xtask package` are byte-reproducible given the same binaries.
`ahash` (through `rhai`) draws constants at compile time with `const-random`; the builds set
`CONST_RANDOM_SEED` to a fixed value, or every build differs. On Windows two builds still differ in 20 bytes (the PE time stamp and
the PDB GUID, both derived from the PDB); the check is meant for Linux and has not yet run there.
The git commit in the binary is
the same for the same commit; a build from a source archive
has none unless `FLUUT_GIT_COMMIT` is set, so it differs.

## 6. Crash reporting [S 06 §6.4]

Panic hook and native crash handler write a minidump-free report (backtrace, versions, OS, last
log lines; no project data) to the data dir. On next start the user is asked whether to view
and send it. Sending goes to a project-run endpoint over HTTPS. Off by default.

Built so far, the local part only (`fluut_core::crash`): both programs install a panic hook
that writes `crash-<time>-<pid>.txt` (panic message and location, thread, backtrace, version
with commit, OS and architecture) to `<data dir>/crashes`. The data dir is `%LOCALAPPDATA%\FluutCAM`,
`~/Library/Application Support/FluutCAM` or `$XDG_DATA_HOME/fluutcam`; `FLUUT_DATA_DIR`
overrides it. On the next start `fluut` says that the last run crashed, where the report is,
and that nothing was sent, and when run in a terminal offers to print it; `fluutcam` writes the
same note to stderr. A report is then renamed to `….seen.txt`. **Nothing is sent**; there is no
endpoint, no opt-in setting and no sending code. Not built yet: the last log lines (the code
has no logging), a native crash handler, and the in-window dialog (it needs translated texts).

## 7. Open-source project setup

Public repository; contribution guide; code of conduct; issue templates for bugs, posts
(machine requests) and strategy requests; Developer Certificate of Origin sign-off for
contributions (keeps the MIT/Apache licence clean).

## 8. Challenges

- None new. CH-01 (determinism) and CH-07 (reference fixtures) apply.
