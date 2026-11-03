# MSI and AppImage Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Add the Windows installer (MSI) and the Linux AppImage to the release, as [D14 §5.2](../design/D14-build-test-release.md) plans. Today a release has a Windows zip, a Linux tarball and a macOS DMG. Both new files must pass through the existing checksum, minisign, attestation and `releases.json` steps, and the workflow must still make unsigned packages when the SignPath settings are missing.

**Architecture:** The WiX source is plain WiX v5 (`packaging/wix/fluutcam.wxs`) built with the `wix` .NET tool pinned to 5.0.2 by `packaging/wix/build-msi.ps1`; `cargo-wix` is not used (it targets WiX 3 and a generated template, and a hand-written source is easier to review). The AppImage is assembled from the Linux tarball by `packaging/appimage/build-appimage.sh` and packed with `appimagetool`, which `packaging/appimage/fetch-tools.sh` downloads at a pinned release and checks against a SHA-256 written in the script (the tool and the type2 runtime, so that `appimagetool` downloads nothing itself). `release.yml` gets two jobs, `package-msi` and `package-appimage`; `packaging.yml` runs the same scripts on pull requests that touch packaging, without signing. `releases.json` needs no change: `cargo xtask manifest` already knows the kinds `msi` and `AppImage` and lists every file of `dist/`.

**Tech Stack:** WiX Toolset 5.0.2 (`wix` .NET tool), appimagetool 1.9.1, type2 runtime 20251108, SignPath GitHub action v3 (already used).

**Design:** [`../design/D14-build-test-release.md`](../design/D14-build-test-release.md) §5.2; [`../CODE_SIGNING.md`](../CODE_SIGNING.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- The MSI must contain the signed executables, and itself be signed. Without SignPath settings the MSI is built unsigned from unsigned executables.
- The MSI never creates, lists or removes `%LOCALAPPDATA%\FluutCAM` (settings, crash reports). It installs to `%LOCALAPPDATA%\Programs\FluutCAM`.
- The `UpgradeCode` and the component GUIDs never change.
- No unpinned download in the workflows: each tool has an exact version and a SHA-256 (appimagetool, runtime), or an exact package version (`wix`, from nuget.org).
- No secret goes to a step that builds code.

## Decisions

1. **Signing path: two SignPath requests.** SignPath can sign files inside an MSI (nested `pe-file` in `msi-file`), but that repacks the MSI and needs the paths of the files as SignPath sees them. Instead the executables are signed first (existing request, existing configuration), the MSI is built from the signed files, and the MSI is submitted again with a second artifact configuration (`artifact-configuration-msi.xml`, repository variable `SIGNPATH_MSI_ARTIFACT_CONFIGURATION_SLUG`). Each request needs one approval. After signing, CI checks the MSI and, by an administrative install, the executables in it.
2. **Per-user install, no wizard.** No administrator prompt and no UI extension to download. `msiexec /i … ADDTOPATH=1` adds the folder to the user's `PATH`. The licences are installed as files, not shown in a dialog (that would need an RTF and the UI extension).
3. **Upgrades.** One `UpgradeCode`; `MajorUpgrade` with same-version upgrades allowed, because the pre-release suffix is dropped from the MSI version (`1.2.3-rc.1` becomes `1.2.3`).
4. **AppImage from the tarball.** AppRun starts `fluutcam`, or `fluut` when the AppImage is started through a link named `fluut`. No zsync update information: it needs `zsyncmake` and a second file, and the app does not install updates anyway.
5. **Not reproducible:** an MSI holds time stamps and identifiers; the AppImage honours `SOURCE_DATE_EPOCH`, but squashfs and the runtime are not compared bit for bit. The zip and tarball stay the reproducible files.

## Tasks

### Task 1: WiX source and build script
**Files:** `packaging/wix/fluutcam.wxs`, `packaging/wix/build-msi.ps1`.
- [x] Source with a stable UpgradeCode, per-user scope, Start menu shortcut, icon, licence files, optional PATH.
- [x] Script: numeric version, pinned `wix`, input checks, ICE validation.
- [x] Build and validate locally (WiX 5.0.2 on Windows).

### Task 2: AppImage scripts
**Files:** `packaging/appimage/build-appimage.sh`, `packaging/appimage/fetch-tools.sh`.
- [x] Assemble the AppDir from a real `cargo xtask package` tarball (checked locally).
- [x] Pins checked by downloading the two files and running `sha256sum -c`.

### Task 3: Workflows
**Files:** `.github/workflows/release.yml`, `.github/workflows/packaging.yml`, `packaging/signpath/artifact-configuration-msi.xml`.
- [x] `package-msi`, `package-appimage`, `check` output `sign_windows_msi`; `release` needs both.
- [x] Dry-run workflow.
- [x] `actionlint` clean.

### Task 4: Documents
**Files:** `docs/design/D14-build-test-release.md`, `docs/install.md`, `docs/CODE_SIGNING.md`, `README.md`.
- [x] Tables, install and uninstall texts, settings list.

## Review Focus

1. **MSI contents:** the files in the MSI come from `bin-windows-x86_64`, which holds the signed executables when SignPath is on.
2. **Uninstall:** nothing outside `%LOCALAPPDATA%\Programs\FluutCAM`, the Start menu folder and the optional PATH entry is touched.
3. **Pins:** version and hash change together.

## Not verified locally

GitHub Actions itself, the SignPath requests (including that `msi-file` with a wildcard `path` matches), the signature checks, the AppImage build and run (needs Linux), installing and uninstalling the MSI (not run on the development machine).
