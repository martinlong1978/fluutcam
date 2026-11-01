# D15b — Toolpath Parameter Definitions, part 2 (`fluut-model`, `fluut-cam`)

Implements: [S 03 §3.10], [S 04 §4.13.7] for the 19 v1 types that D15 does not cover. Builds on
D15 (conventions §1, common fields §2, decisions §7), D08 §7 (in-process stock) and D09
(strategies). Needed by M6.

D15 §1 applies unchanged: field tables, units, defaults (**†** = a constant the spec does not
fix), expressions, validation codes and invalidation classes. Common fields are defined once in
D15 §2 and are not repeated here. Each type section gives:

- a description and its role (roughing or finishing, [S 03 §3.10.4]);
- the common fields used and not used, and the type defaults for common fields;
- the `passes` fields, with class (D15 §1.8);
- rules, validation codes and a TOML example;
- whether it updates or reads in-process stock (D15 §1.9, §1 here).

A † default in this document is provisional until the open questions (§9) are decided.

Types:

| Group | Types | Section |
|---|---|---|
| 2-D | `face`, `adaptive_2d`, `trace`, `engrave`, `chamfer_2d`, `bore` | §2 |
| Holes | `drill` | §3 |
| 3-D roughing | `adaptive_3d`, `pocket_3d` | §4 |
| 3-D finishing | `contour`, `horizontal`, `spiral`, `scallop`, `steep_and_shallow` | §5 |
| Rotary | `rotary_rough`, `rotary_along_x`, `rotary_around_a`, `rotary_spiral` | §6 |
| Utility | `manual_nc` | §7 |

## 1. Common additions

These extend D15 §1–§2 for the types here. They do not change the D15 types.

### 1.1 Duration unit

| Quantity | In the file and in code | In the inspector |
|---|---|---|
| Duration | seconds, `f64`; keys end in `_s` | seconds |

A Duration field accepts an expression (arithmetic only). Kind: `Duration { min, max }` in
`FieldKind` (new variant, D06 §3.3).

### 1.2 Setup kinds per type

`param.type.setup_kind` (D15 §2.1) uses this table:

| Types | Allowed Setup kinds |
|---|---|
| 2-D, holes, 3-D (§2–§5) | `three_axis`, `indexed` |
| rotary (§6) | `rotary` |
| `manual_nc` | `three_axis`, `indexed`, `rotary` |

### 1.3 Heights in rotary Setups

In a `rotary` Setup every height is a radius from the rotary axis, not a Z [S 03 §3.10.2]. The
form `{ ref, offset }` and the order rule (`param.heights.order`) are unchanged; "higher" means
larger radius.

| Reference | Resolves to (rotary Setup) |
|---|---|
| `setup_clearance` | Setup `clearance` cylinder radius [S 02 §2.1] |
| `stock_top` | stock cylinder radius |
| `stock_bottom` | 0 (the axis) |
| `model_top`, `model_bottom` | largest and smallest radius of the Setup's bodies within the X range (§1.4) |
| `work_zero` | 0 (the axis; Y and Z of the zero lie on it [S 03 §3.9.3]) |
| `selection` | as `model_top` / `model_bottom`, over the drive faces |

Defaults for rotary types: `retract = { ref = "stock_top", offset = 5 }` †, `feed` as D15 §2.4.
`param.heights.below_stock` becomes `param.heights.below_axis` (W): `bottom` < 0 is impossible
and is clamped to 0.

### 1.4 Geometry: X range (rotary types)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `x_start` | `{ ref, offset }` along X of W | | `{ ref = "model_left" }` | offset: yes | Setup bodies | R |
| `x_end` | `{ ref, offset }` along X of W | > `x_start` | `{ ref = "model_right" }` | offset: yes | Setup bodies | R |

References: `stock_left`, `stock_right` (stock cylinder ends), `model_left`, `model_right`
(X extent of the drive faces; of the Setup's bodies when `drive_faces` is empty), `work_zero`
(X = 0). The region of a rotary type is the band [`x_start`, `x_end`] × full A. A tool-centre
region is not offset by R at the ends: the tool centre runs from `x_start` to `x_end`, so set an
offset to stay clear of a chuck.

| Code | Sev | Rule |
|---|---|---|
| `param.geometry.x_range.order` | E | `x_end` ≤ `x_start` |
| `param.geometry.x_range.stock` | W | the range goes beyond the stock cylinder ends |

### 1.5 Geometry: hole filter (`drill`, `bore`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `hole_diameter` | inline table `{ min, max }` of Lengths | 0 ≤ `min` ≤ `max` | absent = no automatic holes | yes | — | R |

With `hole_diameter` set and `drive_faces` and `drive_curves` empty, the type takes every
cylindrical hole of the Setup's bodies whose axis is parallel to the tool axis and whose diameter
is in [`min`, `max`] (D09 §3.1). Either key may be absent (no limit on that side).
`param.geometry.drive.empty` is not raised when `hole_diameter` is set.

### 1.6 Per-feature heights (`drill`, `bore`)

For `drill` and `bore` the `selection` reference resolves per hole: `top` is the top of that
hole's cylinder (or coaxial stack, D09 §3.1) and `bottom` its bottom. For a `drive_curves`
circle, `selection` gives the circle's Z for both. Other references resolve once, as in D15
§2.4.

### 1.7 Common Passes fields: wider use

- `stock_to_leave_axial` (D15 §2.5) is also used by `adaptive_3d`, `pocket_3d` and
  `horizontal`. It applies to the horizontal faces of the model: flats are cut at their Z +
  `stock_to_leave_axial`; elsewhere the radial `stock_to_leave` applies.
- In rotary types `stock_to_leave` is a uniform normal offset, as for 3-D types, with the same
  lower bound (> −r).

### 1.8 Shared type fields

Several types reuse a field defined by a D15 type. The key, range and meaning are the same;
codes take the new type's id.

| Fields | Defined in | Reused by |
|---|---|---|
| `stepover` / `scallop_height` pair, with `stepover_and_scallop`, `scallop.radius`, `scallop.range` | D15 §5.1, §5.2 | `spiral`, `scallop`, `steep_and_shallow` (shallow), rotary finishing (§6) |
| `min_slope_deg`, `max_slope_deg`, with `slope.order`, `slope.range` | D15 §5.1, §5.2 | `contour`, `scallop` |
| `pattern`, `offset_order`, `pass_angle_deg` (raster) | D15 §4.1 | `pocket_3d`, `horizontal` (no `offset_order`) |
| `step_down` / `depth_count` pair, `depths.both` | D15 §3.2, §3.3 | `trace` |
| `side`, `direction` (climb/conventional) of closed and open chains | D15 §3.1 | `chamfer_2d` |

The scallop-height rule (one worst-case stepover per toolpath, OQ-05) applies to every type that
takes `scallop_height`. Rotary types use the curvature across their passes in the cylindrical
frame (§6).

### 1.9 Tool-shape checks

Types that need a tool shape report `param.<type>.tool_shape`. Shapes come from the tool
`shape` key [S 02 §2.4]: `flat`, `ball`, `bull`, `v`, `taper`. "V tool" below means `v` or
`taper` with half-angle α > 0 (D07 §1).

### 1.10 In-process stock, all types

Extends D15 §1.9. Updating follows D08 §7: 2-D, hole and roughing types subtract their swept
region at each level; 3-D and rotary finishing types do not (CH-14).

| Type | Updates in-process stock | Reads it |
|---|---|---|
| `face` | yes | no |
| `adaptive_2d` | yes | when `rest_from` is set |
| `trace` | yes | no |
| `engrave` | yes (swept width = V width at each level) | no |
| `chamfer_2d` | yes | no |
| `bore` | yes | no |
| `drill` | yes (a disc of tool radius at each level down to the hole bottom) | no |
| `adaptive_3d` | yes | always (§4) |
| `pocket_3d` | yes | always (§4) |
| `contour`, `horizontal`, `spiral`, `scallop`, `steep_and_shallow` | no (CH-14) | no |
| `rotary_rough` | yes (cylindrical layers) | always (§6.1) |
| `rotary_along_x`, `rotary_around_a`, `rotary_spiral` | no | no |
| `manual_nc` | no | no |

**Starting stock of 3-D and rotary roughing.** These types always need a starting stock, to know
where material is. With `rest_from = "off"` it is the Setup's stock (block, cylinder or body;
for `from_previous` the stock carried from the parent Setup). With `rest_from = "previous"` it
is the in-process stock after all earlier enabled toolpaths in the Setup (D15 §2.3.2).

## 2. 2-D types

### 2.1 `face`

Clears the stock top down to a flat Z with zig-zag passes (D09 §2.1). Roughing type
[S 03 §3.10.4].

**Common fields used:** item keys; all of `tool_params`; `drive_faces` (planar horizontal faces:
their outline joins the area and the highest one sets `selection`), `boundary`; all heights;
`machining_tolerance`, `max_segment_length`, `output_arcs`, `pass_layout`,
`stock_to_leave_axial`; `retract`, `safety_distance`, `transitions`, `lead_in`, `lead_out`.
**Not used:** `drive_curves`, `check_faces`, `excluded_faces`, `rest_from`, `stock_to_leave`
(no walls), `ramp` (every entry is from outside the stock), `order`.

**Type defaults for common fields:** `heights.top = { ref = "stock_top" }`,
`heights.bottom = { ref = "model_top" }`, `lead_in.kind = "none"`,
`linking.retract = "stay_down"` † (links between passes are outside or above the part).
`param.geometry.drive.empty` does not apply: with no drive faces the area is the stock outline.

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `stepover` | Length | (0, D] | preset `stepover` | yes | tool, material | R |
| `direction` | Enum `zigzag` \| `climb` \| `conventional` | | `zigzag` † | — | — | R |
| `pass_angle_deg` | Angle | any; used mod 180 | `0` | yes | — | R |
| `overlap` | Length | ≥ 0 | `0.6 · tool.diameter` † | yes | tool | R |
| `step_down` | Length | > 0 | preset `step_down` | yes | tool, material | R |
| `final_step_down` | Length | (0, `step_down`] | absent | yes | `step_down` | R |

- **Area.** The stock outline (top view of the Setup stock) united with the outlines of the
  drive faces, cut by `boundary` if set. The tool centre runs `overlap` beyond the area on every
  side, so the default 0.6·D leaves the stock fully (0.1·D clear).
- **Levels.** The floor is `bottom + stock_to_leave_axial`. Without `final_step_down`, levels
  are `layout(top, floor, step_down)` in even mode. With it, levels are `layout(top, floor +
  final_step_down, step_down)` and one more level at the floor.
- **Passes.** Lines at `pass_angle_deg`, laid out by `pass_layout` across the grown area, clipped
  exactly. `zigzag` alternates; `climb` and `conventional` cut one way and link back at the
  level (above the cut face) or by retract, as `linking.retract` says.

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.face.overlap.short` | W | `overlap` < tool radius: the tool does not clear the stock edge at pass ends |
| `param.face.final_step_down.range` | E | `final_step_down` > `step_down` |
| `param.face.tool_shape` | W | tool is not `flat` or `bull` (the face will be scalloped) |
| `param.passes.stepover.max` | E | `stepover` > tool diameter |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t1"
  name = "Face top"
  type = "face"
  tool = "user/router-bits#flat-20"

    [setup.toolpath.heights]
    bottom = { ref = "model_top" }

    [setup.toolpath.passes]
    stepover        = 14
    step_down       = 1.5
    final_step_down = 0.3
    overlap         = 12
```

**In-process stock:** updates; does not read.

### 2.2 `adaptive_2d`

Clears closed regions in Z steps with constant tool engagement: no full-width cuts, smooth
direction changes (D09 §2.3). Roughing type.

**Common fields used:** as `pocket_2d` (D15 §4): item keys; all of `tool_params`;
`drive_curves` (closed chains; nested chains become islands), `drive_faces` (planar floor faces),
`excluded_faces`, `boundary`, `rest_from`; all heights; `machining_tolerance`,
`max_segment_length`, `output_arcs`, `stock_to_leave`, `stock_to_leave_axial`; all of `linking`.
**Not used:** `check_faces` (D15 §7, decision 5), `pass_layout` (paths are not laid out).

**Type defaults for common fields:** `heights.top = { ref = "stock_top" }`,
`heights.bottom = { ref = "selection" }`, `lead_in.kind = "none"`, `ramp.kind = "helix"`
(allowed: `helix`, `plunge`; fallback helix → plunge), `stock_to_leave = 0`.

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `optimal_load` | Length | (0, 0.5·D] | `0.1 · tool.diameter` † | yes | tool | R |
| `step_down` | Length | > 0 | preset `step_down` | yes | tool, material | R |
| `min_cutting_radius` | Length | ≥ 0 | `0.1 · tool.diameter` † | yes | tool | R |
| `direction` | Enum `climb` \| `conventional` | | `climb` | — | — | R |

- **Engagement.** `optimal_load` is the radial width of cut in a straight cut. The target
  engagement angle is φ* = acos(1 − load/R) (D09 §2.3). The path keeps φ ≤ φ* everywhere and
  = φ* where it can.
- **Smoothing.** The tool-centre path has curvature ≤ 1/`min_cutting_radius`; 0 turns
  smoothing off.
- **Levels.** As `pocket_2d` (D15 §4.1): even levels plus the exact floor
  `bottom + stock_to_leave_axial`.
- **Entry.** Where the region reaches outside the stock (an open pocket), the first cut enters
  from outside the stock and no ramp is used. Otherwise the ramp of `linking` is used.
- **Final pass.** Each level ends with a contour of the region at R + `stock_to_leave`, in
  `direction`.
- **Rest.** With `rest_from = "previous"` the region at each level is intersected with the
  remaining stock grown by R, as `pocket_2d`.

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.adaptive_2d.optimal_load.range` | E | `optimal_load` > 50 % of tool diameter (D09 §7) |
| `param.adaptive_2d.optimal_load.high` | W | `optimal_load` > 30 % of tool diameter |
| `param.adaptive_2d.min_cutting_radius.large` | W | `min_cutting_radius` > tool radius (narrow parts may be left uncut) |
| `param.passes.step_down.flute` | W | `step_down` > flute length |
| `cam.pocket_2d.region.open` | E | a drive chain is not closed (code shared with `pocket_2d`) |
| `cam.adaptive_2d.region.unreachable` | W | part of a region is narrower than the tool and is left uncut |
| `cam.adaptive_2d.incomplete` | W | material above tolerance is left after the path search stops (CH-16) |
| `cam.linking.ramp_fallback` | W | the helix does not fit and a plunge was used |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t2"
  name = "Adaptive pocket"
  type = "adaptive_2d"
  tool = "user/router-bits#flat-6"

    [setup.toolpath.geometry]
    drive_faces = [{ face = "f-41" }]

    [setup.toolpath.passes]
    optimal_load         = 0.6
    step_down            = 12
    min_cutting_radius   = 0.5
    stock_to_leave       = 0.2
    stock_to_leave_axial = 0.1

    [setup.toolpath.linking]
    ramp = { kind = "helix", angle_deg = 2, radius = 2 }
```

**In-process stock:** updates; reads when `rest_from` is set.

### 2.3 `trace`

Follows curves at their own Z, or at depths set by the heights, with an optional side offset
(D09 §2.5). No region clipping beyond the Setup. Finishing type.

**Common fields used:** item keys; all of `tool_params`; `drive_curves` (open or closed chains;
3-D curves allowed); all heights; `machining_tolerance`, `max_segment_length`, `output_arcs`;
`retract`, `safety_distance`, `transitions`, `lead_in`, `lead_out`, `ramp`, `order`.
**Not used:** `drive_faces`, `check_faces`, `excluded_faces`, `boundary`, `rest_from`,
`pass_layout`, `stock_to_leave`, `stock_to_leave_axial`.

**Type defaults for common fields:** `heights.top = { ref = "stock_top" }`,
`heights.bottom = { ref = "selection" }`, `lead_in.kind = "none"`, `ramp.kind = "plunge"`
(allowed: `plunge`, `profile`).

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `depth_mode` | Enum `heights` \| `curve` | | `heights` † | — | — | R |
| `curve_depth` | Length | ≥ 0 | `0` | yes | `depth_mode` = `curve` | R |
| `side_offset` | Length | any | `0` | yes | — | R |
| `step_down` | Length | > 0 | preset `step_down` | yes | tool, material | R |
| `depth_count` | Count | ≥ 1 | absent | — | replaces `step_down` | R |
| `direction` | Enum `any` \| `selection` \| `reversed` | | `any` † | — | — | R |

- **`heights` mode.** Chains are projected onto XY. Levels go from `top` to `bottom` as
  `contour_2d` depths (D15 §3.2); the last level is exactly `bottom`.
- **`curve` mode.** The tool tip follows each chain's own Z, lowered by `curve_depth`. With
  multiple depths, levels are depth offsets below the curve: `layout(0, curve_depth, step_down)`
  without the 0 level. `top` and `bottom` do not set the cut depth in this mode; `feed` still
  takes its default from `top`.
- **Side offset.** The tool centre is offset in XY from the projected chain: positive is
  outward for closed chains and to the left of the selection direction for open chains. In
  `curve` mode the Z at each point is the chain's Z at the nearest point.
- **Direction.** `any` lets the linker reverse open chains to shorten links. `selection` and
  `reversed` fix the travel direction for every chain.
- No gouge check against the model in v1 (D15 §7, decision 5).

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.trace.depths.both` | E | `step_down` and `depth_count` both set locally |
| `param.trace.curve_depth.unused` | W | `curve_depth` set with `depth_mode = "heights"` (ignored) |
| `cam.trace.offset_empty` | W | a closed chain vanishes under a negative `side_offset` |
| `cam.trace.below_stock` | W | in `curve` mode, the path goes below `stock_bottom` |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t7"
  name = "Score fold lines"
  type = "trace"
  tool = "user/router-bits#v-90"

    [setup.toolpath.geometry]
    drive_curves = [{ body = "b-a2c4…" }]

    [setup.toolpath.heights]
    bottom = { ref = "stock_top", offset = -0.8 }

    [setup.toolpath.passes]
    depth_mode = "heights"
    direction  = "any"
```

**In-process stock:** updates; does not read.

### 2.4 `engrave`

V-carving: a V tool follows the medial axis of closed shapes; the depth follows the local width
(D09 §2.6). Finishing type.

**Common fields used:** item keys; all of `tool_params`; `drive_curves` (closed chains; nested
chains become holes, as `pocket_2d`); `clearance`, `retract`, `feed`, `top`;
`machining_tolerance`, `max_segment_length`, `output_arcs`; `retract`, `safety_distance`,
`transitions`, `ramp`, `order`. **Not used:** `drive_faces`, `check_faces`, `excluded_faces`,
`boundary`, `rest_from`, `heights.bottom` (depth is limited by `max_depth`), `pass_layout`,
`stock_to_leave`, `stock_to_leave_axial`, `lead_in`, `lead_out` (branch ends start at depth 0).

**Type defaults for common fields:** `heights.top = { ref = "stock_top" }`, `ramp.kind =
"plunge"` (only allowed kind). `param.heights.order` checks clearance ≥ retract ≥ feed ≥ top
only.

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `max_depth` | Length | (0, cone height] | cone height of the tool | yes | tool | R |
| `step_down` | Length | > 0 | absent = one pass | yes | — | R |
| `corner_sharpening` | Bool | | `false` † | — | — | R |

- **Depth.** On each medial-axis branch with clearance radius ρ(t), depth(t) =
  min((ρ(t) − Rt) / tan α, `max_depth`), Rt = tip radius (D07 §1). The cone height is
  (R − Rt) / tan α.
- **Flat areas.** Where the full depth would exceed `max_depth`, the tool runs along the region
  boundary offset inward by Rt + `max_depth`·tan α, at `top − max_depth`. The area inside is
  left flat for a separate clearing toolpath. Its boundary is kept on the result as derived
  curves the user can pick as `drive_curves` of a `pocket_2d` (D09 §2.6).
- **Step-down.** With `step_down` set, levels are `layout(top, top − max_depth, step_down)`;
  at each level the path is the full path with depth clipped to that level.
- **Corner sharpening** extends branches into convex corners until the depth reaches `top`, so
  corners come out sharp.

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.engrave.tool_shape` | E | tool is not a V tool (D09 §7) |
| `param.engrave.max_depth.cone` | E | `max_depth` > cone height (the shank side would cut the walls) |
| `param.engrave.max_depth.flute` | W | `max_depth` > flute length |
| `cam.pocket_2d.region.open` | E | a drive chain is not closed |
| `cam.engrave.flat_area` | W | flat areas are left (informs the user to add a clearing toolpath) |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t8"
  name = "V-carve sign"
  type = "engrave"
  tool = "user/router-bits#v-60"

    [setup.toolpath.geometry]
    drive_curves = [{ body = "b-text…" }]

    [setup.toolpath.passes]
    max_depth         = 4
    step_down         = 2
    corner_sharpening = true
```

**In-process stock:** updates (swept width at each level is the V width there); does not read.

### 2.5 `chamfer_2d`

Chamfers edges with a V tool along the edge chain (D09 §2.7). Finishing type.

**Common fields used:** item keys; all of `tool_params`; `drive_curves` (horizontal edges or
curves), `drive_faces` (vertical wall faces: their top edges); `clearance`, `retract`, `feed`,
`top`; `machining_tolerance`, `max_segment_length`, `output_arcs`; all of `linking` except
`order`. **Not used:** `check_faces`, `excluded_faces`, `boundary`, `rest_from`,
`heights.bottom` (depth comes from the width), `pass_layout`, `stock_to_leave`,
`stock_to_leave_axial`, `order`.

**Type defaults for common fields:** `heights.top = { ref = "selection" }` (the edge Z),
`lead_in.kind = "arc"` †, `ramp.kind = "plunge"` (allowed: `plunge`, `profile`).

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `chamfer_width` | Length | > 0 | `1` † | yes | — | R |
| `tip_offset` | Length | ≥ 0 | `0.5` † | yes | — | R |
| `side` | as `contour_2d` without `on` | | `outside` closed, `right` open † | — | chain closure | R |
| `direction` | Enum `climb` \| `conventional` | | `climb` | — | — | R |

- **Geometry** (tool half-angle α, tip radius Rt; the chamfer angle is the tool angle). The tip
  is d = `chamfer_width` / tan α + `tip_offset` below `top`. The tool axis is offset from the
  edge to the side away from the material by o = Rt + `tip_offset` · tan α. The cone then meets
  the top face at `chamfer_width` from the edge.
- `side` and `direction` mean the same as for `contour_2d` (D15 §3.1): the side names where the
  tool is, away from the material.

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.chamfer_2d.tool_shape` | E | tool is not a V tool (D09 §7) |
| `param.chamfer_2d.width.tool` | E | o + `chamfer_width` > R: the chamfer is wider than the cone |
| `param.chamfer_2d.depth.flute` | W | d > flute length |
| `param.chamfer_2d.side.closure` | E | `inside`/`outside` with an open chain, or `left`/`right` with a closed chain |
| `param.geometry.drive_faces.kind` | E | a drive face is not a vertical wall |
| `cam.chamfer_2d.edge.not_horizontal` | E | a drive chain is not at constant Z |
| `cam.chamfer_2d.tip_gouge` | E | the tip at depth d would touch the model below the edge (reduce `tip_offset`) |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t9"
  name = "Chamfer top edge"
  type = "chamfer_2d"
  tool = "user/router-bits#v-90"

    [setup.toolpath.geometry]
    drive_curves = [{ edge = "e-3f2a…" }, { edge = "e-3f2b…" }]

    [setup.toolpath.passes]
    chamfer_width = 1.5
    tip_offset    = 0.3
    side          = "outside"
```

**In-process stock:** updates; does not read.

### 2.6 `bore`

Helical milling of a cylindrical hole or boss (D09 §2.8). Output is HelixSegments
[S 03 §3.4]. Finishing type.

**Common fields used:** item keys; all of `tool_params`; `drive_faces` (cylindrical faces with
axis parallel to the tool axis), `drive_curves` (full circles), `hole_diameter` (§1.5); all
heights (per hole, §1.6); `machining_tolerance`, `output_arcs`, `stock_to_leave`,
`stock_to_leave_axial`; `retract`, `safety_distance`, `lead_in`, `lead_out`. **Not used:**
`check_faces`, `excluded_faces`, `boundary`, `rest_from`, `max_segment_length`, `pass_layout`,
`transitions`, `ramp` (the helix is the entry), `order`.

**Type defaults for common fields:** `heights.top = { ref = "selection" }`,
`heights.bottom = { ref = "selection" }`, `lead_in.kind = "arc"` † (from the hole centre).

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `feature` | Enum `hole` \| `boss` | | `hole` | — | `drive_curves` only | R |
| `pitch` | Length | > 0 | `0.1 · tool.diameter` † | yes | tool | R |
| `direction` | Enum `climb` \| `conventional` | | `climb` | — | — | R |
| `finishing_pass` | Bool | | `false` | — | — | R |
| `finishing_stepover` | Length | (0, D] | `0.1 · tool.diameter` † | yes | `finishing_pass` | R |
| `bottom_circle` | Bool | | `true` | — | — | R |

- **Feature.** A drive face is a hole when its material is outside the cylinder, a boss when
  inside; `feature` applies to circles only.
- **Helix.** Radius rh − R − `stock_to_leave` for a hole, rh + R + `stock_to_leave` for a boss,
  minus (hole) or plus (boss) `finishing_stepover` when `finishing_pass` is on. It runs from
  `top` to `bottom + stock_to_leave_axial` at `pitch` per turn; the last turn ends exactly at
  the bottom (pitch made even: p' = depth / ceil(depth / pitch)).
- `bottom_circle` adds a full circle at the bottom. `finishing_pass` adds a full circle at the
  final radius at the bottom, after the bottom circle.
- The tool enters above the hole centre (hole) or outside the boss, and leaves the same way.

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `cam.bore.hole_small` | E | hole helix radius ≤ 0: hole radius ≤ R + `stock_to_leave` (+ `finishing_stepover`) (D09 §7) |
| `cam.bore.core_left` | W | hole helix radius > R: a core is left in the centre (clear it first) |
| `cam.bore.axis` | E | a cylinder axis is not parallel to the tool axis |
| `cam.bore.not_circle` | E | a drive curve is not a full circle |
| `param.bore.finishing_stepover.max` | E | `finishing_stepover` > tool diameter |
| `param.bore.helix.post` | W | the resolved post has no helix support; helices are linearized |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t10"
  name = "Bore 22 mm"
  type = "bore"
  tool = "user/router-bits#flat-6"

    [setup.toolpath.geometry]
    hole_diameter = { min = 21.9, max = 22.1 }

    [setup.toolpath.passes]
    pitch              = 1.0
    finishing_pass     = true
    finishing_stepover = 0.2
```

**In-process stock:** updates; does not read.
