# App Project Operations Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A working prototype loop in the app: start a new project, import a STEP file, add Setups and toolpaths, calculate, post, save. Until now the app opened an existing project from a typed path and could only calculate, post and simulate it. This plan adds the operations to `Jobs` (off the UI thread, one undo step each, a snapshot after each, diagnostics on failure), native file dialogs, and a temporary File menu to reach them. The ribbon rebuild (`ui-ribbon-icons`) wires the operations to ribbon commands later; this plan does not touch `ui/ribbon.rs` or `vm/ribbon.rs`.

**Architecture:** The logic lives in `fluut-core` (`ops.rs`, headless and testable without a window): `Session::new_project`, `save_as`, `is_dirty`, `import_model`, `add_setup`, `add_toolpath`. The app's worker (`session.rs`) calls them for new requests; `jobs.rs` gets one `Jobs` method per operation. App edits (`Edit::SetParam`) go through `Session::apply` now, so parameter changes are undo steps too. `dialogs.rs` runs `rfd` dialogs on a thread and hands the chosen path back through a channel the frame polls.

**Tech Stack:** as M5. One new dependency, `rfd` (below).

**Design:** [D06](../design/D06-project-model.md) §6–§7, [D13](../design/D13-ui-architecture.md) §8, spec 03 §3.9, spec 08 §8.2–§8.5.

## Global Constraints

All earlier Global Constraints apply. In addition:

- No dialogs inside jobs: a job gets its paths as arguments. Dialogs live in the UI layer and never block the frame loop.
- Every operation is one undo step (`Session::apply`; multi-edit operations use a group). Undo and redo restore the previous snapshot exactly.
- Errors (unreadable file, no solids, bad extension) are `Outcome::Failed` diagnostics, never panics, and leave the project unchanged.
- Saving is `fluut_model::persist::save`, the same bytes as `Session::save`.
- All labels go through `fluut-i18n`; a new catalog file `ops.ftl` holds the new English messages.
- `BTreeMap` where iteration order reaches output.

## Decisions

1. **Geometry files are referenced, or copied into the project folder.** Spec 02 §2.1 stores `path` relative to the project file. A file already inside the project folder is referenced where it is. A file elsewhere is copied into the project folder (name clash with different content gets `-2`, `-3`, … ; identical content is reused), so a project folder is self-contained and moves as a unit. The copy is made only after the file has loaded and has bodies. Save As to another folder copies the referenced geometry files and the project-layer `tools/`, `machines/`, `posts/`, `templates/` folders beside the project, so the new project opens as before.
2. **Units of STL, DXF and SVG.** Spec 08 §8.10a asks for the unit on import, which a job cannot do. `import_model(path)` assumes the project's display unit (`mm` or `in`) and says so in a warning (`model.import.unit_assumed`); `import_model_unit(path, Some(unit))` takes it explicitly for the dialog that follows. STL files give no bodies yet (no toolpath type takes a mesh), so they fail with `model.import.empty`.
3. **Body ids must not clash between sources.** The store keys bodies by id across sources; a second source whose body ids collide with a loaded one (two DXF files both have `b-1`) is refused with `model.import.body_ids`. Open issue; needs per-source body ids.
4. **New Setup defaults** (spec 03 §3.9, §3.9.6; spec 02 §2.1 example):
   - `kind` as asked; `machine` `fluut/generic-3axis-router@1` (three_axis) or `fluut/generic-4axis-router@1` (rotary, indexed; new built-in, a router with a continuous A axis along X and post `fluut/linuxcnc-4x`); `material` `MDF` (the starter tools' cutting data is for MDF, so toolpaths calculate at once).
   - three_axis: `stock = block, margin = [2, 2, 1]` (spec 02 §2.1 example: the only margin the specification gives; the model bounds plus 2 mm each side in X and Y and 1 mm on top), `zero = stock_top_corner front_left` (the spec 03 §3.9.3 default, written out so it shows in the inspector). Clearance, work offset (G54), orientation (+Z, +X) and geometry (all bodies) are left to the defaults.
   - rotary and indexed: `stock = cylinder` on the +X axis through the model origin, radius = largest distance of the model bounds from the axis + 1 mm, length = X extent + 2 mm; `zero = on axis, stock_left`; `clearance = cylinder`, radius + 10 mm. The axis is the default orientation (+X); the user picks a face axis for a part that is not on it.
   - `based_on`: only `based_on`, `stock = from_previous` (spec 03 §3.9.6 default for Setups with `based_on`) and `kind` when it differs from the parent's; everything else is inherited (§3.9.4 rules 1 and 3: toolpaths are not inherited). A kind in the other class (three_axis against rotary or indexed) is refused: orientation and zero could not be inherited.
   - Name `Setup N`, N the first number whose name is unused; id `s<N>`.
5. **New toolpath defaults** (D15 metadata): no parameter is written, so every field takes its D15 default (heights from the Setup, feeds and speeds from the tool preset for the Setup's material). Tool from the built-in `fluut/starter` library: `flat-12` for `face`; `vbit-60` for `chamfer_2d` and `engrave`; `drill-5` for `drill`; `ball-6` for the 3-D finishing and rotary finishing types; no tool for `manual_nc`; `flat-6` for everything else. 3-D types leave the drive selection empty, which means every face (D15 §2.3.1); 2-D types leave it empty too and show the inspector's `param.geometry.drive.empty` message. Name `<Type label> N`, id `t<N>`.
6. **Starter tool library.** `fluut/starter` (new built-in, five tools: 6 and 12 mm flat, 6 mm ball, 60° V bit, 5 mm drill) with cautious MDF cutting data. A new project had no tools at all; without them no toolpath could be calculated. Users copy tools into their own library to change them.
7. **Dirty state** is `project != last saved project` (Setups are `Arc`s, so the comparison and the clone are cheap); undoing back to the saved state is clean. The title shows `*`.
8. **A model with no Setup is drawn.** The snapshot has a scene under the id `""` (all bodies, model frame) while the project has no Setup, so an import shows in the viewport at once.
9. **Dialog crate.** `rfd` 0.17 (MIT), `default-features = false, features = ["xdg-portal"]`: native dialogs on Windows (Win32 common item dialog) and macOS (Cocoa, run on the main thread by rfd), and on Linux the xdg-desktop-portal over D-Bus (`zbus`, already in the tree through eframe's accesskit), not GTK 3, which would add about 25 crates and a system library. The `wayland` feature (parent window handle) is off; the dialogs are not modal to the window. `cargo deny check` and a Windows build: see the result below. The dialogs use `AsyncFileDialog` under `pollster::block_on` on a worker thread, so the frame loop never waits.

## Review Focus

1. `save_as` to another folder: geometry and definition files are copied, paths stay relative and unchanged, the undo history is kept (a path that climbs out of the folder with `..` is rewritten to a copy beside the project, which restarts the history).
2. Import never leaves a half state: a failed load changes neither the project nor the store; a failed edit removes a file the import copied.
3. Undo of every operation restores the snapshot (project text equal); the model scene cache is keyed by the hash of the sources, not by revision.

---

### Task 1: Plan and built-in definitions

**Files:** this plan, `docs/plan/README.md`, `crates/fluut-model/{machines,tools}/fluut/*`, `crates/fluut-post/posts/fluut/linuxcnc-4x.post.toml`.

- [ ] **Step 1:** Plan; starter tools, 4-axis machine and post registered as built-ins, with load and compile tests.

### Task 2: Core operations

**Files:** `crates/fluut-core/src/{ops.rs, session.rs, lib.rs}`, `crates/fluut-core/tests/ops.rs`, `crates/fluut-model/src/params/value.rs` (`PValue::to_toml`).

- [ ] **Step 1:** Tests first (new project reopens; save_as round-trip byte-identical and keeps geometry; import STEP bodies and copy rule; bad files; add_setup defaults against the frame (stock bounds = model bounds + margin; zero); `based_on`; add_toolpath defaults and validation; undo of each).
- [ ] **Step 2:** Fail, implement, pass.

### Task 3: Jobs and worker

**Files:** `crates/fluut-app/src/{jobs.rs, session.rs, session/tests.rs, vm/status.rs}`, `crates/fluut-i18n/locales/en/{app,ops}.ftl`.

- [ ] **Step 1:** Tests first (end to end headless: new, import, add setup, add `parallel`, calculate, post; save and reopen byte-identical; undo; bad file).
- [ ] **Step 2:** Fail, implement, pass.

### Task 4: Dialogs and the temporary File menu

**Files:** `crates/fluut-app/src/{dialogs.rs, ui/mod.rs, ui/menu.rs}`, `Cargo.toml`, `deny.toml` if needed.

- [ ] **Step 1:** `dialogs.rs` with filters; kittest smoke test of the menu; Ctrl+Z/Y, Ctrl+S, Ctrl+O, Ctrl+N, Ctrl+I shortcuts.
- [ ] **Step 2:** Workspace checks, merge `main`, commit.

(Results and open issues are recorded below when the plan is done.)
