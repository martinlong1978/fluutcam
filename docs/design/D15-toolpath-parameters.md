# D15 — Toolpath Parameter Definitions (`fluut-model`, `fluut-cam`)

Implements: [S 02 §2.1], [S 03 §3.3, §3.10], [S 04 §4.3.4, §4.12.4, §4.13.7], [S 08 §8.5,
§8.10a]. Builds on D06 §3 (parameter storage and field metadata), D08 (framework) and D09 §2.2,
§2.4, §4.3, §7 (strategies). Covers the common parameters and the first three types needed by
M3: `contour_2d`, `pocket_2d`, `parallel`. The other v1 types follow in M6 in the same form.

## 1. Conventions

### 1.1 How a field is described

Each field table gives:

| Column | Meaning |
|---|---|
| Key | TOML key inside its group table (`tool_params`, `geometry`, `heights`, `passes`, `linking`) [S 02 §2.1]; also the script key [S 09] |
| Type | Value kind (§1.3) and unit (§1.2) |
| Range | Allowed values. A value outside the range is a validation error unless the rule says warning |
| Default | Value used when the key is absent (§1.5) |
| Expr | **yes**: the inspector accepts an expression for it (§1.6); **—**: it does not |
| Depends on | Other fields this field's default, range, visibility or meaning depends on |

Validation rules are listed per section with a stable code and a severity (§1.7).

### 1.2 Units: stored and displayed

| Quantity | In the file and in code | In the inspector |
|---|---|---|
| Length | mm, `f64`, always, whatever the project `units` [S 02 §2.1] | project unit (`mm` or `inch`); an explicit unit in the entry wins (`6mm`, `0.25in`) [S 08 §8.10a] |
| Feed | mm/min | mm/min or in/min, by project unit |
| Spindle speed | rev/min | rev/min |
| Angle | degrees in the file (keys end in `_deg`, as `angle_deg` in [S 02 §2.1]); radians in code (design conventions) | degrees |
| Count | integer | integer |
| Ratio | plain number | plain number |

The writer stores lengths with shortest round-trip formatting (D06 §7.2). A value entered in
inches is converted once, at entry, and stored in mm. Re-display may therefore show a rounded
inch value; the stored mm value is the truth.

### 1.3 Value kinds

| Kind | TOML | `FieldKind` (D06 §3.3) |
|---|---|---|
| Length | float (integer accepted) | `Length { min, max }` |
| Angle | float (integer accepted) | `Angle` |
| Feed, Speed | float (integer accepted) | `Length`-like with feed unit |
| Count | integer | `Count` |
| Bool | boolean | `Bool` |
| Enum | snake_case string | `Enum` |
| Height | inline table `{ ref, offset }` (§2.4) | `Height` |
| Selection | array of inline tables (§2.3.1) | `FaceSet` / `Curve` |
| Point | array of 3 floats, model frame M, mm | `Point` |

A wrong TOML type is a load error (D06 §7.2), not a validation diagnostic.

### 1.4 Absent, default and set

- Every parameter is `Option<T>` in the document (D06 §3.1). Absent = `None` = use the default.
- A key present in the file is a local value, even when it equals the default. The writer never
  writes defaults.
- Unknown keys in `passes` are errors; in the other groups they are warnings (D06 §7.2). A key
  that belongs to a group but that the type does not declare (e.g. `boundary` on `contour_2d`)
  is an error [S 03 §3.10.3].

### 1.5 Default rules

Defaults use the `DefaultRule` forms of D06 §3.3:

| Form in the tables | Rule |
|---|---|
| `0.01` | `Const` |
| preset `step_down` | `FromTool`: the tool's cutting-data preset for the Setup's material [S 02 §2.4, S 03 §3.10.2] |
| Setup `…` | `FromSetup` |
| text with a formula | `Computed`, evaluated on the resolved values at calculation time |

Defaults are resolved at calculation time, so a preset edit changes every toolpath that uses
the default (D06 §3.1). A default marked **†** is provisional: the spec does not fix it and it is
listed in §7.

When a `FromTool` default has no preset to come from (no matching preset, or the preset lacks
the key) and the key is absent, the field has no value. This is validation error
`param.required` (§1.7).

### 1.6 Expressions

Every Length, Angle, Feed, Speed and Ratio field accepts an expression in the inspector:
arithmetic, units, and the names `tool.diameter`, `tool.radius`, `tool.corner_radius`,
`tool.flute_length` [S 08 §8.5]. In v1 the inspector evaluates the expression at entry and stores
the number (D13 §4). Templates may store an expression as a string (D06 §4.4); it is evaluated
when the template is applied, against the tool chosen then. A project file holds numbers only.

### 1.7 Validation codes and messages

- Rules run on resolved parameters (defaults applied) in `ToolpathType::validate` (D08 §1),
  after every edit and before calculation. Rules that need geometry (chaining, closed regions)
  run at calculation start and are reported as calculation diagnostics.
- Severity **E** (error): the toolpath cannot be calculated; the inspector shows the message
  under the field. **W** (warning): calculation runs; the message is shown.
- Codes: `param.<key>.<rule>` for common fields, `param.<type>.<key>.<rule>` for type fields.
  Calculation diagnostics keep the `cam.` prefix (D08 §4.4).
- The Fluent message id is the code with `.` replaced by `-` (Fluent ids do not allow dots),
  e.g. `param.heights.order` → `param-heights-order`. Messages take the field values as
  arguments. Field labels and help use `param-<key>-label` and `param-<key>-help`.

### 1.8 Invalidation classes

Each field belongs to one class. The class decides which cached results change when the field
changes (D01 §4, D06 §6.1):

| Class | Invalidates | Keeps |
|---|---|---|
| **R** (result) | this toolpath's `ToolpathResult`; through it, its simulation, its post output, and — if the type updates in-process stock (§1.9) — every later toolpath in the Setup with `rest_from` set, and child Setups with `stock = from_previous` | earlier toolpaths |
| **P** (post) | post output and time estimate only. Numeric feeds are resolved at post time (D08 §9) | `ToolpathResult` and its cache file |
| **N** (none) | nothing computed (display only) | everything |

For P fields to keep the cache, the `ToolpathResult` node hash must leave out the P fields.
D01 §4 and [S 03 §3.4] say the hash covers "Toolpath parameters"; this document narrows that to
R fields (§7, question 1).

### 1.9 Types and in-process stock

| Type | Updates in-process stock (D08 §7) | Reads it |
|---|---|---|
| `contour_2d` | yes (2-D types update stock) | no |
| `pocket_2d` | yes | when `rest_from` is set |
| `parallel` | no (CH-14) | no |

## 2. Common parameters

Each type declares which common fields it uses (§3–§5). This section defines them once.

### 2.1 Toolpath item keys

Keys directly in `[[setup.toolpath]]`, outside the groups.

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `id` | string | unique in project | assigned | — | — | N |
| `name` | string | non-empty | type label + number | — | — | P (comments in output) |
| `type` | Enum | catalogue ids [S 04 §4.13] | — (required) | — | Setup `kind` | fixed at creation |
| `tool` | DefRef `library#tool` | existing tool | — (required) | — | — | R |
| `enabled` | Bool | | `true` | — | — | R for downstream stock; P for itself |
| `template` | DefRef | | absent | — | — | N (informational, D06 §4.4) |

| Code | Sev | Rule |
|---|---|---|
| `param.type.setup_kind` | E | type allowed in the Setup's kind [S 03 §3.10.3]; all three types here: `three_axis`, `indexed` |
| `param.tool.missing` | E | `tool` resolves in the definition library |
| `param.tool.diameter` | E | tool diameter > 0 (D09 §7) |

### 2.2 Tool group — `tool_params`

| Key | Type | Range | Default | Expr | Depends on |
|---|---|---|---|---|---|
| `spindle_rpm` | Speed, rev/min | > 0 | preset `rpm` | yes | tool, Setup material |
| `feed.cut` | Feed, mm/min | > 0 | preset `feed` | yes | tool, Setup material |
| `feed.plunge` | Feed | > 0 | preset `plunge` | yes | tool, Setup material |
| `feed.ramp` | Feed | > 0 | `feed.cut` † | yes | `feed.cut` |
| `feed.lead_in` | Feed | > 0 | `feed.cut` † | yes | `feed.cut` |
| `feed.lead_out` | Feed | > 0 | `feed.lead_in` † | yes | `feed.lead_in` |
| `feed.transition` | Feed | > 0 | `feed.cut` † | yes | `feed.cut` |
| `coolant` | Enum `off` \| `flood` \| `mist` | | `off` † | — | post `coolant_on` event [S 05] |

`feed` is an inline table; each sub-key is optional on its own.

All Tool-group fields are class **P**.

| Code | Sev | Rule |
|---|---|---|
| `param.required` | E | a field with a `FromTool` default has neither a value nor a preset |
| `param.tool_params.spindle_rpm.machine_range` | W | `spindle_rpm` within the machine's `spindle.rpm` [S 02 §2.3] |
| `param.tool_params.feed.machine_max` | W | each feed ≤ machine `feed.max_cut` |
| `param.tool_params.coolant.unsupported` | W | coolant ≠ `off` and the resolved post has no `coolant_on` block |

### 2.3 Geometry group — `geometry`

#### 2.3.1 Selections

A selection is an array of inline tables, each naming one persistent entity:

```toml
drive_curves = [{ body = "b-91c0…" }, { edge = "e-3f2a…" }, { face = "f-12" }]
```

| Item | Meaning |
|---|---|
| `{ body = id }` | every curve of a WireBody, or every face of a Sheet/Solid body |
| `{ face = id }` | one face. In a curve selection, a planar face gives its loops |
| `{ edge = id }` | one B-rep edge or one WireBody curve |

Curve selections are chained: curves whose ends meet within modelling tolerance join into one
chain, in selection order. Chains are projected along the tool axis onto the XY plane of W for
2-D types; their Z is kept for the `selection` height reference (§2.4).

#### 2.3.2 Fields

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `drive_faces` | Selection (faces) | faces of the Setup's bodies | empty = all faces of the Setup's bodies [S 03 §3.3] | — | Setup `geometry` | R |
| `drive_curves` | Selection (curves) | edges, wire curves, planar faces | — (required by 2-D types) | — | Setup `geometry` | R |
| `check_faces` | Selection (faces) | | all faces not in `drive_faces` or `excluded_faces` [S 03 §3.3] | — | `drive_faces`, `excluded_faces` | R |
| `excluded_faces` | Selection (faces) | | empty | — | — | R |
| `boundary.curves` | Selection (curves) | closed chains | absent = no boundary | — | — | R |
| `boundary.side` | Enum `inside` \| `outside` \| `on` | | `inside` † | — | `boundary.curves` | R |
| `boundary.tool_containment` | Enum `inside` \| `centre` \| `outside` | | `inside` † | — | `boundary.curves`, tool radius | R |
| `rest_from` | Enum `off` \| `previous` | | `off` | — | earlier toolpaths in the Setup | R |

- `boundary` is one inline table with the three keys [S 03 §3.3]. `tool_containment` offsets the
  boundary by −R, 0 or +R before it limits the region (D08 §3).
- `rest_from = "previous"` takes the starting stock from the in-process stock after all earlier
  enabled toolpaths in the Setup, including stock carried from a parent Setup [S 03 §3.10.5,
  §3.9.6]. Choosing a subset of earlier toolpaths is §7, question 6.

| Code | Sev | Rule |
|---|---|---|
| `param.geometry.drive_curves.empty` | E | a type that needs curves has an empty `drive_curves` |
| `param.geometry.selection.missing` | E | a selected id is not in the model (after re-import, [S 03 §3.5]) |
| `param.geometry.overlap` | E | a face is in more than one of `drive_faces`, `check_faces`, `excluded_faces` |
| `param.geometry.boundary.open` | E | a `boundary` chain is not closed |
| `cam.geometry.chain_self_intersects` | E | a chain intersects itself (at calculation start) |

### 2.4 Heights group — `heights`

Each height is `{ ref = <reference>, offset = <Length> }` [S 03 §3.10.2]. `offset` defaults to
0. Values resolve to Z in the work frame W.

| Reference | Resolves to |
|---|---|
| `setup_clearance` | Setup `clearance` plane |
| `stock_top`, `stock_bottom` | Setup stock bounds along the tool axis |
| `model_top`, `model_bottom` | bounds of the Setup's bodies along the tool axis |
| `selection` | top/`feed`/`retract`/`clearance`: highest Z of the drive selection; `bottom`: lowest Z of the drive selection |

| Key | Range | Default | Depends on |
|---|---|---|---|
| `clearance` | ≥ `retract` | `{ ref = "setup_clearance" }` | Setup clearance |
| `retract` | ≥ `feed` | `{ ref = "stock_top", offset = 5 }` † | stock |
| `feed` | ≥ `top` | `{ ref = "top", offset = 2 }` † (relative to resolved `top`) | `top` |
| `top` | > `bottom` (≥ for `parallel`) | per type | per type |
| `bottom` | | per type | per type |

`offset` is a Length (mm in the file), accepts expressions, and may be negative. All Heights
fields are class **R**: they move cuts or links.

`feed`'s default refers to the resolved `top`; this is the only height that may take `top` as a
reference. Whether users may also use `top` (or the work zero) as a reference is §7, question 4.

| Code | Sev | Rule |
|---|---|---|
| `param.heights.order` | E | clearance ≥ retract ≥ feed ≥ top, and top > bottom (top ≥ bottom for `parallel`) |
| `param.heights.reference.empty` | E | `selection` used but the drive selection is empty |
| `param.heights.below_stock` | W | `bottom` below `stock_bottom` (cuts the fixture) |

### 2.5 Passes group — common fields

The type-specific Passes fields are in §3–§5. These are shared:

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `machining_tolerance` | Length | ≥ 1e-4 (D09 §7) | `0.01` [S 03 §3.3] | yes | — | R |
| `max_segment_length` | Length | > 0 | absent = no cap | yes | — | R |
| `output_arcs` | Bool | | `true` [S 03 §3.3] | — | post arc support | R |
| `pass_layout` | Enum `even` \| `fixed_from_edge` | | `even` [S 04 §4.12.4] | — | — | R |
| `stock_to_leave` | Length | ≥ 0 (§7, question 3) | `0` [S 03 §3.10.4] | yes | — | R |
| `stock_to_leave_axial` | Length | ≥ 0 | `stock_to_leave` | yes | `stock_to_leave` | R |

`stock_to_leave` is radial (walls) for 2-D types and a uniform normal offset for 3-D finishing
types. `stock_to_leave_axial` applies to floors and is used by 2-D types only.

| Code | Sev | Rule |
|---|---|---|
| `param.passes.machining_tolerance.min` | E | ≥ 1e-4 mm |
| `param.passes.machining_tolerance.coarse` | W | > 0.1 mm † |
| `param.passes.max_segment_length.short` | W | < 10 × machining tolerance (very long output) |
| `param.passes.stepover.max` | E | stepover (given or computed) ≤ tool diameter (D09 §7) |
| `param.passes.stepover.flat_width` | W | flat and bull tools: stepover > D − 2·corner radius (floor cusps) |
| `param.passes.step_down.flute` | W | step-down > tool flute length |

### 2.6 Linking group — `linking`

| Key | Type | Range | Default | Expr | Depends on |
|---|---|---|---|---|---|
| `retract` | Enum `full` \| `minimum` \| `stay_down` | | `minimum` † | — | — |
| `safety_distance` | Length | ≥ 0 | `1` † | yes | `retract` = `minimum` |
| `transitions` | Enum `direct` \| `smooth` | | `direct` † | — | `retract` ≠ `full` |
| `lead_in.kind` | Enum `none` \| `arc` \| `linear` | | per type | — | — |
| `lead_in.radius` | Length | > 0 | `0.5 · tool.diameter` † | yes | `lead_in.kind` = `arc` |
| `lead_in.length` | Length | ≥ 0 | `0` (arc) / `tool.radius` (linear) † | yes | `lead_in.kind` |
| `lead_out` | as `lead_in` | | same as `lead_in` | | `lead_in` |
| `ramp.kind` | Enum `plunge` \| `zigzag` \| `helix` \| `profile` | per type | per type | — | — |
| `ramp.angle_deg` | Angle | (0, 90] | `3` † (value in [S 02 §2.1] example) | yes | `ramp.kind` ≠ `plunge` |
| `ramp.radius` | Length | > 0 | `0.5 · tool.diameter` † | yes | `ramp.kind` = `helix` |
| `order` | Enum `by_level` \| `by_area` | | `by_level` (D08 §8.1) | — | types with levels |

- Leads: `arc` is a tangent quarter arc of `radius`, in the plane of the pass (2-D types) or
  vertical (3-D finishing) (D08 §8.3). `length` adds a straight tangent before an arc lead-in
  (after an arc lead-out); for `linear` it is the whole lead. `lead_out` absent copies `lead_in`.
- `safety_distance` is the clearance above the highest drop along a link for `minimum` retracts
  (D08 §8.2).
- Ramps apply to entries into material. If the chosen ramp does not fit the region, the
  framework falls back helix → zigzag → plunge and reports `cam.linking.ramp_fallback` (W).

All Linking fields are class **R**.

| Code | Sev | Rule |
|---|---|---|
| `param.linking.lead.radius` | E | `arc` lead with radius ≤ 0 |
| `param.linking.ramp.angle` | E | angle outside (0, 90] |
| `param.linking.ramp.plunge_tool` | W | `plunge` ramp with a tool not marked centre-cutting (§7, question 8) |

## 3. `contour_2d`

Profile along curves, or along the projection of walls, at one or more depths, with optional
finishing passes and tabs (D09 §2.4). Finishing type: no stock-clearing layers
[S 03 §3.10.4].

**Common fields used:** item keys; all of `tool_params`; `drive_curves`, `drive_faces` (vertical
wall faces: their bottom edges are profiled), `excluded_faces`; all heights; `machining_tolerance`,
`max_segment_length`, `output_arcs`, `stock_to_leave`, `stock_to_leave_axial`; all of
`linking`. **Not used:** `check_faces` (§7, question 5), `boundary`, `rest_from`, `pass_layout`
(depth levels are always `even`, D09 §2).

**Type defaults for common fields:** `heights.top = { ref = "stock_top" }`,
`heights.bottom = { ref = "selection" }`, `lead_in.kind = "arc"` †,
`ramp.kind = "plunge"` (allowed: `plunge`, `profile`).

### 3.1 Side and direction

Closed chains are first oriented counter-clockwise seen from +Z of W. Then `left` means inside
and `right` means outside. Open chains keep their selection direction. The tool centre path is
the chain offset by R + `stock_to_leave` to the chosen side (`on`: no offset).

`direction` sets the travel direction along the chain. With the spindle turning clockwise (M3),
climb means the material is on the right of the tool in the direction of travel. `direction`
reverses the traversal when needed; it never changes `side`. With `side = "on"` both sides cut
and `direction` is not used; open chains are then cut in selection direction.

### 3.2 Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on |
|---|---|---|---|---|---|
| `side` | Enum `left` \| `right` \| `on` | | `right` † (outside of closed chains) | — | chain orientation |
| `direction` | Enum `climb` \| `conventional` | | `climb` | — | `side` ≠ `on` |
| `compensation` | Enum `computer` \| `control` \| `wear` | | `computer` | — | `side`, post |
| `step_down` | Length | > 0 | preset `step_down` | yes | tool, material |
| `depth_count` | Count | ≥ 1 | absent | — | replaces `step_down` |
| `finishing_pass_count` | Count | 0 … 10 † | `0` | — | — |
| `finishing_stepover` | Length | (0, D] | `0.1 · tool.diameter` † | yes | `finishing_pass_count` > 0 |
| `spring_pass_count` | Count | 0 … 5 † | `0` | — | — |
| `start_point` | Point | | absent = nearest to the previous end (D09 §2.4) | — | closed chains |
| `tabs.mode` | Enum `off` \| `count` \| `distance` \| `points` | | `off` | — | — |
| `tabs.count` | Count | ≥ 1 | `4` † | — | `tabs.mode` = `count` |
| `tabs.distance` | Length | > `tabs.width` | `100` † | yes | `tabs.mode` = `distance` |
| `tabs.points` | array of Point | ≥ 1 point | — | — | `tabs.mode` = `points` |
| `tabs.width` | Length | > 0 | `tool.diameter` † | yes | `tabs.mode` ≠ `off` |
| `tabs.height` | Length | (0, top − bottom] | `1` † | yes | heights |
| `tabs.ramp_angle_deg` | Angle | (0, 90] | `45` † | yes | `tabs.mode` ≠ `off` |

- **Depths.** If `depth_count` is set, levels are `depth_count` even steps from top to bottom.
  Otherwise levels are `layout(top, bottom, step_down)` in even mode (D09 §2), so no step
  exceeds `step_down`. The last level is always exactly `bottom + stock_to_leave_axial`.
  `step_down ≥ top − bottom` gives one depth.
- **Finishing passes.** At each level, passes at offsets R + `stock_to_leave` +
  i·`finishing_stepover`, for i = `finishing_pass_count` … 0; the last pass is on the final
  offset (D09 §2.4). Whether these run at every level or only the last is §7, question 7.
- **Spring passes** repeat the final pass at the final level, unchanged.
- **Compensation.** `computer`: the offset is in the path. `control`: the path is the chain
  offset by `stock_to_leave` only, and the post emits `cutter_comp_left/right` on the lead-in and
  `cutter_comp_off` on the lead-out (D10 §4.6). `wear`: as `computer`, and the post also emits
  the compensation events, so the operator can adjust wear.
- **Tabs.** `count` places tabs evenly by arc length on each closed chain (and on each open
  chain, from its ends inward). `distance` places one tab every `distance` along each chain.
  `points` stores model-frame points; each snaps to the nearest point of the projected chains.
  Below tab top (`bottom + tabs.height`), the path rises to tab top over each tab, with ramps at
  `tabs.ramp_angle_deg` (D09 §2.4). Tab ends are exact.

All `contour_2d` fields are class **R**.

### 3.3 Validation

| Code | Sev | Rule |
|---|---|---|
| `param.contour_2d.depths.both` | E | `step_down` and `depth_count` both set locally |
| `param.contour_2d.depth.flute` | W | top − bottom > tool flute length |
| `param.contour_2d.compensation.on` | E | `compensation` ≠ `computer` with `side = "on"` |
| `param.contour_2d.compensation.post` | E | `control` or `wear` and the resolved post has no `cutter_comp_*` blocks |
| `param.contour_2d.compensation.lead` | W | `control` or `wear` and the straight part of the lead-in is shorter than the tool radius (most controls need a linear move to start compensation) |
| `param.contour_2d.finishing_stepover.max` | E | `finishing_stepover` > tool diameter |
| `param.contour_2d.tabs.height` | E | `tabs.height` > top − bottom |
| `param.contour_2d.tabs.points.empty` | E | `tabs.mode = "points"` with no points |
| `cam.contour_2d.tabs.overlap` | W | tabs plus their ramps overlap on a chain; overlapping tabs merge |
| `cam.contour_2d.offset_empty` | W | an inside offset vanishes (chain too small for the tool) |
| `param.contour_2d.start_point.open` | W | `start_point` set but every chain is open (ignored) |

### 3.4 Example

```toml
  [[setup.toolpath]]
  id   = "t5"
  name = "Profile outside"
  type = "contour_2d"
  tool = "user/router-bits#flat-6"

    [setup.toolpath.tool_params]
    spindle_rpm = 18000
    feed        = { cut = 2000, plunge = 600 }

    [setup.toolpath.geometry]
    drive_curves = [{ body = "b-91c0…" }]          # WireBody from a DXF outline

    [setup.toolpath.heights]
    top    = { ref = "stock_top" }
    bottom = { ref = "stock_bottom", offset = -0.2 }   # cut through into the spoilboard

    [setup.toolpath.passes]
    side                 = "right"                 # outside of the closed outline
    direction            = "climb"
    step_down            = 4.0
    finishing_pass_count = 1
    finishing_stepover   = 0.3
    stock_to_leave       = 0
    tabs = { mode = "count", count = 4, width = 8, height = 1.5, ramp_angle_deg = 45 }

    [setup.toolpath.linking]
    lead_in = { kind = "arc", radius = 3, length = 2 }
    ramp    = { kind = "profile", angle_deg = 5 }
```

## 4. `pocket_2d`

Clears closed regions in Z steps with an offset or raster pattern (D09 §2.2). Roughing type.

**Common fields used:** item keys; all of `tool_params`; `drive_curves` (closed chains; nested
chains become islands), `drive_faces` (planar floor faces: their loops give the region and their
Z the floor), `excluded_faces`, `boundary`, `rest_from`; all heights; all common Passes fields;
all of `linking`. **Not used:** `check_faces` (§7, question 5).

**Type defaults for common fields:** `heights.top = { ref = "stock_top" }`,
`heights.bottom = { ref = "selection" }`, `lead_in.kind = "none"`, `ramp.kind = "helix"`,
`stock_to_leave = 0` † (§7, question 2).

### 4.1 Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on |
|---|---|---|---|---|---|
| `pattern` | Enum `offset` \| `raster` | | `offset` † | — | — |
| `stepover` | Length | (0, D] | preset `stepover` | yes | tool, material |
| `step_down` | Length | > 0 | preset `step_down` | yes | tool, material |
| `direction` | Enum `climb` \| `conventional` | | `climb` | — | — |
| `offset_order` | Enum `inside_out` \| `outside_in` | | `inside_out` (D09 §2.2) | — | `pattern` = `offset` |
| `pass_angle_deg` | Angle | any; stored as given, used mod 180 | `0` | yes | `pattern` = `raster` |

- **Levels:** `layout(top, bottom, step_down)` in even mode, plus the exact floor
  `bottom + stock_to_leave_axial` (D09 §2).
- **Offset:** rings at −(R + `stock_to_leave`) − k·`stepover` until the region is empty; the
  first ring is the finishing boundary (D09 §2.2). `pass_layout` does not apply.
- **Raster:** lines at `pass_angle_deg` laid out by `pass_layout` across the region, then a
  final contour pass on the boundary ring. Raster lines alternate direction (zig-zag);
  `direction` applies to the rings and the final contour pass (§7, question 9).
- **Rest:** with `rest_from = "previous"`, the region at each level is intersected with the
  remaining stock grown by R (D09 §2.2).

All `pocket_2d` fields are class **R**. Because `pocket_2d` updates the in-process stock, a
change also makes later rest toolpaths in the Setup and `from_previous` child Setups stale.

### 4.2 Validation

| Code | Sev | Rule |
|---|---|---|
| `param.passes.stepover.max` | E | `stepover` > tool diameter |
| `param.passes.stepover.flat_width` | W | `stepover` > D − 2·corner radius |
| `param.passes.step_down.flute` | W | `step_down` > flute length |
| `param.pocket_2d.tool_shape` | W | tool is not flat or bull (floor will be scalloped) |
| `cam.pocket_2d.region.open` | E | a drive chain is not closed |
| `cam.pocket_2d.region.unreachable` | W | a region (or part of one) is narrower than the tool and is left uncut |
| `cam.linking.ramp_fallback` | W | the helix ramp does not fit and a fallback was used |

### 4.3 Example

```toml
  [[setup.toolpath]]
  id   = "t3"
  name = "Pocket flat"
  type = "pocket_2d"
  tool = "user/router-bits#flat-6"

    [setup.toolpath.geometry]
    drive_faces = [{ face = "f-41" }]               # planar floor face: region + floor Z

    [setup.toolpath.heights]
    top    = { ref = "stock_top" }
    bottom = { ref = "selection" }

    [setup.toolpath.passes]
    pattern              = "offset"
    stepover             = 2.4
    step_down            = 3.0
    direction            = "climb"
    offset_order         = "inside_out"
    stock_to_leave       = 0.2
    stock_to_leave_axial = 0.1

    [setup.toolpath.linking]
    retract = "minimum"
    ramp    = { kind = "helix", angle_deg = 3, radius = 2.5 }
```

## 5. `parallel`

Parallel passes in XY that follow the surface in Z (D09 §4.3). Finishing type.

**Common fields used:** item keys; all of `tool_params`; `drive_faces`, `check_faces`,
`excluded_faces`, `boundary`; all heights; `machining_tolerance`, `max_segment_length`,
`output_arcs`, `pass_layout`, `stock_to_leave`; `retract`, `safety_distance`, `transitions`,
`lead_in`, `lead_out`. **Not used:** `drive_curves`, `rest_from`, `stock_to_leave_axial`,
`ramp`, `order`.

**Type defaults for common fields:** `heights.top = { ref = "model_top" }`,
`heights.bottom = { ref = "model_bottom" }`, `lead_in.kind = "none"` †. CL points below `bottom`
are raised to `bottom`; pass portions whose CL is above `top` are not cut and are linked over.

### 5.1 Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on |
|---|---|---|---|---|---|
| `stepover` | Length | (0, D] | preset `stepover` | yes | tool, material; not with `scallop_height` |
| `scallop_height` | Length | (0, r) where r = corner radius | absent | yes | tool shape |
| `pass_angle_deg` | Angle | any; used mod 180 | `0` † | yes | — |
| `direction` | Enum `zigzag` \| `climb` \| `conventional` | | `zigzag` † | — | — |
| `min_slope_deg` | Angle | [0, 90] | `0` | yes | `max_slope_deg` |
| `max_slope_deg` | Angle | [0, 90] | `90` | yes | `min_slope_deg` |

- **Scallop height.** If `scallop_height` is set, the stepover is computed once per toolpath
  from the worst-case convex curvature of the drive faces (OQ-05, D09 §4.3), using the tool's
  corner radius r in place of the ball radius (exact for ball tools, conservative for bull
  tools). The computed value is shown read-only in the inspector next to `stepover`.
- **Direction.** `zigzag` alternates passes. `climb` and `conventional` cut one way; climb puts
  the side of the next pass on the right of travel (spindle M3).
- **Slope** is the angle between the contact normal and +Z of W: 0° flat, 90° vertical. Only
  contacts with slope in [`min_slope_deg`, `max_slope_deg`] are cut; cut-off points are exact
  (D09 §4.3).
- **Stock to leave** is a uniform offset of the drive faces, computed as a drop with the cutter
  profile grown by `stock_to_leave` (D07 §1).

All `parallel` fields are class **R**. `parallel` does not update in-process stock (CH-14), so a
change does not affect later toolpaths.

### 5.2 Validation

| Code | Sev | Rule |
|---|---|---|
| `param.parallel.stepover_and_scallop` | E | `stepover` and `scallop_height` both set locally |
| `param.parallel.scallop.radius` | E | `scallop_height` set and the tool has no corner radius (flat, or V with r = 0) |
| `param.parallel.scallop.range` | E | `scallop_height` ≥ corner radius |
| `param.passes.stepover.max` | E | given or computed stepover > tool diameter |
| `param.parallel.slope.order` | E | `min_slope_deg` ≥ `max_slope_deg` |
| `param.parallel.slope.range` | E | a slope limit outside [0, 90] |
| `param.parallel.drive_faces.mesh_scallop` | W | `scallop_height` on mesh faces: curvature from triangles is zero inside facets, so the stepover is the flat-area value |
| `cam.parallel.scallop.concave` | W | a concave region has radius ≤ R and is reported as rest material [S 04 §4.3.4] |

### 5.3 Example

```toml
  [[setup.toolpath]]
  id   = "t6"
  name = "Parallel finish"
  type = "parallel"
  tool = "user/router-bits#ball-6"

    [setup.toolpath.tool_params]
    spindle_rpm = 18000
    feed        = { cut = 2500 }

    [setup.toolpath.geometry]
    drive_faces = [{ body = "b-7f3a…" }]
    boundary    = { curves = [{ body = "b-55e1…" }], side = "inside", tool_containment = "centre" }

    [setup.toolpath.heights]
    bottom = { ref = "model_bottom", offset = 0.5 }

    [setup.toolpath.passes]
    machining_tolerance = 0.005
    scallop_height      = 0.01
    pass_angle_deg      = 45
    direction           = "zigzag"
    pass_layout         = "even"
    max_slope_deg       = 80
    stock_to_leave      = 0

    [setup.toolpath.linking]
    retract     = "minimum"
    transitions = "smooth"
```

## 6. Implementation notes

- Each `*Passes` struct gets `#[derive(Params)]` attributes carrying the tables above: key,
  kind, range, default rule, `visible_if` (from the Depends on column), class (R/P/N) (D06 §3.3).
  The class is new metadata: the invalidation graph uses it to build the result hash (§1.8).
- Validation rules are plain functions over `ResolvedParams`, one per code, so each code has
  one test with a passing and a failing value.
- `visible_if` hides fields; a hidden field set in the file is kept and written back, but not
  used (e.g. `offset_order` while `pattern = "raster"`). It is not an error, so switching a
  pattern back and forth loses nothing.

Tests:

- Every example in this document parses, resolves and re-serialises byte-identically (D06 §8).
- Every validation code: one passing, one failing case.
- Changing each P field leaves the `ToolpathResult` hash unchanged; changing each R field
  changes it.
- Unit round trip: entering each Length field in inches and reading it back gives the same mm
  value to 1e-12.

## 7. Open questions

The spec does not settle these. The design uses the provisional choice shown (marked † in the
tables) until the product owner decides.

| # | Question | Provisional choice |
|---|---|---|
| 1 | [S 03 §3.4] and D01 §4 put "Toolpath parameters" into the result hash. D08 §9 says a feed change needs no recalculation. Should the hash cover only result-affecting (R) fields? | Yes: hash R fields only (§1.8). Needs a one-line change to S 03 §3.4 and D01 §4 |
| 2 | Default `stock_to_leave` for roughing types (`pocket_2d`). The spec gives 0 only for finishing types. | `0` |
| 3 | Negative stock to leave (deliberate undercut), and its lower bound. | Not allowed in v1 (≥ 0) |
| 4 | Height references beyond the spec's list: the work zero (Z = 0), and other heights (`top`) as a reference. | Only `feed` defaults to `top`; no user-visible extra references |
| 5 | Do 2-D types protect check faces (gouge check against the model), or cut exactly what the curves say? | Not in v1; 2-D types do not use `check_faces` |
| 6 | `rest_from`: all earlier toolpaths only, or a chosen subset? | `off` \| `previous` |
| 7 | `contour_2d` finishing passes at every depth level, or only at the final level (or a choice)? | Every level |
| 8 | Tool library has no "centre cutting" flag [S 02 §2.4]; plunge ramps cannot be checked. | Warning only when a flag exists; add `centre_cutting` to the tool format |
| 9 | One-way raster for `pocket_2d` (so raster passes are all climb)? | Zig-zag only |
| 10 | Spec 02 lists no key for 2-D drive curves, and no persistent id form for edges and wire curves (03 §3.5 covers bodies and faces). | `drive_curves`; `{ edge = id }` with ids from the same scheme as faces |
| 11 | Where the Setup's material is stored, which selects the tool preset [S 03 §3.10.2]. Spec 02 has no `material` key on a Setup. | A `material` string on the Setup, matched to `tool.preset.material` |
| 12 | `boundary.side = "on"` [S 03 §3.3]: meaning for a containment boundary. | Treated as `inside` with `tool_containment = "centre"`; may be dropped |
| 13 | Persistence of `contour_2d` tab positions across re-import and chain changes. | Model-frame points, snapped to the nearest chain point |
| 14 | `contour_2d` side for closed chains: `left`/`right` after CCW orientation, or explicit `inside`/`outside`? | `left`/`right` as the spec says, with closed chains oriented CCW (§3.1) |
| 15 | Provisional constants: feed defaults for ramp, leads and transitions; heights (`retract` stock top + 5, `feed` top + 2); `safety_distance` 1; lead and helix radii 0.5·D; ramp angle 3°; tab defaults; `coolant` off; tolerance warning above 0.1 mm; `finishing_pass_count` ≤ 10, `spring_pass_count` ≤ 5; default `pattern`, `side`, `pass_angle_deg`, `direction`, `retract`, `transitions` and lead kinds. | As in the tables |
| 16 | Spindle direction: climb/conventional assume M3 (clockwise). Machines or tools that run M4. | M3 assumed; revisit with the machine format |

## 8. Challenges

- None new. Question 1 is a spec change, not a design problem; it is raised with the product
  owner with the other open questions.
