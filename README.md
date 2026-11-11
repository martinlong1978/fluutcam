<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/fluutcam-D-logo-dark.svg">
    <img src="assets/fluutcam-D-logo.svg" alt="FluutCAM" width="440">
  </picture>
</p>

# FluutCAM

Open-source CAM software. Exact B-rep geometry core, layered post-processors, written in Rust.

- Specification: [`docs/spec/`](docs/spec/README.md)
- Detailed design: [`docs/design/`](docs/design/README.md)
- Implementation roadmap: [`docs/plan/`](docs/plan/README.md)
- Brand assets: [`assets/`](assets/README.md)

## Building

Requires the Rust toolchain pinned in `rust-toolchain.toml` (installed automatically by
`rustup`). On Windows, the MSVC C++ build tools are needed for linking.

```
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask check-layers
```

## Download

Releases are on the [releases page](https://github.com/martinlong1978/fluutcam/releases).
Windows: an MSI installer (per user) or a zip. macOS: a disk image. Linux: an AppImage, a Flatpak
bundle or a tarball. [Installing](docs/install.md) covers each platform, including how to open the
unnotarised macOS app and how to uninstall.

### Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/). Every release file also carries a GitHub build
provenance attestation. Who approves signatures, what is signed and how to check a download:
[Code signing policy](docs/CODE_SIGNING.md).

### Privacy

FluutCAM sends nothing over the network unless you ask it to. The only network use is the
update check, which is off until you turn it on. See the [Privacy policy](PRIVACY.md).

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT licence](LICENSE-MIT) at your option.

## Contributing

Contributions are welcome. Unless you state otherwise, a contribution you submit is licensed
under the same dual licence, with no additional terms. Each commit needs a `Signed-off-by`
line (Developer Certificate of Origin). Details are in [CONTRIBUTING.md](CONTRIBUTING.md).
