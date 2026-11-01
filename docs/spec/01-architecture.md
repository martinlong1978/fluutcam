# 01 — Architecture

## 1.1 Scope of this section

This section defines the subsystem structure of FluutCAM: an exact-geometry core rather than a
mesh-and-height-field core. FluutCAM is written in Rust (§1.8).

## 1.2 Core principle — exact geometry is authoritative

CAM systems that machine a triangle mesh by first converting it into height-field grids lose
precision at import: a cylinder becomes an N-sided prism, and every later stage inherits that
error.

FluutCAM MUST hold imported geometry in a **boundary representation (B-rep)** whose entity
set maps closely to STEP (ISO 10303) geometry and topology ([03 §3.2](03-data-model.md)).
Toolpath generation MUST query this exact geometry. Analytic surfaces (plane, cylinder, cone,
sphere, torus) MUST be evaluated in closed form. Free-form surfaces (B-spline/NURBS) MUST be
evaluated to a stated tolerance.

Triangle meshes MUST remain supported (STL import) as one body kind among others. A mesh body is
machined as a set of planar triangular faces. It does not gain precision it never had.

Derived representations (display tessellation, voxel stock for simulation display, spatial
indexes) MAY exist. They MUST be caches, rebuildable from the B-rep, and MUST NOT be the
source of truth for cutter-contact positions.

## 1.2a No grids in toolpath computation

Toolpaths computed on a regular XY grid have pass start and end points on the nearest grid node,
not on the true boundary.

FluutCAM MUST NOT use any regular grid, lattice, or integer-quantized coordinate space to
compute toolpath geometry. This includes height fields, Z-maps, voxel grids, and
integer-scaled polygon libraries (e.g. Clipper's int64 coordinates). Every toolpath coordinate
is solved from exact geometry: faces, edges, region boundaries, and stock boundaries.

The only quantization of toolpath coordinates is number formatting in the post-processor
(05 §5.8). Rules for exact endpoints are in [04 §4.12](04-toolpath-algorithms.md).

Spatial indexes (bounding-volume hierarchies) are allowed. They only cull candidates and never
supply coordinates.

Material-removal simulation for display and verification MAY use a discretized stock model
(04 §4.10). Its results MUST NOT feed back into toolpath coordinates.

## 1.3 Subsystems

```
┌──────────────────────────────────────────────────────────────────────────┐
│ UI layer: project tree · parameter editors · 3D viewport · simulation    │
└───────────────┬──────────────────────────────────────┬───────────────────┘
                │                                       │
        ┌───────▼─────────┐                    ┌────────▼─────────┐
        │ Scripting / API  │                   │ Settings          │
        └───────┬─────────┘                    └──────────────────┘
                │  (same property interface as project file )
        ┌───────▼──────────────────────────────────────────────────┐
        │ Domain model: Project → Setup → Toolpath   (03)          │
        └───┬───────────────────────────┬──────────────────────────┘
            │                           │
  ┌─────────▼──────────┐      ┌─────────▼─────────────────────────────┐
  │ Geometry import     │      │ Geometry kernel                │
  │  STEP  (primary)    │─────▶│  B-rep model: bodies/faces/edges       │
  │  STL   (mesh body)  │      │  exact evaluators per surface type     │
  │  DXF/SVG (2D curves)│      │  spatial index (BVH over face bounds)  │
  │  bitmap (relief)    │      │  display tessellator (cache)           │
  └─────────────────────┘      └─────────┬─────────────────────────────┘
                                          │ cutter-contact queries
                              ┌───────────▼───────────────────────────┐
                              │ Toolpath engine (04)                   │
                              │  strategies → curve-native toolpath    │
                              │  (lines, arcs, helices, rotary sweeps) │
                              └───────────┬───────────────────────────┘
                     ┌────────────────────┴──────────────┐
           ┌─────────▼─────────┐               ┌─────────▼──────────┐
           │ Simulation         │              │ Post-processor (05) │
           │ (stock model cache)│              │  G-code text        │
           └────────────────────┘              └────────────────────┘
```

Key design choices:

| FluutCAM | Reason |
|---|---|
| Import produces B-rep bodies; STL produces a mesh body | DC-01 |
| Geometry kernel answers exact cutter-contact queries; no grids in toolpath computation | DC-01, DC-02 |
| Setup → Toolpath tree; Setups chain with `basedOn`; index angle is a Setup parameter | — see 03 §3.9 |
| Add a specific toolpath type; editor shows only its parameter groups; roughing and finishing are separate toolpaths; ~40-type catalogue | — see 03 §3.10, 04 §4.13 |
| Model-centred dark UI: ribbon by workflow stage, contextual inspector, on-model controls, command palette, program timeline | — see 08 |
| Hierarchical native definitions plus importers for other CAM products | — see 02, 05 |
| Toolpath = ordered typed segments (line/arc/helix/rotary sweep) | — keeps exact curves through to output |
| Rotary strategies work in cylindrical coordinates against exact surfaces | — see 04 §4.7 |


## 1.4 Geometry kernel

Responsibilities:

1. Hold B-rep bodies in model units with a recorded modelling tolerance per body.
2. Evaluate any face: point `S(u,v)`, normal, partial derivatives, bounds, closest point.
3. Evaluate any edge: point `C(t)`, tangent, closest point.
4. Answer cutter queries (04 §4.2): drop-cutter, push-cutter, cutter-surface offset.
5. Provide a spatial index (bounding-volume hierarchy over face bounds) for query culling.
6. Produce display tessellation on demand, to a view-dependent chord tolerance.

Kernel implementation strategy **[DECIDED — OQ-01]**: see the decision below. DC-03 (Rust only, §1.8) rules out the
C++ kernels considered before (Open CASCADE, Parasolid, ACIS) unless an FFI exception is
granted (OQ-08). Remaining options:

| Option | For | Against |
|---|---|---|
| A. Own CAM-oriented kernel in Rust, with own or third-party Part 21 parser | Full control; only the entities and queries CAM needs (03 §3.2); no general modelling operations required | Effort; STEP edge cases (trimming, tolerances, healing) are hard |
| B. Build on `truck` crates (`truck-geometry`, `truck-topology`, `truck-stepio`) | Existing pure-Rust B-rep and NURBS; STEP I/O started | Maturity and STEP coverage to be verified against 02 §2.6.1 |
| C. Own kernel, borrowing selected `truck` geometry crates | Reuse NURBS evaluation; own topology and queries | Two code styles; dependency coupling |

**Decision (OQ-01):** option C, confirmed by a spike. FluutCAM has its own CAM-oriented kernel
(topology, cutter queries, Part 21 reader). `truck` geometry crates (NURBS evaluation) are
borrowed if they pass the spike. The spike imports a STEP corpus (AP203/214/242 files from at
least four CAD systems) and measures entity coverage, evaluation accuracy and speed. Its result
is recorded here before kernel work starts. Cutter queries stay behind an internal trait so
kernel internals can change.

**Spike result (S1, 2026-10-31):** no `truck` crate passed. `truck-stepio` read 17 of 33 corpus
files and gave 4 usable closed shells; `truck-geometry` evaluates NURBS correctly but 2.3× slower
than FluutCAM's own `fluut-geom`. FluutCAM therefore uses its own Part 21 reader, topology and
NURBS evaluation (option A in effect). Details: `docs/plan/spikes/s1-kernel-step.md`.

## 1.5 Tolerance model

Three distinct tolerances MUST exist and MUST NOT be conflated:

| Tolerance | Owner | Meaning |
|---|---|---|
| Modelling tolerance | Body (from STEP `UNCERTAINTY_MEASURE_WITH_UNIT` or import default) | Gap size treated as coincident in the B-rep |
| Machining tolerance | Toolpath | Maximum allowed deviation of the cutter path from the ideal path (chordal + fitting error) |
| Display tolerance | Viewport | Chord error of display tessellation; never affects toolpaths |

Machining tolerance, not a fixed sampling step, is the primary accuracy control. See 04 §4.2.

## 1.6 Calculation pipeline

Steps:

1. Resolve region, stock, and cutter.
2. Select faces in scope. Default: all faces of the target bodies. The user MAY select or
   exclude faces individually ("check surfaces" vs "drive surfaces").
3. Dispatch on strategy.
4. Generate cutter-contact (CC) and cutter-location (CL) paths against exact faces, with pass
   endpoints solved on exact region boundaries.
5. Fit CL paths into typed segments within machining tolerance (04 §4.3).
6. Link, lead in/out.
7. Optional simulation (the stock model is a discretized cache).

Threading model: **[OPEN]**.

## 1.7 External surfaces

All imported files are untrusted input: STEP (`.step`, `.stp`, `.stpz`), STL, DXF, SVG,
bitmaps and project files. The STEP reader MUST treat input as untrusted (bounded recursion, bounded entity
counts, no external references followed without user consent).

Imported post-processor, machine and tool definitions from other CAM products (05 §5.10) are also untrusted input. Procedural posts (Fusion `.cps` JavaScript) MUST run only in
a sandbox with no file, network, or process access, and with time and memory limits.

## 1.8 Implementation language and stack

FluutCAM MUST be written entirely in Rust.

| Concern | Requirement / candidate |
|---|---|
| Language | Rust, stable toolchain, edition 2024. No C or C++ in first-party code. |
| Dependencies | SHOULD be pure Rust. **[DECIDED — OQ-08]** A non-Rust dependency (C/C++ via FFI) is allowed only by a recorded exception: a written reason, no suitable pure-Rust alternative, and confined to one crate that is the only place with `unsafe` for it. Exceptions are listed in §1.8.3. System APIs reached through Rust bindings (graphics drivers, OS file dialogs) are not counted. |
| Numerics | `f64` throughout for geometry and toolpaths. Linear algebra: `glam` (`DVec3`) or `nalgebra` **[OPEN]**. |
| Parallelism | `rayon` data parallelism for per-pass and per-face work. Every long calculation takes a cancellation token and reports progress. |
| 2D offsetting | Arc-aware, floating-point. **[DECIDED — OQ-06]** `cavalier_contours` (polylines with arc segments), wrapped behind an internal trait. Integer-grid libraries are excluded by DC-02. |
| STEP parsing | Own Part 21 reader (OQ-01 decision, §1.4); `ruststep` may be evaluated in the spike. |
| Rendering | `wgpu`. |
| UI toolkit | **[DECIDED — OQ-09, 2026-11-01]** `egui` with `egui_dock`, on `wgpu` and AccessKit. Chosen after the S4 prototypes of `egui` and `slint` ([report](../plan/spikes/s4-ui-toolkit.md)): `egui` keeps shipped binaries under permissive licences and has docking; `slint`'s royalty-free licence is not open source. `iced` dropped. |
| Scripting | `rhai`, embedded and sandboxed, in v1. See [09](09-scripting.md). |
| JavaScript (Fusion post import) | Candidate: `boa_engine` (pure Rust). |
| Serialization | `serde`; formats per 02. |
| Target platforms | **[DECIDED — OQ-10]** Windows, macOS and Linux, all in v1. CI builds and tests on all three. |

### 1.8.1 Code-safety rules

- `#![forbid(unsafe_code)]` in every crate, except crates listed as exceptions (GPU or OS
  bindings) with a written reason.
- Parsers for untrusted input (STEP, STL, DXF, SVG, imported posts and tool libraries) MUST
  return errors and MUST NOT panic on any input. Each parser MUST have a fuzz target
  (`cargo-fuzz`).
- Geometry routines SHOULD have property-based tests (`proptest`), e.g. "every pass endpoint lies
  on the region boundary within ε_s".
- Dependency licences and advisories are checked in CI (`cargo-deny`).

### 1.8.2 Crate layout (initial)

```
fluut-geom        math, curves, surfaces, evaluators, tolerances
fluut-topo        B-rep topology, adjacency, BVH
fluut-io-step     STEP Part 21 reader → topo
fluut-io-mesh     STL reader/writer → MeshBody
fluut-io-2d       DXF, SVG → WireBody
fluut-model       project, model, setups, toolpaths, definitions (serde)
fluut-cam         cutter queries, strategies, fitting, linking
fluut-sim         stock simulation (display / verification)
fluut-post        native post engine, resolver, emitter
fluut-post-import Vectric, DeskProto, Fusion importers
fluut-script      rhai engine, sandbox, object API (09)
fluut-cli         headless: load project, calculate, post, run scripts (automation, tests)
fluut-app         UI
```

`fluut-cli` gives a headless path for every core function. Automated tests and scripting use
it. The UI is a client of the same crates.

### 1.8.3 Recorded FFI exceptions (OQ-08)

| Crate | Native dependency | Reason | Status |
|---|---|---|---|
| — | — | none recorded yet; candidate: SQLite for Vectric `.vtdb` import (05 §5.10.5) if no pure-Rust reader fits | — |

## 1.9 Reliability

| Item | Requirement |
|---|---|
| Atomic save | Save writes a temporary file and renames it over the project, so a crash during save never corrupts the project. |
| Autosave and recovery | Every edit is appended to a journal (`<project>.fluut-autosave`). After a crash, the app offers to recover unsaved work on next start. The journal is deleted after a successful save. |
| File locking | A project open in one FluutCAM instance is locked; a second instance opens it read-only with a notice. |
| Crash reports | Opt-in, see [06 §6.4](06-security-and-distribution.md). |
| Help | Online documentation, opened from the app (contextual links from the inspector and ribbon). Bundled offline help is not required in v1. |
