# 08 — User Interface

## 8.1 Design goals

FluutCAM's UI is modern, dark-first and visually polished, in the style of DaVinci Resolve and
GitKraken rather than a traditional engineering application.

| Goal | Meaning |
|---|---|
| **Model-centred** | The 3D viewport is the largest element and stays in view in every workflow stage. Panels frame it; they never replace it. |
| **Controls where they are needed** | A control appears next to what it changes: on the model (handles, gizmos), in the inspector for the selected item, or in a context menu. The ribbon holds commands that *create* things or *change stage*, not parameters. |
| **Ribbon for commands** | A ribbon-style toolbar with large, labelled, grouped commands, organised by workflow stage. |
| **Never blocked** | Calculation, import and posting run in the background (01 §1.8). The UI stays responsive and shows progress where the work belongs. |
| **Visible state** | Units, setup kind, staleness of toolpaths, and errors are always visible in context. |

FluutCAM avoids modal parameter dialogs and linear wizards.

## 8.2 Window layout

```
┌───────────────────────────────────────────────────────────────────────────────────┐
│ ● FluutCAM   Shaft.fluut            [ ⌘K  Search commands… ]       mm   ⚙   ▣    │ title bar
├───────────────────────────────────────────────────────────────────────────────────┤
│  MODEL   SETUP   TOOLPATHS   SIMULATE   POST   LIBRARY                            │ ribbon tabs
│ ┌────────────┬──────────────────────────────┬───────────────┬──────────────────┐  │ = workflow
│ │ ⬚ Import   │ ◧ 2D  ▾  ◩ 3D ▾  ◎ Rotary ▾  │ ⊙ Drill       │ ▶ Calculate all  │  │   stages
│ │   STEP/STL │ (gallery of toolpath types)  │               │                  │  │ ribbon
│ └────────────┴──────────────────────────────┴───────────────┴──────────────────┘  │ groups
├──────────────┬─────────────────────────────────────────────────┬──────────────────┤
│ BROWSER      │                                                 │ INSPECTOR        │
│ ▾ Setup 1  ◎ │                                                 │ Parallel finish  │
│   ✓ Rough    │             3D VIEWPORT (full bleed)            │ Tool Geom Hts    │
│   ⟳ Finish   │                                                 │ Passes Linking   │
│ ▾ Setup 2  ⊞ │    ┌──────┐   floating context toolbar         │ ───────────────  │
│   ! Pocket   │    │ ⊕ ✥ ⟲│   next to selection                │ Stepover  0.40mm │
│ ▸ Model      │    └──────┘                                     │ Angle     45°    │
│              │                                  view cube ▣    │ Tolerance 0.01mm │
├──────────────┴─────────────────────────────────────────────────┴──────────────────┤
│ PROGRAM TIMELINE  ▕Setup 1: Rough ▏Finish ▏▕Setup 2 A0°: Pocket ▏▕Setup 3 A90°…  │ timeline
│                   0:00 ──────────────●─────────────────────────────── 42:17 est.  │
└───────────────────────────────────────────────────────────────────────────────────┘
```

Regions:

| Region | Contents | Behaviour |
|---|---|---|
| Title bar | project name, command palette, units badge, settings | Units badge is always visible and clickable (addresses). |
| Ribbon | workflow-stage tabs with grouped commands (§8.3) | Collapsible to tabs only. |
| Browser (left) | Setups → Toolpaths tree; Model bodies; status icons (§8.6) | Collapsible; drag to reorder toolpaths and setups (within `basedOn` rules, 03 §3.9.4). |
| Viewport (centre) | model, stock, work zero, toolpaths, simulation | Always present. Floating context toolbar and in-view handles (§8.4). |
| Inspector (right) | parameters of the selected item (§8.5) | Collapsible; may float or pop out to a second monitor. |
| Program timeline (bottom) | machining order as clips (§8.7) | Collapsible. |

All side panels are dockable and remember their state per workflow stage.

## 8.3 Ribbon

The ribbon tabs are the workflow stages. Selecting a tab switches the ribbon commands and
adapts the panels for that stage, as DaVinci Resolve's pages do. The viewport stays.

| Tab | Ribbon groups (commands only) | Panels emphasised |
|---|---|---|
| **Model** | Import (STEP, STL, DXF/SVG, bitmap); Re-import; Body visibility; Measure; Section view | Browser (Model) |
| **Setup** | New Setup (3-axis / Rotary / Indexed); New Setup based on selected; Stock; Work zero; Machine | Inspector (Setup) |
| **Toolpaths** | Galleries per category: 2D, Holes, 3D, Rotary, Multi-axis, Utility (04 §4.13); Templates; Calculate selected / all | Browser, Inspector (Toolpath) |
| **Simulate** | Play / step / speed; Stock display; Gouge and rest checks; Compare to model | Timeline, viewport |
| **Post** | Post selected / all; Machine and post choice; Preview G-code; Save to file or folder; direct send to controllers after v1 (OQ-19) | G-code panel beside the viewport |
| **Library** | Tools; Machines; Posts (layer explorer, `explain`, `flatten`, import) | Library browser replaces tree |

Galleries show only toolpath types allowed in the selected Setup's kind (03 §3.10.3). Hover
shows a short animated preview of the strategy pattern.

The ribbon MUST NOT hold toolpath or setup parameters. Rule of thumb: if a control changes a
property of something that exists, it belongs in the inspector or on the model, not in the
ribbon.

## 8.4 Controls on the model

Direct-manipulation controls in the viewport, shown only when relevant to the selection:

| Object | In-view control |
|---|---|
| Stock (block) | Face handles drag margins; dimensions shown live |
| Stock (cylinder) | Radius and length handles along the rotary axis |
| Work zero | Triad gizmo; snaps to stock corners/centres, vertices, arc centres (03 §3.9.3); Y/Z locked to the rotary axis in rotary and indexed Setups |
| Rotary axis / index angle | Ring dial around the rotary axis; drag or type an angle; snap to 15° and to face normals |
| Drive / check faces | Click faces to add; selection chips appear in the inspector; hover highlights face, edge or body by filter |
| Boundary | Drag curve vertices or offset handle |
| Contour tabs | Drag tab markers along the contour; add with click (04 §4.13.7) |
| Leads and ramps | Handle at the entry point; drag to move the entry |
| Pass preview | While editing stepover, angle or direction, a ghost of the pass pattern updates live |

A floating context toolbar appears beside the selection with its 3–5 most common actions
(e.g. for a toolpath: calculate, simulate, duplicate as…, hide). Right-click opens a context menu
with the full set.

## 8.5 Inspector

The inspector shows the selected item's parameters, never a modal dialog.

- Toolpaths: the five tabs of 03 §3.10.2 (Tool, Geometry, Heights, Passes, Linking), showing
  only fields the type declares.
- Setups: kind, geometry, orientation, stock, zero, index angle. Inherited fields are shown
  dimmed with an "inherited from Setup 1" chip and a reset control.
- Numeric fields: show units; accept expressions (`6/2`, `1/4in`, `tool.diameter*0.4`);
  drag on the label to scrub the value; show the default and the tool-preset source.
- Validation messages appear under the field they concern, with the rule text (03 §3.10.3).
- Changes apply live. Dependent toolpaths are marked stale (§8.6), not recalculated
  automatically unless the user enables auto-calculate.

## 8.6 Background work and status

| State | Browser icon | Viewport |
|---|---|---|
| Not calculated | hollow circle | — |
| Calculating | progress ring with % | pass preview grows as passes complete |
| Done | filled circle (toolpath colour) | toolpath shown |
| Stale (inputs changed) | amber dot | toolpath dimmed |
| Error / warning | red / amber triangle | marker at the problem location (e.g. unreachable region, non-converged sample, 04 §4.2.4) |

Each calculation can be cancelled from its icon. Notifications for finished long jobs appear as
toasts and in a notification list. No modal progress dialogs.

## 8.7 Program timeline

The bottom strip shows the program as a sequence of clips, like a video editor timeline:
Setups as groups, Toolpaths as clips whose width is the estimated machining time.

- Scrubbing the timeline moves the simulation to that point (Simulate tab) or highlights the
  toolpath under the playhead (other tabs).
- Tool changes, index moves and clamp events show as markers.
- Dragging a clip reorders toolpaths, with the same rules as the browser (after v1).
- Total and per-setup estimated time are shown. The estimate uses machine feed limits and
  acceleration where known (02 §2.3).

**[DECIDED — OQ-20]** The timeline is in v1 with clips, markers, time estimates and scrubbing.
Reordering by dragging clips follows later; in v1, reorder in the browser.

## 8.8 Command palette and keyboard

- `Ctrl+K` (macOS `⌘K`) opens a searchable command palette, as in GitKraken: every command,
  every toolpath type ("add parallel"), every object by name ("go to Setup 2").
- Every command is reachable by keyboard. Shortcuts are listed in the palette and editable.
- Unlimited undo/redo covering all project edits, with a history list. This resolves
  H3 (undo depth unknown) as a requirement.

## 8.9 Visual style

| Aspect | Requirement |
|---|---|
| Theme | Dark theme default; light theme available. Theme colours are tokens, so themes are data. |
| Surfaces | Layered dark greys with subtle elevation; one accent colour for focus and selection |
| Typography | One UI sans-serif family; tabular figures for numeric fields; monospace for G-code |
| Icons | One consistent icon set, line style, designed for the toolpath types and setup kinds |
| Motion | Short transitions (≤ 150 ms) for panel changes and hover; no animation on data values; respect OS "reduce motion" |
| Viewport | Anti-aliased, GPU-rendered (`wgpu`, 01 §1.8); soft lighting; gradient background; edge outlines on B-rep faces |
| Toolpath display | Colour by toolpath, by motion type (rapid / feed / plunge / lead), by feed, or by segment type (line / arc / helix / rotary) to make DC-01 arc output visible |
| HiDPI | Resolution-independent UI scaling; per-monitor DPI |

Accessibility: WCAG AA contrast for text; colour is never the only signal (icons and shapes
too); toolpath colour palettes are colour-blind safe; full keyboard operation; UI scale setting.

## 8.10 First-run and guided flows

Instead of wizards, FluutCAM offers:

- Setup and toolpath templates (03 §3.10.6), applied from the ribbon;
- an optional guided overlay for a first project (import → setup → toolpath → simulate → post)
  that points at the real controls instead of opening separate wizard screens.

## 8.10a Localization and units

**Languages.** v1 ships in English. All user-visible strings are externalised from the start in
Fluent (`.ftl`, `fluent-rs`) files, so translations can be added without code changes. FluutCAM adds languages through community translation
after v1.

**Number display.** The UI formats numbers with the user's locale (e.g. decimal comma) and
accepts both `.` and the locale separator on entry. Files always use `.` (02 §2.0). G-code uses
the post's decimal character (05 §5.3).

**Units.**

- Internal unit is always mm (02 §2.1).
- Each project has a display and entry unit, `mm` or `inch` (`units` key, 02 §2.1), shown on the
  units badge in the title bar.
- Any numeric field accepts an explicit unit that overrides the project unit: `6mm`,
  `0.25in`, `1/4"`. The field shows the converted value in the project unit.
- Feeds display as mm/min or in/min; angles always in degrees.
- G-code output unit comes from the post (`output.units`, 05 §5.3), independent of the project
  display unit.
- STL and DXF imports always ask for the file's unit (02 §2.6.2).

## 8.11 Toolkit requirements

**[DECIDED — OQ-09]** The toolkit is chosen by prototyping in `egui` (with a docking crate) and
`slint`. The choice must meet these requirements:

1. Custom-drawn widgets and full theming (ribbon, galleries, timeline, dials).
2. Docking, floating and multi-window panels.
3. Embedding a `wgpu` viewport with overlays drawn on top.
4. Smooth animation at display refresh rate.
5. Accessibility support (screen-reader tree, keyboard focus).
6. HiDPI and per-monitor scaling.

Each prototype builds the ribbon (with a toolpath gallery), the `wgpu` viewport with an overlay
handle, the inspector with a scrub-able numeric field, and a docked/floating panel. Judge on look,
the six requirements above, and code effort. For `slint`, check licence terms against
FluutCAM's distribution model. Record the result in 01 §1.8.

## 8.12 Decisions

| Id | Decision |
|---|---|
| OQ-09 | Prototype `egui` and `slint`; choose against §8.11 |
| OQ-19 | v1 saves files (to a file or chosen folder, e.g. USB stick or network share). Direct send to controllers (e.g. GRBL serial, LinuxCNC remote) later, through per-controller adapters. |
| OQ-20 | Timeline in v1 without drag-reorder (§8.7) |
