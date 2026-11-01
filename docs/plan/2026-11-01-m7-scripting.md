# M7 — Scripting Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `fluut-script` (D12) with the API of D12b: scripts that read and change a project as
one undo step, calculate and post through the session, inside a sandbox that stops hostile
scripts. Exit criterion from the roadmap: the D12 §6 tests pass.

**Architecture:**
- `fluut-model` (layer 3): `Document::begin_group` / `end_group`. While a group is open, edits
  apply and are journalled but add no undo step; closing it adds one step holding the root from
  before the group.
- `fluut-core` (layer 6), small additions in their own commits: `Session::begin_group` /
  `end_group`; `Session::post_setups` (post a subset of the Setups; `post` calls it with all);
  `entities` module with read-only body and face descriptions (kind, parameters, bounds) for
  the script `Body` and `Face` objects.
- `fluut-script` (layer 7), on `rhai`:
  - `engine`: `Limits`, `make_engine` (no default packages; standard package without time;
    `eval` disabled; limits; progress callback for cancellation and the script-time limit;
    `print` and `debug` to the console; `libm` versions of the transcendental functions;
    the `fluut` global through the variable resolver).
  - `perm`: `Permissions`, path checks, and `LibraryResolver` for `import`.
  - `host`: `ScriptHost` (UI requests, console output, cancellation) and `Console`, a host
    that collects lines.
  - `api`: the D12b objects as handles over shared run state (`Arc<Mutex<State>>` holding the
    session), mapped onto `Edit`s. Values convert TOML ⇄ `Dynamic` one-to-one.
  - `run`: `Runner::run(source, name)` → `RunOutcome { result, exit_code, edits }`; checks the
    `//! fluut >= v` header; opens and closes the undo group.
- `fluut-cli` (layer 8): `fluut run <script> [project] [--tools f]… [--date d] [--time t] [-- args…]`
  (spec 09 §9.2, D12 §4).
- `fuzz/`: target `script` (any text into a run on the `block` project with small limits).

**Tech Stack:** as M4. Added: `rhai` 1.27 (MIT OR Apache-2.0) with `std`, `sync`, `no_time`,
`no_custom_syntax`. Its dependency `smartstring` is MPL-2.0 (allowed by spec 06 §6.1, added to
`deny.toml`), and `tiny-keccak` (through `const-random`, a build-time seed for `ahash`) is
CC0-1.0, a public-domain dedication, also added.

**Design:** [D12](../design/D12-scripting.md), [D12b](../design/D12b-scripting-api.md), D06 §6,
D01 §5–§6; spec 09, 06 §6.3.

## Global Constraints

All earlier Global Constraints apply. In addition:

- Scripts are untrusted input (spec 06 §6.3): no panics on any script text; every limit stops
  a hostile script with an error, never a hang or an abort.
- Determinism (D01 §6) for all a script can produce: no clock, environment or randomness;
  `libm` for transcendental functions; maps ordered by key.
- Do not modify `fluut-cam`, `fluut-app`, `fluut-io-*` or `third_party/` (parallel sessions).
  `fluut-core` and `fluut-model` additions are small and in their own commits.
- No DeskProto-derived content.

## Scope decisions

- **Undo group in the model.** The script's edits are applied one by one (later lines see the
  result) inside a `Document` group, rather than collected and applied once at the end.
- **The journal** keeps the single edits of a group, so a recovered document has them as
  separate steps. Group markers in the journal would change its record format; not worth it yet.
- **Calculation is not interruptible.** Session jobs run to the end; cancellation and the time
  limit act when the call returns. Time inside calculation, post and import does not count
  towards the script-time limit.
- **Headless only:** `fluut.open`. `fluut.ui` is wired to `ScriptHost::ui_request`; the app's
  dialogs belong to M5.
- **Approval of foreign scripts** (spec 09 §9.3, D12 §4) is the app's job (M5): the engine gets
  its permissions from the caller.
- **API documentation generation** (D12 §5) waits for `fluut-i18n`.

## Review Focus

1. **Sandbox:** infinite loops, deep recursion, huge strings, arrays and maps, `eval`, `import`
   outside the library and paths outside the permissions all stop with the right error.
2. **One undo step** per run, also when the script fails half-way.
3. **File keys = script properties:** values round-trip; unknown keys fail as in the file.
4. **Same G-code:** a script building the `block` project's toolpaths posts the same file as the
   TOML project.

---

### Task 1: Plan

- [x] **Step 1:** This plan; README table row. Commit.

### Task 2: Undo groups (`fluut-model`, `fluut-core`)

**Files:** `crates/fluut-model/src/document.rs`, `tests/undo.rs`; `crates/fluut-core/src/session.rs`, `tests/session.rs`.

- [ ] **Step 1:** Tests: three edits in a group are one undo step with the group label; undo
  gives the original and redo the final document; an empty group adds no step; a group closed
  after a failed edit keeps the edits before it; undo while a group is open closes it first;
  nested `begin_group` calls make one step.
- [ ] **Step 2:** Implement; pass; commit.

### Task 3: Core additions: entities and Setup posts (`fluut-core`)

**Files:** Create `crates/fluut-core/src/entities.rs`, `tests/entities.rs`; change `session.rs`.

- `bodies(&ModelStore) -> Vec<BodyInfo>`: id, name, source, kind, bounds, faces with kind,
  parameters (D12b §6) and bounds.
- `Session::post_setups(post_id, posts, opts, setups: Option<&[String]>)`.
- [ ] **Step 1:** Tests: the `holes` project's cylinders, their radii, axes and `inside`; the
  `block` project's planes and their normals; posting one Setup of a two-Setup project gives
  only its toolpaths.
- [ ] **Step 2:** Implement; pass; commit.

### Task 4: Engine, limits and permissions (`fluut-script`)

**Files:** Create `crates/fluut-script/{Cargo.toml, src/lib.rs, src/engine.rs, src/perm.rs, src/host.rs, tests/sandbox.rs}`; `deny.toml`; `Cargo.toml`.

- [ ] **Step 1:** Tests: each limit stops its hostile script (loop, recursion, string, array,
  map, nested arrays, variables); `eval` is a parse error; `import` of `../x`, an absolute path
  and a missing module fail, a library module loads; cancellation and the script-time limit end
  a loop; path checks (`..`, symlinks, allowed roots, write roots); `timestamp` is absent;
  `sin` is `libm::sin`.
- [ ] **Step 2:** Implement; `cargo deny check`; pass; commit.

### Task 5: Object API (`fluut-script`)

**Files:** Create `src/api.rs`, `src/api/*.rs`, `src/value.rs`, `src/run.rs`, `tests/api.rs`, `tests/undo.rs`.

- [ ] **Step 1:** Tests: every D12b member on the reference projects; values round-trip TOML ⇄
  script; unit strings; unknown keys and types fail as in the file; inherited fields and
  `reset`; permission denials for `open`, `import`, `read_csv`, `save_as` and `post`; one run
  is one undo step, also on error; the version header; headless `ui` errors; the spec 09 §9.5
  example.
- [ ] **Step 2:** Implement; pass; commit.

### Task 6: Calculate and post; the G-code test (`fluut-script`)

**Files:** `tests/post.rs`.

- [ ] **Step 1:** Test: a script adds the three `block` toolpaths to an empty Setup, calculates
  them and posts with `fluut/linuxcnc`; the file equals the CLI's G-code for `block.fluut`
  (golden `block-linuxcnc.ngc`). Reports, `state`, `result_summary` and `diagnostics`.
- [ ] **Step 2:** Implement; pass; commit.

### Task 7: `fluut run` (`fluut-cli`)

**Files:** `crates/fluut-cli/src/main.rs`, `tests/cli.rs`.

- [ ] **Step 1:** Tests: argument parsing; `fluut run` on a script that edits, saves and posts;
  exit codes from `fluut.exit(n)`, a script error (1) and bad arguments (2); `fluut.args`; a
  path outside the project and the arguments is denied.
- [ ] **Step 2:** Implement; pass; commit.

### Task 8: Fuzzing

**Files:** `fuzz/Cargo.toml`, `fuzz/fuzz_targets/script.rs`, `fuzz/seeds/script/*.rhai`, `fuzz/rhai.dict`, `fuzz/README.md`.

- [ ] **Step 1:** Target; seeds from the tests; run; fix what it finds; commit.

### Task 9: Close

- [ ] **Step 1:** Mark this plan done with deviations; README rows; D12 updates. Commit and
  push `m7-scripting`.
