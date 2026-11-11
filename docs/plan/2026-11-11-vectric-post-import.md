# Vectric Post Import Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Convert Vectric `.pp` post processors into FluutCAM posts statically (spec 05 §5.10.1, 02 §2.5, D10 §6.1), with the value-format reading of CH-21 decided and checked against a real corpus.

**Architecture:**
- `fluut-post` (layer 4): small engine extensions the conversion needs, none changing the built-in posts' golden output: the header sees the first toolpath's values; `[N]` is the line number; `[ZH]` is the highest safe Z; `output.substitute_values`; `[LBRACKET]`/`[RBRACKET]`; `state.moved`; `first_*` also after a feed change; a rotary post without `rotary_index` indexes with a rapid.
- `fluut-post-import` (layer 5, now depends on `fluut-post`): `vectric::{import, import_with_base}` → standalone `fluut.post/1` text, `import.*` diagnostics, a report; the result is loaded and compiled before it is returned.
- `fluut-cli`: `fluut posts import <file.pp> -o <post.toml> [--id] [--force]` in `posts_cmd.rs` (exit 0/1/2), `POST_BASE` read beside the input.
- `fluut-app`: unchanged by tasks 1 to 6. The GUI entry (native file dialogs in the Library group) is built in the GUI entry section below.

**Design:** [D10 §6.1, §9](../design/D10-post-engine.md); [CH-21](../design/challenges.md); [S 05 §5.3, §5.4, §5.6, §5.10.1](../spec/05-postprocessor-engine.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Vectric's post files are proprietary: read locally, never copied, quoted or closely paraphrased into the repository. Fixtures and test text are hand-written from the documented syntax. A local check confirmed that no added line contains a corpus line longer than 25 characters.
- No DeskProto-derived content.
- Imported files are untrusted: size, line, block and `VAR` limits, escaped output, no execution (a script section is refused).
- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask check-layers`, `cargo deny check` and `cargo xtask wiki-reference --check`.

## Task 1: survey and decisions

- [x] **Step 1:** Survey the local corpus (547 `.pp` files) for settings, `VAR` forms, blocks and placeholders, in aggregate only.
- [x] **Step 2:** CH-21: decide the value-format reading (`printf`-like, separator spelled out, decimals always written) and record what still needs Vectric-made outputs.
- [x] **Step 3:** Decide refusals (no units, inverse time, speed steps, scripts, missing base, no feed block) and warnings for everything else left out.

## Task 2: engine extensions

- [x] **Step 1:** Header values, `[N]`, `[ZH]`, `substitute_values`, bracket placeholders, `state.moved`, with emitter tests.
- [x] **Step 2:** `first_*` on a feed change; index fallback for rotary posts without `rotary_index`; tests. Built-in golden files unchanged.

## Task 3: importer

- [x] **Step 1:** Reader (comments, settings, `VAR`, blocks, quoted lines, encodings, limits, `POST_BASE` overlay).
- [x] **Step 2:** Mapping (settings, words, blocks, placeholders and character codes, arcs, helices, wrapped rotary output, comment style) and the TOML writer; load-and-compile check.
- [x] **Step 3:** Unit tests: one per value-format form, settings, refusals, blocks, placeholders, base posts, escaping.

## Task 4: command line

- [x] **Step 1:** `fluut posts import`, usage text, exit status 0/1/2, no overwrite without `--force`, input and base read with a byte cap, `.cps` answered with `import.format.unsupported`.
- [x] **Step 2:** CLI tests, including posting the `plate` project with an imported post.

## Task 5: verification

- [x] **Step 1:** Hand-written fixtures (`grbl-like`, `linuxcnc-like`, `comma-inch`, `wrap-y`) posted on the reference suite, golden files, and compared with `fluut/grbl`, `fluut/linuxcnc`, `fluut/gcode`, `fluut/linuxcnc-4x`: every built-in endpoint in order, same tools and spindle speeds.
- [x] **Step 2:** Corpus test (`FLUUT_VECTRIC_PP_DIR`) and Vectric output-pair test (`FLUUT_VECTRIC_PAIRS_DIR`), skipped when unset. Local corpus run: 187 converted cleanly, 339 with warnings, 21 refused; all 526 converted posts posted the reference suite.

## Task 6: docs

- [x] **Step 1:** CH-21 dated entry; D10 §6.1 status and mapping, §9 tests; spec 05 §5.3, §5.4.1, §5.4.2, §5.6, §5.10.1; spec 02 §2.5.
- [x] **Step 2:** Wiki `Import-Posts` (sidebar, Home, Machines and posts), `Command-Line`, `Known-Limits`; UAT-PPI-01 to 10 in `UAT-Posts-Import`, registered in `UAT`; `UAT-CLI-02` lists the new usage line. UAT-PPI-01 to 09 were run through the binary and the expected text checked.

## Done

**Done.** The Vectric importer and `fluut posts import` are built with tests and docs. Deviations from the brief: the engine gained the extensions above (a faithful conversion needs them; the built-in posts' output is unchanged); `--name` is not offered (the brief asked for `--id` and `--force`).

## Not done

- Comparison with G-code written by Vectric software (CH-21): the pair test is ready for `<post>.pp` + G-code files in a local folder; a byte comparison needs the same job posted by both programs.
- Rebasing onto the closest built-in post (D10 §6.3): the result is standalone.
- A GUI entry (native file dialogs in the Library group), as for tools: built later the same day, see the GUI entry section below.
- Posts refused today: inverse-time (G93) wrapped posts, speed-step spindles, script sections.
- Values FluutCAM does not supply (file name, material size, tool and toolpath lists, toolpath notes, wrap diameter, laser power) and laser blocks.

## GUI entry (2026-11-11)

Branch `post-import-gui` (task fu-7). Design: [D10 section 6.1](../design/D10-post-engine.md). Prototype GUI work, so it mirrors the **Import tools...** flow ([2026-11-11-tool-import](2026-11-11-tool-import.md), GUI section) and the app's own view-model, dialog and job conventions (D13).

- [x] **Step 1: ribbon.** `Command::ImportPost` (`library.import_post`, label "Import post...", new icon `import-post.svg` in the existing style) after **Add tool library...** in the Library group; enabled when no job runs; no project needed. No path text box (checked by a window test).
- [x] **Step 2: dialogs.** `DialogKind::ImportPostSource` (filter `.pp`) and `ImportPostTarget` (Save, `.toml`), through the existing `Dialogs` so tests script them; the same overwrite rule as tool import (`confirms_overwrite`).
- [x] **Step 3: import.** `vm::post_import`: a `.cps` gets the CLI's `import.format.unsupported` text, other extensions the CLI's other text, with no second dialog; the importer (`vectric::import_with_base`, `POST_BASE` read from beside the input, size cap) on its own worker thread; `create_new` unless the dialog confirmed replacing; never over its own input. The Save dialog offers `<name>.post.toml` and a name with no extension gets `.post.toml`; it starts in the project's `posts` folder if there is one, else in the project's folder.
- [x] **Step 4: result.** A window with the post id and name, settings and blocks mapped, words written, base files, the warning count, and every warning with its severity and `import.*` code. A refusal shows "Nothing was imported." and the errors. A small "Importing..." window shows while the worker runs.
- [x] **Step 5: use the post.** Before this, the Post stage listed the built-in posts only and the post job used the built-in store, so a project's own `posts/` posts were reachable from the command line only. Now: `Session::reload_definitions` (fluut-core, with `Library::remove_defs` and `load_defs_dir` in fluut-model) loads the project's `posts`, `machines` and `templates` again; `Edit::ReloadDefinitions` runs it on the worker (no undo step, project not changed); `Snapshot.posts` lists the library's posts that load as a post (built-in and the project's), the Post stage's list and the output file extension come from it, and the post job resolves the post from the open project's library (as `fluut post` does). **Use this post** is offered when the file is a `*.post.toml` directly in the project's `posts` folder: it reloads the definitions and chooses the post in the Post stage. For a post saved elsewhere the window says FluutCAM does not find it there. With no project it says to open one. The choice is a window setting (`UiState.post_id`), not saved in the project.
- [x] **Step 6: tests.** 12 view-model tests and 12 window tests (scripted dialogs on the importer's hand-written fixtures `grbl-like.pp`, `comma-inch.pp` and others; no file from a Vectric install): happy path, warnings with their codes, cancel at each dialog, `.cps`, bad file, overwrite, never over the source, use in the project then Post with the imported post, a post outside `posts`, no project, disabled during a job, `POST_BASE`. Two fluut-core tests for `reload_definitions`. Ribbon and icon tests updated.
- [x] **Step 7: docs.** `Import-Posts` (window steps first, command line second), `Machines-and-Posts`, `Post-G-code`, `Interface-Tour`, `Known-Limits`, `UAT-Posts-Import` (UAT-PPI-11 to 16), `UAT`, D10 section 6.1 status, screenshot `import-post-result.png` (`cargo xtask screenshots`, new picture only).

**Not done:** the chosen post is not saved in the project (a machine's `post` field is not used by the window or the command line to choose); a `posts` folder shared between projects (the user definition layer exists in fluut-core but the app does not pass a user folder); making a post appear when it is saved by hand while the project is open (only **Use this post** reloads); a real-platform check of the system Save dialog's replace question (relied on, as for tools).
