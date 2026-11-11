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

### 1.1 Setup fields of a `three_axis` Setup (M3)

M3 read these Setup keys; §1.2 lists the forms M4 added.

| Key | Form | Meaning |
|---|---|---|
| `geometry` | `{ bodies = [ids] }` | bodies machined; absent = every body of every source |
| `orientation` | `{ tool_axis = "+Z", x_axis = "+X" }` | signed model axes (±X, ±Y, ±Z) that become +Z and +X of W; picks from faces later |
| `stock` | `{ kind = "block", margin = [x, y, z] }` | model bounds in the aligned frame grown by `x` on both X sides, `y` on both Y sides, `z` on top; the stock bottom is the model bottom |
| `zero` | `{ method = "stock_top_corner" \| "stock_bottom_corner", corner = "front_left" … }`, `{ method = "stock_top_centre" }`, `{ method = "point", at = [x, y, z] }` | work zero; front is −Y, left is −X; `point` is in M |
| `clearance` | `{ kind = "plane", offset = 10 }` | Setup clearance plane `offset` above the stock top (default 10 mm) |

Entity ids: body `b-<step id>`, its faces `f-<step id>.<i>` and edges `e-<step id>.<i>`, by
index in the body. WireBody curves use the edge form. Re-import matches them across file
versions (§5.1).

### 1.2 Setup fields added in M4

Every key is optional; an absent key is inherited (§2). Canonical order: `id`, `name`,
`based_on`, `kind`, `machine`, `material`, `work_offset`, `geometry`, `orientation`,
`index_angle`, `stock`, `zero`, `region`, `clearance`.

| Key | Forms added |
|---|---|
| `geometry` | `scale = [x, y, z]`, `mirror = [bool, bool, bool]`; applied before orientation |
| `orientation` | each axis is `"+X"`…, `{ face = id }` (plane normal, cylinder or cone axis) or `{ edge = id }` (line); X is projected normal to the tool axis. Rotary: `{ rotary_axis = pick, a_zero_toward = pick }` |
| `index_angle` | `{ mode = "absolute" \| "relative", value = degrees }` |
| `stock` | `{ kind = "block", size = [x, y, z] }` (centred in XY on the model, on the model bottom); `{ kind = "cylinder", radius, length }`; `{ kind = "body", body = id }`; `{ kind = "from_previous" }` |
| `zero` | rotary and indexed: `{ x = "stock_left" \| "stock_right" \| number }` |
| `region` | `{ floor_z = z }` (read and inherited; not yet used by the strategies) |
| `clearance` | `{ kind = "plane" }` (offset 10), `{ kind = "cylinder", radius }` |

Defaults without a parent: `three_axis`, orientation +Z/+X (rotary: axis +X), zero
`stock_top_corner` `front_left` (rotary: `x = "stock_left"`), block stock with no margin and a
plane clearance for `three_axis`; rotary and indexed roots need a stock
(`setup.stock.missing`). A toolpath's `tool` may be absent (no tool chosen yet); validation
reports `param.tool.missing`.

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
    OptimisedLoad2d(OptimisedLoad2dPasses),
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
extends = "fluut/optimised-load-default@1"
[toolpath]
type = "optimised_load_3d"
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
re-imported. A missing or changed file triggers the re-import flow [S 02 §2.6.3]. (M4: no body
cache yet; bodies are imported on every open.)

### 5.1 Re-import matching (M4)

`Session::reimport(source)` loads the file again and matches old to new:

- bodies by id, then by a unique STEP name, then by the most faces and edges in common;
- faces and edges by persistent id with an equal signature, then by signature (kind,
  parameters and bounds within the modelling tolerance), then by a unique surface (or curve)
  match. The STEP reader gives no AP242 persistent ids or face names yet, so the persistent id
  is the id above.

Signatures: plane normal (with face sense) and offset; cylinder and torus axis line and radii;
cone axis, origin, radius and angle; sphere centre and radius; line and conic curves by their
axis and size; bounds from the loop edges. Matched ids are rewritten (`RemapIds`) with the new
hash and body list in one edit group. Selections of unmatched entities stay in the project and
are reported (`model.reimport.unmatched`).

## 6. Edits and undo

### 6.1 Edits

As built (M4), payloads are in file syntax: a Setup, toolpath or source as its `[[…]]` text, a
field value as its inline TOML value. The project reader checks them, and the journal needs no
serde on the model types. The variants are `SetName`, `SetUnits`, `AddGeometry`, `SetGeometry`,
`RemoveGeometry`, `AddSetup { at }`, `SetSetupField`, `MoveSetup`, `RemoveSetup`,
`AddToolpath { at }`, `SetToolpathField` (item key or parameter path), `MoveToolpath`,
`RemoveToolpath`, `DuplicateAs`, `RemapIds` (re-import) and `Group`. Templates are expanded when
applied (`Library::apply_template` returns a `Group`), so replaying a journal needs no library.
An edit that leaves a `based_on` chain broken is refused. The design sketch:

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

### 7.2.1 Tool library references (2026-11-11)

Before this, tool libraries came only from `<project>/tools/*.toml` (loaded when the project
opens) and from `--tools <file>` on the command line. A library kept somewhere else, such as a
shop-wide Fusion import, could not be used by a project. The project file gets one key.

```toml
format         = "fluut.project/1.1"
name           = "Bracket"
units          = "mm"
tool_libraries = ["../../shared/fusion-mill.toml", "C:/shop/router-bits.toml"]
```

- **Key.** `tool_libraries`, a top-level array of strings after `units`, one library file each.
  `Project.tool_libraries: Vec<String>` holds the strings as written. Empty or absent means
  none; an empty list is not written.
- **Paths.** A reference is relative to the folder of the project file, written with `/` on every
  system, or absolute. The app writes a relative path whenever the library and the project share
  a folder below the root (`tool_ref_for`, purely lexical), else an absolute path. `Save As` into
  another folder rewrites the relative references so that they name the same files from the new
  place; the libraries themselves are shared and are not copied (unlike `tools/`, which is).
- **Order and duplicates.** All of these are the project layer, loaded in this order: the files
  of `tools/` by name, then the references in the order of the list, then `--tools` files. The
  rule that already held for `--tools` stays: of two libraries with the same id and version, the
  one loaded later replaces the earlier one (a different version is a different library). A
  reference whose library replaces one loaded from `tools/` or from an earlier reference is a
  warning, `project.tools.replaces`, so a shadowed library does not go unnoticed. The same file
  listed twice (after normalising `.` and `..`) is loaded once, with `project.tools.duplicate`.
  The built-in and user layers are below the project layer as before.
- **Missing or bad files.** A missing reference (or a folder) is the warning
  `project.tools.missing`, naming the file; the project opens, the other libraries load, and a
  toolpath that needs a tool of the missing library reports its missing tool as usual. The
  reference stays in the list (and in the file), so putting the file back and reopening works. A
  file that is not a valid tool library is `model.tools.load`, as for a bad file in `tools/`.
- **Format version.** The key is a minor change, `fluut.project/1.1`. The writer writes
  `fluut.project/1.1` only when `tool_libraries` is not empty and `fluut.project/1` otherwise, so
  every project without references is written byte for byte as before (the golden projects do
  not change). The reader accepts the key in any file of major 1. An older build reads
  `1.1` with the existing newer-minor warning ("newer fields are ignored and will not be saved");
  it does not know the key, so saving there would drop the references, and the warning says so.
- **Security.** A reference is a file path and nothing else. URLs (`https:`, `file:`, any
  `scheme:`) and network paths (`\server\share`, `//server/share`) are not used: the warning
  `project.tools.rejected` is given, nothing is fetched and no network share is contacted when a
  project is opened (on Windows a network path would start an authentication to that server).
  Control characters and references longer than 1024 bytes are rejected the same way. The file is
  parsed as data by the tool library reader; nothing in it is executed. Opening a project from
  someone else therefore reads the files it names, as `tools/` already did; the app lists the
  references in the browser so that they can be seen.
- **Edit.** `Edit::SetToolLibraries(Vec<String>)` replaces the whole list ("Change tool
  libraries", one undo step, journalled, marks the project changed). It refuses an unusable
  reference or a repeated one. `Session::apply`, `undo` and `redo` load the project-layer tool
  libraries again (`Session::reload_tool_libraries`: `tools/`, references, `--tools` files; the
  load warnings are replaced) when the list differs from the one loaded. So the app shows a new
  library at once, with no reload of the project, and `fluut calc`, `sim`, `post` and `run`
  honour the references through `Session::open_with` with nothing more.
- **App.** The Library ribbon group has **Add tool library...** (native open dialog, never a path
  box). The browser lists the references under **Tool libraries** with a Remove button and a
  mark on the ones that did not load. The **Import tools** result window offers **Use in this
  project**: for a library in `tools/` it only reads the libraries again; for any other it adds a
  reference.

### 7.3 Journal (autosave)

```
header:  magic "FLUJ", version, hash of the project file at journal start
records: [u32 length][postcard(Edit)][u32 crc32]   appended after every apply
```

- Records are `Edit`, `Undo` or `Redo` (as built), so recovery restores the undo state too.
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

As built (M4): `<hex>.tpr` = `FLUT` | schema `u16` | postcard of `{ version, hash, passes,
diagnostics }` (mirror types, so `fluut-path` needs no serde) | crc32. "Used" is the file time;
a hit refreshes it. Decoding rebuilds `ToolpathResult::new`, so a damaged file is a miss. A
result that cannot be stored bit for bit (spline segments, or an arc normal that is not
unchanged by `UnitVec3::new`) is not cached. The session uses the cache and the journal only
when `OpenOptions` asks for them; the CLI does not, so it writes nothing beside a project. The
hash stays SHA-256 (D01 §4).

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
