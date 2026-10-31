# D01 — Workspace and Runtime Architecture

Implements: [S 01 §1.3, §1.6, §1.8, §1.9]. Applies to every crate.

## 1. Crate graph

The spec's initial crate list [S 01 §1.8.2] is refined here. One crate is added: `fluut-path`,
which holds the toolpath result types shared by calculation, simulation, post and persistence.
Without it, `fluut-post` and `fluut-sim` would depend on the whole of `fluut-cam`.

```
                       ┌──────────────┐
                       │ fluut-geom   │  math, curves, surfaces, tolerances
                       └──────┬───────┘
                ┌─────────────┼──────────────┐
         ┌──────▼──────┐ ┌────▼─────┐  ┌─────▼──────┐
         │ fluut-topo  │ │fluut-path│  │ fluut-i18n │  Fluent messages, units formatting
         └──┬───────┬──┘ └──┬───┬───┘  └─────┬──────┘
   ┌────────▼──┐ ┌──▼──────────┐ │            │
   │fluut-io-  │ │fluut-io-mesh│ │            │
   │step       │ │fluut-io-2d  │ │            │
   └────────┬──┘ └──┬──────────┘ │            │
            └───┬───┘            │            │
          ┌─────▼────────────────▼──┐         │
          │ fluut-model             │◄────────┘  project, setups, toolpaths, definitions,
          └─────┬──────────┬────────┘            persistence, undo, cache
        ┌───────▼───┐ ┌────▼─────┐ ┌──────────┐
        │ fluut-cam │ │fluut-sim │ │fluut-post│◄── fluut-post-import
        └───────┬───┘ └────┬─────┘ └────┬─────┘
                └──────┬───┴────────────┘
                 ┌─────▼──────┐
                 │ fluut-core │  job system, invalidation graph, session (ties the above)
                 └──┬──────┬──┘
          ┌─────────▼┐  ┌──▼──────────┐
          │fluut-    │  │ fluut-cli   │
          │script    │  └─────────────┘
          └────┬─────┘
          ┌────▼─────┐
          │fluut-app │  UI
          └──────────┘
```

Rules:

1. Dependencies point downwards only. CI checks the graph (`cargo-deny` bans + a custom
   `xtask check-layers`).
2. `fluut-geom`, `fluut-topo`, `fluut-path` have no I/O and no threads of their own.
3. `fluut-cam`, `fluut-sim`, `fluut-post` are pure functions of their inputs plus a
   `JobContext` (§3). They never read the project or settings directly.
4. Only `fluut-core` owns mutable session state and schedules work.
5. `fluut-app` and `fluut-cli` are thin clients of `fluut-core` and `fluut-script`.

| Crate | Owns |
|---|---|
| `fluut-geom` | `Point3`, `Vec3`, transforms, tolerances, curve and surface types and evaluators (D02) |
| `fluut-topo` | B-rep arena, mesh bodies, adjacency, BVH, tessellation for display (D03) |
| `fluut-path` | `ToolpathResult`, `Pass`, `Segment` variants, linearize/split helpers (D08 §2) |
| `fluut-io-step` | Part 21 parser, entity mapping, assemblies, validation, sewing (D04) |
| `fluut-io-mesh` | STL read/write, mesh welding (D05) |
| `fluut-io-2d` | DXF, SVG, bitmap relief (D05) |
| `fluut-i18n` | Fluent bundles, unit parsing/formatting, locale number formats |
| `fluut-model` | Project document, definitions, layered resolution, persistence, journal, undo (D06) |
| `fluut-cam` | tool model, cutter queries, framework, strategies, linking, in-process stock (D07–D09) |
| `fluut-sim` | material removal simulation, verification, time estimate (D11) |
| `fluut-post` | post resolution, emission, formatting (D10) |
| `fluut-post-import` | Vectric, DeskProto importers; Fusion host later (D10) |
| `fluut-core` | session, job system, invalidation, result cache (§3–§5) |
| `fluut-script` | rhai engine, sandbox, API bindings (D12) |
| `fluut-cli` | `fluut` binary: `calc`, `post`, `run`, `check` |
| `fluut-app` | UI binary (D13) |

## 2. Shared conventions

### 2.1 Numbers and units

- Lengths: `f64` millimetres. No unit newtypes in hot code; conversion only at boundaries
  (`fluut-i18n` for UI and scripts, `fluut-model` for files, `fluut-post` for output).
- Angles: `f64` radians internally. Degrees in files, UI and scripts.
- Tolerances are passed explicitly as a `Tol` struct (D02 §2), never read from globals.

### 2.2 Identifiers

| Id kind | Type | Scope |
|---|---|---|
| Arena keys (faces, edges, vertices inside a body) | `slotmap` keys (`FaceKey`, …) | in-memory, one body |
| Persistent ids (bodies, faces for selection, setups, toolpaths) | `PersistId(u128)` printed as short base32 | stored in files |
| Definition ids | `DefId` string `namespace/name` + version | definitions |

Mapping persistent face ids to arena keys is the job of `fluut-topo` (D03 §6).

### 2.3 Errors and diagnostics

Two separate channels:

```rust
// Failure: the operation could not produce a result.
#[derive(thiserror::Error, Debug)]
pub enum Error { /* per crate */ }

// Diagnostic: the operation produced a result, with something the user should know.
pub struct Diagnostic {
    pub severity: Severity,            // Info | Warning | Error
    pub code: &'static str,            // stable, e.g. "cam.collision.trimmed"
    pub message: MessageRef,           // Fluent id + arguments (fluut-i18n)
    pub location: Option<Location>,    // FaceId, point in model, toolpath param, file line…
    pub help: Option<MessageRef>,
}
```

Diagnostics are collected in a `Diagnostics` sink passed through `JobContext`. A strategy that
trims a collision zone, for example, returns its result and pushes a warning; it does not fail.

### 2.4 Logging

`tracing` throughout. Library crates emit spans and events but never configure subscribers.
`fluut-app` and `fluut-cli` configure output (file log in the OS data dir; console for CLI).

## 3. Job system

All long work (import, calculate, simulate, post, script) runs as jobs.

```rust
pub struct JobContext<'a> {
    pub cancel: &'a CancelToken,         // cooperative cancellation
    pub progress: &'a Progress,          // hierarchical progress reporting
    pub diag: &'a Diagnostics,           // diagnostic sink
}

pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken { pub fn check(&self) -> Result<, Cancelled>; }

pub struct Progress { /* atomic counters + optional parent */ }
impl Progress {
    pub fn child(&self, weight: f64, label: MessageRef) -> Progress;
    pub fn set_total(&self, units: u64);
    pub fn advance(&self, units: u64);
}
```

- Work runs on a `rayon` thread pool owned by `fluut-core`. The UI thread never runs jobs.
- Cancellation is cooperative. Algorithms call `cancel.check?` at least once per pass and per
  1 000 samples, so cancel latency stays under ~50 ms.
- Progress is lock-free (atomics) and sampled by the UI at frame rate.
- Each job gets a `JobId`; the UI subscribes to job events (`Started`, `Progress`, `Diagnostic`,
  `Finished(Result)`) over a channel (`crossbeam-channel`).

### 3.1 Snapshots, not locks

Jobs work on immutable snapshots. The project document is a persistent (structurally shared)
data structure (`im` / `rpds`-style, or `Arc`-shared nodes with copy-on-write). Starting a job
clones `Arc`s, not data. The UI keeps editing while jobs run. When a job finishes, its result is
accepted only if its input hash still matches the current inputs (§4); otherwise it is dropped
as stale.

Geometry (bodies, BVHs) is immutable after import and shared as `Arc<Body>`.

## 4. Invalidation graph and result cache

Every computed artefact is a node in a dependency graph keyed by a content hash of its inputs.

```
GeometrySource(file hash) ──► Body set ──► SetupFrame(setup params, bodies)
                                              │
                         Tool def ──┐         ▼
                    Toolpath params ─┼──► ToolpathResult ──► InProcessStock(after tp)
                                     │            │
                    previous stock ──┘            ├──► SimulationResult
                                                  └──► PostOutput(+ resolved post, machine)
```

- Hash: `blake3` over a canonical serialization of the node's inputs, including input node
  hashes and the FluutCAM algorithm version of the producing crate.
- A changed input changes the hash of every downstream node. Nodes whose hash changed are
  **stale** [S 08 §8.6]; they are not recomputed automatically unless auto-calculate is on.
- The on-disk cache [S 03 §3.4] stores `ToolpathResult` files named by node hash. Loading a
  project re-derives hashes and picks up matching cache files.
- In-process stock chains make toolpath order significant: changing toolpath 2 of a Setup makes
  toolpaths 3… that use rest machining stale too.

## 5. Session (`fluut-core`)

```rust
pub struct Session {
    doc: DocumentHandle,          // fluut-model document + undo stack (D06)
    jobs: JobScheduler,
    graph: InvalidationGraph,
    cache: ResultCache,           // memory LRU + disk sidecar
    library: DefinitionLibrary,   // tools, machines, posts, templates (D06)
}

impl Session {
    pub fn open(path: &Path) -> Result<Self>;
    pub fn apply(&mut self, edit: Edit) -> Result<EditOutcome>;   // all mutations go through here
    pub fn calculate(&mut self, targets: Targets) -> JobId;
    pub fn simulate(&mut self, target: SimTarget) -> JobId;
    pub fn post(&mut self, target: PostTarget, out: OutputSpec) -> JobId;
    pub fn events(&self) -> Receiver<SessionEvent>;
}
```

All mutations, from the UI, scripts, or CLI, are `Edit` values applied through
`Session::apply`. This gives one place for validation, undo recording, journalling (autosave)
and invalidation (D06 §6).

## 6. Determinism

Same inputs must give bit-identical outputs, on any thread count and platform. Reasons: cache
keys, golden-file tests of posts [S 05 §5.11], and the translation-invariance test
[S 04 §4.12.7].

Rules:

- Parallelism only over independent units (passes, faces, levels); results are collected in
  input order, never in completion order.
- No parallel floating-point reductions with non-deterministic order. Reductions use a fixed
  tree order.
- No `HashMap` iteration order in outputs; use `BTreeMap` or `IndexMap`.
- Results must be bit-identical across Windows, macOS and Linux (CH-01, resolved). All
  transcendental functions in `fluut-geom`, `fluut-cam`, `fluut-sim` and `fluut-post` use the
  pure-Rust `libm` crate; `f64::sin` and friends are banned there via clippy
  `disallowed_methods`.

## 7. Settings and paths

- App settings: TOML in the OS config dir (`directories` crate), e.g. units default, theme,
  update-check opt-out, crash-report consent.
- Definition library: user library dir (OS data dir) layered over built-ins embedded in the
  binary (`include_dir`), read-only.
- Logs and crash dumps: OS data dir.

## 8. Tests

- `xtask check-layers` enforces §1 rules.
- Determinism test: calculate the reference project set with 1, 2 and N threads; compare hashes.
- Cancellation test: every strategy, cancelled at random points, returns `Cancelled` within
  100 ms and leaks nothing.

## 9. Challenges

- CH-01 Cross-platform floating-point determinism (resolved: bit-identical required).
- CH-02 Persistent document structure choice and undo memory (deferred).
