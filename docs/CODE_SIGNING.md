# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/).

This page says what FluutCAM release files are signed, who may approve a signature, and how
you can check a download.

## What is signed

| File | Signature |
|---|---|
| `fluutcam.exe`, `fluut.exe` (Windows) | Authenticode, through SignPath. The publisher shown by Windows is SignPath Foundation |
| Every release file | GitHub build provenance attestation (Sigstore), made by the release workflow |
| `releases.json`, `SHA256SUMS` | minisign (Ed25519); the app checks `releases.json` before it offers an update |
| macOS app and disk image | Ad hoc only; see [macOS](#macos) |
| Linux tarball | No platform signature; use the attestation or the checksums |

Only files built by the release workflow ([`.github/workflows/release.yml`](../.github/workflows/release.yml))
from a tagged commit of this repository, on GitHub-hosted runners, are signed. FluutCAM has no
proprietary components. Third-party Rust crates are compiled from source into the two
executables; their licences are checked by `cargo deny` ([`deny.toml`](../deny.toml)) and
listed in the SBOM of each release.

## Team roles

| Role | Who | Trusted to |
|---|---|---|
| Authors | [Repository members](https://github.com/martinlong1978/fluutcam/graphs/contributors) with write access: [@martinlong1978](https://github.com/martinlong1978) | change the repository without further review |
| Reviewers | [@martinlong1978](https://github.com/martinlong1978) | review every change from other contributors (pull requests) before it is merged |
| Approvers | [@martinlong1978](https://github.com/martinlong1978) | approve each signing request in SignPath, after checking that it comes from a release tag |

Every team member uses multi-factor authentication on GitHub and on SignPath. The build
scripts and workflows are source code: changes to them are reviewed like any other change.

## Release process

1. A release is a tag `vX.Y.Z` on `main`. The tag must equal the workspace version.
2. The release workflow builds the executables and checks their version resource (product
   name `FluutCAM`, product version equal to the tag).
3. It sends the Windows executables to SignPath. An approver checks the request and approves
   it; SignPath signs them with the SignPath Foundation certificate.
4. The workflow verifies the signatures, packages every platform, writes checksums, the SBOM
   and the update manifest, signs the manifest and checksums with minisign, and attests every
   file.
5. The result is a draft GitHub release. A person publishes it after looking at it.

## Checking a download

- **Attestation** (any file), with the [GitHub CLI](https://cli.github.com/):

  ```bash
  gh attestation verify fluutcam-0.1.0-windows-x86_64.zip --repo martinlong1978/fluutcam
  ```

- **Checksums:** `sha256sum -c SHA256SUMS --ignore-missing`. To check `SHA256SUMS` itself:

  ```bash
  minisign -Vm SHA256SUMS -P <public key from the release notes>
  ```

- **Windows signature:** file Properties → Digital Signatures, or
  `Get-AuthenticodeSignature fluutcam.exe` in PowerShell.

## macOS

FluutCAM is not signed with an Apple Developer ID and is not notarised. Apple has no free
signing programme for open-source projects. The app is signed ad hoc, which Apple Silicon
needs to run it, so Gatekeeper warns the first time it opens. See [Installing](install.md#macos)
for how to open it. Check the download with its attestation or checksum first.

## Privacy

This program will not transfer any information to other networked systems unless
specifically requested by the user or the person installing or operating it.

The update check is off unless you turn it on; the app asks on first start. When on, it downloads the signed release list
from GitHub and sends nothing beyond that request. Crash reports are written to local files
only; nothing is sent ([spec 06 §6.4](spec/06-security-and-distribution.md)).

## Reporting misuse

If you find a signed file that was not built from this repository, or that does something
this page does not allow, open an issue or contact the maintainer through GitHub. We will help
SignPath investigate.
