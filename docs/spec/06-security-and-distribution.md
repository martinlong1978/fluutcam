# 06 — Security, Licensing and Distribution

## 6.1 Licence

| Item | Decision |
|---|---|
| FluutCAM licence | Open source, **MIT OR Apache-2.0** (dual, the Rust ecosystem default) |
| Licence enforcement | None. No licence keys, activation or online checks |
| Dependency licences | Permissive licences allowed (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode, BSL-1.0; OFL-1.1 for bundled fonts); MPL-2.0 allowed (file-level copyleft). GPL/LGPL/AGPL dependencies are not allowed in shipped binaries, so the dual permissive licence stays usable. Enforced by `cargo-deny` in CI. |
| Third-party definitions | Imported posts and tool libraries keep their own licences; FluutCAM ships none unless their licence allows it (02 §2.5) |

Consequence for the UI toolkit choice (08 §8.11): `slint` is offered under GPL-3, a
royalty-free licence with conditions, or a commercial licence; none keeps the shipped binary
open source and permissive. `egui` is MIT/Apache and was chosen (OQ-09).

## 6.2 Distribution and updates

| Platform | Package | Signing |
|---|---|---|
| Windows | MSI installer (and portable zip) | Authenticode through the SignPath Foundation open-source programme, verified in CI after signing |
| macOS | DMG, universal binary (arm64 + x86_64) | Ad hoc only: no Developer ID and no notarisation, because Apple has no free programme for open-source projects (decided 2026-11-01). Users open the app once with right-click → Open |
| Linux | AppImage and Flatpak | Flathub signing for Flathub installs; the Flatpak bundle, AppImage and tarball rely on the attestation |

Every release file also gets a GitHub build provenance attestation (Sigstore), and the
checksums are signed with minisign. The public signing rules, team roles and privacy statement
are in [`docs/CODE_SIGNING.md`](../CODE_SIGNING.md), as the SignPath Foundation terms require.

Updates:

- The app checks for updates over **HTTPS only**.
  The check is opt-in: the app asks on first start and the answer can be changed in settings.
  It sends no data beyond the request itself.
- The update manifest is signed (Ed25519, minisign). The app verifies the signature, with a public
  key compiled into the binary, before it reads the manifest. It never downloads or installs an
  update: it offers the version, the release notes and the download page, and the user downloads
  the file. A helper checks a downloaded package against the hash in the manifest (D14 §5.3).
- A check the user starts ("Check now", `fluut update-check`) is allowed whatever the setting is;
  the automatic check runs at most once a day.
- Installation always needs user confirmation. No silent updates.
- Release builds SHOULD be reproducible, and each release publishes an SBOM (CycloneDX) and
  checksums.

CI verifies every signed artefact; a release with a failed signature check is not published.

## 6.3 Hardening

| Threat | FluutCAM requirement |
|---|---|
| Memory-safety exploits | Rust memory safety for first-party code (`forbid(unsafe_code)`, 01 §1.8.1). Windows builds enable CFG (`-C control-flow-guard`); all platforms build with ASLR/PIE and DEP/NX. |
| User-writable post templates inject G-code | Built-in definitions read-only and hash-checked; diff shown when a user or imported post changed; provenance header in every output (05 §5.9). |
| Outdated or vulnerable dependencies | `cargo-deny` (licences, advisories, duplicate versions) and `cargo-audit` in CI; dependency updates reviewed each release. |
| Attack surface: parsers of untrusted files | Every parser returns errors and never panics; fuzz targets for STEP, STL, DXF, SVG, bitmap, TOML definitions, Vectric/DeskProto imports, scripts (01 §1.8.1). |
| Scripts | Sandboxed `rhai` (09 §9.3). |
| Hosted Fusion posts (later) | Sandboxed JavaScript (05 §5.10.3). |

## 6.4 Privacy

| Item | Decision |
|---|---|
| Crash reports | Opt-in, off by default. After a crash, the user is asked and can view the full report before sending. Reports contain no geometry or project data unless the user attaches it. |
| Telemetry / usage analytics | **Still open.** If any is added it MUST be opt-in, documented, and viewable. |
| Update check | Opt-in (asked on first start, "No" if the question is dismissed; stored in the user settings); sends nothing beyond the two requests for the signed release list and its signature (fixed User-Agent, no cookies). Opt-in so that the code-signing privacy statement holds: the program transfers nothing unless the user asks (`docs/CODE_SIGNING.md`). |
| Network use | No other network access, except actions the user starts (e.g. opening online help). |

## 6.5 Still open

| Item |
|---|
| Telemetry policy (6.4) |
| Release channels (stable / beta) and update cadence |
