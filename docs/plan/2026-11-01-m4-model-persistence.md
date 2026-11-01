# M4 — Model and Persistence Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The full project model of D06: layered definitions, every Setup kind with `based_on` inheritance, templates, `.fluut` save and load, undo, the edit journal and the disk result cache, plus re-import identity (spec 03 §3.5). Exit criterion from the roadmap: the D06 §8 tests pass and the reference projects round-trip.

**Architecture:**
- `fluut-model` (layer 3):
  - **Document.** `Project.setups` becomes `Vec<Arc<Setup>>`, so a project clone shares unchanged Setups (CH-02). Every Setup field is `Option` (absent = inherited or default). New fields: `based_on`, `index_angle`, `region`; the stock kinds `block`, `cylinder`, `body`, `from_previous`; rotary orientation and zero forms; cylinder clearance; `geometry` scale and mirror. Saved views are kept as raw tables.
  - **Inheritance.** `resolve_setup(project, id) -> ResolvedSetup`: every field concrete, with its origin (local, inherited from a Setup, default). It also checks the `from_previous` rule (spec 03 §3.9.4).
  - **Format.** The reader checks `format` and runs a migration list for older minors. Unknown keys are warnings and are not saved again. The writer always writes the current format.
  - **Definitions.** A `Library` of layers (built-in, user, project; D06 §4.1). It holds posts, machines and templates in one `DefStore`, and tool libraries next to them. `fluut/` ids are reserved for the built-in layer. A typed `Machine` is built from the resolved table (spec 02 §2.3). Tool libraries get an optional `version` and `extends`, and tool refs take `@version` (`shop@1#flat-6`).
  - **Templates** (D06 §4.4). Toolpath templates and Setup templates are resolved through the generic merge. Applying one gives `Edit`s. Expressions such as `"0.4*tool.diameter"` are evaluated against the chosen tool.
  - **Edits and undo** (D06 §6). `Edit` values carry their payloads as TOML text, so the journal needs no serde on the model types. `Document` holds the root `Arc<Project>`, the undo and redo stacks, coalescing within 750 ms and a 256 MB cap.
  - **Persistence** (D06 §7). Atomic save to `name.fluut` with a `.bak`. The journal `name.fluut-autosave` holds `FLUJ` and a header hash, then length + postcard + crc32 records.
- `fluut-core` (layer 6):
  - Frames and the graph use resolved Setups. A `from_previous` stock takes its box from the parent chain until in-process stock exists.
  - The session opens the definition library and the disk cache, and saves the project.
  - **Cache.** `<project>.fluut-cache/<hash>.tpr`: `FLUT`, schema, FluutCAM version, input hash, postcard of a mirror of `ToolpathResult` and its diagnostics, then crc32. Garbage-collected on save.
  - **Re-import** (spec 03 §3.5). Bodies, faces and edges are matched by id with an equal signature, then by signature, then by a unique surface match. Selections are rewritten as one undoable edit group, and unmatched selections are reported.

**Tech Stack:** as M3. Added: `postcard` (journal and cache) and `crc32fast` (MIT OR Apache-2.0).

**Design:** [D06](../design/D06-project-model.md) (all), [D01](../design/D01-workspace-architecture.md) §4–§5, [D15](../design/D15-toolpath-parameters.md) §1.4–§1.8; spec 02 §2.1–§2.4 and §2.8, 03 §3.4, §3.5, §3.7, §3.9.

## Global Constraints

All earlier Global Constraints apply. In addition:

- Determinism (D01 §6): `BTreeMap` wherever order reaches a file, a hash or a report. No clock value goes into any file except cache file times (GC). Undo coalescing takes the time as an argument.
- Do not modify `fluut-cam` strategies, `fluut-io-*`, `fluut-sim` or `third_party/` (parallel sessions). Changes to `tools.rs` are additive, so they merge with M6's drill keys.
- No DeskProto-derived content.

## Scope decisions

- **Hash:** SHA-256 stays. D01 §4 allows the switch to `blake3` with the disk cache; that would change every file hash already in projects, and speed is not an issue yet.
- **Setups in calculation:** `rotary` and `indexed` Setups load, resolve, inherit and round-trip. Calculating them stays `setup.kind.unsupported` until the rotary types (M6).
- **`from_previous`:** for calculation, the stock box is the nearest ancestor's non-`from_previous` stock. In-process stock (D08 §7) is M6.
- **Toolpath `tool`:** optional on read, so the spec 02 §2.1 example (where some toolpaths have only `# …`) loads. A toolpath without a tool fails validation with `toolpath.tool.missing`.
- **Unknown toolpath types:** an error, as in M3. The spec example's rotary types are tested with the types replaced by M3 types.
- **Key order:** the M3 order stays canonical (`stock` before `zero`). It matches D06 §1 and the reference projects. The two spec 02 §2.1 snippets that list `zero` first are reordered.
- **Templates are expanded at apply time.** `ApplyTemplate` is not an `Edit`. `Library::apply_template` returns an `Edit::Group`, so replaying a journal does not need the library.
- **Journal records** are `Edit`, `Undo` or `Redo`, so a recovered document has the same undo state.
- **Body cache (`*.bodies`) and `index.toml`** are not written. Bodies are re-imported on open.
- **Re-import persistent ids:** the STEP reader exposes no AP242 persistent ids or face names. The persistent id is the D06 id (`f-<body>.<index>`), accepted only when the signature also matches.
- **Lock file** (D06 §7.4) is not part of this plan.

## Review Focus

1. **Byte-identical round trip** of the reference projects, the spec 02 §2.1 project and the D15 examples.
2. **Inheritance:** an absent key is never written into a child. A parent edit flows to its children. Origins are right.
3. **Journal:** a truncation at every byte offset recovers a valid prefix of the edits.
4. **Undo:** undoing everything gives a document equal to the original.
5. **Cache:** a P edit keeps the cache file; an R edit misses it. A corrupt file is a miss, not an error.

---

### Task 1: Plan

- [x] **Step 1:** This plan; README table row. Commit.

### Task 2: Full project document and format versioning (`fluut-model`)

**Files:** `crates/fluut-model/src/project.rs`, `project/{read.rs, write.rs, tests.rs}`, `src/hash.rs`; callers in `fluut-core`, `fluut-cli` tests.

- `Setup`: every field `Option`; `kind`, `based_on`, `material`, `index_angle`, `region`. `SetupGeometry { bodies, scale, mirror }`. `Orientation::{ThreeAxis { tool_axis, x_axis }, Rotary { rotary_axis, a_zero_toward }}` with `AxisPick::{Axis, Face, Edge}`. `StockSpec::{Block { margin, size }, Cylinder { radius, length }, Body { body }, FromPrevious}`. `ZeroSpec` gains `OnAxis { x }`. `ClearanceSpec::{Plane { offset }, Cylinder { radius }}`. `IndexAngle { mode, value }`. `Region { floor_z }`.
- `Project.setups: Vec<Arc<Setup>>`; `views: Vec<toml::Table>`.
- Format: `fluut.project/1` and `/1.0`; a newer minor warns that newer keys will not be saved; migrations list; the writer writes `fluut.project/1`.
- [x] **Step 1:** Tests: the spec 02 §2.1 project (comments and `# …` stripped, rotary types swapped for M3 types) round-trips; every reference project round-trips; each new field form parses, rejects a wrong form and round-trips; unknown keys are warnings and are dropped on write; format versions.
- [x] **Step 2:** Implement; pass; commit.

### Task 3: Setup inheritance (`fluut-model`, `fluut-core`)

**Files:** Create `crates/fluut-model/src/setup.rs` and `tests/inherit.rs`; change `fluut-core` `frame.rs`, `graph.rs`, `session.rs`.

- `resolve_setup(project, id) -> Result<ResolvedSetup, Diagnostic>` with `FieldOrigin::{Local, Inherited(id), Default}`, memoised per call over the chain. Index angle: absolute, or relative to the parent's resolved angle (0 under a rotary parent).
- Validation (spec 03 §3.9.4): `setup.based_on.missing`, `setup.based_on.order` (parent later or self), `setup.stock.from_previous.parent` (no parent), `setup.stock.from_previous.changed` (machine, geometry, orientation or zero differs from the parent's resolved value).
- `fluut-core`: frames, the graph hash and material from the resolved Setup. A parent edit changes the child's Setup node hash.
- [x] **Step 1:** Tests: property test over random chains (random local fields at each level) against a manual walk; origins; relative index angles; each validation code passing and failing; a parent edit restales the child's toolpaths.
- [x] **Step 2:** Implement; pass; commit.

### Task 4: Definition library: layers, machines, tools (`fluut-model`, `fluut-core`)

**Files:** Create `crates/fluut-model/src/library.rs`, `src/machine.rs`, `tests/library.rs`; change `src/tools.rs`, `fluut-core/src/session.rs`.

- `Library { layers }` with `LayerKind::{BuiltIn, User, Project}`. `load_dir(dir, kind)` reads `posts/*.post.toml`, `machines/*.machine.toml`, `templates/*.template.toml` and `tools/*.toml`. Lookup by `DefRef` across layers, highest version, and the higher layer wins a tie. Each definition records its origin.
- `fluut/` ids only in the built-in layer (`def.namespace.reserved`).
- `Machine::from_resolved` (spec 02 §2.3): travel, spindle, feed, rotary axes, tool changer, `post`, `post_overrides`; `format = "fluut.machine/1"`. Built-in `fluut/generic-3axis-router@1.0`.
- Tool libraries: optional `version` (default `1.0`) and `extends`. Tools merge by id: the child replaces or adds. `ToolRef` takes `lib@req#tool`.
- `fluut-core`: `Session::open_with(path, &OpenOptions { tool_files, user_dir })` builds the library from built-in posts and machines, the user dir and the project dir. The Setup's `machine` gives the post's machine info and the validation machine limits.
- [x] **Step 1:** Tests: layers and ties; reserved namespace; `@3` and `@3.2` across layers; cycles and missing parents through the library; mixin conflict warning and provenance for machines; machine typed load and errors; tool `extends` and `@version` refs.
- [x] **Step 2:** Implement; pass; commit.

### Task 5: Templates (`fluut-model`)

**Files:** Create `crates/fluut-model/src/template.rs`, `src/expr.rs`, `tests/template.rs`.

- `fluut.template/1`, `kind = "toolpath" | "setup"`, `extends` through the generic resolver.
- Toolpath template: `[toolpath]` with `type` and the five groups; string values in number fields are expressions over `tool.diameter`, `tool.radius`, `tool.corner_radius` and `tool.flute_length` (D15 §1.6), evaluated at apply time.
- Setup template: Setup fields without `geometry`, and `[[toolpath]]` entries, each either inline or `{ template = "ref" }`.
- `instantiate_toolpath(...) -> Toolpath` with `template` set; `apply_template(...) -> Edit::Group`.
- [x] **Step 1:** Tests: the D06 §4.4 form (with an M3 type); `extends` overrides a parent's pass value; expressions; a type mismatch between the template and its parent is an error; Setup template creates its toolpaths in order; later template edits do not change existing toolpaths.
- [x] **Step 2:** Implement; pass; commit.

### Task 6: Edits, undo and save (`fluut-model`)

**Files:** Create `crates/fluut-model/src/edit.rs`, `src/document.rs`, `src/persist.rs`, `tests/undo.rs`, `tests/save.rs`.

- `Edit` (D06 §6.1, payloads as TOML text): project name and units; add and remove geometry; set a geometry source; add, move and remove Setups; set a Setup field (`None` = reset to inherited); add, move and remove toolpaths; set a toolpath field or parameter; `DuplicateAs`; `RemapSelections`; `Group`.
- `apply(&Project, &Edit) -> Result<(Project, Changed), EditError>`: validates ids, kinds and value forms, and the Setup order rule.
- `Document`: `apply`, `apply_at(edit, ms)` (coalescing), `undo`, `redo`, `can_undo`; 256 MB cap estimated from the written size of changed Setups.
- `persist::save(path, &Project)`: write `name.fluut.tmp`, `sync_all`, rename the current file to `.bak`, rename the temp file into place. `persist::load(path)`.
- [x] **Step 1:** Tests: property test of random edit sequences: undo all equals the original, redo all equals the final state; each edit's error cases; coalescing; the cap drops the oldest entries; save then load is byte-identical, with `.bak` holding the previous file.
- [x] **Step 2:** Implement; pass; commit.

### Task 7: Journal (`fluut-model`)

**Files:** Create `crates/fluut-model/src/journal.rs`, `tests/journal.rs`.

- Header: `FLUJ`, `u16` version, 32-byte SHA-256 of the project file. Records: `u32` length (LE), postcard of `Record::{Edit, Undo, Redo}`, `u32` crc32 (LE).
- `Journal::create(path, file_hash)`, `append(&Record)`, `sync_if_due(now)` (at most once per second), `sync()`; `recover(path, file_hash) -> Option<Vec<Record>>` up to the first bad record; `reset(file_hash)` after a save.
- `Document::with_journal` writes every apply, undo and redo.
- [x] **Step 1:** Tests: truncate the journal at every byte offset; each recovers a prefix and replays to a valid document; a flipped byte stops at that record; a header for another file hash is not offered.
- [x] **Step 2:** Implement; pass; commit.

### Task 8: Result cache sidecar (`fluut-core`)

**Files:** Create `crates/fluut-core/src/cache.rs`, `tests/cache.rs`; change `session.rs`.

- `DiskCache::new(dir)`, `get(hash) -> Option<Entry>`, `put(hash, &ToolpathResult, &[Diagnostic])`, `gc(referenced, now)`: unreferenced files older than 7 days are deleted. A hit refreshes the file time.
- File: `FLUT`, schema `u16`, then postcard of `{ version, hash, passes, diagnostics }` (a mirror of the `fluut-path` types), then crc32. Decoding rebuilds `ToolpathResult::new`, so the invariants are checked again.
- Session: memory cache, then disk, then calculate and store. `Session::save` writes the project, resets the journal and runs GC.
- [x] **Step 1:** Tests: encode/decode is bit-identical for every segment kind; a corrupt, truncated, wrong-schema or wrong-hash file is a miss; reload gives a cache hit with no calculation; an R edit misses and a P edit hits; GC keeps referenced and recent files.
- [x] **Step 2:** Implement; pass; commit.

### Task 9: Re-import identity (`fluut-core`)

**Files:** Create `crates/fluut-core/src/reimport.rs`, `tests/reimport.rs`.

- Signatures: plane (normal, offset), cylinder, cone, sphere, torus (axis line, radii), others by kind; face bounds from loop edges. Edges: curve kind and end points.
- Matching per source: bodies by id, then by name, then by most faces matched. Faces and edges: id with equal signature, then equal signature with bounds, then a unique surface match. Tolerance is the body's modelling tolerance (at least 1e-6 mm).
- `Session::reimport(source) -> ReimportReport { faces, edges, bodies, unmatched }`, applied as one edit group: rewritten selections and Setup bodies, the new hash and body list. Unmatched selections are kept and reported.
- [x] **Step 1:** Tests: the boss with every entity number shifted and its top raised: the walls and the bottom match, the top is reported for the toolpath that selects it, and the other selections are rewritten; an unchanged file maps every id to itself; undo restores the old selections.
- [x] **Step 2:** Implement; pass; commit.

### Task 10: Close

- [ ] **Step 1:** Mark this plan done with deviations; README row; D06 updates. Commit and push `m4-model-persistence`.
