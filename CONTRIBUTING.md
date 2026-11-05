# Contributing to FluutCAM

Thank you for helping. This page covers the terms for contributions and the checks a change
must pass.

## Licence of contributions

FluutCAM is licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the
[MIT licence](LICENSE-MIT), at the user's option. This includes the brand assets in `assets/`.

Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion
in FluutCAM, as defined in the Apache-2.0 licence, is dual licensed as above, without any
additional terms or conditions.

This means your contribution also carries the Apache-2.0 patent licence (section 3) for any
patent claims of yours that it infringes.

## Developer Certificate of Origin

Every commit in a pull request must carry a `Signed-off-by` line. With it you certify the
[Developer Certificate of Origin 1.1](https://developercertificate.org/) for that commit. In
short:
- you wrote the change, or have the right to submit it under the project's licence;
- you understand that the contribution and your sign-off are public and kept permanently.

Add the line with `git commit -s`. It must use your real name and an email address you can be
reached at:

```
Signed-off-by: Jane Doe <jane@example.com>
```

To add it to commits you have already made on your branch:

```bash
git rebase --signoff origin/main
```

CI checks every commit of a pull request for the line (the `dco` job in
`.github/workflows/ci.yml`).

## What not to contribute

- Code, data or documentation copied or derived from other CAM products, their file formats'
  private specifications, or their decompiled binaries. FluutCAM is a clean implementation. An
  importer may read another product's files from published or independently observed format
  information only.
- Files whose licence is not compatible with MIT OR Apache-2.0. Dependencies must pass
  `cargo deny check` ([`deny.toml`](deny.toml), spec 06 §6.1).
- Test files from third parties, unless their terms allow redistribution. Large test corpora
  stay outside the repository.

## Checks

Every commit must pass:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask check-layers
cargo xtask wiki-reference --check
cargo deny check
```

Results must be the same on Windows, macOS and Linux:
- use the `libm` crate for transcendental functions;
- never let `HashMap` iteration order reach an output; use `BTreeMap`.

Design D01 §6 has the details.

## How the work is organised

- [`docs/spec/`](docs/spec/README.md): what FluutCAM does.
- [`docs/design/`](docs/design/README.md): how it is built.
- [`docs/plan/`](docs/plan/README.md): milestones and plans. Larger changes start as a plan in
  `docs/plan/`, in the format of the existing plans.

Write tests first, match the style of the surrounding code, and write docs in plain, concise
English.

## Keep the wiki current

The user documentation is a GitHub wiki, written in [`docs/wiki/`](docs/wiki/README.md) and
copied to the wiki by a workflow when a change reaches `main`. Edit the pages there, never in the
wiki's web editor. A change that alters what a user sees or does updates the matching pages in the
same pull request:

- a changed or new window, menu or command: edit the how-to and reference pages that describe
  it, and the test steps in the `UAT-*` pages;
- run `cargo xtask screenshots` and commit the new pictures in `docs/wiki/images/` when the
  window looks different (each shot is one row in
  `crates/fluut-app/examples/screenshots.rs`; add a row for a new window);
- run `cargo xtask wiki-reference` when a toolpath setting or a diagnostic changes, and commit
  the result (`cargo xtask wiki-reference --check` fails in CI when a generated page is out of
  date). A new setting needs a line in `docs/wiki/reference/help.toml`; a new diagnostic can get
  advice in `docs/wiki/reference/advice.toml`;
- a feature that is planned but not built is "not yet" in the pages, and goes on the
  [Known limits](docs/wiki/Known-Limits.md) page.
