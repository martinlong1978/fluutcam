# Setup Gaps Plan: Scale and Mirror, Rotary Body Stock, Stock Handles, Model Snapping

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Close the smaller Setup gaps left by [the Setup kind plan](2026-11-10-setup-kind-orientation.md): scale and mirror controls, a body as the stock of a rotary or indexed Setup, handles for cylinder stock and for a block given by size, and work zero snapping to model vertices and arc or circle centres.

**Architecture:** `fluut-core` frame takes the stock body into the rotary and indexed frames and records the X of the work zero in the aligned frame. `fluut-app` scenes gain snap points (model vertices and arc centres, in W and in M). View models stay free of `egui`: `vm::setup` gains the scale and mirror rows and edits, `vm::handles` gains the stock handles and the model snap targets. Every change is a `fluut_model::edit::Edit`, so each is one undo step. The file format does not change (`geometry.scale` and `geometry.mirror` already round trip).

**Design:** spec 03 §3.9.2, §3.9.3, §3.9.6; spec 08 §8.4, §8.5; [D13](../design/D13-ui-architecture.md) §4, §5.

## Global Constraints

All earlier Global Constraints apply. In addition:

- The ribbon holds commands only, never parameters; no file paths in it.
- Labels go through `tr`, with English catalog entries.
- Nothing is derived from another CAM program's files or behaviour.

## Decisions

1. **Scale and mirror.** A new inspector group "Scale and mirror" after Bodies. **Scale, all axes** is one number row; **Mirror X/Y/Z** are check boxes. A factor of 0 is refused ("Must not be 0."). All ones and no mirror remove the `geometry` field again (a child Setup writes `{}` so it still overrides its parent). The bodies edit keeps the scale and mirror, and the other way round. The solids (B-rep) and drawings with arcs cannot move under a scale with different factors per axis (`transform_brep` and `WireBody::transformed` refuse a non-similarity); the frame now says "cannot be scaled by different factors per axis; use one factor for every axis" instead of "cannot be moved into the work frame". A file that has different factors shows **Scale X/Y/Z** rows beside the empty **all axes** row, so the user sees what is there.
2. **Rotary and indexed body stock.** The stock body is already carried to the frame and taken out of the machined bodies. In an **indexed** Setup the stock is the box of the body in the aligned frame at A = 0, like the 3-axis case (`cylinder = None`, so the turned box and the turn radius are right). In a **rotary** Setup the stock must be a cylinder (the rotary toolpaths, the simulation and the rotary height envelope all take a radius and an X range), so the stock is the **bounding cylinder** of the body about the axis: its X extent, and the radius of its farthest point (the largest face radius for an exact body, the box radius for a mesh). That is sound (it never has less material than the body) but not the true shape of a square bar or a casting; this is documented in the wiki and in Known limits, with Cylinder as the way to type exact round stock. The "Body" choice is offered to every kind.
3. **Stock handles.** Cylinder stock (rotary and indexed): a square at the top of the middle for the radius, a square at the right end on the axis for the length. The cylinder is centred on the model, so the length square moves half as far as the length (`Handle::gain = 2`). A block given by size: the same three face squares as the margins (+X, +Y, top) for Size X, Y, Z; the block is centred in X and Y so those two have gain 2, the top has gain 1. A drag changes nothing until it ends; the release is one `setup_edit` (`stock.radius`, `stock.length`, `stock.size.<i>`), so one undo step. Values step by 0.1 mm and never go below a step (margins may reach 0).
4. **Work zero snapping to the model.** `SetupScene::snap` holds the vertices of exact bodies, the centres of circle and arc curves, and the end points of wire curves (not the corners of mesh triangles; at most 4000 points). Each has its place in W, in M, and (rotary, indexed) its X in the aligned frame, which `SetupFrame::zero_x` provides. `zero_targets` appends them after the stock places as `point:x,y,z` keys (3-axis; writes `ZeroSpec::Point`, a point in M) or `axis:x` keys (rotary and indexed; writes `AxisX::At`, since their zero stays on the axis; one target per X). A model point at the same place as a stock place keeps the stock name. While dragging, only model points within three snap radii of the pointer are drawn.
5. **Index angle dial.** Not done: it needs a ring drawn in the YZ plane, a pointer-to-angle mapping through the camera, a snap to the usual angles and a relative-angle mode. It stays in Known limits.

## Tasks

### Task 1: Plan
- [x] Plan committed.

### Task 2: Scale and mirror
- [x] Tests first: rows, edits, zero refused, bodies and scale keep each other, round trip back to no field, per-axis explained, undo.
- [x] Group, `scale_mirror_edit`, `geometry_edit`; frame message.

### Task 3: Rotary and indexed body stock
- [x] Core tests: rotary bounding cylinder and indexed box on the shaft and a mesh blank.
- [x] Frame change; the "Body" choice for every kind; vm and UI tests.

### Task 4: Stock handles
- [x] Tests first: the handle list for size and cylinder, gain, drag value, one undo step.
- [x] `vm::handles` kinds, `ui::handles` drawing and drag; UI drag tests (GPU adapter).

### Task 5: Model snapping
- [x] Tests first: targets from a part with holes (arc centres), the zero written as a point in M, rotary targets by X, malformed keys.
- [x] `snap_points`, `SetupFrame::zero_x`, `zero_targets`, `zero_edit`, drawing near the pointer.

### Task 6: Wiki and end to end
- [x] `Setups-Stock-and-Zero`, `Add-a-Setup`, `Interface-Tour`, `Known-Limits`, UAT-SET-37 to 43.
- [x] Merge `main`, all checks, report.

## Done

Scale and mirror have controls; a rotary Setup takes a body as the stock (as its bounding cylinder) and an indexed Setup takes its box; cylinder stock and fixed-size blocks have handles; the work zero snaps to model vertices and arc or circle centres. No file format change.

## Open issues

- No ring dial for the index angle.
- The window sets one scale factor for every axis; different factors per axis are refused by the frame for solids and for drawings with arcs.
- A rotary body stock is the bounding cylinder, not the body's own shape.
- Snap points are not taken from STL meshes.
- The Stock list of an indexed Setup still offers only the cylinder; the frame also takes a block there.
