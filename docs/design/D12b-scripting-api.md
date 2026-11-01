# D12b — Scripting API Member List

Implements: [S 09 §9.4, §9.7]. Extends: [D12](D12-scripting.md) §3. Owed by the roadmap before M7.

This is every object, property and function a script can use. Choices still open are marked †
with the provisional choice taken; the M7 plan builds them as written here.

## 1. Conventions

**Types.** `int` is `i64`, `float` is `f64`, `string`, `bool`, `array`, `map` (`#{…}`, ordered by
key) and `()` (no value). Handles (`Project`, `Setup`, …) are thin references to objects in the
current document (D12 §3.1); they read the current state on every access.

**Values are the file's values.** A Setup field or toolpath parameter reads and writes the same
value the project file holds (spec 02 §2.1), converted one-to-one:

| TOML | Script |
|---|---|
| string | `string` |
| integer | `int` |
| float | `float` |
| boolean | `bool` |
| array | `array` |
| inline table | `map` |

Numbers are mm, mm/min, rpm, degrees and seconds, as in the file. Where the file takes a
length, a script may also give a string with a unit: `"0.25in"`, `"6mm"` (D15 §1.2). A plain
number string (`"6"`) is mm. † Unit strings are converted in every value except the identifier
keys (`id`, `name`, `based_on`, `machine`, `material`, `work_offset`, `tool`, `template`,
`type`), since no other string field can hold a number followed by `mm` or `in`.

**Errors.** Every failing call raises a runtime error whose message names the object and the
reason, in the same words as the file reader ("`passes.stepover_x` is not a field of
pocket_2d"). Scripts may catch them with `try { … } catch (e) { … }`. Errors that end a run
(limits, cancellation, `fluut.exit`) cannot be caught.

**Edits.** A member marked *E* applies one `Edit` (D06 §6.1) at once, so the next line sees the
change. The whole run is one undo step (D12 §3.4); see §9.

**Permissions.** A member marked *R* reads a file and *W* writes one; the path is checked as in
§8. Paths are strings; a relative path is relative to the project folder.

## 2. Global `fluut`

Visible everywhere, including inside functions and imported modules.

| Member | Type | Notes |
|---|---|---|
| `fluut.version` | `string` | FluutCAM version, e.g. `"0.1.0"` |
| `fluut.require(v)` | `()` | error unless `fluut.version >= v` (`"1"`, `"1.2"`, `"1.2.3"`); the runtime form of `//! fluut >= v` |
| `fluut.project` | `Project` | the current project; error when none is open |
| `fluut.has_project` | `bool` | † added so scripts can test before `fluut.project` |
| `fluut.open(path)` | `Project` | *R*. Opens a project file and makes it current. † Headless only: in the app the user opens projects |
| `fluut.new_project(units)` | `Project` | `"mm"` or `"inch"`; a project in memory, current until another is opened. It has no folder until `save_as` |
| `fluut.log(msg)` | `()` | info line in the script console (any value; converted with `to_string`) |
| `fluut.warn(msg)` | `()` | † warning line in the console |
| `fluut.args` | `array` of `string` | headless arguments after `--`; empty in the app |
| `fluut.exit(code)` | — | ends the run with exit code `code` (`int`, 0–255); the run's edits are kept |
| `fluut.read_csv(path)` | `array` of `map` | *R*. First row is the header. † Cells that parse as numbers are `float`, others `string`; empty cells are `()` |
| `fluut.library` | `Library` | the definition library of the current project (built-in, user, project layers) |
| `fluut.ui` | `Ui` | the app's dialogs (§7) |
| `print(x)`, `debug(x)` | `()` | Rhai built-ins, to the console (info and debug) |

## 3. `Project`

| Member | Type | Notes |
|---|---|---|
| `name` | `string` | *E* (`SetName`) when set |
| `units` | `string` | `"mm"` or `"inch"`; *E* (`SetUnits`) when set. Display unit only: values stay mm |
| `path` | `string` or `()` | the project file |
| `model` | `Model` | |
| `setups` | `array` of `Setup` | machining order |
| `setup(name_or_id)` | `Setup` | by id, then by name; error when absent or ambiguous |
| `toolpaths` | `array` of `Toolpath` | every toolpath, in machining order |
| `toolpath(name_or_id)` | `Toolpath` | by id, then by name |
| `add_setup(map)` | `Setup` | *E* (`AddSetup`). `map` holds Setup keys (§4). † `id` is generated (`s1`, `s2`, …, the first free) unless given; `name` defaults to `"Setup n"` |
| `apply_template(ref, map?)` | `Setup` | *E*. Applies a Setup template (D06 §4.4) as a new last Setup. `map` may hold `id`, `name`, `tool` (the default tool of its toolpaths) |
| `save()` | `()` | *W*. Saves to `path` (atomic, D06 §7.2); error when the project has none |
| `save_as(path)` | `()` | *W*. Saves to a new file, which becomes `path`; geometry paths are rewritten relative to the new folder (*E*) |
| `calculate_all()` | `array` of `map` | calculates every stale toolpath; one report per toolpath (§5.3) |
| `post(target, path, opts?)` | `map` | *W*. Calculates what is stale, then posts. `target` is `"all"` or a `Setup`. `opts`: `post` (reference; default the machine's post), `date`, `time`, `notes`. Returns `#{ files: [paths], warnings: [strings] }`. Fails with the error diagnostics joined |

## 4. `Setup`

Setup keys as properties, one-to-one with the file (spec 02 §2.1, D06 §1):

| Property | Read | Write |
|---|---|---|
| `id` | `string` | read-only |
| `name` | `string` | *E* `string` |
| `based_on` | `string` or `()` | *E* `string`, or `()` to clear |
| `kind`, `machine`, `material`, `work_offset` | `string` | *E* |
| `geometry`, `orientation`, `index_angle`, `stock`, `zero`, `region`, `clearance` | `map` or `string` (file form) | *E* |

Reading an inheritable field gives its **resolved** value: local, inherited or default (D06 §2).
Writing makes it local (`SetSetupField`). Assigning `()` resets it to inherited.

| Member | Type | Notes |
|---|---|---|
| `is_inherited(key)` | `bool` | true when the field is not local (inherited or default) |
| `origin(key)` | `string` | † `"local"`, `"default"`, or the id of the Setup it comes from |
| `reset(key)` | `()` | *E*. Same as assigning `()` |
| `toolpaths` | `array` of `Toolpath` | |
| `toolpath(name_or_id)` | `Toolpath` | |
| `add_toolpath(type, map)` | `Toolpath` | *E* (`AddToolpath`). `map` holds item keys (`id`, `name`, `tool`, `enabled`) and parameter groups (`tool_params`, `geometry`, `heights`, `passes`, `linking`) as maps. Unknown keys are errors (spec 03 §3.10.3). † `id` defaults to `t1`, `t2`, … (first free); `name` to the type |
| `apply_template(ref, map?)` | `Toolpath` | *E*. Applies a toolpath template as a new last toolpath. `map` may hold `id`, `name`, `tool` |
| `calculate()` | `array` of `map` | reports for this Setup's toolpaths |
| `remove()` | `()` | *E* (`RemoveSetup`) with its toolpaths |

## 5. `Toolpath`

### 5.1 Item keys

| Property | Type | Notes |
|---|---|---|
| `id` | `string` | read-only |
| `type` | `string` | read-only (spec 09 §9.4) |
| `name` | `string` | *E* |
| `tool` | `string` | *E*; a tool reference `lib#tool` or `lib@v#tool` |
| `enabled` | `bool` | *E* |
| `template` | `string` or `()` | read-only † (the template a toolpath came from is a record, not a setting) |
| `setup` | `Setup` | the Setup holding it |

### 5.2 Parameter groups

`tool_params`, `geometry`, `heights`, `passes`, `linking` each give a `ParamGroup`:

| Member | Type | Notes |
|---|---|---|
| `group.key`, `group["key"]` | file form | the **local** value, or `()` when the field takes its default. † Defaults that depend on the tool are not resolved here |
| `group.key = v` | — | *E* (`SetToolpathField`, path `group.key`). `()` clears it. A key the type does not declare is an error |
| `group.keys()` | `array` of `string` | local keys, in field order |
| `group.fields()` | `array` of `string` | every key the type declares in this group |
| `group.to_map()` | `map` | the local values |
| `toolpath.passes = map` | — | *E*. Replaces the group: keys absent from `map` are cleared (one edit group) |

Inline tables read as maps; `tp.linking.ramp.angle_deg = 3` reads the map, changes it and writes
the whole `linking.ramp` back (one edit).

### 5.3 Calculation

| Member | Type | Notes |
|---|---|---|
| `calculate()` | `map` | calculates this toolpath (a session job); the report below |
| `state` | `string` | `"not_calculated"`, `"done"`, `"stale"` or `"error"` |
| `result_summary` | `map` or `()` | `cut_length`, `rapid_length` (mm), `min_z`, `max_z` (W), `min`, `max` (`[x, y, z]`); `()` unless `state == "done"`. † No time estimate until the machine model gives one (D11) |
| `diagnostics` | `array` of `map` | from the last calculation: `code`, `severity` (`"error"` or `"warning"`), `message`, `field` |
| `remove()` | `()` | *E* (`RemoveToolpath`) |

A report is `#{ toolpath: id, state: "calculated" | "up_to_date" | "failed" | "disabled",
diagnostics: [...] }` plus `cut_length` and `rapid_length` when there is a result.

† `state` is `"done"` when an up-to-date result exists, `"error"` when the latest calculation of
the toolpath failed and nothing changed since, `"stale"` when a result was calculated before but
its inputs have changed, and `"not_calculated"` otherwise.

## 6. Model, `Body`, `Face`

| Member | Type | Notes |
|---|---|---|
| `model.import(path)`, `model.import(path, unit)` | `array` of `Body` | *R*, *E* (`AddGeometry`). Kind from the extension (`step`/`stp`, `stl`, `dxf`, `svg`). `unit` is needed for STL and DXF (spec 02 §2.1). † The project must have a folder (opened or saved): source paths are stored relative to it |
| `model.reimport(source_id)` | `map` | *R*, *E* (`SetGeometry` and `RemapIds`). `#{ matched: n, unmatched: [ids], diagnostics: [...] }` (spec 03 §3.5) |
| `model.sources` | `array` of `map` | `id`, `path`, `kind`, `unit` |
| `model.bodies` | `array` of `Body` | every body of every source |
| `model.body(name_or_id)` | `Body` | |

`Body` and `Face` are read-only geometry queries in the model frame M (spec 03 §3.2), mm.

| Member | Type | Notes |
|---|---|---|
| `body.id`, `body.name`, `body.source` | `string` | |
| `body.kind` | `string` | `"solid"` (B-rep) or `"wire"` (2-D curves) |
| `body.faces` | `array` of `Face` | empty for wire bodies |
| `body.face(id)` | `Face` | |
| `body.bounds` | `map` | `#{ min: [x, y, z], max: [x, y, z] }` |
| `face.id`, `face.body` | `string` | `f-<body>.<n>` (D06 §1.1) |
| `face.kind` | `string` | `"plane"`, `"cylinder"`, `"cone"`, `"sphere"`, `"torus"`, `"bspline"`, `"extrusion"`, `"revolution"`, `"offset"` |
| `face.params` | `map` | by kind, below |
| `face.bounds` | `map` | from the face's edges, as `body.bounds` |

| Kind | `params` |
|---|---|
| plane | `normal` (outward, `[x, y, z]`), `origin`, `offset` (`normal · origin`), `normal_is_vertical` |
| cylinder | `axis`, `origin` (a point on the axis), `radius`, `diameter`, `axis_is_vertical`, `inside` (a hole: the face's normal points to the axis) |
| cone | `axis`, `origin`, `radius` (at `origin`), `half_angle_deg`, `axis_is_vertical`, `inside` |
| sphere | `centre`, `radius`, `inside` |
| torus | `axis`, `centre`, `major_radius`, `minor_radius`, `axis_is_vertical`, `inside` |
| others | empty map |

† "Vertical" means parallel to model Z within 1e-9 in the direction cosines.

## 7. `Library` and `Ui`

| Member | Type | Notes |
|---|---|---|
| `library.tools(lib)` | `array` of `map` | tools of library `lib` (`id`, `name`, `number`, `shape`, `diameter`, `corner_radius`, `flute_length`, `ref`) |
| `library.tool(ref)` | `map` or `()` | one tool, same keys |
| `library.tool_libraries` | `array` of `string` | † library ids |
| `library.machines`, `library.posts`, `library.templates` | `array` of `string` | references `id@major.minor`, all layers, sorted |
| `ui.message(text)` | `()` | |
| `ui.ask_number(label, default, unit)` | `float` | `unit` is a display hint (`"mm"`, `"deg"`, `""`) |
| `ui.ask_choice(label, options)` | `string` | one of `options` |
| `ui.pick_faces(prompt)` | `array` of `string` | face ids |
| `ui.pick_file(filter)` | `string` | the path is added to the run's allowed paths |

Every `ui` member fails with "no UI in headless mode" outside the app; a script may catch it.
In the app the call blocks the script until the user answers (D12 §2). Cancelling a dialog is a
catchable error.

## 8. Permissions and limits

A run has `Permissions` (D12 §1): the project folder, the library folders (read only †), the
allowed paths (files or folders picked in a dialog or given as headless arguments), and
whether a UI is present. A path is allowed when, after removing `.` and `..` and resolving
symbolic links of its existing part, it lies inside one of these roots. Write access is the
project folder and the allowed paths. A denied path is a catchable error naming the path.

`import "name" as m;` loads `scripts/name.rhai` from the library folders only (D12 §1); names
are relative, without `..`, and without the extension.

Limits (D12 §1), all configurable per run; defaults never hit by normal jobs:

| Limit | Default |
|---|---|
| operations | 50 000 000 |
| call depth | 64 |
| expression depth | 64 (32 in functions) |
| string length | 1 MiB |
| array length | 1 Mi elements (nested arrays counted) |
| map size | 64 Ki entries |
| variables in scope | 10 000 † (rhai checks it where a variable is not resolved at compile time; others are bounded by the script's length and the call depth) |
| imported modules | 64 † |
| memory of all variables | 16 Mi cells † (an array element or map entry is a cell, 16 bytes of string a cell); checked every few variable reads, because rhai checks a value's size only when it is passed or assigned whole |
| script time | 300 s †, counting only time in script code, not in calculation or post jobs |

A limit, a cancellation or `fluut.exit` ends the run; none can be caught.

## 9. Undo and determinism

The session opens an undo group when the run starts and closes it when the run ends, also on
an error: the run is one undo step labelled `Script <file name>` (spec 09 §9.2). A run that
changed nothing leaves no step.

Scripts cannot read the clock, the environment or random numbers (Rhai's `timestamp` is not
built in). Maps iterate in key order. The trigonometric, exponential and power functions are
replaced by `libm` versions (D01 §6). So a script run on the same inputs gives the same
document and the same outputs on every platform.

## 10. Not in v1

- Edges as script objects (`body.edges`), and picking edges in `Ui`.
- Moving Setups and toolpaths, and `duplicate_as` (the edits exist; scripts can remove and add).
- Generated API documentation from the registered functions (D12 §5).
- Cancelling a calculation in progress: cancellation takes effect when the call returns.
