# FluutCAM — Detailed Design

Status: **first pass complete** (D01–D14 written, 2026-10-31). 23 challenges logged; the six needing product-owner
decisions are resolved, 17 are deferred to spikes or later features
(see [challenges.md](challenges.md#end-of-pass-review-2026-10-31)).

This folder holds the detailed design for FluutCAM. It turns the specification in
[`../spec/`](../spec/README.md) into module structure, Rust types and APIs, algorithms, and
test plans that an engineer can implement from.

## Relationship to the specification

- The spec says **what** FluutCAM does and the rules it must keep. This design says **how**.
- Every design document cites the spec sections it implements, as `[S 04 §4.12]`.
- If the design finds the spec wrong or incomplete, the problem is logged as a challenge
  ([challenges.md](challenges.md)), not silently fixed in the design.

## Conventions

- **Rust signatures** are design intent. Names may change during implementation; semantics
  should not.
- `f64` everywhere for geometry. Lengths are millimetres. Angles are radians in code and
  degrees at the UI and file boundaries.
- Errors: each crate has one `Error` enum (`thiserror`). Library crates never panic on bad input.
- Complexity notes use n = faces, m = samples, k = passes unless stated.
- Each document ends with **Tests** and **Challenges** sections.

## Challenges

Problems found while designing are recorded in [challenges.md](challenges.md) with an id
(`CH-nn`). Each challenge is either deferred (worked around in the design, revisited at the end)
or blocking (needs a decision before design can continue). Blocking challenges are raised with
the product owner straight away.

## Document map

| Doc | Title | Crates | Spec sections |
|---|---|---|---|
| [D01](D01-workspace-architecture.md) | Workspace and runtime architecture | all (adds `fluut-path`, `fluut-i18n`, `fluut-core`) | 01 |
| [D02](D02-geometry-core.md) | Geometry core: math, curves, surfaces | `fluut-geom` | 01 §1.4–1.5, 03 §3.2 |
| [D03](D03-topology-bvh.md) | B-rep topology and spatial index | `fluut-topo` | 03 §3.2 |
| [D04](D04-step-reader.md) | STEP reader | `fluut-io-step` | 02 §2.6.1 |
| [D05](D05-mesh-2d-import.md) | STL, DXF, SVG, bitmap import | `fluut-io-mesh`, `fluut-io-2d` | 02 §2.6 |
| [D06](D06-project-model.md) | Project model, persistence, undo | `fluut-model` | 02 §2.0–2.4, 03 |
| [D07](D07-cutter-queries.md) | Tool model and cutter-contact queries | `fluut-cam` | 04 §4.2, §4.14 |
| [D08](D08-toolpath-framework.md) | Toolpath framework | `fluut-cam` | 04 §4.3, §4.12, 03 §3.10 |
| [D09](D09-strategies.md) | v1 strategy designs | `fluut-cam` | 04 §4.4–4.8, §4.13 |
| [D10](D10-post-engine.md) | Post engine and importers | `fluut-post`, `fluut-post-import` | 05 |
| [D11](D11-simulation.md) | Simulation and time estimate | `fluut-sim` | 04 §4.10 |
| [D12](D12-scripting.md) | Scripting engine | `fluut-script` | 09 |
| [D12b](D12b-scripting-api.md) | Scripting API member list | `fluut-script` | 09 §9.4, §9.7 |
| [D13](D13-ui-architecture.md) | UI architecture | `fluut-app` | 08 |
| [D14](D14-build-test-release.md) | Build, test, release | CI | 01 §1.8, 06 |
| [D15](D15-toolpath-parameters.md) | Toolpath parameter definitions (common, `contour_2d`, `pocket_2d`, `parallel`) | `fluut-model`, `fluut-cam` | 02 §2.1, 03 §3.10, 04 §4.13.7 |
| [D15b](D15b-toolpath-parameters-2.md) | Toolpath parameter definitions, part 2 (the other 19 v1 types) | `fluut-model`, `fluut-cam` | 03 §3.10, 04 §4.13.7 |
| [D16](D16-tool-library-import.md) | Tool library import (Fusion JSON built; Vectric `.vtdb` designed) | `fluut-post-import`, `fluut-cli` | 02 §2.5, 05 §5.10.5 |
| [D17](D17-telemetry-and-channels.md) | Telemetry policy (recommendation, pending the maintainer) and release channels (stable, beta) | CI, `fluut-core::update` | 06 §6.4, §6.5 |
