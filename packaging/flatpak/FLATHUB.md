# Submitting FluutCAM to Flathub

This lists what the maintainer does to put FluutCAM on Flathub. The submission
itself (a pull request to `flathub/flathub`) is the maintainer's action; nothing
in this repository does it. Background: the "Flathub source build" section of
[`docs/plan/2026-11-11-flatpak.md`](../../docs/plan/2026-11-11-flatpak.md),
design D14 section 5.2 and D17 section 3.5.

## What is in this folder

| File | Use |
|---|---|
| `io.github.martinlong1978.FluutCAM.flathub.yml` | The **Flathub** manifest: builds from source, offline, on `org.freedesktop.Sdk.Extension.rust-stable` (runtime 24.08) |
| `cargo-sources.json` | The vendored crates for it, made from `Cargo.lock` |
| `update-cargo-sources.sh` | Regenerates `cargo-sources.json`; `--check` fails if it is out of date (CI: job `flatpak-sources` in `ci.yml`, and `flathub-source` in `packaging.yml`) |
| `stage-source-build.sh` | Makes the dry-run copy of the manifest for CI, with the `git` source replaced by the checkout |
| `io.github.martinlong1978.FluutCAM.yml` | The *binary* manifest, for our own `.flatpak` bundle in GitHub releases. Not for Flathub |
| `.desktop`, `.metainfo.xml`, `validate.sh`, `smoke-test.sh` | Shared by both |

The generator is `flatpak-cargo-generator.py` from `flatpak/flatpak-builder-tools`,
pinned in `update-cargo-sources.sh` to commit
`41c20aa10819cdb2a4f3ca171758a96d1955c018` (SHA-256 of the file
`0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380`). The script
downloads that one file into a scratch folder, checks the hash and runs it with
`python -I` in a virtualenv. To move to a newer generator, change both values in
the script together, run it and review the diff of `cargo-sources.json` (it should
not change for the same `Cargo.lock`); then update the commit above.

## Before the first submission

1. **Check the dry run.** Run the *Packaging* workflow by hand (Actions, Packaging,
   Run workflow) on the branch you will release from. The `flathub-source` job
   must pass: it builds the source manifest in the Flathub container, validates the
   launcher and metainfo, runs `flatpak-builder-lint` on the manifest and the
   built repository, and runs the smoke test (no network, no filesystem
   permission). Fix what it reports first.
2. **Make a release that contains these files.** The Flathub manifest builds a
   *tag*, and the tag must hold `packaging/flatpak/` (the launcher and metainfo
   are installed from the checkout), so `v0.1.0`, made before this folder had the
   source manifest, cannot be used. Release as D14 and D17 describe: set the
   workspace version, add a `<release version=... date=...>` entry to the metainfo
   (with the real date; 0.1.0 has a placeholder), commit, tag `vX.Y.Z`, push the tag.
3. **Pin the manifest.** In `io.github.martinlong1978.FluutCAM.flathub.yml`, between
   the `release-source` markers, replace the placeholders:
   - `tag:` the release tag;
   - `commit:` the commit the tag points to (`git rev-parse vX.Y.Z^{commit}`: the
     commit, not the tag object).

   Keep the two marker comments: CI needs them. Commit this to `main` and run the
   dry run once more.
4. **Regenerate the crates** if `Cargo.lock` differs from the one the file was made
   from: `packaging/flatpak/update-cargo-sources.sh`, and commit. Flathub builds the
   `Cargo.lock` of the pinned commit, so the file must match *that* lock; the
   `--check` in CI proves it for `main`, so the pinned commit should be on `main`
   with nothing after it that changes `Cargo.lock`, or re-check at the tag.
5. **Screenshots must resolve.** The metainfo points at
   `raw.githubusercontent.com/martinlong1978/fluutcam/main/docs/wiki/images/...`;
   Flathub checks them. Make sure the repository is public and the images exist at
   those addresses.

## The submission (maintainer)

Follow <https://docs.flathub.org/docs/for-app-authors/submission>:

1. Fork `flathub/flathub`, and create a branch from `new-pr` (not `master`).
2. Add to it, in the top folder: the manifest as
   `io.github.martinlong1978.FluutCAM.yml` (a copy of the `.flathub.yml` file; the
   marker comments may stay or go) and `cargo-sources.json` beside it. Nothing else
   is needed: the launcher, metainfo and icons come from the tagged source.
3. Open the pull request against `new-pr`, titled `Add io.github.martinlong1978.FluutCAM`,
   and fill in the checklist. The Flathub bot builds it and posts a test build and
   lint results; answer the reviewers there.
4. Flathub then makes the app repository `flathub/io.github.martinlong1978.FluutCAM`;
   accept the invitation. The app id ties to the GitHub account `martinlong1978`,
   which Flathub verifies.

## After it is accepted

- **A new stable release:** in the Flathub app repository change `tag` and `commit`
  in the manifest, replace `cargo-sources.json` with a fresh one for that release's
  `Cargo.lock`, and open a pull request there.
- **Pre-releases (`-beta.N`, `-rc.N`):** push them to a `beta` branch of the
  Flathub app repository, which Flathub publishes as the `beta` Flatpak branch
  (users add the `flathub-beta` remote). The default branch carries stable releases
  only ([D17](../../docs/design/D17-telemetry-and-channels.md) section 3.5). A
  stable build is a new commit and tag, not the beta renamed.
- Every release needs its `<release>` entry in the metainfo.
- A new permission (for example `--share=network`) must go in both manifests and
  in `PRIVACY.md`, and be justified to the Flathub reviewers.

## Notes

- The Rust compiler is the SDK extension's, not `rust-toolchain.toml` (there is no
  rustup in the sandbox). The extension tracks the latest stable release; the
  workspace needs 1.99 or later (`rust-version`). If the extension were older than
  that, the build fails with cargo's message.
- The build runs the `icons` command of `xtask` (`cargo run -p xtask -- icons icons`)
  to render the hicolor icons from `assets/`, as the tarball build does.
- Flatpak-builder fetches the crates from `static.crates.io` with the SHA-256 from
  `Cargo.lock`. `Cargo.lock` has no git dependencies; the one patched crate,
  `third_party/cavalier_contours`, is a path dependency inside the repository.
