# 09 — Scripting

## 9.1 Purpose and scope

Scripts automate what a user does by hand:

- batch jobs: import a STEP file, apply a Setup template, calculate, post (e.g. per order);
- macros: repeated sequences in the app, run from the ribbon, command palette or a key;
- project checks: custom validation before posting (e.g. "all toolpaths use tools from library X");
- parametric work: create Setups and Toolpaths from data (e.g. drill patterns from a CSV).

Not in v1: custom toolpath strategies written in script (performance and safety), and hooks that
run automatically on events. Both MAY follow later.

## 9.2 Engine

| Item | Decision |
|---|---|
| Language | `rhai` (pure Rust, embeddable, sandboxable; DC-03) |
| File | `.rhai`; scripts live in the definition library (`scripts/`) or next to a project |
| Version guard | First line MAY declare `//! fluut >= 1.2`; older FluutCAM refuses to run the script with a clear message |
| Where scripts run | in the app (script console, ribbon, command palette) and headless: `fluut run job.rhai [args…]` (`fluut-cli`, 01 §1.8.2) |
| Undo | One script run is one undo step in the app |
| Cancellation | Long calls (calculate, import) report progress and can be cancelled; the script receives an error it may handle |

## 9.3 Sandbox

Scripts from other people are untrusted (01 §1.7). The engine runs with:

- no direct file system, network, process or environment access. File access only through the
  API calls below, limited to: the project folder, the definition library, and paths the user
  picks in a dialog (in the app) or passes as arguments (headless);
- limits on operations, call depth, string and array size, and run time (configurable; defaults
  set so normal jobs never hit them);
- no access to settings outside the project, and no ability to change built-in definitions.

The first time a script that did not come from the user's own library runs, the app shows its
source and the file locations it may write, and asks for confirmation.

## 9.4 Object API

The API mirrors the data model (03) and uses the **same keys as the project file** (02 §2.1).
File keys and scripting properties are one-to-one.

| Object | Main members |
|---|---|
| `fluut` | `version`, `project` (current, in app), `open(path)`, `new_project(units)`, `log(msg)`, `args` (headless) |
| `Project` | `model`, `setups`, `add_setup(map)`, `save`, `save_as(path)`, `calculate_all`, `post(setup_or_all, path)` |
| `Model` | `import(path, unit?)`, `reimport(source)`, `bodies`, `body(name)` |
| `Body`, `Face` | `name`, `faces`, `kind` (`"plane"`, `"cylinder"`, …), `params` (radius, axis, normal…), `bounds` — read-only geometry queries (03 §3.2) |
| `Setup` | all Setup keys as properties (`kind`, `index_angle`, `stock`, `zero`, …); `based_on`; `toolpaths`; `add_toolpath(type, map)`; `apply_template(name)`; `is_inherited(key)` / `reset(key)` (03 §3.9.4) |
| `Toolpath` | `type` (read-only), parameter groups as maps (`passes`, `heights`, `linking`, …); `calculate`; `state` (not calculated / done / stale / error); `result_summary` (time estimate, length, min/max Z) |
| `Library` | `tools(lib)`, `tool(id)`, `machines`, `posts`, `templates` |
| `Ui` (app only) | `message(text)`, `ask_number(label, default, unit)`, `ask_choice(label, options)`, `pick_faces(prompt)`, `pick_file(filter)` |

Setting a key a toolpath type does not declare is an error, as in the file format (03 §3.10.3).
Values with units accept strings with explicit units (`"0.25in"`), as in the UI (08 §8.13).

## 9.5 Example

```rust
//! fluut >= 1.0
// Drill every vertical hole of 6 mm or smaller in the top setup.
let p = fluut.project;
let top = p.setups[0];

let holes = [];
for body in p.model.bodies {
    for f in body.faces {
        if f.kind == "cylinder" && f.params.radius <= 3.0 && f.params.axis_is_vertical {
            holes.push(f.id);
        }
    }
}

top.add_toolpath("drill", #{
    tool: "user/drills#d6",
    geometry: #{ drive_faces: holes },
    passes: #{ cycle: "peck", peck_depth: 2.0 },
});
fluut.log(`Added drilling for ${holes.len} holes`);
```

## 9.6 API stability

The scripting API is versioned with FluutCAM's major version. Within a major version, members
are only added. Removed or changed members are deprecated for at least one minor release, and
the engine warns when a script uses them.

## 9.7 Still open

| Item |
|---|
| Full member list with types, generated from the Rust API (single source of truth) |
| Whether script-defined toolpath strategies or event hooks come after v1 |
