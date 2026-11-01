# D12 — Scripting Engine (`fluut-script`)

Implements: [S 09]. Depends on: `rhai` (features `sync`, `no_closure` off, `serde`),
`fluut-core`, `fluut-model`.

## 1. Engine configuration

```rust
pub fn make_engine(perm: Permissions, limits: Limits, host: Arc<dyn ScriptHost>) -> rhai::Engine {
    let mut e = rhai::Engine::new_raw;               // no default packages
    e.register_global_module(StandardPackage::new.as_shared_module); // core, math, string, array, map
    e.disable_symbol("eval");                          // no dynamic code
    e.set_module_resolver(LibraryResolver::new(perm.library_dirs.clone)); // `import` only from library
    e.set_max_operations(limits.ops);                  // default 50 M
    e.set_max_call_levels(64);
    e.set_max_string_size(1 << 20);
    e.set_max_array_size(1 << 20);
    e.set_max_map_size(1 << 16);
    e.on_progress(move |ops| host.poll(ops));          // cancellation + wall-clock limit
    e.on_print(…); e.on_debug(…);                      // → script console / log
    register_api(&mut e, host, perm);                  // §3
    e
}
```

No file, network, process or environment access exists in the engine; all I/O goes through API
functions that check `Permissions` [S 09 §9.3].

```rust
pub struct Permissions {
    pub project_dir: Option<PathBuf>,
    pub library_dirs: Vec<PathBuf>,
    pub allowed_paths: Vec<PathBuf>,     // picked by the user in dialogs, or CLI arguments
    pub ui: bool,                        // Ui.* available (app only)
}
```

## 2. Threading and host

Scripts run on a job thread (D01 §3), never on the UI thread.

```rust
pub trait ScriptHost: Send + Sync {
    fn session(&self) -> &SessionHandle;               // apply edits, read state (D01 §5)
    fn ui_request(&self, req: UiRequest) -> UiResponse;    // blocks script thread; UI answers via channel
    fn poll(&self, ops: u64) -> Option<Dynamic>;       // Some = terminate (cancelled / time limit)
    fn log(&self, level: Level, msg: &str);
}
```

- `ui_request` sends a request (message, ask number, pick faces, pick file) to the UI and waits.
  In headless runs it returns an error ("no UI in headless mode") that the script may handle.
- Long API calls (`calculate`, `import`) start session jobs and wait for them, forwarding
  progress to the script job's progress, and honouring cancellation.

## 3. API binding

### 3.1 Handles

Script objects are thin handles: `ProjectH`, `SetupH(PersistId)`, `ToolpathH(PersistId)`,
`BodyH`, `FaceH`, `LibraryH`. A handle reads the current document revision on each access, so
scripts always see the latest state. Handles to deleted objects fail with a clear error.

### 3.2 Generated properties

Setup and toolpath properties are generated from field metadata (D06 §3.3), not written per
field:

- `setup.kind`, `setup.index_angle`, … : getter returns the resolved value; setter applies
  `Edit::SetSetupField` with a local value. `setup.reset("stock")` applies the inherit edit;
  `setup.is_inherited("stock")` reads `FieldOrigins`.
- `toolpath.passes` returns a `ParamGroupH` handle. Rhai's property-access fallback to indexers
  makes `toolpath.passes.stepover = 2.0` call the group's indexer setter → `Edit::SetToolpathParam`.
  Unknown keys raise the same error as the file format [S 09 §9.4].
- Values with units accept numbers (mm, degrees) or strings with units (`"0.25in"`, parsed by
  `fluut-i18n`).

### 3.3 Functions

As listed in [S 09 §9.4]; the full member list with types is [D12b](D12b-scripting-api.md). Additions found necessary during design:

| Function | Reason |
|---|---|
| `fluut.require(version)` | runtime equivalent of the `//! fluut >= x` header |
| `project.toolpath(name_or_id)` / `setup(name_or_id)` | lookup by name |
| `toolpath.diagnostics` | read warnings (e.g. collision trims) after calculation |
| `fluut.read_csv(path)` | parametric jobs from data [S 09 §9.1]; permission-checked |

### 3.4 Edits and undo

Every mutating call applies an `Edit` immediately (so later script lines see the result). The
session opens an undo group when the script starts and closes it when it ends; the whole run is
one undo step [S 09 §9.2]. If the script fails, the user is offered "undo script changes".

## 4. Running scripts

| Entry | Behaviour |
|---|---|
| Script console (app) | REPL on the current project; history; output pane |
| Ribbon / palette / shortcut | runs a library script; arguments via `Ui.ask_*` |
| `fluut run job.rhai [--project p.fluut] [-- args…]` | headless; `fluut.args`; exit code from `fluut.exit(n)` or 1 on error |

First run of a script not from the user's library shows the source and requested paths for
confirmation [S 09 §9.3]; approval is remembered by script hash.

## 5. API documentation

Generated at build time from the registered functions and field metadata (names, types, units,
help text from Fluent) into the online help [S 01 §1.9]. This keeps one source of truth
[S 09 §9.7].

## 6. Tests

- Sandbox escape tests: scripts attempting file access outside permissions, `eval`, deep
  recursion, huge strings, infinite loops — all stopped with the correct error.
- API round trip: a script that builds the spec's example project produces a document equal to
  the parsed example file.
- Headless golden runs: `fluut run` on sample jobs; output G-code compared to golden files.

## 7. Challenges

- None new. Depends on CH-02 (handles read the current document revision).
