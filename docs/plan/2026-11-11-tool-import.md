# Tool Library Import Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Import tool libraries from other CAM products into native `fluut.tools/1` libraries (spec 05 §5.10.5, 02 §2.5). Fusion tool-library JSON is built; Vectric `.vtdb` is designed and blocked on facts that are not public.

**Architecture:**
- `fluut-post-import` (new crate, layer 5, depends on `fluut-model` and `serde_json`): `fusion::import(bytes, &ImportOptions) -> Result<ImportResult, Vec<Diagnostic>>`, `limits`, a TOML writer. Static and bounded; no code is run.
- `fluut-cli`: `fluut tools import <file> -o <library.toml> [--id] [--name] [--force]` in `tools_cmd.rs`.
- `fluut-app`: unchanged by tasks 1 to 4. The GUI entry (native file dialogs in the Library group) is built in the GUI section below (D16 §10.2).

**Tech Stack:** as M7. No new dependencies (`serde_json` was already in the workspace).

**Design:** [D16](../design/D16-tool-library-import.md); [S 05 §5.10.5](../spec/05-postprocessor-engine.md); [S 02 §2.5](../spec/02-data-formats.md); [S 01 §1.7](../spec/01-architecture.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- No DeskProto-derived content, no proprietary sample files. Fixtures are hand-written from the field table in D16 §3.
- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask check-layers`, `cargo deny check` and `cargo xtask wiki-reference --check`.
- No path text box in the ribbon (native dialogs only), when the GUI entry is built.
- Imported files are untrusted: size, count and range limits, escaped output, no evaluation.

## Task 1: design and decisions

- [x] **Step 1:** D16: sources, field mapping (units, defaults, drops), tool type table, overall length and holder rules, ids and duplicates, merge rule (new file only, `extends` to combine), limits, diagnostics, entry points, tests, challenges CH-26 to CH-28.
- [x] **Step 2:** The SQLite decision: a purpose-built read-only reader when `.vtdb` is built; no `rusqlite`, no FFI exception; recorded in D16 §7 and in spec 01 §1.8.3 and 05 §5.10.5.
- [x] **Step 3:** The number: D16 (D15b was the last used; there was no D16 or D17).

## Task 2: the Fusion importer

- [x] **Step 1:** Tests first on a hand-written fixture (`tests/fixtures/fusion-sample.json`), round trip through `ToolLibrary::parse`.
- [x] **Step 2:** `fusion.rs`: type table, units, presets, holder collapse, ids, duplicate numbers, summary of dropped keys.
- [x] **Step 3:** Limits and bad-input tests (not JSON, no `data`, not UTF-8, deep nesting, too many tools, too large, bad id, escaped output).

## Task 3: command line

- [x] **Step 1:** `fluut tools import`, usage text, exit status 0/1/2, no overwrite without `--force`, input read with a byte cap, `.vtdb` answered with `import.format.unsupported`.
- [x] **Step 2:** CLI tests (`tests/tools_import.rs`).

## Task 4: wiki and UAT

- [x] **Step 1:** `Import-Tools` page (sidebar, Home), `Command-Line`, `Tools` and `Known-Limits` updated.
- [x] **Step 2:** `UAT-Tools`, UAT-TLS-01 to 08, registered in `UAT`; `UAT-CLI-02` lists the new usage line.
- [x] **Step 3:** The UAT-TLS-01 and 02 file was run through the binary and the expected text checked.

## Done

**Done.** The Fusion importer and `fluut tools import` are built with tests; Vectric `.vtdb` is designed (decision: own read-only reader, no `rusqlite`) and blocked on the table layout, which is not public. Deviations from the brief: the design is D16, not D18 (the next free number); the GUI menu entry is not added because the Library commands are greyed out and `fluut-app` is under work elsewhere.

## Not done

- Vectric `.vtdb` (D16 §7.3: needs the table and column names from a user's own file).
- Verification against a real Fusion file (CH-26): the key table is from public structure and the documented `tool` properties, not an official schema.
- A tool-by-tool merge into an existing library.

## GUI entry (2026-11-11)

Branch `tool-import-gui`. Design: [D16 §10.2](../design/D16-tool-library-import.md). Prototype GUI work, so it follows the app's own view-model, dialog and job conventions (D13).

- [x] **Step 1: ribbon.** `Command::ImportTools` (`library.import_tools`, label "Import tools...", new icon `import-tools.svg` in the existing style) first in the Library group; enabled when no job runs. `fluut-app` depends on `fluut-post-import` (layer 5, allowed by `check-layers`). No path text box (checked by a window test).
- [x] **Step 2: dialogs.** Two new `DialogKind`s, `ImportToolsSource` (filters `.json` and `.vtdb`) and `ImportToolsTarget` (Save, `.toml`), through the existing `Dialogs` so tests script them. `Dialogs::confirms_overwrite` is true for the system dialogs and false for scripted ones unless `script_confirms_overwrite(true)`.
- [x] **Step 3: import.** `vm::tool_import`: format check (`.vtdb` and other extensions give `import.format.unsupported`, no second dialog), the importer on its own worker thread (not the project job system: it changes no document and must not block Cancel or Save), `create_new` unless the dialog confirmed replacing, never over its own input.
- [x] **Step 4: result.** A window with the library id, counts (read, written, skipped, keys not used), a list of the unused keys and every warning with its code. Errors: `Nothing was imported.` and the errors.
- [x] **Step 5: use in the project.** The project model has no reference to a tool library: libraries load from `<project>/tools/*.toml` when the project is opened. So: a library saved there gets **Reload project to use it** (the existing Open path, with its unsaved-changes question); otherwise the window says it cannot be used by a project yet. Recorded in Known limits.
- [x] **Step 6: tests.** 11 view-model tests, 9 window tests (scripted dialogs on `fusion-sample.json`: happy path, cancel at each dialog, `.vtdb`, bad JSON, overwrite, reload, library elsewhere, disabled during a job); ribbon and icon tests updated.
- [x] **Step 7: docs.** `Import-Tools` (window steps first, command line second), `UAT-Tools` (UAT-TLS-09 to 14), `Known-Limits`, `Interface-Tour`, `Tools`, `UAT`, D16 §10.2, screenshot `import-tools-result.png` (`cargo xtask screenshots`, new picture only).

**Not done:** a project field that names a tool library file (so a library outside `tools/` could be used, and a live "add library" without reload); import of `.vtdb`; the tool library browser. A real-platform check of the system Save dialog's replace question (it is relied on, not observed).
