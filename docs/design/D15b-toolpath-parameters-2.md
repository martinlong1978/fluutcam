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

The §9 decisions were accepted by the product owner on 2026-11-01. A † default is a constant
the spec does not fix; it may be tuned against real cuts.

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
`shape` key [S 02 §2.4]: `flat`, `ball`, `bull`, `v`, `taper`, `drill` (§9, decision 3). "V tool" below means `v` or
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
| `optimal_load` | Length | (0, 0.5·D] | preset `optimal_load`, else `0.1 · tool.diameter` † (§9, decision 1) | yes | tool, material | R |
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

## 3. Hole types

### 3.1 `drill`

Drills holes with canned cycles: drill, spot, dwell, peck, chip-break, tap, ream, bore (D09
§3.1). Holes come from cylindrical faces, with axis and depth exact from the B-rep (DC-01), or
from circles. Hole type; the post emits `cycle_start`, `cycle_point`, `cycle_end`
[S 05 §5.4.1].

**Common fields used:** item keys; all of `tool_params`; `drive_faces` (cylindrical faces; a
coaxial stack is one hole), `drive_curves` (full circles: their centres), `hole_diameter`
(§1.5), `boundary` (keeps holes whose axis is inside); `clearance`, `retract`, `feed`, `top`,
`bottom` (per hole, §1.6); `machining_tolerance` (to match coaxial faces and circles into one
hole). **Not used:** `check_faces`, `excluded_faces`, `rest_from`, `max_segment_length`,
`output_arcs`, `pass_layout`, `stock_to_leave`, `stock_to_leave_axial`; all of `linking`
(moves between holes follow `retract_mode`; they are still checked against model and stock and
raised if blocked [S 04 §4.14]).

**Type defaults for common fields:** `heights.top = { ref = "selection" }`,
`heights.bottom = { ref = "selection" }`. `heights.feed` is the cycle's R plane; `heights.retract`
is its initial plane.

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `cycle` | Enum `drill` \| `spot` \| `dwell` \| `peck` \| `chip_break` \| `tap` \| `ream` \| `bore` | | `drill` | — | — | R |
| `peck_depth` | Length | > 0 | `tool.diameter` † | yes | `cycle` ∈ {`peck`, `chip_break`} | R |
| `chip_break_retract` | Length | > 0 | `0.5` † | yes | `cycle` = `chip_break` | R |
| `dwell_s` | Duration | ≥ 0 | `0.5` † for `dwell`, `spot`, `bore`; else `0` | yes | `cycle` | R |
| `pitch` | Length | > 0 | the tool's `pitch` (§9, decision 4) | yes | `cycle` = `tap` | R |
| `depth_to` | Enum `tip` \| `full_diameter` | | `tip` | — | tool `point_angle_deg` | R |
| `to_cone_tip` | Bool | | `false` | — | holes with a cone bottom | R |
| `retract_mode` | Enum `retract` \| `feed` | | `retract` † | — | — | R |
| `hole_order` | Enum `shortest` \| `selection` | | `shortest` | — | — | R |

- **Holes.** A hole is an axis, a top Z, a bottom Z and a diameter. A coaxial stack of
  cylinders (a counterbore) is one hole whose bottom is the deepest cylinder whose diameter is
  at least the tool diameter. Circles take `top` and `bottom` from the heights.
- **Depth.** `bottom` is the end of the cylinder. With `to_cone_tip` and a cone bottom face,
  it is the cone tip. With `depth_to = "full_diameter"` the depth is extended by
  (D/2) / tan(point angle / 2), so the full diameter reaches `bottom` (D09 §3.1).
- **Cycles.** `peck` retracts to the R plane after each `peck_depth`; `chip_break` retracts by
  `chip_break_retract` only. `dwell_s` waits at the bottom. `tap` feeds at
  `spindle_rpm × pitch`; `feed.cut` is not used for it (the feed is set at post time, class P
  as D15 §2.2). `ream` and `bore` feed out; `bore` dwells and stops the spindle as the post's
  cycle says.
- **Retract mode.** `retract` returns to `heights.retract` between holes (G98); `feed` returns
  to `heights.feed` (G99). A move between holes at the R plane that is blocked by the model is
  raised to the retract height.
- **Order.** `shortest`: nearest neighbour then 2-opt from the point nearest the previous
  position (D08 §8.1). `selection`: the order of the drive selection.
- **Expansion.** If the post has `cycles.enabled = false`, or the cycle is not in
  `cycles.supported`, the post expands it into rapid, feed and dwell moves [S 05 §5.5].

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.drill.tool_shape` | W | tool `shape` is not `drill`; `flat` tools also warn when `centre_cutting = false` |
| `param.drill.pitch.required` | E | `cycle = "tap"` and neither the toolpath nor the tool has a `pitch` |
| `param.drill.pitch.override` | W | the toolpath `pitch` differs from the tool's `pitch` |
| `param.drill.tap.post` | E | `cycle = "tap"` and the resolved post cannot emit a tap cycle (taps cannot be expanded) |
| `param.drill.cycle.expanded` | W | the post does not support the cycle; it is expanded |
| `param.drill.peck_depth.deep` | W | `peck_depth` > flute length |
| `cam.drill.diameter_mismatch` | W | a hole's diameter differs from the tool diameter by more than 5 % (not for `spot`) (D09 §3.1) |
| `cam.drill.axis` | W | a hole is not parallel to the tool axis; it is skipped |
| `cam.drill.not_circle` | E | a drive curve is not a full circle |
| `cam.drill.depth.flute` | W | a hole is deeper than the flute length |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t11"
  name = "Drill 5 mm"
  type = "drill"
  tool = "user/drills#drill-5"

    [setup.toolpath.geometry]
    hole_diameter = { min = 4.9, max = 5.1 }

    [setup.toolpath.heights]
    feed = { ref = "selection", offset = 2 }

    [setup.toolpath.passes]
    cycle        = "peck"
    peck_depth   = 4
    depth_to     = "full_diameter"
    retract_mode = "feed"
```

**In-process stock:** updates (a disc of tool radius at each level down to the hole bottom);
does not read.

## 4. 3-D roughing types

Both types clear the stock in Z levels against the 3-D model (D09 §4.1, §4.2). At each level z:

- machinable(z) = the region minus the section of the bodies at z − `stock_to_leave_axial`,
  offset by R, plus `stock_to_leave` on drive faces (D08 §7.1);
- clear(z) = machinable(z) ∩ the starting stock at z (§1.10), grown by R.

The region is the stock outline, cut by `boundary` if set. The sections are taken through the
drive faces, grown by `stock_to_leave`, and the check faces, with no allowance. Excluded faces
are left out of the sections, so the tool may cut through them (for example a cap face over a
pocket).

**Common fields used (both):** item keys; all of `tool_params`; `drive_faces`, `check_faces`,
`excluded_faces`, `boundary`, `rest_from`; all heights; `machining_tolerance`,
`max_segment_length`, `output_arcs`, `stock_to_leave`, `stock_to_leave_axial`; all of
`linking`. **Not used:** `drive_curves`. `pass_layout` is used by `pocket_3d` raster only.

**Type defaults for common fields (both):** `heights.top = { ref = "stock_top" }`,
`heights.bottom = { ref = "model_bottom" }`, `lead_in.kind = "none"`, `ramp.kind = "helix"`,
`stock_to_leave = 0` (D15 §7, decision 2), `order = "by_level"`.

**Levels (both):** `layout(top, bottom, step_down)` in even mode, plus, with
`flat_detection`, the Z + `stock_to_leave_axial` of every horizontal planar face between top and
bottom [S 04 §4.4]. Flat levels use CH-03 handling (D09 §4.1). Mesh bodies find flats by the
heuristic of [S 04 §4.4].

### 4.1 `adaptive_3d`

Constant-engagement clearing per level, with the `adaptive_2d` algorithm (§2.2). Roughing type.

Allowed ramps: `helix`, `plunge` (fallback helix → plunge). Open regions (reaching outside the
stock) are entered from outside.

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `optimal_load` | Length | (0, 0.5·D] | preset `optimal_load`, else `0.1 · tool.diameter` † (§9, decision 1) | yes | tool, material | R |
| `step_down` | Length | > 0 | preset `step_down` | yes | tool, material | R |
| `fine_step_down` | Length | (0, `step_down`) | absent = off | yes | `step_down` | R |
| `min_cutting_radius` | Length | ≥ 0 | `0.1 · tool.diameter` † | yes | tool | R |
| `direction` | Enum `climb` \| `conventional` | | `climb` | — | — | R |
| `flat_detection` | Bool | | `true` | — | — | R |

- **Fine step-down.** Between main levels, intermediate levels every `fine_step_down` clear only
  the stepped material left on shallow areas: where the remaining stock at that level differs
  from machinable(z) by more than `fine_step_down` (D09 §4.2). They do not cut full-width.
- Engagement, smoothing and the final contour pass at each level are as `adaptive_2d`.

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.adaptive_3d.optimal_load.range` | E | `optimal_load` > 50 % of tool diameter |
| `param.adaptive_3d.optimal_load.high` | W | `optimal_load` > 30 % of tool diameter |
| `param.adaptive_3d.fine_step_down.range` | E | `fine_step_down` ≥ `step_down` |
| `param.adaptive_3d.min_cutting_radius.large` | W | `min_cutting_radius` > tool radius |
| `param.passes.step_down.flute` | W | `step_down` > flute length |
| `cam.adaptive_3d.incomplete` | W | material above tolerance is left after the path search stops (CH-16) |
| `cam.linking.ramp_fallback` | W | the helix does not fit and a plunge was used |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t12"
  name = "Adaptive rough"
  type = "adaptive_3d"
  tool = "user/router-bits#flat-10"

    [setup.toolpath.passes]
    optimal_load         = 1.2
    step_down            = 15
    fine_step_down       = 2
    stock_to_leave       = 0.5
    stock_to_leave_axial = 0.3

    [setup.toolpath.linking]
    ramp = { kind = "helix", angle_deg = 2, radius = 4 }
```

**In-process stock:** updates; reads always (from the Setup stock, or with `rest_from` the
stock after earlier toolpaths, §1.10).

### 4.2 `pocket_3d`

Offset or raster clearing per level, with the `pocket_2d` patterns (D15 §4.1) on clear(z).
Roughing type.

Allowed ramps: all four (fallback helix → zigzag → plunge, D15 §2.6).

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `pattern` | Enum `offset` \| `raster` | | `offset` † | — | — | R |
| `stepover` | Length | (0, D] | preset `stepover` | yes | tool, material | R |
| `step_down` | Length | > 0 | preset `step_down` | yes | tool, material | R |
| `direction` | Enum `climb` \| `conventional` | | `climb` | — | — | R |
| `offset_order` | Enum `inside_out` \| `outside_in` | | `inside_out` | — | `pattern` = `offset` | R |
| `pass_angle_deg` | Angle | any; used mod 180 | `0` | yes | `pattern` = `raster` | R |
| `flat_detection` | Bool | | `true` | — | — | R |

Pattern rules are those of `pocket_2d` (D15 §4.1), applied to clear(z) at each level. Entries
use a ramp where clear(z) cannot be reached from outside the stock (D09 §4.1).

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.passes.stepover.max` | E | `stepover` > tool diameter |
| `param.passes.stepover.flat_width` | W | `stepover` > D − 2·corner radius |
| `param.passes.step_down.flute` | W | `step_down` > flute length |
| `cam.pocket_3d.region.unreachable` | W | part of clear(z) is narrower than the tool and is left uncut |
| `cam.linking.ramp_fallback` | W | the ramp does not fit and a fallback was used |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t13"
  name = "Pocket rough"
  type = "pocket_3d"
  tool = "user/router-bits#flat-6"

    [setup.toolpath.geometry]
    rest_from = "previous"

    [setup.toolpath.passes]
    pattern        = "offset"
    stepover       = 2.4
    step_down      = 3
    stock_to_leave = 0.3
```

**In-process stock:** updates; reads always (§1.10).

## 5. 3-D finishing types

All five are finishing types that follow the surface. They share these common-field rules,
the same as `parallel` (D15 §5):

**Common fields used:** item keys; all of `tool_params`; `drive_faces`, `check_faces`,
`excluded_faces`, `boundary`; all heights; `machining_tolerance`, `max_segment_length`,
`output_arcs`, `stock_to_leave` (uniform normal offset); `retract`, `safety_distance`,
`transitions`, `lead_in`, `lead_out`. **Not used:** `drive_curves`, `rest_from`, `ramp`.
`stock_to_leave_axial` is used by `horizontal` only; `pass_layout` and `order` as each type
says.

**Type defaults for common fields:** `heights.top = { ref = "model_top" }`,
`heights.bottom = { ref = "model_bottom" }`, `lead_in.kind = "none"` †. CL points below
`bottom` are raised to `bottom`; portions above `top` are not cut and are linked over.

**In-process stock (all five):** do not update (CH-14); do not read.

### 5.1 `contour`

Constant-Z (waterline) passes for steep walls (D09 §4.4, [S 04 §4.5]).

Also uses `linking.order` (`by_level` | `by_area`). **Not used:** `pass_layout` (levels are
even).

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `step_down` | Length | > 0 | `0.1 · tool.diameter` † | yes | tool | R |
| `scallop_height` | Length | (0, r) | absent | yes | tool corner radius | R |
| `min_slope_deg` | Angle | [0, 90] | `0` | yes | `max_slope_deg` | R |
| `max_slope_deg` | Angle | [0, 90] | `90` | yes | `min_slope_deg` | R |
| `direction` | Enum `climb` \| `conventional` | | `climb` | — | — | R |

- **Levels.** `layout(top, bottom, step_down)` in even mode, plus the Z of every horizontal
  planar face [S 04 §4.5]. `step_down` is the largest gap between levels.
- **Scallop-adaptive levels.** With `scallop_height` set, levels are added between two levels
  wherever a wall between them has slope β (from horizontal) with gap Δz > sin β · √(8·r·h),
  r = tool corner radius, until no gap exceeds it. Added levels cut only on the shallow-enough
  parts of the walls that needed them.
- **Slope.** As `parallel` (D15 §5.1): only contacts with slope in [`min_slope_deg`,
  `max_slope_deg`] are cut; cut-off points are exact.
- **Direction.** The material is on the right of the tool for climb (spindle M3); closed loops
  are oriented accordingly.

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.contour.scallop.radius` | E | `scallop_height` set and the tool has no corner radius |
| `param.contour.scallop.range` | E | `scallop_height` ≥ corner radius |
| `param.contour.slope.order` | E | `min_slope_deg` ≥ `max_slope_deg` |
| `param.contour.slope.range` | E | a slope limit outside [0, 90] |
| `param.passes.step_down.flute` | W | `step_down` > flute length |
| `cam.contour.loop_open` | W | a contour ends at a limit inside the region (linked over) |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t14"
  name = "Waterline walls"
  type = "contour"
  tool = "user/router-bits#ball-6"

    [setup.toolpath.passes]
    step_down      = 0.5
    scallop_height = 0.01
    min_slope_deg  = 30
```

### 5.2 `horizontal`

Finishes flat faces only. Flats are planar faces whose normal is within the angle tolerance of
+Z, found exactly from the B-rep (D09 §4.5).

Also uses `stock_to_leave_axial` (on flats), `pass_layout` (raster). `stock_to_leave` is the
gap kept from walls. **Not used:** `order`.

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `pattern` | Enum `offset` \| `raster` | | `offset` | — | — | R |
| `stepover` | Length | (0, D] | preset `stepover` | yes | tool, material | R |
| `pass_angle_deg` | Angle | any; used mod 180 | `0` | yes | `pattern` = `raster` | R |
| `direction` | Enum `climb` \| `conventional` | | `climb` | — | — | R |

- **Flats** are the planar horizontal faces among the drive faces with Z in [`bottom`, `top`].
  Flats at equal Z are cleared together.
- **Area per flat:** the face region offset by −R (the tool stays on the flat), minus the
  walls: the section just above the flat offset by R + `stock_to_leave`. Cut at Z = flat +
  `stock_to_leave_axial`.
- **Patterns** as `pocket_2d` (D15 §4.1), with offset order fixed to `outside_in`: the boundary
  ring goes first, so walls are finished at once.
- The tool descends onto each flat at feed rate from `feed` height; no ramp (the material above
  was roughed).

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.horizontal.tool_shape` | W | tool is not `flat` or `bull` |
| `param.passes.stepover.max` | E | `stepover` > tool diameter |
| `param.passes.stepover.flat_width` | W | `stepover` > D − 2·corner radius |
| `cam.horizontal.no_flats` | W | no flat faces in the selection between bottom and top |
| `param.horizontal.drive_faces.mesh` | W | mesh drive faces: flats are found by the mesh heuristic [S 04 §4.4] |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t15"
  name = "Finish flats"
  type = "horizontal"
  tool = "user/router-bits#flat-6"

    [setup.toolpath.passes]
    pattern              = "offset"
    stepover             = 3
    stock_to_leave       = 0.05
    stock_to_leave_axial = 0
```

### 5.3 `spiral`

An Archimedean spiral from a centre point, clipped to the region, following the surface in Z
(D09 §4.6).

**Not used** besides the shared list: `pass_layout` (the pitch is made even), `order`.

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `centre` | Point | | absent = centroid of the region | — | — | R |
| `stepover` | Length | (0, D] | preset `stepover` | yes | tool, material; not with `scallop_height` | R |
| `scallop_height` | Length | (0, r) | absent | yes | tool shape | R |
| `direction` | Enum `inside_out` \| `outside_in` | | `inside_out` † | — | — | R |
| `rotation` | Enum `ccw` \| `cw` | | `ccw` † (seen from +Z of W) | — | — | R |
| `machine_corners` | Bool | | `false` † | — | — | R |

- **Region:** the silhouette of the drive faces, cut by `boundary`.
- **Spiral:** r = s'·φ/2π from 0 to r_max, the largest distance from the centre to the region
  boundary; s' = r_max / ceil(r_max / s), so the last turn reaches r_max exactly. The spiral is
  a G1 biarc chain within tol/4 (D09 §4.6), clipped exactly by the region.
- `centre` is a model-frame point; only its XY in W is used.
- `machine_corners` adds a contour pass along the region boundary after the spiral.

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.spiral.stepover_and_scallop` | E | `stepover` and `scallop_height` both set locally |
| `param.spiral.scallop.radius` | E | `scallop_height` set and the tool has no corner radius |
| `param.spiral.scallop.range` | E | `scallop_height` ≥ corner radius |
| `param.passes.stepover.max` | E | given or computed stepover > tool diameter |
| `cam.spiral.centre_outside` | W | `centre` is outside the region (the inner turns are linked over) |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t16"
  name = "Spiral dome"
  type = "spiral"
  tool = "user/router-bits#ball-6"

    [setup.toolpath.passes]
    centre         = [0, 0, 0]
    scallop_height = 0.005
    direction      = "outside_in"
```

### 5.4 `scallop`

Passes offset inward from the region boundary over the surface at constant 3-D stepover
(D09 §4.7, [S 04 §4.3.4]).

**Not used** besides the shared list: `pass_layout`, `order`.

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `stepover` | Length | (0, D] | preset `stepover` | yes | tool, material; not with `scallop_height` | R |
| `scallop_height` | Length | (0, r) | absent | yes | tool shape | R |
| `direction` | Enum `outside_in` \| `inside_out` | | `outside_in` | — | — | R |
| `cut_direction` | Enum `climb` \| `conventional` | | `climb` | — | — | R |
| `continuous` | Bool | | `false` | — | — | R |
| `min_slope_deg` | Angle | [0, 90] | `0` | yes | `max_slope_deg` | R |
| `max_slope_deg` | Angle | [0, 90] | `90` | yes | `min_slope_deg` | R |

- **Stepover** is measured along the surface. Pass 0 is the region boundary dropped onto the
  surface; pass k+1 is pass k offset in XY by s·cos(slope), conservatively by the smallest such
  value over the pass, with extra passes where the 3-D gap still exceeds s (CH-18).
- `direction` sets the order of the loops; `inside_out` cuts the same loops in reverse order.
- `cut_direction` sets the travel direction along each loop (climb: the uncut side on the
  right, spindle M3).
- `continuous` joins consecutive loops into one spiral by blending over one step, with no
  retracts.
- Slope limits as `parallel` (D15 §5.1).

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.scallop.stepover_and_scallop` | E | `stepover` and `scallop_height` both set locally |
| `param.scallop.scallop.radius` | E | `scallop_height` set and the tool has no corner radius |
| `param.scallop.scallop.range` | E | `scallop_height` ≥ corner radius |
| `param.scallop.slope.order` | E | `min_slope_deg` ≥ `max_slope_deg` |
| `param.scallop.slope.range` | E | a slope limit outside [0, 90] |
| `param.passes.stepover.max` | E | given or computed stepover > tool diameter |
| `cam.scallop.extra_passes` | W | passes were inserted to keep the 3-D stepover (informational, CH-18) |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t17"
  name = "Scallop finish"
  type = "scallop"
  tool = "user/router-bits#ball-3"

    [setup.toolpath.passes]
    stepover   = 0.3
    continuous = true
```

### 5.5 `steep_and_shallow`

Contour passes on steep areas and parallel or scallop passes on shallow areas, split by a
slope threshold (D09 §4.8).

Also uses `pass_layout` (shallow parallel passes) and `order` (steep levels). The steep and
shallow settings are inline tables; each key is optional on its own.

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `threshold_deg` | Angle | (0, 90) | `45` † | yes | — | R |
| `overlap` | Length | ≥ 0 | `0.2 · tool.diameter` † | yes | tool | R |
| `region_order` | Enum `steep_first` \| `shallow_first` | | `steep_first` | — | — | R |
| `steep.step_down` | Length | > 0 | `0.1 · tool.diameter` † | yes | tool | R |
| `steep.scallop_height` | Length | (0, r) | absent | yes | tool shape | R |
| `steep.direction` | Enum `climb` \| `conventional` | | `climb` | — | — | R |
| `shallow.pattern` | Enum `parallel` \| `scallop` | | `parallel` † | — | — | R |
| `shallow.stepover` | Length | (0, D] | preset `stepover` | yes | tool, material; not with `shallow.scallop_height` | R |
| `shallow.scallop_height` | Length | (0, r) | absent | yes | tool shape | R |
| `shallow.pass_angle_deg` | Angle | any; used mod 180 | `0` | yes | `shallow.pattern` = `parallel` | R |
| `shallow.direction` | Enum `zigzag` \| `climb` \| `conventional` | | `zigzag` (parallel), `climb` (scallop) | — | `shallow.pattern` | R |

- **Split.** A contact is steep when its slope (D15 §5.1) is ≥ `threshold_deg`. Steep parts are
  cut as `contour` (§5.1) with slope limits [`threshold_deg`, 90]. The shallow region is the
  XY projection of the shallow parts, grown by `overlap`, and cut as `parallel` (D15 §5) or
  `scallop` (§5.4) inside it.
- `steep.*` and `shallow.*` keys mean what the matching keys of those types mean.
- `region_order` sets which part runs first.

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.steep_and_shallow.threshold.range` | E | `threshold_deg` outside (0, 90) |
| `param.steep_and_shallow.shallow.stepover_and_scallop` | E | `shallow.stepover` and `shallow.scallop_height` both set locally |
| `param.steep_and_shallow.scallop.radius` | E | a scallop height set and the tool has no corner radius |
| `param.steep_and_shallow.scallop.range` | E | a scallop height ≥ corner radius |
| `param.steep_and_shallow.shallow.direction` | E | `shallow.direction = "zigzag"` with `shallow.pattern = "scallop"` |
| `param.passes.stepover.max` | E | given or computed shallow stepover > tool diameter |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t18"
  name = "Steep and shallow"
  type = "steep_and_shallow"
  tool = "user/router-bits#ball-6"

    [setup.toolpath.passes]
    threshold_deg = 40
    overlap       = 1
    steep         = { step_down = 0.4 }
    shallow       = { pattern = "parallel", scallop_height = 0.01, pass_angle_deg = 45 }
```

## 6. Rotary types

Run only in `rotary` Setups (§1.2). The work frame has the rotary axis along X; the tool
approaches radially; the drive domain is the unrolled cylinder (x, θ) (D09 §5). Heights are radii
(§1.3); the region is [`x_start`, `x_end`] × A (§1.4).

**Common fields used (all four):** item keys; all of `tool_params`; `drive_faces` (empty = all
faces of the Setup's bodies), `check_faces`, `excluded_faces`, `x_start`, `x_end`; all heights
(as radii); `machining_tolerance`, `max_segment_length`, `output_arcs`, `stock_to_leave`
(normal offset, §1.7); `retract`, `safety_distance`, `transitions`, `lead_in`, `lead_out`.
**Not used:** `drive_curves`, `boundary` (no boundary curves on the cylinder in v1),
`stock_to_leave_axial`, `order`. `rest_from`, `pass_layout` and `ramp` as each type says.

**Type defaults for common fields:** `heights.top = { ref = "model_top" }`,
`heights.bottom = { ref = "model_bottom" }`, `lead_in.kind = "none"` †. CL radii below `bottom`
are raised to `bottom`.

**Feed** in rotary moves is resolved by the post's `feed.rotary_method` from the CL radius on
each RotarySegment [S 04 §4.7.5]; the feeds in `tool_params` are surface feeds.

**Scallop height in rotary types** gives the stepover from the worst-case convex curvature across
the passes, as OQ-05; angular steps are converted at the largest model radius in the range, so
the scallop is never exceeded at smaller radii.

**Validation (all four):**

| Code | Sev | Rule |
|---|---|---|
| `param.type.setup_kind` | E | the Setup is not `rotary` |
| `param.rotary.machine_axis` | E | the Setup's machine has no rotary axis [S 02 §2.3] |
| `param.rotary.limits` | E | the A motion needs more rotation than the axis allows (not `continuous` and `limits_deg` too narrow) |

### 6.1 `rotary_rough`

Radial layers at constant radius, each cleared from the section of the bodies by a cylinder
(D09 §5.1). Roughing type.

Also uses `rest_from` (cylindrical in-process stock), `pass_layout` (X positions), `ramp`.
**Type defaults:** `heights.top = { ref = "stock_top" }`, `ramp.kind = "plunge"` (allowed:
`plunge`, `helix`: a radial descent while A turns, at `ramp.angle_deg`), `stock_to_leave = 0`.

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `radial_step_down` | Length | > 0 | preset `step_down` | yes | tool, material | R |
| `stepover_x` | Length | (0, D] | preset `stepover` | yes | tool, material | R |
| `direction` | Enum `zigzag` \| `climb` \| `conventional` | | `zigzag` † | — | — | R |

Keys as in the [S 02 §2.1] example.

- **Radii.** `layout(top, bottom, radial_step_down)` in even mode, plus the exact radius of
  every cylindrical face coaxial with the rotary axis (as flat levels in Z, [S 04 §4.7.3]).
- **Per radius r:** the section of the bodies by the cylinder r, unrolled to (x, r·θ), offset by
  R + `stock_to_leave` (CH-19); the rest of the band [`x_start`, `x_end`] × full A that still
  holds stock is cleared with full A sweeps at X positions `stepover_x` apart, laid out by
  `pass_layout` (§9, decision 8).
- **Direction.** `zigzag` alternates the A sense between sweeps; `climb` and `conventional`
  keep one sense (the uncut side on the right of travel for climb, spindle M3).
- **Starting stock** as §1.10: the stock cylinder, the carried stock, or with `rest_from` the
  stock after earlier toolpaths.

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.passes.stepover.max` | E | `stepover_x` > tool diameter |
| `param.passes.step_down.flute` | W | `radial_step_down` > flute length |
| `cam.rotary_rough.unrolled_offset` | W | R > 0.2 × the layer radius †: the unrolled offset is approximate (CH-19) |

#### Example

As [S 02 §2.1]:

```toml
  [[setup.toolpath]]
  id   = "t1"
  name = "Rotary rough"
  type = "rotary_rough"
  tool = "user/router-bits#flat-6"

    [setup.toolpath.heights]
    clearance = { ref = "setup_clearance" }
    top       = { ref = "stock_top" }

    [setup.toolpath.passes]
    machining_tolerance = 0.01
    radial_step_down    = 2.0
    stepover_x          = 2.4
    pass_layout         = "even"
    stock_to_leave      = 0.3

    [setup.toolpath.linking]
    retract = "minimum"
    ramp    = { kind = "helix", angle_deg = 3 }
```

**In-process stock:** updates (cylindrical layers); reads always (§1.10).

### 6.2 `rotary_along_x`

Passes along X at fixed A, with A indexed between passes (D09 §5.2). Finishing type.

Also uses `pass_layout` (A positions over a partial range). **Not used:** `rest_from`, `ramp`.

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `stepover_deg` | Angle | (0, 90] | preset `stepover` / r_max, in degrees | yes | tool, material, heights | R |
| `scallop_height` | Length | (0, r) | absent | yes | tool shape | R |
| `a_start_deg` | Angle | [0, 360) | `0` | yes | — | R |
| `a_end_deg` | Angle | (`a_start_deg`, `a_start_deg` + 360] | `a_start_deg + 360` | yes | `a_start_deg` | R |
| `direction` | Enum `zigzag` \| `climb` \| `conventional` | | `zigzag` † | — | — | R |

- r_max is the resolved `top` radius. The surface step at radius r is r·Δθ, so it is largest at
  r_max.
- **A positions.** A full turn (`a_end_deg − a_start_deg` = 360) is split into
  n = ceil(360 / `stepover_deg`) equal steps, with no pass repeated at 360. A partial range is
  laid out by `pass_layout`, first and last pass on the range ends.
- Each pass: a drive line along X between `x_start` and `x_end`, sampled with the radial drop;
  output is linear moves with A fixed per pass.
- `direction`: `zigzag` alternates X direction; `climb`/`conventional` cut one way.

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.rotary_along_x.stepover_and_scallop` | E | `stepover_deg` and `scallop_height` both set locally |
| `param.rotary_along_x.scallop.radius` | E | `scallop_height` set and the tool has no corner radius |
| `param.rotary_along_x.scallop.range` | E | `scallop_height` ≥ corner radius |
| `param.rotary_along_x.a_range` | E | `a_end_deg` ≤ `a_start_deg` or the range exceeds 360° |
| `param.passes.stepover.max` | E | r_max · Δθ > tool diameter |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t19"
  name = "Along X finish"
  type = "rotary_along_x"
  tool = "user/router-bits#ball-6"

    [setup.toolpath.geometry]
    x_start = { ref = "model_left", offset = -3 }
    x_end   = { ref = "model_right", offset = 3 }

    [setup.toolpath.passes]
    scallop_height = 0.01
    direction      = "zigzag"
```

**In-process stock:** does not update; does not read.

### 6.3 `rotary_around_a`

Full A sweeps at fixed X, with X stepped between sweeps. On faces coaxial with the axis a sweep
is one RotarySegment (D09 §5.3, [S 04 §4.7.3]). Finishing type.

Also uses `pass_layout` (X positions). **Not used:** `rest_from`, `ramp`.

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `stepover_x` | Length | (0, D] | preset `stepover` | yes | tool, material; not with `scallop_height` | R |
| `scallop_height` | Length | (0, r) | absent | yes | tool shape | R |
| `direction` | Enum `zigzag` \| `climb` \| `conventional` | | `climb` † | — | machine axis `continuous` | R |
| `ring_link` | Enum `step` \| `helical` | | `step` † | — | — | R |

- **X positions** from `x_start` to `x_end`, laid out by `pass_layout` with `stepover_x`.
- **Sweep.** θ over a full turn with the radial drop; a sweep that stays on one coaxial face is
  one RotarySegment; otherwise CL points (θ, r) are fitted to rotary segments within tolerance.
- **Direction.** `climb`/`conventional` turn A the same way on every ring and need a continuous
  axis; `zigzag` reverses A on each ring, so no unwinding is needed.
- **Ring link.** `step`: move along X at the current A, staying down. `helical`: the X step is
  spread over the last part of the ring (one tool radius of arc †), so the path has no corner.

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.rotary_around_a.stepover_and_scallop` | E | `stepover_x` and `scallop_height` both set locally |
| `param.rotary_around_a.scallop.radius` | E | `scallop_height` set and the tool has no corner radius |
| `param.rotary_around_a.scallop.range` | E | `scallop_height` ≥ corner radius |
| `param.rotary_around_a.direction.unwind` | E | `climb`/`conventional` and the A axis is not continuous |
| `param.passes.stepover.max` | E | given or computed `stepover_x` > tool diameter |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t2"
  name = "Rotary finish"
  type = "rotary_around_a"
  tool = "user/router-bits#ball-6"

    [setup.toolpath.passes]
    stepover_x = 0.4
    direction  = "climb"
    ring_link  = "helical"
```

**In-process stock:** does not update; does not read.

### 6.4 `rotary_spiral`

Continuous A with X advancing a set distance per turn: one helical pass, no retracts (D09 §5.4).
On a coaxial cylinder the whole spiral is one RotarySegment. Finishing type.

**Not used:** `rest_from`, `pass_layout` (the advance is made even), `ramp`.

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `advance` | Length | (0, D] | preset `stepover` | yes | tool, material; not with `scallop_height` | R |
| `scallop_height` | Length | (0, r) | absent | yes | tool shape | R |
| `direction` | Enum `climb` \| `conventional` | | `climb` | — | — | R |
| `start` | Enum `x_start` \| `x_end` | | `x_start` | — | — | R |

- **Helix** x = x0 ± p'·θ/2π, from `start` to the other end. p' = L / ceil(L / `advance`),
  L = `x_end − x_start`, so the spiral ends exactly at the far end of the range.
- `direction` sets the A sense (climb: the uncut side on the right of travel, spindle M3).
- Needs a continuous A axis, or limits that allow L / p' turns (`param.rotary.limits`).

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.rotary_spiral.stepover_and_scallop` | E | `advance` and `scallop_height` both set locally |
| `param.rotary_spiral.scallop.radius` | E | `scallop_height` set and the tool has no corner radius |
| `param.rotary_spiral.scallop.range` | E | `scallop_height` ≥ corner radius |
| `param.passes.stepover.max` | E | given or computed `advance` > tool diameter |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t20"
  name = "Spiral finish"
  type = "rotary_spiral"
  tool = "user/router-bits#ball-6"

    [setup.toolpath.passes]
    advance   = 0.5
    direction = "climb"
```

**In-process stock:** does not update; does not read.

## 7. `manual_nc`

Inserts one block in the program at its place in the Setup: stop, optional stop, comment, dwell,
or pass-through text (D09 §6.1). Utility type; no geometry, no motion. The result is one pass
with a `Marker` segment (D08 §2); the post emits the `manual_nc` block [S 05 §5.4.1].

**Common fields used:** `id`, `name`, `type`, `enabled`, `template`. **Not used:** `tool`
(`param.tool.missing` and `param.tool.diameter` do not apply; the active tool is unchanged and
no tool change is emitted); all of `tool_params`, `geometry`, `heights`, `linking`; all common
Passes fields (`machining_tolerance` included: there is no geometry).

#### Fields (`passes`)

| Key | Type | Range | Default | Expr | Depends on | Class |
|---|---|---|---|---|---|---|
| `kind` | Enum `stop` \| `optional_stop` \| `comment` \| `dwell` \| `pass_through` | | `optional_stop` † | — | — | R |
| `text` | string | non-empty for `comment`, `pass_through` | absent | — | `kind` | R |
| `dwell_s` | Duration | > 0 | `1` † | yes | `kind` = `dwell` | R |

- `stop` and `optional_stop` emit the post's stop blocks (`M0`, `M1` in common dialects).
  `text` on them is emitted as a comment before the stop.
- `comment` emits `text` through the post's comment format (the post escapes or strips
  characters its dialect forbids).
- `pass_through` emits `text` verbatim, one output line per line of `text`. FluutCAM does not
  check it; the user is responsible for it.
- `dwell` emits a dwell of `dwell_s`.
- All fields are R because the Marker holds them; recalculation is trivial and nothing
  downstream depends on it.

#### Validation

| Code | Sev | Rule |
|---|---|---|
| `param.manual_nc.text.required` | E | `kind` is `comment` or `pass_through` and `text` is absent or empty |
| `param.manual_nc.text.unused` | W | `text` set with `kind = "dwell"` (ignored) |
| `param.manual_nc.post` | E | the resolved post has no `manual_nc` block for the kind |
| `param.manual_nc.dwell.range` | E | `dwell_s` ≤ 0 |

#### Example

```toml
  [[setup.toolpath]]
  id   = "t21"
  name = "Check clamps"
  type = "manual_nc"

    [setup.toolpath.passes]
    kind = "stop"
    text = "Move the clamps clear of the outline"
```

**In-process stock:** does not update; does not read.

## 8. Implementation notes

- D15 §6 applies: field tables as data in `fluut-model::params`, one function and one test per
  validation code, `visible_if` from the Depends on column, range codes
  `param.<type>.<key>.range` where no code is listed.
- New metadata: `FieldKind::Duration` (§1.1); the height resolver takes the Setup kind and
  resolves radii in `rotary` Setups (§1.3) and per hole for `drill` and `bore` (§1.6).
- Inline tables in `passes` (`steep`, `shallow`, `hole_diameter` in `geometry`) follow the
  `tabs` and `feed` rule: each sub-key is optional on its own.
- Tests as D15 §6: every example parses, resolves and re-serialises byte-identically; every
  code has a passing and a failing case; P fields leave the result hash unchanged and R fields
  change it.

## 9. Decisions

The spec did not settle these. The product owner accepted the answers below on 2026-11-01;
decisions 1, 3 and 4 differ from the first draft and changed spec 02 §2.4 the same day.

| # | Question | Decision |
|---|---|---|
| 1 | Default `optimal_load` (`adaptive_2d`, `adaptive_3d`): a constant, or a new preset key? | Preset key `optimal_load` in `[[tool.preset]]` [S 02 §2.4]; `0.1 · tool.diameter` † when the preset has none |
| 2 | Rotary height references (§1.3): `stock_bottom` and `work_zero` both resolve to the axis | As §1.3 |
| 3 | The tool format has no drill shape and no point angle (`drill`, `depth_to`) | `shape = "drill"` and `point_angle_deg` (default `118`) added to [S 02 §2.4] |
| 4 | Tap `pitch`: on the toolpath or on the tool? | On the tool (`pitch` in [S 02 §2.4]); the toolpath may override it, with a warning |
| 5 | Starting stock of 3-D and rotary roughing with `rest_from = "off"` | The Setup stock, including stock carried from a parent Setup (§1.10) |
| 6 | Stock to leave on check faces in 3-D roughing | None (§4); a separate check-face allowance later |
| 7 | `face` with drive faces inside the stock outline (a boss top in a pocket) | The area always includes the stock outline; no ramped entries in v1 |
| 8 | `rotary_rough` pattern: D09 §5.1 allows passes along X or around A | Around A only in v1 (matches the [S 02 §2.1] key `stepover_x`); along X later |
| 9 | `engrave` flat-area boundary: how does a later toolpath select it? | A command creates a WireBody from it; no live link to the `engrave` result in v1 |
| 10 | Gouge checks for `trace`, `engrave`, `chamfer_2d` | None in v1, as D15 §7 decision 5, except `cam.chamfer_2d.tip_gouge` |
| 11 | `trace` default `depth_mode` | `heights` † (flat curves from DXF are the common case) |
| 12 | Finishing step-downs: `contour` and `steep.step_down` default | `0.1 · tool.diameter` † (the preset `step_down` is a roughing value) |
| 13 | `bore` default `pitch` | `0.1 · tool.diameter` † |
| 14 | `horizontal` offset order | Fixed `outside_in`; no `offset_order` field |
| 15 | `steep_and_shallow` keys: inline tables or flat keys (`steep_step_down`)? | Inline tables `steep` and `shallow` |
| 16 | `manual_nc` has no tool. Does the next toolpath see the earlier tool as current? | Yes: `manual_nc` never changes or emits a tool |
| 17 | Rotary scallop height: the angular step is taken at the largest radius | Yes (conservative, §6) |
| 18 | `spiral` defaults: `direction`, `rotation`, `machine_corners` | `inside_out` †, `ccw` †, `false` † |
| 19 | `drill` default `retract_mode` | `retract` (G98) †: safer over clamps |
| 20 | Rotary direction defaults (`rotary_rough`, `rotary_along_x`: `zigzag`; `rotary_around_a`: `climb`) | As the tables †; `rotary_around_a` needs a continuous axis for `climb` |
