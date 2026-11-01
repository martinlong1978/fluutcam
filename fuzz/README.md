# Fuzz targets

`cargo-fuzz` targets for the STEP reader (design D04 §10). This package is not
a member of the FluutCAM workspace, like `spikes/`; it needs a nightly
toolchain and `cargo install cargo-fuzz`.

| Target | Input |
|---|---|
| `parse` | any bytes into `fluut_io_step::parse` |
| `map` | bytes that parse, into `schema`, `census` and `map_bodies` (including p-curves) |

Run from this directory, seeded with the small files in `seeds/` (the unit-test
fixtures: a cube, a cylinder with and without p-curves, a square with p-curves):

```bash
mkdir -p corpus/parse && cp seeds/* corpus/parse/
cargo +nightly fuzz run -O parse corpus/parse -- -dict=step.dict -max_total_time=600 -timeout=10 -rss_limit_mb=2048 -max_len=16384
```

The same for `map`. The corpora and crash artefacts stay out of git; every
crash found becomes a regression test next to the code it broke.
