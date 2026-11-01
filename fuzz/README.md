# Fuzz targets

`cargo-fuzz` targets for the STEP, DXF, SVG and STL readers (design D04 §10,
D05, spec 06 §6.3). This package is not a member of the FluutCAM workspace,
like `spikes/`; it needs a nightly toolchain and `cargo install cargo-fuzz`.

| Target | Input |
|---|---|
| `parse` | any bytes into `fluut_io_step::parse` |
| `map` | bytes that parse, into `schema`, `census` and `map_bodies` (including p-curves) |
| `parse_parallel` | first byte: chunk size 1–256; the rest into `parse_chunked` and `parse_serial`, which must agree (model, or error and position) |
| `dxf` | any bytes into `fluut_io_2d::read_dxf`, with small curve and entity limits |
| `svg` | any bytes into `fluut_io_2d::read_svg`, with small curve and element limits |
| `stl` | any bytes into `fluut_io_mesh::read_stl`, welding and adjacency included |

Run from this directory, seeded with the small files in `seeds/`. The STEP
seeds are the unit-test fixtures (a cube, a cylinder with and without
p-curves, a square with p-curves):

```bash
mkdir -p corpus/parse && cp seeds/*.stp corpus/parse/
cargo +nightly fuzz run -O parse corpus/parse -- -dict=step.dict -max_total_time=600 -timeout=10 -rss_limit_mb=2048 -max_len=16384
```

The same for `map`. Seeds for `parse_parallel` need a leading chunk-size
byte:

```bash
mkdir -p corpus/parse_parallel
for f in seeds/*.stp; do (printf '\x07'; cat "$f") > "corpus/parse_parallel/$(basename "$f")"; done
```

The 2-D and mesh seeds are hand-written files in
`seeds/dxf`, `seeds/svg` and `seeds/stl`, which reach every entity and element
handler. Their tests (`crates/fluut-io-2d/tests/seeds.rs`,
`crates/fluut-io-mesh/tests/seeds.rs`) check that they still read cleanly:

```bash
mkdir -p corpus/dxf && cp seeds/dxf/* corpus/dxf/
cargo +nightly fuzz run -O -a dxf corpus/dxf -- -dict=dxf.dict -max_total_time=600 -timeout=10 -rss_limit_mb=2048 -max_len=16384
```

The same for `svg` (with `svg.dict`) and `stl` (no dictionary). The corpora and crash artefacts stay out of git; every
crash found becomes a regression test next to the code it broke.
