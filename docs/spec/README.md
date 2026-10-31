# FluutCAM — Specification

Status: **core design decisions DC-01 to DC-07 adopted; open questions decided**. Next: strategy
specification and the kernel and UI spikes.

## Conventions

- `DC-nn` — one of the core design decisions below.
- `[DECIDED — OQ-nn]` — a decided open question; listed under Decisions on open questions.
- `[OPEN]` — undecided.

Requirement keywords MUST, MUST NOT, SHOULD, MAY are used as in RFC 2119.

## Core design decisions

| ID | Title | Affects | Summary |
|---|---|---|---|
| DC-01 | Exact (STEP-mapped) geometry core | 01, 02, 03, 04, 05 | Core geometry becomes a B-rep whose entities map to STEP ISO 10303-42 (planes, cylinders, cones, spheres, tori, B-splines; lines, circles, ellipses, B-spline curves; full topology). STL kept as a mesh body kind. Toolpaths solved against exact faces to a machining tolerance, not a height-field grid. Toolpaths become typed segments (line, arc, helix, rotary sweep, spline) carried through to G2/G3 and single-line rotary output. Rotary machining on coaxial surfaces of revolution needs no radial oscillation. |
| DC-02 | No grids; exact pass geometry | 01, 03, 04, 05 | No grid, Z-map, voxel or integer-quantized space in toolpath computation. Pass endpoints solved exactly on region boundaries and contact transitions; passes laid out from the region, not a lattice. Only quantization is post number formatting, with arc-centre correction. Simulation may stay discretized but never feeds toolpaths. |
| DC-03 | Rust implementation | 01, 03 | Entirely Rust. Pure-Rust dependencies preferred; FFI needs a recorded exception. Rules out C++ kernels (OCCT etc.) by default. Safety rules: no `unsafe` by default, fuzzed parsers, property tests. Initial crate layout and headless CLI. |
| DC-04 | New definition formats; layered posts; imports | 01, 02, 03, 05 | Native TOML formats for project, posts, machines, tools. Posts are layered (`extends` + mixins, version ranges, line-level patching) on a G-code dialect tree, with Vectric-style blocks and placeholders. Importers for Vectric `.pp`, DeskProto, Autodesk `.cps` (sandboxed), tool libraries. |
| DC-05 | Setup → Toolpath model | 01, 02, 03, 04, 05 | Setup → Toolpath tree (as Fusion 360). A Setup defines geometry, scaling, model-to-machine axis alignment, stock, work zero, and kind (`three_axis`, `rotary`, `indexed`). Setups chain with `basedOn`, inheriting unset fields; stock can carry over. Index angle is one Setup parameter (absolute or relative). Rotary/indexed zero lies on the rotary axis so one work offset serves all sides. Post gains setup, index and clamp events. |
| DC-06 | Typed toolpaths; separate roughing; wider strategy range | 01, 02, 03, 04, 05 | The user adds a specific toolpath type (as in Fusion); its editor shows only that type's parameters, in five common groups (Tool, Geometry, Heights, Passes, Linking). Type fixed at creation. Roughing and finishing are separate toolpaths; finishing types have no roughing option. Catalogue of ~40 types (2D, holes, 3D, rotary, multi-axis, utility) modelled on Fusion strategies, with a proposed v1 subset. Drilling adds canned-cycle segments and post events. |
| DC-07 | Modern model-centred UI | all; new 08 | Dark, polished UI in the style of DaVinci Resolve and GitKraken. Viewport always central. Ribbon tabs are workflow stages (Model, Setup, Toolpaths, Simulate, Post, Library) holding commands only; parameters live in a contextual inspector and on-model handles (stock, work zero, index dial, faces, tabs, leads). Command palette, background calculation with in-place status, program timeline, unlimited undo.Product name Fluut / FluutCAM (ids `fluut/…`, crates `fluut-*`, `.fluut` projects). |

## Document map

| File | Contents | Decisions |
|---|---|---|
| [01-architecture.md](01-architecture.md) | Subsystems, kernel options, no-grid rule, tolerance model, pipeline, Rust stack and crate layout | DC-01 to DC-06 |
| [02-data-formats.md](02-data-formats.md) | Format policy; project (setups, typed toolpaths), post, machine, tool formats; import sources; STEP/STL import | DC-01, DC-02, DC-04, DC-05, DC-06 |
| [03-data-model.md](03-data-model.md) | Setup → Toolpath tree, B-rep geometry model, toolpath fields and results, re-import identity, definitions, Setup model, toolpath types and parameter groups | DC-01 to DC-06 |
| [04-toolpath-algorithms.md](04-toolpath-algorithms.md) | Exact cutter queries, sampling and fitting, rotary and indexed machining, exact pass geometry, toolpath type catalogue | DC-01, DC-02, DC-05, DC-06 |
| [05-postprocessor-engine.md](05-postprocessor-engine.md) | Layered posts, words, blocks (incl. setup/index/cycle events), capabilities, emission, rounding, provenance, importers | DC-01, DC-02, DC-04, DC-05, DC-06 |
| [06-security-and-distribution.md](06-security-and-distribution.md) | Licence, distribution and updates, hardening, privacy | decisions 2026-10-31 |
| [08-user-interface.md](08-user-interface.md) | Layout, ribbon, on-model controls, inspector, status, timeline, command palette, visual style, toolkit requirements | DC-07 |
| [09-scripting.md](09-scripting.md) | Embedded `rhai` scripting: scope, sandbox, object API, example | decisions 2026-10-31 |

Section 07 is not used; the UI section is numbered 08 to keep section numbers stable.

## Decisions on open questions

All numbered open questions were decided on 2026-10-31. Each is marked **[DECIDED — OQ-nn]** at
its point in the spec.

| Id | Question | Decision | Section |
|---|---|---|---|
| OQ-01 | Geometry kernel | Own CAM-oriented kernel and Part 21 reader; borrow `truck` NURBS crates if a STEP-corpus spike confirms them | 01 §1.4 |
| OQ-02 | Mesh-to-analytic recognition | Not in v1; later as an explicit opt-in action that adds a new body | 03 §3.2.5 |
| OQ-03 | Toolpath result storage | Disposable sidecar `<project>.fluut-cache/`, versioned `serde` binary keyed by input hash | 03 §3.4 |
| OQ-04 | STEP healing | Report problems; sew within modelling tolerance; no deeper repair; invalid faces excluded with notice | 02 §2.6.1 |
| OQ-05 | Scallop-driven stepover | v1: scallop input, one worst-case stepover per toolpath; variable stepover later | 04 §4.3.4 |
| OQ-06 | Arc-aware 2D offset | `cavalier_contours` behind an internal trait | 04 §4.8 |
| OQ-07 | Spline (NURBS) output | Not in v1 | 05 §5.12 |
| OQ-08 | Non-Rust dependencies | Per-case recorded exception, confined to one crate | 01 §1.8, §1.8.3 |
| OQ-09 | UI toolkit | Prototype `egui` and `slint`, choose against 08 §8.11 | 08 §8.11 |
| OQ-10 | Platforms | Windows, macOS and Linux in v1 | 01 §1.8 |
| OQ-11 | Rest regions | In-process stock as layered exact 2D regions (cylindrical layers in rotary Setups) | 04 §4.12.6 |
| OQ-12 | File extensions | `.fluut`; `.post.toml`, `.machine.toml`, `.tools.toml`, `.template.toml` | 02 §2.0 |
| OQ-13 | Procedural post import | v1 declarative only (Vectric, DeskProto); Fusion `.cps` hosted later; SheetCAM, FreeCAD not planned | 05 §5.10.3 |
| OQ-14 | Post expression grammar | Small own grammar, specified in EBNF | 05 §5.4.3 |
| OQ-15 | Stock between Setups | Carry the layered 2D stock model into the child Setup | 03 §3.9.6 |
| OQ-16 | 5-axis | v1: 3-axis, rotary, indexed (A). Next: 3+2 (A+B). Simultaneous: future | 03 §3.9.2 |
| OQ-17 | v1 toolpath types | Proposed set plus `scallop` and `steep_and_shallow` (22 types) | 04 §4.13 |
| OQ-18 | Templates | Toolpath and Setup templates in v1, TOML, layered | 03 §3.10.6 |
| OQ-19 | Direct send to controllers | v1 files only; per-controller send adapters later | 08 §8.3 |
| OQ-20 | Program timeline | In v1 without drag-reorder | 08 §8.7 |

## Further decisions (2026-10-31)

| Topic | Decision | Section |
|---|---|---|
| Scripting | Embedded `rhai` in v1, sandboxed; app console, ribbon and `fluut run` headless; API keys match the project file | 09 |
| Licence | Open source, MIT OR Apache-2.0; no licence enforcement; no copyleft dependencies in shipped binaries | 06 §6.1 |
| Distribution | Signed installers per OS; HTTPS update check with signed manifest; user-confirmed installs | 06 §6.2 |
| Languages | English in v1; all strings in Fluent files from the start | 08 §8.10a |
| Units | Per-project display unit; explicit units accepted in any field; internal mm | 08 §8.10a |
| Collisions | Holder and shank avoided during calculation (exact trimming, reported); links lifted; simulation verifies | 04 §4.14 |
| Reliability | Atomic save, autosave journal with crash recovery, file locking, online help | 01 §1.9 |
| Crash reports | Opt-in, user sees the report before sending | 06 §6.4 |

## Still open (minor, unnumbered)

| Item | Section |
|---|---|
| Linear algebra crate: `glam` or `nalgebra` | 01 §1.8 |
| Scripting API full member list (generated from Rust API) | 09 §9.7 |
| Per-type parameter definitions (strategy specification) | 02 §2.1, 04 §4.13.7 |
| Vectric features with no equivalent (direct output, drill banks, laser) | 05 §5.10.1 |
| Vectric / Fusion tool library field mapping | 05 §5.10.5 |
| Telemetry policy (opt-in only, if any) | 06 §6.4 |
| Release channels and update cadence | 06 §6.5 |
| UI toolkit licence check (`slint` vs MIT/Apache) | 06 §6.1, 08 §8.11 |
| Result of the kernel spike (OQ-01) and toolkit prototypes (OQ-09) | 01 §1.4, 08 §8.11 |
