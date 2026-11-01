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
