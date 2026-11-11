# Vectric Post Import Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Convert Vectric `.pp` post processors into FluutCAM posts statically (spec 05 §5.10.1, 02 §2.5, D10 §6.1), with the value-format reading of CH-21 decided and checked against a real corpus.

**Architecture:**
- `fluut-post` (layer 4): small engine extensions the conversion needs, none changing the built-in posts' golden output: the header sees the first toolpath's values; `[N]` is the line number; `[ZH]` is the highest safe Z; `output.substitute_values`; `[LBRACKET]`/`[RBRACKET]`; `state.moved`; `first_*` also after a feed change; a rotary post without `rotary_index` indexes with a rapid.
- `fluut-post-import` (layer 5, now depends on `fluut-post`): `vectric::{import, import_with_base}` → standalone `fluut.post/1` text, `import.*` diagnostics, a report; the result is loaded and compiled before it is returned.
- `fluut-cli`: `fluut posts import <file.pp> -o <post.toml> [--id] [--force]` in `posts_cmd.rs` (exit 0/1/2), `POST_BASE` read beside the input.
- `fluut-app`: unchanged (GUI entry is a follow-up).

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
- A GUI entry (native file dialogs in the Library group), as for tools: `fluut-app` is under work elsewhere.
- Posts refused today: inverse-time (G93) wrapped posts, speed-step spindles, script sections.
- Values FluutCAM does not supply (file name, material size, tool and toolpath lists, toolpath notes, wrap diameter, laser power) and laser blocks.
