# Ribbon Stages and Icon Set Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The ribbon of S 08 §8.3 for the first working prototype: workflow-stage tabs (Model, Setup, Toolpaths, Simulate, Post, Library stub) with grouped commands, toolpath galleries filtered by the selected Setup's kind, and one consistent line-style icon set (S 08 §8.9) drawn for FluutCAM. The project operations (new, save, import, add Setup, add toolpath) are built on another branch (`app-project-ops`); here their commands exist, show, and are disabled with a "coming soon" hover text until a later task wires them.

**Architecture:** `vm::ribbon` stays toolkit-free (D13 §1): `Stage`, `Command` (with stable ids, D13 §3), `Shortcut`, `Category`, and `RibbonVm::new(jobs, cx)` that lays out the groups of the current stage and decides enabled and pending per command. The layout is data in code (a `const` table per stage) rather than the embedded TOML of D13 §3; it moves to TOML when commands become user-editable. Toolpath galleries come from `ToolpathTypeId::ALL`, grouped by `Category` (04 §4.13) and filtered by the selected Setup's resolved kind (03 §3.9.2: rotary types in `rotary` Setups only, 2-D, hole and 3-D types in `three_axis` and `indexed`, utility types everywhere). `ui::ribbon` paints the stage tabs and groups with custom-painted buttons (large icon over label for primary commands, small icon beside label for secondary ones), runs wired commands, and turns the shortcuts Ctrl+N, Ctrl+O, Ctrl+S and Ctrl+Shift+S into command ids. `UiState::stage` holds the stage; switching it touches nothing else.

**Icons:** SVG masters under `crates/fluut-app/assets/icons/` on a 24 px grid: 1.75 px strokes, round caps and joins, no fills except small accent dots; ink is `#16181D`, the one accent is cutter orange `#FF5B14`. All are drawn for FluutCAM (none adopted), licensed like the code. `build.rs` rasterises every master with `resvg` (already a build-dependency) at 16, 24, 32 and 48 px and splits each raster into two coverage masks, ink and accent, by colour. At run time `ui::icons` uploads masks lazily as white textures, picks the 2× size when `pixels_per_point > 1.25`, and paints ink tinted with the theme's text colour (weak text, half alpha, when disabled) and accent in cutter orange (greyed when disabled). So one raster set serves both themes and both states.

**Tech Stack:** as M5. One new dev-dependency feature: `egui_kittest`'s `wgpu` renderer, for an `#[ignore]`d test that writes a PNG of each stage in both themes to the target folder (never committed; screenshot tests are not in CI yet). It pulls in `image` (MIT OR Apache-2.0), which passes `cargo deny`. `resvg` becomes a dev-dependency too (already a build-dependency), so a test parses every master.

**Design:** [`../design/D13-ui-architecture.md`](../design/D13-ui-architecture.md) §1-3, §6; spec [08](../spec/08-user-interface.md) §8.2, §8.3, §8.9; [04](../spec/04-toolpath-algorithms.md) §4.13; [03](../spec/03-data-model.md) §3.9.2.

## Global Constraints

All earlier Global Constraints apply. In addition:

- Do not implement the project operations, and do not edit `jobs.rs` or `session.rs` (the `app-project-ops` branch owns them).
- The ribbon holds commands only, never parameters. Post choice and output file stay as they are on the Post stage (they pick what a command acts on).
- Command ids are stable: a test pins them all.
- Every label and tooltip goes through `tr` with English catalog entries.
- Existing commands (Open, Calculate all, Simulate, Post, Cancel) keep working; the window tests switch to the stage that holds them.
- Nothing in the icon set or the layout is derived from another CAM program.

## Review Focus

1. **Enabled rules:** a pending command is never enabled, whatever the state; wired ones keep their M5 rules; Calculate selected needs a selected toolpath or a Setup with toolpaths.
2. **Gallery filter:** the kind is the resolved one (inherited through `based_on`); with no Setup selected, all types show, disabled.
3. **Icons:** every icon a command or gallery names exists, parses, has ink on the 24 px grid, and keeps the style rules (a test reads the masters).

---

### Task 1: Plan

**Files:** this plan, `docs/plan/README.md`.

- [ ] **Step 1:** Write and commit.

### Task 2: Ribbon view model

**Files:** `crates/fluut-app/src/vm/ribbon.rs`.

- `Stage` (`ALL`, `id`, `label`), `Category` (`of(ToolpathTypeId)`, `allowed_in(SetupKind)`), `Command` with `id`, `label`, `icon`, `shortcut`, `wired`, `primary`; `RibbonCx { stage, selection, playback }`; `RibbonVm { stage, groups: Vec<GroupVm>, setup_kind }`, `GroupVm { label, items: Vec<CommandVm> }`, `CommandVm { command, label, tooltip, icon, enabled, pending, large }`.
- [ ] **Step 1:** Tests first: ids are unique and pinned; each stage has its groups; pending commands are disabled with the "coming" tooltip; the gallery holds every implemented type once (no project) and only the allowed types per kind (`block` 3-axis, `shaft` rotary, `flipside` indexed); Calculate selected follows the selection; shortcuts map to Ctrl+N/O/S/Shift+S.
- [ ] **Step 2:** Fail, implement, pass; English messages in `fluut-i18n/locales/en/app.ftl`.

### Task 3: Icon set

**Files:** `crates/fluut-app/assets/icons/*.svg`, `assets/icons/README.md`, `build.rs`, `src/brand.rs` (or `src/icons.rs`).

- [ ] **Step 1:** Tests first: every icon a command or a toolpath type names is in the built set, has ink at 24 px, and its master parses with `usvg` and keeps the style rules (viewBox, stroke width, caps, joins, colours).
- [ ] **Step 2:** Draw the masters; rasterise them in `build.rs`; README with the style rules, how to add one, licence.

### Task 4: Ribbon widgets

**Files:** `crates/fluut-app/src/ui/{ribbon.rs, icons.rs, mod.rs, tests.rs}`, `tests/reference.rs`.

- [ ] **Step 1:** Tests first (kittest): each stage renders with its commands, in both themes; switching stage keeps the camera; a pending command is disabled; shortcuts reach wired commands; the old window tests switch stage.
- [ ] **Step 2:** Stage tabs, group frames, large and small buttons, galleries, tooltips with shortcut, icon textures; check both themes by screenshots (ignored test, PNGs under the target folder).

### Task 5: Checks and index

- [ ] **Step 1:** Merge `main`; fmt, clippy, tests, `xtask check-layers`, `cargo deny check`; plan index row; commit.
