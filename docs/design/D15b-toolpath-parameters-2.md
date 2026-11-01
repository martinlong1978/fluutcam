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
