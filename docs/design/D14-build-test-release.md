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
└─ packaging/                 WiX, macOS bundle/notarization, AppImage, Flatpak manifests
```

Every source file carries `SPDX-License-Identifier: MIT OR Apache-2.0`.

## 2. Build configuration

- Workspace lints: `unsafe_code = "forbid"` (exceptions per crate with a comment, S 01 §1.8.1),
  `clippy::all` and selected pedantic lints as warnings, CI with `-D warnings`.
- Profiles: `release` with `lto = "thin"`, `codegen-units = 1`, `panic = "unwind"` (crash
  reporter needs unwinding to capture state); `bench` inherits release with debug info.
- Windows: `-C control-flow-guard` [S 06 §6.3]. All platforms: PIE/ASLR, NX defaults.
- Determinism: no `-C target-cpu=native` in release builds (CH-01).

## 3. CI pipeline (GitHub Actions)

| Job | Platforms | Runs |
|---|---|---|
| lint | Linux | `cargo fmt --check`, `clippy -D warnings`, `xtask check-layers`, `cargo deny check` |
| test | Windows, macOS (arm64), Linux | `cargo nextest run` (unit, property, integration), golden posts, D08 endpoint/stepover/invariance tests |
| determinism | all three | calculate reference projects with 1 and N threads; result hashes must be identical across all platforms (CH-01, required) |
| bench | Linux (dedicated runner) | `criterion` targets of D02 §8, D07 §8, D11 §7; fail on > 10 % regression |
| fuzz | Linux, nightly toolchain | each fuzz target 10 min nightly; corpus cached |
| audit | Linux, daily | `cargo audit` |
| ui | Linux (headless GPU via lavapipe) | view-model tests, screenshot tests (D13 §10) |

Pull requests run lint + test + determinism; nightly runs everything.

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

### 5.2 Packaging and signing [S 06 §6.2]

| Platform | Artefact | Tooling |
|---|---|---|
| Windows | MSI + portable zip | `cargo-wix`; Authenticode signing (signing service or HSM-backed certificate) |
| macOS | universal DMG | `lipo`; `codesign` + `notarytool` |
| Linux | AppImage, Flatpak | `appimagetool` (signed); Flathub manifest |

CI verifies every signature after signing; a failed check stops the release [S 06 §6.2].

### 5.3 Update manifest

`releases.json` with versions, URLs, SHA-256 and minimum OS, signed with minisign (Ed25519). The
public key is compiled into the app. Published with each release alongside checksums and a
CycloneDX SBOM (`cargo-cyclonedx`).

### 5.4 Reproducibility

Builds use pinned toolchain, `--locked`, `SOURCE_DATE_EPOCH`, and path remapping
(`--remap-path-prefix`). A second CI job rebuilds Linux artefacts and compares hashes.

## 6. Crash reporting [S 06 §6.4]

Panic hook and native crash handler write a minidump-free report (backtrace, versions, OS, last
log lines; no project data) to the data dir. On next start the user is asked whether to view
and send it. Sending goes to a project-run endpoint over HTTPS. Off by default.

## 7. Open-source project setup

Public repository; contribution guide; code of conduct; issue templates for bugs, posts
(machine requests) and strategy requests; Developer Certificate of Origin sign-off for
contributions (keeps the MIT/Apache licence clean).

## 8. Challenges

- None new. CH-01 (determinism) and CH-07 (reference fixtures) apply.
