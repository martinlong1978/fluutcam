# FluutCAM

Open-source CAM software. Exact B-rep geometry core, layered post-processors, written in Rust.

- Specification: [`docs/spec/`](docs/spec/README.md)
- Detailed design: [`docs/design/`](docs/design/README.md)
- Implementation roadmap: [`docs/plan/`](docs/plan/README.md)

## Building

Requires the Rust toolchain pinned in `rust-toolchain.toml` (installed automatically by
`rustup`). On Windows, the MSVC C++ build tools are needed for linking.

```
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask check-layers
```

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT licence](LICENSE-MIT) at your option.
