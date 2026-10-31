# D13 — UI Architecture (`fluut-app`)

Implements: [S 08], OQ-09 (prototype `egui` vs `slint`), OQ-20 (timeline v1). Depends on:
`fluut-core`, `fluut-script`, `fluut-i18n`, `wgpu`, toolkit (pending), `accesskit`, `nucleo`
(fuzzy search).

The toolkit is not chosen yet (OQ-09). This design keeps everything except the widget layer
toolkit-independent, so the prototype result changes only §6.

## 1. Layers

```
┌──────────────────────────────────────────────────────────────┐
│ Widget layer (toolkit-specific): ribbon, browser, inspector, │
│ timeline, palette, dialogs, toasts                          │
├──────────────────────────────────────────────────────────────┤
│ View models (toolkit-independent): BrowserVM, InspectorVM,   │
│ RibbonVM, TimelineVM, StatusVM — derived from state           │
├──────────────────────────────────────────────────────────────┤
│ App state: Session (fluut-core) + UiState + CommandRegistry  │
├──────────────────────────────────────────────────────────────┤
│ Viewport renderer (wgpu): scene, overlays, picking           │
└──────────────────────────────────────────────────────────────┘
```

Data flow is one-directional:

```
input event ──► Command (by id) ──► handler ──► Edit(s) via Session::apply  or  UiState change
                                                    │
SessionEvent (doc changed, job progress, result) ◄──┘
        │
        ▼
view models recomputed (memoised by document revision) ──► widgets redraw
```

Widgets never mutate the document directly. This gives one path for validation, undo, journal
and invalidation (D06 §6), and makes every UI action scriptable and testable without a window.

## 2. App state

```rust
pub struct App {
    session: Session,                     // D01 §5
    ui: UiState,
    commands: CommandRegistry,
    renderer: ViewportRenderer,
}

pub struct UiState {
    pub stage: Stage,                     // Model | Setup | Toolpaths | Simulate | Post | Library (S 08 §8.3)
    pub selection: Selection,             // setups, toolpaths, bodies, faces, edges, handles
    pub hover: Option<Pickable>,
    pub interaction: Interaction,         // idle | dragging handle | picking faces for a field | …
    pub panels: PanelLayout,              // per stage; persisted in settings
    pub camera: Camera,
    pub display: DisplayOptions,          // toolpath colouring mode, visibility, section view
    pub timeline: TimelineState,          // playhead, zoom
    pub notifications: Vec<Notification>,
}
```

## 3. Commands

```rust
pub struct CommandDef {
    pub id: &'static str,                 // "toolpath.add.parallel", "setup.new_based_on"
    pub label: MessageId, pub icon: IconId,
    pub shortcut: Option<KeyChord>,
    pub stages: &'static [Stage],         // where it appears in the ribbon
    pub enabled: fn(&App) -> bool,
    pub run: fn(&mut App, CommandArgs) -> Result<, UiError>,
}
```

- The ribbon is data: an embedded TOML file lists tabs → groups → command ids [S 08 §8.3].
  Toolpath galleries are generated from the strategy registry (D09 §1), filtered by the
  selected Setup's kind.
- The command palette searches command labels, toolpath types and object names with `nucleo`
  [S 08 §8.8].
- Shortcuts are editable and stored in settings.
- Scripts in the library appear as commands (`script.<name>`).

## 4. Inspector

Generated from field metadata (D06 §3.3):

| Field kind | Widget |
|---|---|
| Length, Angle, Percent | numeric field: unit suffix, expression entry, drag-to-scrub label [S 08 §8.5] |
| Enum | segmented control (≤ 4 options) or dropdown |
| Bool | toggle |
| FaceSet | selection chips + "pick in viewport" button (enters `Interaction::PickFaces`) |
| Curve | chip + pick |
| Height | reference dropdown + offset field |
| DefRef (tool, machine) | searchable picker with preview |

- Inherited Setup fields show a dimmed value, an "inherited from …" chip and a reset button,
  from `FieldOrigins` (D06 §2).
- Validation diagnostics appear under their field (D08 §1 `validate`).
- Edits are applied on each change; scrubbing coalesces into one undo step (D06 §6.2).
- Numeric entry parses expressions and units in `fluut-i18n`; `tool.diameter` and similar
  references are evaluated at entry time and stored as numbers (not live formulas) in v1.

## 5. Viewport renderer (`wgpu`)

### 5.1 Scene layers

| Layer | Source | GPU representation |
|---|---|---|
| Bodies | display tessellation (D03 §7) | indexed triangles, per-face id buffer |
| Edges | edge polylines | line list (screen-space width via quads) |
| Stock (static) | Setup stock spec | translucent mesh |
| Simulation stock | tri-dexel mesh tiles (D11 §4) | per-tile meshes, deviation colour map |
| Toolpaths | `ToolpathResult` segments | polylines in GPU buffers; arcs tessellated at screen-space tolerance; per-vertex attributes: motion, feed, segment type, toolpath id |
| Overlays | handles, gizmos, work zero triad, rotary dial, tab markers, pass preview | separate pass, depth-aware with x-ray option |

Toolpath colouring modes [S 08 §8.9] are a shader uniform; switching mode does not rebuild
buffers.

### 5.2 Picking

An ID render target (u32 per pixel: layer + object id) is rendered on demand at the cursor
(1×1 or small region), giving face, edge, toolpath segment or handle under the cursor without
CPU ray casting. Exact 3-D points (for snapping work zero to arc centres, etc.) are then
computed on the exact geometry (D03 §4.2 `ray`).

### 5.3 Handles and gizmos

Each handle is an object with `hit_test`, `drag(start, current) -> Edit`, and `render`:

| Handle | Edit produced |
|---|---|
| Stock face handle | `SetSetupField(stock margins)` |
| Work zero triad | `SetSetupField(zero)` with snapping to stock points, vertices, arc centres |
| Rotary dial | `SetSetupField(index_angle)`; snaps 15° and face normals |
| Tab marker | `SetToolpathParam(tabs)` |
| Lead-in handle | `SetToolpathParam(entry point)` |
| Boundary vertex / offset | `SetToolpathParam(boundary)` |

Drags send edits continuously (coalesced into one undo step) so dependent toolpaths go stale and
the pass preview updates live [S 08 §8.4].

### 5.4 Performance targets

| Case | Target |
|---|---|
| Frame time, 2 M toolpath segments + 50 k-face model | ≤ 16 ms on a mid-range GPU |
| Upload of a new 1 M-segment result | ≤ 200 ms, off the UI thread (buffer built on job thread) |
| Pick latency | ≤ 1 frame |
| Cold start to interactive | ≤ 2 s |

## 6. Widget layer (toolkit-specific)

Depends on OQ-09. Requirements [S 08 §8.11] and what the prototype must show:

| Component | egui notes | slint notes |
|---|---|---|
| Ribbon with galleries | custom painted; easy | `.slint` components; strong styling |
| Docking panels | `egui_dock` | custom (no built-in docking) |
| wgpu viewport + overlays | `egui_wgpu` paint callback | `slint` wgpu underlay/texture import |
| Timeline | custom painted | custom component |
| Accessibility | AccessKit integrated | built-in accessibility |
| Theming | style tokens via `egui::Style` | design tokens in `.slint` globals |
| Licence | MIT/Apache | GPL-3 / royalty-free / commercial — check vs MIT/Apache [S 06 §6.1] |

Prototype scope [S 08 §8.11]; decision recorded in [S 01 §1.8].

### 6.1 Branding

The window icon, the About dialog and the start page use the logos and icons in `assets/` (see
`assets/README.md`), following the system light or dark theme with the light and dark logo
variants.

## 7. Timeline (v1, OQ-20)

`TimelineVM` = list of Setups → toolpath clips with start time and duration (D11 §6), markers
(tool change, index, clamp). Scrubbing sets `timeline.playhead` and, on the Simulate tab, asks
`fluut-sim` for the stock at that time (D11 §4). Reordering by dragging clips is not in v1.

## 8. Background work display

Session job events update `StatusVM`: browser icons, progress rings, stale markers, diagnostic
markers in the viewport (location from `Diagnostic.location`), toasts on completion
[S 08 §8.6]. Every job is cancellable from its icon (`CancelToken`).

## 9. Localization and units

All strings via `fluut-i18n` (Fluent). Numbers formatted with the user locale; files and G-code
are locale-independent [S 08 §8.10a]. A pseudo-locale (accented, 30 % longer strings) is used in
tests to catch truncation and hard-coded text.

## 10. Tests

- View-model tests without a window: command → edit → view model output.
- Screenshot tests (headless wgpu) for the viewport layers on reference projects, compared with
  tolerance.
- Accessibility tree checks: every control has a role and label.
- Input automation for core flows (import → setup → toolpath → calculate → post) via the
  command layer.

## 11. Challenges

- CH-23 Live pass preview cost while dragging (deferred).
