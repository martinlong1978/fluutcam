# D06 — Project Model, Definitions, Persistence and Undo (`fluut-model`)

Implements: [S 02 §2.0–2.4], [S 03 §3.1, §3.3, §3.7, §3.9–3.10], [S 01 §1.9], OQ-03, OQ-12,
OQ-18. Depends on: `fluut-geom`, `fluut-topo`, `fluut-path`, `fluut-io-*`, `serde`, `toml`,
`serde_ignored`, `postcard`, `blake3`, `tempfile`, `crc32fast`.

## 1. Document structure

The document is a tree of `Arc` nodes with copy-on-write at Setup and Toolpath granularity
(CH-02). Geometry is not in the document; it is referenced by id and held by the session's
model store (§5).

```rust
pub struct Project {
    pub format: FormatVersion,              // "fluut.project/1"
    pub name: String,
    pub units: DisplayUnit,                 // mm | inch  (S 08 §8.10a)
    pub geometry: Vec<Arc<GeometrySource>>,
    pub setups: Vec<Arc<Setup>>,            // machining order
    pub views: Vec<SavedView>,
}

pub struct GeometrySource {
    pub id: PersistId, pub path: RelPath, pub kind: SourceKind,
    pub hash: Blake3Hash, pub unit: Option<LengthUnit>,   // required for stl/dxf/svg
    pub bodies: Vec<BodyEntry>,                            // id, name, include
}

pub struct Setup {
    pub id: PersistId,
    pub name: String,
    pub based_on: Option<PersistId>,
    pub f: SetupFields,                     // every field inheritable (§2)
    pub toolpaths: Vec<Arc<Toolpath>>,
}

pub struct SetupFields {
    pub kind: Inh<SetupKind>,               // three_axis | rotary | indexed
    pub machine: Inh<DefRef>,
    pub work_offset: Inh<WorkOffset>,
    pub geometry: Inh<SetupGeometry>,       // bodies, scale, mirror
    pub orientation: Inh<Orientation>,
    pub index_angle: Inh<IndexAngle>,
    pub stock: Inh<StockSpec>,
    pub zero: Inh<ZeroSpec>,
    pub region: Inh<Region>,
    pub clearance: Inh<Clearance>,
}
pub type Inh<T> = Option<T>;                // None = inherited

pub struct Toolpath {
    pub id: PersistId,
    pub name: String,
    pub enabled: bool,
    pub tool: ToolRef,
    pub common: CommonParams,               // Tool, Geometry, Heights, Linking groups (S 03 §3.10.2)
    pub kind: ToolpathKind,                 // type + Passes group (D09 lists the variants)
    pub template: Option<DefRef>,           // template it came from, for display only
}
```

## 2. Setup inheritance

```rust
pub struct ResolvedSetup { /* every field concrete */ pub origin: FieldOrigins }

pub fn resolve_setup(project: &Project, id: PersistId) -> Result<ResolvedSetup, ModelError>;
```

```
resolve(s):
    parent = s.based_on.map(resolve)            // memoised per project revision
    for each field f:
        value(f)  = s.f.f  or  parent.f  or  default_for(kind, f)
        origin(f) = Local | Inherited(from setup id) | Default
    index_angle: absolute → value; relative → parent.angle (0 if parent is rotary) + value
    validate (S 03 §3.9.4): parent earlier in order; if stock = from_previous then
        machine, geometry, rotary axis line and zero must equal the parent's resolved values
```

`FieldOrigins` drives the "inherited from Setup 1" chips in the inspector [S 08 §8.5].

## 3. Toolpath parameters

### 3.1 Common groups

```rust
pub struct CommonParams {
    pub tool:     ToolGroup,      // rpm, feeds (cut, lead_in, lead_out, ramp, plunge, transition), coolant
    pub geometry: GeometryGroup,  // drive/check/excluded faces, boundary, rest_from
    pub heights:  HeightsGroup,   // clearance, retract, feed, top, bottom: Height { reference, offset }
    pub linking:  LinkingGroup,   // retract policy, leads, ramp, transitions, ordering
}
```

Every field is `Option<T>`; `None` means "default", resolved from the tool preset
[S 02 §2.4] and the Setup (heights from stock and clearance). Resolution happens at calculation
time, so changing a tool preset updates every toolpath that uses its defaults.

### 3.2 Type-specific groups

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "passes", rename_all = "snake_case")]
pub enum ToolpathKind {
    Face(FacePasses),
    Adaptive2d(Adaptive2dPasses),
    // … all v1 types (D09 §1)
}
```

Each `*Passes` struct:

- derives `Serialize`, `Deserialize` with `deny_unknown_fields` (unknown parameter = error,
  [S 02 §2.1]);
- implements `ToolpathType` (D08 §1) to declare allowed Setup kinds, used common fields,
  validation, and UI metadata.

### 3.3 Field metadata

The UI editor and the script API are generated from metadata, not hand-written per type:

```rust
pub struct FieldMeta {
    pub key: &'static str,           // file and script key
    pub group: Group,                // Tool | Geometry | Heights | Passes | Linking
    pub label: MessageId,            // Fluent id
    pub help: MessageId,
    pub kind: FieldKind,             // Length{min,max} | Angle | Percent | Count | Enum(&[..]) | Bool | FaceSet | Curve | …
    pub default: DefaultRule,        // Const | FromTool(key) | FromSetup(key) | Computed(fn)
    pub visible_if: Option<Cond>,    // e.g. finishing_passes > 0
}
```

A derive macro (`#[derive(Params)]` in `fluut-model-derive`) generates `FieldMeta` tables from
attributes on the struct fields.

## 4. Definitions

### 4.1 Library layers

```
built-in   (embedded in the binary, read-only, hash-checked)  ── fluut/…
user       (OS data dir: posts/, machines/, tools/, templates/, scripts/) ── user/…
imported   (user dir, origin = imported)                       ── user/… with origin
project    (optional: definitions stored beside the project)   ── project/…
```

### 4.2 Versions

Definition versions are `major.minor` (e.g. `3.2`; CH-11, confirmed). References:

| Syntax | Meaning |
|---|---|
| `fluut/linuxcnc` | latest available |
| `fluut/linuxcnc@3` | any 3.x, latest |
| `fluut/linuxcnc@3.2` | exactly 3.2 |

A minor version only adds or fixes; a major version may change output. `extends` should use the
major form.

### 4.3 Generic layered merge

Posts, machines and templates share one resolver working on TOML value trees before typed
deserialization:

```
resolve(def):
    if def in visiting: error Cycle
    if depth > 16: error TooDeep
    base = def.extends ? resolve(lookup(def.extends)) : {}
    for m in def.mixins: base = merge(base, resolve(lookup(m)), conflicts → warnings)
    out = merge(base, def.own)
    record provenance: for each leaf path, the layer (id@version) that supplied it
merge(a, b): tables deep-merge; scalars and arrays: b replaces a;
             tables with a "mode" key are handed to a kind-specific merger (posts: D10 §2)
```

The provenance map backs the post `explain` view [S 05 §5.2.4] and the "inherited" display for
templates.

### 4.4 Templates (OQ-18)

```toml
format  = "fluut.template/1"
id      = "user/mdf-roughing"
version = "1.0"
kind    = "toolpath"                # toolpath | setup
extends = "fluut/adaptive-default@1"
[toolpath]
type = "adaptive_3d"
[toolpath.passes]
optimal_load = "0.4*tool.diameter"
```

A setup template holds Setup fields (without geometry selection) and an ordered list of toolpath
templates or inline toolpaths. Applying a template creates normal toolpaths; later template
edits do not change existing toolpaths (the `template` field is informational).

## 5. Model store

```rust
pub struct ModelStore {
    sources: HashMap<PersistId, LoadedSource>,     // bodies as Arc<Body>, import report
    frames:  LruCache<FrameKey, Arc<SetupGeometry>>,  // transformed bodies per Setup frame (D03 §4.3)
}
```

On open, each `GeometrySource` is loaded: if a body cache file `<hash>.bodies` exists in the
sidecar and matches the file hash and importer version, it is used; otherwise the file is
re-imported. A missing or changed file triggers the re-import flow [S 02 §2.6.3].

## 6. Edits and undo

### 6.1 Edits

```rust
pub enum Edit {
    SetProjectField(ProjectField),
    AddGeometry { path: PathBuf, unit: Option<LengthUnit> },
    ReimportGeometry(PersistId),
    AddSetup { after: Option<PersistId>, init: SetupInit },
    SetSetupField { setup: PersistId, field: SetupFieldValue },   // Some = local, None = reset to inherited
    MoveSetup { id: PersistId, to: usize },
    RemoveSetup(PersistId),
    AddToolpath { setup: PersistId, after: Option<PersistId>, init: ToolpathInit },
    SetToolpathParam { toolpath: PersistId, key: ParamKey, value: Option<ParamValue> },
    DuplicateAs { toolpath: PersistId, new_type: ToolpathTypeId },
    MoveToolpath { id: PersistId, setup: PersistId, to: usize },
    RemoveToolpath(PersistId),
    ApplyTemplate { target: TemplateTarget, template: DefRef },
    Group { label: MessageRef, edits: Vec<Edit> },                  // scripts, multi-step UI actions
}
```

`apply` validates, produces a new root `Arc<Project>`, records undo, appends to the journal and
returns the set of changed node ids for invalidation (D01 §4).

### 6.2 Undo

Undo stack entries: `(label, previous root Arc<Project>, changed ids)`. Because Setups and
Toolpaths are shared `Arc`s, an entry costs only the nodes that changed. Limits: unlimited by
count, capped at 256 MB of unshared nodes (oldest dropped) [S 08 §8.8].

Coalescing: consecutive `SetToolpathParam`/`SetSetupField` on the same key within 750 ms (e.g.
scrubbing a value) merge into one entry.

## 7. Persistence

### 7.1 Files

| File | Content |
|---|---|
| `name.fluut` | project TOML [S 02 §2.1] |
| `name.fluut.bak` | previous saved version |
| `name.fluut-cache/` | `*.tpr` toolpath results, `*.bodies` imported bodies, `index.toml` |
| `name.fluut-autosave` | edit journal (§7.3) |
| `name.fluut.lock` | lock (§7.4) |

### 7.2 Reading and writing TOML

- Read: parse to `toml::Value`, check `format` (unknown major → error; newer minor → warning),
  run migrations for older minors, then deserialize with `serde_ignored` to collect unknown
  keys as warnings (except `passes` groups, which deny unknown keys).
- Write: canonical key order (matches the spec examples), inherited (`None`) fields omitted
  [S 02 §2.1], numbers with shortest round-trip formatting (`ryu`), so saving an unchanged
  project produces an identical file (clean diffs).
- Atomic save: write `name.fluut.tmp` in the same directory, `fsync`, rename current to `.bak`,
  rename temp to `name.fluut` (`tempfile::NamedTempFile::persist`; on Windows `ReplaceFileW`
  semantics).

### 7.3 Journal (autosave)

```
header:  magic "FLUJ", version, hash of the project file at journal start
records: [u32 length][postcard(Edit)][u32 crc32]   appended after every apply
```

- `fsync` at most once per second (batched).
- On open: if a journal exists whose header hash equals the current project file hash, offer
  recovery; replay records up to the first bad CRC. After a successful save the journal is
  truncated and its header updated.

### 7.4 Lock

`name.fluut.lock` holds pid, hostname, user and start time. A second instance opening the
project sees the lock and opens read-only [S 01 §1.9]. A lock whose process no longer exists on
the same host is treated as stale and taken over.

### 7.5 Result cache (OQ-03)

```
<cache>/<node-hash>.tpr = magic "FLUT" | schema u16 | fluut version | input hash | postcard(ToolpathResult) | crc32
```

Lookup by node hash (D01 §4). On save, cache files not referenced by the current project and
not used in the last 7 days are deleted.

## 8. Tests

- Round trip: every spec example project parses and re-serialises byte-identically.
- Inheritance: property tests over random Setup chains; resolved values equal manual
  computation; `from_previous` validation.
- Journal: kill-at-random-point test (truncate journal at every byte offset) recovers to a
  valid prefix.
- Undo: random edit sequences, undo all, document equals the original (structural equality).
- Definitions: cycles, missing parents, mixin conflicts, provenance correctness.

## 9. Challenges

- CH-02 Document data structure and undo memory (from D01).
- CH-11 Definition version scheme (resolved: major.minor).
