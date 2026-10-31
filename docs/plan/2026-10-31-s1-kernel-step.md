# S1 — Kernel and STEP Spike Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decide OQ-01: own Part 21 reader and B-rep mapping on `fluut-geom`, or the `truck` crates. Exit criterion from the roadmap: on the corpus, % of files loaded and % of shells valid; the decision recorded; CH-03, CH-04, CH-08 and CH-12 updated.

**Architecture:** The spike starts two crates meant to be kept: `fluut-io-step` (layer 2: lexer, parser, raw model, schema, units, entity mapping) and `fluut-topo` (layer 1: B-rep arena, validity checks, face-box BVH). The `truck` comparison lives in `spikes/s1-truck`, a crate outside the workspace (its own `[workspace]`), so its dependencies never reach the product, `cargo deny` or the layer check.

**Tech Stack:** as S3; `fluut-io-step` adds `memchr`. `spikes/s1-truck` uses `truck-stepio`, `truck-modeling` and `truck-geometry` at their latest versions.

**Design:** [`../design/D04-step-reader.md`](../design/D04-step-reader.md) §1–§9, [`../design/D03-topology-bvh.md`](../design/D03-topology-bvh.md) §1, §2, §4.

**Corpus:** NIST MBE PMI test files used by the CAx-IF (33 files, AP203 and AP242, exported by CATIA V5, NX and others), outside the repository at the path in `FLUUT_STEP_CORPUS` (here `C:\source\step-corpus`). Corpus tests are `#[ignore]`d and skip when the variable is unset; CI does not run them. Source and terms are in the corpus `README.md`.

## Global Constraints

All M0 to S3 Global Constraints apply. In addition:

- STEP input is untrusted (spec 06 §6.3): no panics on any input, limits from D04 §2, reference cycles are errors.
- The lexer and parser are tested on hand-written snippets; corpus numbers are measurements, not pass/fail tests.
- No corpus file or excerpt larger than a few lines is committed.

## Review Focus

1. **Parser robustness**: truncated files, unterminated strings, dangling and cyclic references, deep nesting give errors with line and column, never panics.
2. **Edge sense**: `EDGE_CURVE.same_sense` and `ORIENTED_EDGE.orientation` combine correctly; the manifold check (every edge of a closed shell used once in each direction) is the test.
3. **Units**: inch files are converted to mm.

---

### Task 1: Lexer and parser

**Files:** Create `crates/fluut-io-step/{Cargo.toml, src/lib.rs, src/lex.rs, src/parse.rs, src/parse/tests.rs}`.

- Tokens and `RawModel` as D04 §2–§3. Entities in a `Vec`, `#id → slot` in a `BTreeMap` (determinism rule); keywords interned to `KwId`.
- Single-threaded in the spike; parallel parsing is measured as a later option.
- [ ] **Step 1:** Tests: every token kind; `''` and `\X2\…\X0\` in strings; complex entities `(A()B())`; typed parameters `LENGTH_MEASURE(1.)`; errors with positions for the Review Focus 1 cases.
- [ ] **Step 2:** Fail, implement, pass. Corpus: every file parses; MB/s.

### Task 2: Schema, units and entity census

- Schema from `FILE_SCHEMA`; length and angle units from the geometric context (`SI_UNIT` with prefix, `CONVERSION_BASED_UNIT` for inch and degree); uncertainty.
- Census: for each file, counts of entity kinds inside shape representations, split into supported (D04 §5.1) and unsupported.
- [ ] **Step 1:** Tests on snippets: mm, inch, metre; AP203 and AP242 schema names.
- [ ] **Step 2:** Corpus census table for the report.

### Task 3: `fluut-topo` arena and checks

**Files:** Create `crates/fluut-topo/{Cargo.toml, src/lib.rs, src/brep.rs, src/check.rs, src/bvh.rs}`.

- Arena of vertices, edges (curve, parameter range, end vertices), coedges (edge, forward), loops, faces (surface, sense, loops), shells, bodies, as D03 §1 without p-curves and trim caches.
- Checks: loop closure (consecutive coedge ends within `tol_model`), manifold edge use in closed shells, face with no outer loop.
- BVH over face bounding boxes (from edge samples and surface corner samples), median split; query by box.
- [ ] **Step 1:** Tests on a hand-built cube and a cube with one flipped coedge and one gap.
- [ ] **Step 2:** Fail, implement, pass.

### Task 4: Geometry and topology mapping

**Files:** `crates/fluut-io-step/src/map.rs`.

- Product and representation walk (D04 §4, without assemblies: every `MANIFOLD_SOLID_BREP`, `BREP_WITH_VOIDS` and `SHELL_BASED_SURFACE_MODEL` becomes a body with the unit and uncertainty of its context).
- Curves and surfaces of D04 §5.1 that `fluut-geom` has; others counted as unsupported and their faces skipped.
- [ ] **Step 1:** Tests on a hand-written STEP cube (planes, lines) and a cylinder with a seam.
- [ ] **Step 2:** Corpus: per file, bodies, faces mapped and skipped, shells passing the checks, time.

### Task 5: `truck` comparison

**Files:** Create `spikes/s1-truck/{Cargo.toml, src/main.rs}`.

- Load every corpus file with `truck-stepio`: success, shells, faces, time.
- NURBS surface evaluation: the same random bicubic surfaces evaluated (point and first derivatives) by `truck-geometry` and `fluut-geom`; speed and agreement.
- [ ] **Step 1:** Run; record numbers. Licences of the `truck` crates checked.

### Task 6: Report and decisions

**Files:** Create `docs/plan/spikes/s1-kernel-step.md`; modify `docs/design/challenges.md` (CH-03, CH-04, CH-08, CH-12), D04 §9, `docs/spec/01-architecture.md` §1.4 (OQ-01 decision), `docs/plan/README.md`.

- [ ] **Step 1:** Report: load and validity percentages for both, speed, gaps, decision. CH-04 and CH-12 are about cutter queries; record what S1 found for them and leave the rest to S2.
- [ ] **Step 2:** Workspace checks; commit.
