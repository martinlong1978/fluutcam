# Scripting

A **script** is a short program that drives FluutCAM: it opens a project, changes settings, calculates, posts and saves, without you clicking. Use scripts to repeat a job on many projects, to change a setting across many toolpaths, or to build a project from a table.

Scripts are written in [Rhai](https://rhai.rs), a small language that looks like Rust and JavaScript. Files end in `.rhai`.

> **Today scripts run from the command line only**: `fluut run script.rhai project.fluut`. Running a script from the window, and the dialogs scripts can ask for (`fluut.ui`), are **not yet** available. See [Known limits](Known-Limits).

## A first script

Save this as `job.rhai` next to a copy of the `block` reference project:

```rhai
// Make the pockets coarser, calculate, post, save.
let p = fluut.project;
fluut.log(`${p.name} has ${p.setups.len} Setup(s)`);
for t in p.toolpaths {
    if t.type == "pocket_2d" {
        t.passes.stepover = 2.0;
    }
}
for r in p.calculate_all() {
    fluut.log(`${r.toolpath}: ${r.state}`);
}
let cut = p.toolpath("t3");
fluut.log(`${cut.name}: ${cut.state}, cut ${cut.result_summary.cut_length} mm`);
let out = p.post("all", fluut.args[0], #{ post: "fluut/linuxcnc" });
fluut.log(`wrote ${out.files.len} file(s)`);
p.save();
```

Run it:

```
fluut run job.rhai block.fluut -- out.ngc
```

```
Block has 1 Setup(s)
t1: calculated
t2: calculated
t3: calculated
Cut out: done, cut 1864.3432229977511 mm
wrote 1 file(s)
```

Everything after `--` goes to the script as `fluut.args` (here `out.ngc`) and the script may write those paths. The whole run is one undo step, but a headless run keeps no undo history, so call `p.save()` to keep your changes.

## The language in a minute

- `let x = 5;` makes a variable. `if`, `for item in list { }`, `while`, functions (`fn name(a) { }`) work as you expect.
- Text in backticks can hold `${expressions}`: `` `cut ${n} mm` ``.
- Lists are `[1, 2, 3]`. Maps are `#{ key: value }`. Both are written in file order.
- `try { ... } catch (e) { ... }` catches an error.
- One quirk: you cannot assign to the result of a call. Write `let t = p.toolpath("t2"); t.enabled = false;`, not `p.toolpath("t2").enabled = false;`. A property chain works: `t.passes.stepover = 2.0`.

## Values

A setting reads and writes the same value the project file holds, in the same units: millimetres, mm/min, rev/min, degrees and seconds. Where the file takes a length, a script may give text with a unit: `"0.25in"`, `"6mm"`.

| In the file | In a script |
|---|---|
| text | text |
| whole number | int |
| decimal number | float |
| true or false | bool |
| list | array |
| inline table | map |

## What a script can use

### `fluut`

| Member | What it is |
|---|---|
| `fluut.version` | The FluutCAM version as text |
| `fluut.require("1.2")` | Stops with an error unless the version is at least that |
| `fluut.project` | The current project (an error when none is open) |
| `fluut.has_project` | True when a project is open |
| `fluut.open(path)` | Opens a project file and makes it current (headless only) |
| `fluut.new_project("mm")` | A new project in memory, `"mm"` or `"inch"`; it has no folder until `save_as` |
| `fluut.log(x)`, `fluut.warn(x)` | Print a line (`print(x)` works too) |
| `fluut.args` | The arguments after `--` |
| `fluut.exit(code)` | Ends the run with that exit code (0 to 255); the edits are kept |
| `fluut.read_csv(path)` | Reads a CSV file into a list of maps, from the header row. Numbers become numbers |
| `fluut.library` | The tools, machines, posts and templates the project can see |

### Project

| Member | What it does |
|---|---|
| `name`, `units` | Read or set. `units` is `"mm"` or `"inch"` and is for display only |
| `path` | The project file |
| `setups`, `setup(name_or_id)` | The Setups, in order, and one by id or name |
| `toolpaths`, `toolpath(name_or_id)` | Every toolpath, and one by id or name |
| `model` | The model (below) |
| `add_setup(map)` | Adds a Setup; `map` holds Setup keys. The id and name are made up if you leave them out |
| `apply_template(ref)` | Adds a Setup from a template |
| `calculate_all()` | Calculates what is out of date; one report per toolpath |
| `post(target, path, opts)` | Calculates what is out of date, then posts. `target` is `"all"` or a Setup. `opts` can hold `post`, `date`, `time`, `notes`. Returns `#{ files: [...], warnings: [...] }` |
| `save()`, `save_as(path)` | Saves the project |

### Setup

Its keys are properties: `id` (read only), `name`, `based_on`, `kind`, `machine`, `material`, `work_offset`, `geometry`, `orientation`, `index_angle`, `stock`, `zero`, `region`, `clearance`. Reading gives the value the Setup really uses (its own, the parent's, or the default). Writing makes the value the Setup's own. Writing `()` goes back to inheriting.

Also: `is_inherited(key)`, `origin(key)` (`"local"`, `"default"` or the Setup it comes from), `reset(key)`, `toolpaths`, `toolpath(name)`, `add_toolpath(type, map)`, `apply_template(ref)`, `calculate()`, `remove()`.

### Toolpath

`id` and `type` are read only. `name`, `tool` (as `"library#tool"`) and `enabled` can be set. `setup` is the Setup it is in.

The five groups `tool_params`, `geometry`, `heights`, `passes`, `linking` are objects: `t.passes.stepover = 2.0` sets a value, `t.passes.stepover` reads the value you set (or `()` when it takes its default), `t.passes.stepover = ()` goes back to the default. `group.keys()` lists what you set, `group.fields()` lists everything the type has, and `group.to_map()` gives the values. A key the type does not have is an error: "`passes.stepover_x` is not a field of pocket_2d". The setting names are the ones on the [toolpath type pages](Toolpath-Types).

`t.calculate()` calculates that toolpath. `t.state` is `"not_calculated"`, `"done"`, `"stale"` or `"error"`. `t.result_summary` has `cut_length`, `rapid_length`, `min_z`, `max_z`, `min` and `max` when the state is `"done"`. `t.diagnostics` lists the last calculation's messages, each with `code`, `severity`, `message` and `field`. `t.remove()` deletes the toolpath.

### Model, bodies and faces

`project.model` has `import_file(path)` (and `import_file(path, unit)`; STL and DXF need the unit), `reimport(source_id)`, `sources`, `bodies` and `body(name)`. A body has `id`, `name`, `kind` (`"solid"` or `"wire"`), `faces`, `bounds`. A face has `id`, `kind` (`"plane"`, `"cylinder"`, `"cone"`, `"sphere"`, ...), `params` (for a plane: `normal`, `origin`; for a cylinder: `axis`, `radius`, `diameter`, `inside`, ...) and `bounds`. These are for looking at the model, for example to pick all the holes of one diameter; they never change it.

### Library

`fluut.library.tools("fluut/starter")` lists a library's tools, `fluut.library.tool("fluut/starter#flat-6")` gives one, and `library.machines`, `library.posts` and `library.templates` list the ids.

## Errors, limits and safety

- An error names what went wrong in the same words as the project file reader. Catch it with `try`.
- A script may read and write the **project folder** and the paths given after `--`. Anything else is denied: "permission denied: the script may not write ...". It may load modules (`import "name" as m;`) from the project's `scripts/` folder only.
- A script cannot read the clock, the environment or random numbers. The same script on the same project gives the same result on every computer.
- Limits stop a runaway script: 50 million operations, call depth 64, strings of 1 MiB, arrays of about a million items, and 300 seconds in script code. A limit ends the run and cannot be caught.
- A project is not read or written outside the paths above, and scripts cannot start programs or use the network.

## Not yet

- Running a script from the window, and `fluut.ui` (dialogs for a script that asks you for a number, a choice or faces).
- Edges as script objects, moving a Setup or toolpath, and duplicating one.
- Cancelling a calculation that a script started, before the call returns.
- Generated reference documentation for every function; the member list above follows the [design](https://github.com/martinlong1978/fluutcam/blob/main/docs/design/D12b-scripting-api.md).

## Next

[Settings and updates](Settings-and-Updates)
