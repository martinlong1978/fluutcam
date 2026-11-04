# Ribbon Wiring Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** The ribbon drives every operation the app has, and the temporary File menu goes. After `ui-ribbon-icons` (stages, galleries, icons) and `app-project-ops` (the operations, native dialogs, a temporary menu) the commands exist twice: disabled in the ribbon, enabled in the menu. This plan connects them, moves the shortcuts and the "discard unsaved changes?" prompt out of `ui/menu.rs`, and removes every file path from the ribbon: Open, Save as, Import and Post use native file dialogs.

**Architecture:** `vm::ribbon` decides what is enabled (`Command::wired` is now true for everything with an operation); `ui::ribbon::run` calls `Jobs` or asks `Dialogs`. A dialog answer comes back through `Dialogs::poll` and is acted on by `ui::project` (the former `menu::picked`, `Pending` and `discard_prompt`). `Dialogs` has a scripted mode so tests answer dialogs without a window. The post choice is a dropdown over the built-in store (posts that load, mixins excluded); the default output name is the project name with the post's extension.

**Design:** [D13](../design/D13-ui-architecture.md) §3, §8; spec 08 §8.3, §8.9.

## Global Constraints

All earlier Global Constraints apply. In addition:

- No file path is typed or shown as an editable field in the ribbon.
- A command that is wired never shows "Coming soon". Commands without an operation (Library, and any of Stock or Work zero that cannot do anything) stay disabled and say so.
- Tests never open a real dialog: they use `Dialogs::scripted`.
- Labels go through `tr`; shortcut hints say Cmd on macOS.

## Decisions

1. **New commands.** `edit.undo` and `edit.redo` (icons `undo`, `redo`, shortcuts Ctrl+Z, Ctrl+Y and Ctrl+Shift+Z) in an Edit group on every stage. Shortcuts of Undo and Redo do not fire while a text field has the keyboard. `model.reimport` re-imports every geometry source of the project (`Request::Reimport`, one undo step per source).
2. **New Setup based on selected** makes a Setup of the selected Setup's resolved kind, `based_on` it.
3. **Stock and Work zero** have no operation. They select the current Setup and highlight the matching row of the Setup inspector (`UiState::inspector_focus`), which is read-only until the Setup editor exists.
4. **New project** uses the unit of the open project, else mm. The File menu's separate inch entry goes; a unit choice belongs in the New project dialog later.
5. **Post output.** `DialogKind::PostOutput` is a save dialog named `<project>.<extension of the chosen post>`. A post that writes several files names them `.2`, `.3`, ... beside the chosen name, as `fluut post` does, so no folder choice is needed.
6. **Post list.** `posts::choices()` lists the built-in posts that load as posts (not mixins), so `fluut/linuxcnc-4x` is there for rotary Setups.
7. **Small icons** are painted in full text colour; the ink mask is drawn twice (a heavier mask) at 1x.

## Tasks

### Task 1: Plan
- [x] Plan committed.

### Task 2: Wire the commands
- [x] Tests first (enabling per command; scripted dialogs; shortcuts).
- [x] `Dialogs::scripted`, `PostOutput`; `ui/project.rs`; `ribbon::run`; reimport job; undo and redo commands and icons; delete `ui/menu.rs`.

### Task 3: No paths in the ribbon
- [x] Remove the Open and output fields, `open_path`, `post_output`; post dropdown from the store.

### Task 4: Polish
- [x] Heavier small icons, both themes checked in screenshots (kept out of git); Cmd tooltips.

### Task 5: End to end
- [x] kittest: New project, Import a reference STEP, New 3-axis Setup, a 3-D toolpath, Calculate all, Post; browser and G-code file asserted.
- [x] Duplicate-id test for the Fluent catalogs.
- [x] Merge `main`, all five checks, report.

**Done.** Open issues:
- Stock and Work zero only select the Setup and outline the matching row of the read-only Setup inspector; they edit nothing until the Setup editor exists. The Library commands stay disabled ("Coming soon").
- The New project dialog has no unit choice: a new project takes the unit of the open one, else mm.
- Post offers no folder choice: files after the first are numbered beside the one chosen.
- A rotary Setup does not select `fluut/linuxcnc-4x` by itself; the user picks it in the dropdown.
- Re-import re-imports every geometry source; there is no per-source choice yet.
- A one-off stack overflow was seen once in a parallel run of the `fluut-app` lib tests (not reproduced in later runs; cause unknown).
