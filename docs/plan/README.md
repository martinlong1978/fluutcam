# FluutCAM — Implementation Roadmap

Status: started 2026-10-31. Implements the design in [`../design/`](../design/README.md), which
implements the spec in [`../spec/`](../spec/README.md).

## Where the code lives

The Cargo workspace is at the repository root ([`../../`](../../README.md)), laid out as [D14 §1].
Specification, design and plans are in `docs/`, so they change together with the code.

## Principles

- **Bottom-up, then thin vertical slice.** Build the crates nothing else depends on first
  (`fluut-geom`, `fluut-path`). Then get one toolpath from a STEP file to G-code end to end,
  before widening any layer.
- **Spikes before commitments.** Each high-risk area in [challenges.md](../design/challenges.md)
  gets a time-boxed spike with a written exit criterion. A spike ends with a short report in
  `plan/spikes/` and, where needed, a change to the design. Spike code may be thrown away.
- **TDD and golden files.** Every crate has unit and property tests from its first commit.
  Posts, toolpaths and simulation results are pinned by golden files ([D14 §4]).
- **Determinism from day one.** `libm` only for transcendental functions, no `HashMap`
  iteration in outputs ([D01 §6]). CI checks it once there is something to check (M2).

## Milestones

| # | Milestone | Crates / work | Exit criterion |
|---|---|---|---|
| M0 | Workspace and geometry foundations | workspace, CI, lints, `xtask check-layers`; `fluut-geom` basic types, tolerances, root finders, analytic curves, B-spline curves | `cargo test` and `clippy -D warnings` green on Windows; plan [m0](2026-10-31-m0-workspace-geom.md) done |
| M1 | Geometry complete + path types | `fluut-geom` surfaces, offsets, closest point, plane sections, biarcs; `fluut-path` segment types and invariants | D02 §9 property tests pass; D02 §8 benchmarks measured |
| M2 | Post engine (parallel with M1) | `fluut-post`: resolver merge, templates, expressions, emitter, formatting, arc correction; reference suite; built-in `fluut/gcode`, `fluut/grbl`, `fluut/linuxcnc` | golden G-code for the reference suite ([D10 §8–9]) |
| S1 | **Spike:** kernel + STEP | `fluut-topo` arena + BVH; Part 21 parser; `truck` evaluation (OQ-01) | CAx-IF corpus: % loaded, % valid shells; decision own vs `truck` recorded; CH-03/04/08/12 updated |
| S2 | **Spike:** cutter queries | D07 exact drop with branch-and-bound on S1 models | drop query time per point on a 5 000-face model; decision that "no grids" holds or needs a fallback |
| S3 | **Spike:** 2-D geometry | `cavalier_contours` wrapper; adaptive clearing without a grid (CH-16); medial axis (CH-17) | working adaptive pocket on 3 test shapes; CH-13/16–19 updated |
| S4 | **Spike:** UI toolkit | egui vs slint prototype: viewport (`wgpu`), ribbon, tree, properties panel (OQ-09) | decision recorded with frame times and code size; slint licence checked |
| S5 | **Spike:** simulation | tri-dexel prototype (CH-22) | memory and update time on the reference rotary and relief projects |
| M3 | Vertical slice | `fluut-model` (minimal project), `fluut-core` jobs, `fluut-cam` framework + 3 strategies (`contour_2d`, `pocket_2d`, `parallel`), `fluut-cli` `calc` and `post` | STEP file + TOML project → G-code from the CLI; translation-invariance test passes |
| M4 | Model and persistence | `fluut-model` full: definitions, layered resolution, `.fluut` save/load, undo, journal, cache sidecar | D06 tests; round-trip of reference projects |
| M5 | UI v1 shell | `fluut-app` on chosen toolkit: viewport, Setup/Toolpath tree, properties, job progress | open, calculate, post a reference project from the UI |
| M6 | Strategy catalogue | remaining v1 types of [S 04 §4.13] (22 in all); roughing with in-process stock; holder collisions | D09 tests; golden toolpath summaries for each type |
| M7 | Importers, simulation, scripting | `fluut-io-2d`, `fluut-io-mesh`, `fluut-post-import` (needs CH-21 samples), `fluut-sim`, `fluut-script` | D05/D10 §6/D11/D12 tests |
| M8 | Release engineering | signing, installers, update manifest, crash reports, docs | signed builds on 3 platforms ([D14 §5]) |

M1 and M2 can run in parallel. S1–S5 can start as soon as M0 is done. S2 needs S1.

## Design work still owed

These must be written before the milestone that needs them:

| Item | Needed by |
|---|---|
| Per-type toolpath parameter definitions (fields, units, ranges, defaults, validation) for all 22 v1 types | M3 (first 3 types), M6 (rest) |
| Scripting API member list | M7 |
| Tool-library field mapping (Vectric/Fusion/DeskProto) | M7 |
| Vectric unsupported-feature list | M7 (after CH-21 samples) |
| Telemetry policy, release channels | M8 |

## Plans

| Plan | Milestone | Status |
|---|---|---|
| [2026-10-31-m0-workspace-geom.md](2026-10-31-m0-workspace-geom.md) | M0 | done; CI not yet run on macOS/Linux |
| [2026-10-31-m1-geometry-path.md](2026-10-31-m1-geometry-path.md) | M1 | Done (B-spline closest point 20× over its benchmark target: CH-25) |
| [2026-10-31-m2-post-engine.md](2026-10-31-m2-post-engine.md) | M2 | Tasks 1–2 done; 3–9 to do |
