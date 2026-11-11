# Tool Library Import Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Import tool libraries from other CAM products into native `fluut.tools/1` libraries (spec 05 §5.10.5, 02 §2.5). Fusion tool-library JSON is built; Vectric `.vtdb` is designed and blocked on facts that are not public.

**Architecture:**
- `fluut-post-import` (new crate, layer 5, depends on `fluut-model` and `serde_json`): `fusion::import(bytes, &ImportOptions) -> Result<ImportResult, Vec<Diagnostic>>`, `limits`, a TOML writer. Static and bounded; no code is run.
- `fluut-cli`: `fluut tools import <file> -o <library.toml> [--id] [--name] [--force]` in `tools_cmd.rs`.
- `fluut-app`: unchanged. The GUI entry (native file dialogs under the greyed **Library** button) is designed in D16 §10.2 and not built.

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

- The GUI **Import tools...** entry (D16 §10.2).
- Vectric `.vtdb` (D16 §7.3: needs the table and column names from a user's own file).
- Verification against a real Fusion file (CH-26): the key table is from public structure and the documented `tool` properties, not an official schema.
- A tool-by-tool merge into an existing library.
