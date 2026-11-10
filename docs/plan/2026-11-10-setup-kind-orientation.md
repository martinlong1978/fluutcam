# Setup Kind, Bodies, Orientation and Model Handles Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Close the Setup gaps left by [the viewport pickers plan](2026-11-05-viewport-pickers.md): the Setup kind, the Setup's part bodies, stock from a body, and the Setup orientation become editable in the window, whole bodies can be picked on the model, and the work zero and block stock margins have drag handles (S 08 §8.4).

**Architecture:** View models stay free of `egui`. `vm::setup` gains the kind, bodies, body stock and orientation edits; `vm::pick` gains a Setup target beside the toolpath target; `vm::handles` lists the handles of a Setup and turns a drag into an edit. Every change is a `fluut_model::edit::Edit` sent through `jobs::Edit::Model`, so each is one undo step. A kind change that crosses the 3-axis and rotary classes is one `Group` edit.

**Design:** spec 03 §3.9.2, §3.9.3, §3.9.6; spec 08 §8.4, §8.5; [D13](../design/D13-ui-architecture.md) §4, §5.

## Global Constraints

All earlier Global Constraints apply. In addition:

- The ribbon holds commands only, never parameters; no file paths in it.
- Labels go through `tr`, with English catalog entries.
- Stored picks are persistent ids (`b-<stem>`, `f-<stem>.<i>`, `e-<stem>.<i>`); `Edit::RemapIds` already maps body lists, body stock and orientation picks on re-import.
- Tests drive picks by id and never open a dialog. Handle drags are tested through the view model; the drag itself is tested where a GPU adapter exists.

## Decisions

1. **Kind.** The Kind row is a dropdown of the three kinds. Rotary and indexed share a class; three_axis is the other. Within a class only `kind` is written. Across classes the edit is a `Group` ("Change kind"): kind, the default orientation and zero of the new class, a stock that fits it (a cylinder from the stock box, or the default margin block; `from_previous` stays), and the default clearance. A rotary or indexed kind is refused with a message when the Setup's machine has no rotary axis. Toolpaths the new kind does not allow show their existing validation; they are not removed.
2. **Bodies.** The Setup inspector lists every body of the project's sources with a checkbox, and an "All bodies" choice (no `bodies` key). At least one body stays selected. Pick on the model starts a body pick: the viewport shows every body (the model scene, `Snapshot::all_bodies`), a click on a face or curve toggles its whole body, and the chosen bodies are tinted.
3. **Body stock.** "Body" joins the stock choices. It starts with a body the Setup does not machine, else the first body. The row below is a body dropdown with a Pick button. The stock body is never machined: the frame takes it out of the machined bodies and the stock box is its bounds in the aligned frame. Body stock in a rotary Setup stays unsupported and says so.
4. **Orientation.** A group "Orientation": three_axis has Tool axis (+Z of W) and X axis; rotary and indexed have Rotary axis and A = 0 toward. Each is a dropdown of the six signed model axes plus the current face or edge pick, and a Pick button that takes a face (planar, cylindrical) or an edge. The scene is rebuilt in the new frame, so the viewport follows. Refusals (an axis parallel to the other, a non-straight edge) come from the frame and show as the Setup's diagnostics.
5. **Handles.** Drawn on the viewport when a Setup is selected and nothing is being picked. Work zero: a handle at the origin of W; during a drag the nearest snap point (stock corners and top centre, or the two stock ends on the axis for rotary Setups) is shown, and releasing writes `zero.choice`. Margins: handles at the +X, +Y and top face centres of block stock set by margin; the drag previews the value, and releasing writes `stock.margin.<i>`. One release is one edit, so one undo step. Numeric entry stays in the inspector.

## Tasks

### Task 1: Plan
- [x] Plan committed.

### Task 2: Setup kind
- [ ] Tests first: same-class change, class change as one undoable edit, machine refusal, existing validation for disallowed toolpaths.
- [ ] Kind dropdown and `kind_edit`.

### Task 3: Bodies and body stock
- [ ] Tests first: body list edits, undo, body stock scene, body pick by id, re-import keeps the picks.
- [ ] Frame and app session: stock body; `Snapshot::all_bodies`.
- [ ] Inspector rows, body pick mode in the viewport.

### Task 4: Orientation
- [ ] Tests first: axis choice, face pick, rotary axis, scene follows, undo.
- [ ] Orientation group and pick roles.

### Task 5: Handles
- [ ] Tests first: handle list, snapping, margin from a drag, one undo step.
- [ ] `vm::handles`, viewport overlay and drag.

### Task 6: End to end
- [ ] Merge `main`, all checks, report.

## Open issues

(none yet)
