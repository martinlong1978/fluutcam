# Viewport Pickers and Editors Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** A working prototype: start a project, import a STEP file, add Setups and toolpaths, and finish every toolpath type from the window, with no hand-editing of project files. After `ribbon-wiring` every command runs, but Setups are read-only, tools cannot be chosen, heights and selections are read-only, and the browser cannot rename, delete or reorder anything.

**Architecture:** View models stay free of `egui`. `vm::setup` builds the editable Setup inspector and the edits it makes; `vm::inspector` gains tool, height, selection, text and point widgets; `vm::pick` holds pick filters, toggling and chain offers; `vm::tree` gains the browser's edits. All of them produce `fluut_model::edit::Edit` values, sent through `jobs::Edit::Model`, so each is one undo step (S 08 §8.5, D06 §6). Picking extends the M5 id pass: edges get per-vertex ids, a pick takes a mask of what it accepts, and selected or hovered entities are redrawn from their vertex ranges with a tint, so a selection change uploads nothing.

**Design:** [D13](../design/D13-ui-architecture.md) §4, §5; [D15](../design/D15-toolpath-parameters.md) §2.3, §2.4; spec 03 §3.9, spec 08 §8.2, §8.4, §8.5.

## Global Constraints

All earlier Global Constraints apply. In addition:

- The ribbon holds commands only, never parameters (S 08 §8.3); no file paths in it. Stock and Work zero focus the inspector group.
- Labels go through `tr`, with English catalog entries.
- Stored selections are the persistent ids (`f-<stem>.<i>`, `e-<stem>.<i>`), which `Edit::RemapIds` maps on re-import (spec 03 §3.5). The pick index is never stored.
- Tests never open a real dialog, and drive picks by id; the id pass itself is tested headless where an adapter exists.

## Decisions

1. **One edit path.** `jobs::Edit::Model(fluut_model::edit::Edit)` joins `SetParam`. A refused edit shows in the status line (`Snapshot::edit_error`), which until now nobody displayed.
2. **Setup inspector.** Name, Kind (read-only), Material, Machine, Work offset, Stock, Work zero, Clearance. Bodies and toolpath count stay read-only. Fields a `based_on` Setup inherits show "inherited from <Setup>" with a reset. Machines come from the library through `Snapshot::machines`.
3. **Stock choices** by Setup kind: three-axis: block by margin, block by size; rotary and indexed: cylinder; `from_previous` when the Setup has a parent. `frame.rs` refuses cylinder stock in a three-axis Setup, and block in a rotary one has no default, so those are not offered. A body stock in a project stays shown, read-only: choosing a body is not in this plan.
4. **Work zero choices.** Three-axis: the four top corners, top centre, the four bottom corners, explicit point (model frame X, Y, Z). Rotary and indexed: stock left, stock right, explicit X. There is no offset from a corner in the data model (spec 03 §3.9.3): an explicit point is how an offset zero is written.
5. **Machine** is written as `id@major`, as `add_setup` writes it. Work offset is a dropdown G54 to G59, plus the current value if it is something else.
6. **Tool choice** lists every tool of `Snapshot::tools` (project and built-in libraries) as "<library>: <type> <diameter>, <name>" and writes `library#tool`. A reference that is not in the list stays shown. Unsuitable tools are not hidden: the messages of `validate` with `field = "tool"` show under the dropdown.
7. **Heights.** A reference dropdown and an offset entry, for `Height` and `XPos`. D15 §2.4 says `feed` defaults to `top`, the one height that may take it, but the reader refused `top` everywhere, so changing `feed`'s offset could not be stored. The reader now accepts `top` for `heights.feed` only; the dropdown offers it there and nowhere else.
8. **Other kinds.** So every type can be finished: `Text` is a text entry, `Duration` a number, `Point` a text entry `x y z`, `Points` a text entry `x y z; x y z` (spaces, since a decimal separator may be a comma). `PValue` values use the display unit like every other length.
9. **Pick filters** come from the field: `drive_faces` of `drill` and `bore` take cylinders; `drive_curves` and `boundary.curves` take edges, wire curves and planar faces; other selections take faces. The scene records each face's kind so the filter works without the kernel on the UI thread. Bodies are not picked: clicks choose faces or curves.
10. **Chains.** `SetupScene::edge_loops` holds the closed loops: face loops of a B-rep (horizontal planar faces first) and closed chains of wire curves. Picking an edge in a curve field offers its first loop; the offer is a button, "Add closed loop (n edges)", in the inspector and the pick banner.
11. **Pick mode** lives in `UiState::pick`. Entering it hides the simulated stock, so faces can be hit. Esc or Done leaves. Hover asks the id pass only when the pointer moves. Each toggle is one edit, so undo steps through picks.
12. **Duplicate** is a job, as Add toolpath is, so the copy becomes the selection. `Session::duplicate_toolpath` names it "<name> copy".
13. **Delete** removes the selected row with Del; undo brings it back. Reorder uses `MoveSetup` and `MoveToolpath`; the model refuses a move that breaks `based_on`, and the refusal shows in the status line.
14. **New project unit.** After the file dialog a small window asks mm or inch, with the unit of the open project first. The title bar's unit badge changes the unit of the open project (S 08 §8.2: clickable), as one undoable edit.

## Tasks

### Task 1: Plan
- [x] Plan committed.

### Task 2: Setup inspector editing
- [x] Tests first: `vm::setup` edits, undo, scene update, inherited fields, Stock and Work zero focus.
- [x] `Snapshot::machines`, `jobs::Edit::Model`, status line for refused edits.
- [x] `vm/setup.rs`, inspector group widgets, ribbon focus.

### Task 3: Tool choice
- [x] Tests first: options, writing the reference, messages under the dropdown.
- [x] `ToolStore::libraries`, tool row on the Tool tab.

### Task 4: Height and remaining field editing
- [x] Tests first: reference and offset edits, inch display, `top` for feed only, text, duration, points.
- [x] Reader change for `heights.feed`; widgets.

### Task 5: Selection picking
- [ ] Tests first: filters, toggle, clear, remove, chain offers, id pass of edges, ranges, highlight draw.
- [ ] Scene: face kinds and ranges, edge ids, ranges and loops.
- [ ] Renderer: edge ids, pick mask, tinted redraw.
- [ ] Pick mode in the viewport and inspector.

### Task 6: Browser actions
- [ ] Tests first: rename, delete, move, enable, duplicate; undo of each.
- [ ] `Session::duplicate_toolpath`; context menu, F2, Del, drag.

### Task 7: New project unit choice
- [ ] Tests first; prompt and clickable badge.

### Task 8: End to end
- [ ] kittest: New, import `plate2d.step`, Setup, stock margin, zero, `pocket_2d`, pick the floor by id, tool, heights, Calculate, Post; G-code equal to a hand-written reference project; undo at each step; re-import keeps the selection.
- [ ] Merge `main`, all five checks, report.
