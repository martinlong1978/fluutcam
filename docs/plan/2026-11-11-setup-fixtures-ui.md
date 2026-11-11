# Setup Fixtures UI and Index Angle Dial Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Close two follow-ups in the Setup UI that the [turn fixtures plan](2026-11-11-turn-fixtures.md) and the [Setup gaps plan](2026-11-11-setup-gaps.md) left in Known limits. First, a **Fixtures** group in the Setup inspector for rotary and indexed Setups (list, add a chuck, tailstock or clamp, remove, edit with the model's own validation, inheritance), with the fixtures drawn in the viewport. Second, a viewport **dial** for the index angle of an indexed Setup: a ring round the rotary axis with a draggable pointer.

**Architecture:** `fluut-model` and `fluut-core` do not change: `Setup::fixtures`, its inheritance and `setup.fixtures.ignored` are already there, and `setup_field_text` already writes the `fixtures` field. All the work is in `fluut-app`:
- `vm/setup/fixtures.rs` (new): the group and the edit of its rows. Edits go through `setup_edit` with keys `fixtures.add`, `fixtures.clear`, `fixtures.local`, `fixtures.<i>.remove` and `fixtures.<i>.<field>[.<axis>]`, so they are `fluut_model::edit::Edit::set_setup_field` and each is one undo step.
- `vm/setup.rs`: the group joins the Setup's groups for rotary and indexed kinds, and a new row widget `RowWidget::Action` (a button) carries Remove, Remove all and Make a local copy.
- `scene.rs`: `fixture_lines` and `SetupScene::add_fixtures` build wire frames in W; `session.rs` adds them to the scene of a rotary or indexed Setup; `viewport/render.rs` draws them with a new line tag.
- `vm/handles/dial.rs` (new): the ring, the angle snapping, the pointer-to-angle mapping and the edit; `HandleKind::Dial` in `vm/handles.rs`; `ui/handles/dial.rs` (new) draws the ring and runs the drag; `Camera::ray` in `viewport/camera.rs`.

**Tech Stack:** as M7. One new dependency edge: `fluut-app` uses `libm` (already in the workspace) for the dial, because `clippy.toml` forbids the platform `atan2`, `sin` and `cos`.

**Design:** spec 03 §3.9.5, §3.9.8; spec 08 §8.4, §8.5; [D13](../design/D13-ui-architecture.md) §4, §5; [turn fixtures](2026-11-11-turn-fixtures.md); [Setup gaps](2026-11-11-setup-gaps.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- The ribbon is not touched: no parameters and no file paths there.
- Labels go through `tr`, with English catalog entries; test strings avoid hyphens.
- No file format change; a project without fixtures saves exactly as before.
- Nothing is derived from another CAM program's files or behaviour.

## Decisions

### Fixtures

1. **Where.** A group **Fixtures** after **Clearance** and before **Scale and mirror**, shown for rotary and indexed Setups only. A three_axis Setup shows no group (the model keeps warning `setup.fixtures.ignored` for a list it has, and the viewport draws none).
2. **Rows.** The first row, **In use**, says "None", "1 fixture" or "N fixtures" and carries the inheritance (the existing `origin` and Reset to inherited). **Add** is a dropdown with a prompt entry ("Add a fixture...") and the three kinds. **Remove all fixtures** follows. Both come before the list, because the list can be long and the inspector scrolls. Each fixture has a header row with its title ("Chuck 1") and a **Remove** button, then its number rows: chuck `Diameter`, `Length`; tailstock `Diameter`, `Length`, `Tip diameter` (empty is none); clamp `Min X/Y/Z`, `Max X/Y/Z`. Numbers are lengths in the project unit; expressions work as in other rows.
3. **Validation = load errors.** Diameter and length above 0, tip diameter not below 0 (0 is a point), every maximum of a clamp above its minimum. Messages reuse `setup-entry-positive` and `setup-entry-not-negative` and add `fixture-clamp-order`. A refused entry shows under the row and sends nothing. A clamp corner changes one coordinate at a time, so the check is per axis: to move a clamp past its own other corner, change the far corner first.
4. **Inheritance.** The list is inherited whole, as in the model. Editing any row of an inherited list writes the whole list, with the change, to the Setup (the same as every other inherited field: the edit makes it the Setup's own). **Make a local copy** (only while the list is inherited and not empty) writes the same list without a change. **Remove all fixtures** writes `fixtures = []`, which overrides a parent's list; on a Setup with no parent it removes the field (an empty list means no field). **Reset to inherited** is the existing control and drops the Setup's own list.
5. **New fixtures.** They start with sizes taken from the stock's scene bounds so that they show and do not touch the stock: a chuck three times the stock radius wide and 40 % of the stock length long, a tailstock the stock radius wide and 30 % long, a clamp 20 mm beyond the stock's radius on the +Y side, a tenth of the stock length wide each side of the middle, and as tall as the stock radius. All are rounded to 0.1 mm. A clamp beside the stock in a rotary Setup is outside the clearance cylinder of the default clearance, so the model rightly reports `setup.clearance.index_unsafe`; the user raises the clearance radius.
6. **Drawing.** Wire frames, blue (palette slot 7, which was unused), in the Setup's frame W. A chuck is a drum at the stock's start face, a tailstock a cylinder or cone at its end face, a clamp a box turned by the index angle exactly as the part (an indexed Setup: `to_work` includes the turn; a rotary Setup: A = 0). The stock's X range comes from the frame (`fr.stock`), so the fixtures follow a stock change. They are not pickable and not in the scene bounds, so they do not change how the view is fitted. **Why wire frames and not translucent solids:** the viewport has an opaque mesh pass and a line pass, with no blended pass and no depth sorting of transparent triangles; a translucent solid would be a new pipeline, a draw order and a pick exclusion, which is more than this feature is worth. Outlines read well and cost one tag.

### Dial

7. **What the pointer is.** The ring lies in the YZ plane of W, centred on the axis at the stock's start face, with a radius a fifth larger than the stock's farthest Y or Z (at least 5 mm). The pointer is the model's own +Z direction after the index turn: at angle `a` it is at `(y, z) = R (-sin a, cos a)` (the turn matrix of `frame::index_turn` applied to +Z). A test checks that a model vertex turns the same way. So the pointer moves with the part.
8. **Pointer to angle.** The cursor ray (`Camera::ray`, from the inverse of `view_proj`) is intersected with the plane `x = centre.x`; the angle is `atan2(-y, z)` of the hit, brought within half a turn of the angle the drag had (`unwrap_near`), so a drag past 180 degrees goes on. When the ray is nearly parallel to the plane (less than 0.05 of its direction along X), is pointing away, or hits the centre, there is no answer, and the angle is the one whose ring point is nearest the cursor on the screen (720 samples). A flat ring is symmetric on the screen, so ties within half a pixel go to the angle nearer the one before: the pointer stays on its side.
9. **Snapping.** No modifier: whole steps of 5 degrees. Shift: 1 degree. Alt: free. In all three, within 2 degrees of 0, 90, 180 or 270 the angle is exactly that quarter. The spec said 15 degrees and face normals; 5 degrees is finer and was asked for, face normals need the pick machinery and are not done.
10. **Edit.** Nothing is sent while dragging (as for the other handles); the release is one `set_setup_field(index_angle)`. Absolute angles are written in 0..360. A local relative angle is written as the difference from the angle it is relative to (`resolved - local`), in -180..180, so the resolved angle is what was dragged. A setup that only inherits its angle gets its own absolute angle. A drag to the same angle, or a whole turn away, is no edit.
11. **Where.** An indexed Setup only (not rotary, which has no index angle; not three_axis). The dial is a `HandleKind::Dial` handle, so it is in `handle_marks`, hides in pick mode and for non-selected Setups like the others, and the window tests find it.

## Tasks

### Task 1: Plan
- [x] This plan; README row.

### Task 2: Fixtures group
- [x] Tests first (view model): no group in a three_axis Setup, add of each kind, field edits, removal, `fixtures` gone when empty, rules of the file, bad keys, inheritance (inherited, replace, reset, empty override, local copy), one undo step each.
- [x] `vm/setup/fixtures.rs`, `RowWidget::Action`, i18n entries, inspector button.

### Task 3: Dial
- [x] Tests first: snapping, `unwrap_near`, pointer position and turn direction against the model, ray to angle (including `Camera::ray` from several views), edge-on fallback, relative angles, one undo step.
- [x] `vm/handles/dial.rs`, `HandleKind::Dial`, `Camera::ray`, `ui/handles/dial.rs`.

### Task 4: Drawing
- [x] Test: line counts, extents, taper, turn of a clamp; in the app, fixtures in the scene follow the index angle and the parent's list.
- [x] `scene.rs`, `session.rs`, `viewport/render.rs`.

### Task 5: Window tests and screenshot
- [x] `ui/tests/fixtures.rs`: add, edit, refuse, remove, undo, inheritance; the group is absent in a three_axis Setup; dial drag to 90 and 47 degrees, past 180, no change, an edge-on ring (GPU adapter); fixtures reach the viewport.
- [x] Screenshot `setup-fixtures.png`.

### Task 6: Docs
- [x] Wiki: `Setups-Stock-and-Zero`, `Add-a-Setup`, `Interface-Tour`, `Known-Limits`, `UAT-Setups` (UAT-SET-44 to 51), `UAT`. Spec 08 §8.4 and D13 row for the dial.
- [x] Merge `main`, all checks, report.

## Done

An indexed or rotary Setup has a **Fixtures** group (add, remove, edit with the file's validation, inherited list with Make a local copy, Remove all fixtures and Reset to inherited), fixtures drawn in the viewport as wire frames at the Setup's index angle, and an indexed Setup has a ring dial for its index angle with a pointer that follows the cursor from any view, 5 degree steps with Shift for 1 degree and Alt for free, and exact quarters. Every change is one undo step. No file format or model change.

## Not done

- Fixtures are wire frames, not shaded or translucent solids, and cannot be picked or dragged on the model.
- The part does not turn while the dial is dragged; it turns on release.
- No snap of the dial to face normals (spec 08 §8.4 mentions them).
- A clamp's corners are typed; no box handle.
- Rotary Setups have no dial (no index angle); the fixtures in a rotary Setup are drawn at A = 0.
