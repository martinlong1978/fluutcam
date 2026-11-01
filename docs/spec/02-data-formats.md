# 02 — Data Formats


## 2.0 Format policy

FluutCAM uses its own TOML formats. Other products' formats are supported only as import
sources (§2.5).

| Data | Native format | Extension (provisional) | Authored by |
|---|---|---|---|
| Project | TOML | `.fluut` | application (diffable, hand-readable) |
| Post-processor definition | TOML | `.post.toml` | humans |
| Machine definition | TOML | `.machine.toml` | humans |
| Tool library | TOML | `.tools.toml` | humans and application |
| Computed toolpath cache | binary sidecar (03 §3.4) | `<project>.fluut-cache/` | application |
| Toolpath / Setup template | TOML | `.template.toml` | humans and application |
| G-code output | text | per post | application |

Rationale: TOML is readable, supports comments, diffs well in version control, and has mature
Rust support (`toml` + `serde`). Every native file starts with a `format` key giving the schema
name and version, e.g. `format = "fluut.post/1"`. Readers MUST reject unknown major versions
and MUST report unknown keys as warnings (typo protection), not ignore them silently.

**[DECIDED — OQ-12]** Extensions as listed: `.fluut` for projects, `*.toml` double extensions for
definitions so editors highlight them as TOML.

## 2.1 Project file

The project holds the Model (imported geometry) and an ordered list of Setups, each with an
ordered list of Toolpaths (03 §3.1, §3.9). Geometry placement and transforms are Setup fields.

```toml
format = "fluut.project/1"
name   = "Shaft"
units  = "mm"                                  # display/entry units; internal is always mm

[[geometry]]                                   # Model: one per imported file  
id   = "g1"
path = "parts/shaft.step"                      # relative to project file
kind = "step"                                  # step | stl | dxf | svg | bitmap
hash = "sha256:…"                              # change detection (§2.6.3)
unit = "mm"                                    # REQUIRED for stl, dxf (no unit in file)
                                               # no placement: positioning is per Setup
  [[geometry.body]]
  id   = "b-7f3a…"                             # stable body id (03 §3.5)
  name = "Shaft"

# ── Setup 1: rotary ──────────────────────────────────────────────
[[setup]]
id          = "s1"
name        = "Turn profile"
kind        = "rotary"                         # three_axis | rotary | indexed   (03 §3.9.2)
machine     = "user/my-router@2"               # latest 2.x  
material    = "MDF"                            # selects tool cutting-data presets (2.4)
work_offset = "G54"
geometry    = { bodies = ["b-7f3a…"], scale = [1, 1, 1], mirror = [false, false, false] }
orientation = { rotary_axis = { face = "f-12" }, a_zero_toward = "+Z" }   # 03 §3.9.3
zero        = { x = "stock_left" }             # Y, Z fixed on rotary axis for rotary/indexed
stock       = { kind = "cylinder", radius = 26, length = 120 }
clearance   = { kind = "cylinder", radius = 35 }

  [[setup.toolpath]]
  id   = "t1"
  name = "Rotary rough"
  type = "rotary_rough"                           # fixed at creation (04 §4.13)  
  tool = "user/router-bits#flat-6"                # tool library id + tool id

    [setup.toolpath.tool_params]                  # group: Tool   (03 §3.10.2)
    spindle_rpm = 18000
    feed        = { cut = 1500, plunge = 750, lead_in = 1500 }

    [setup.toolpath.heights]                      # group: Heights (radii in rotary Setups)
    clearance = { ref = "setup_clearance" }
    top       = { ref = "stock_top" }

    [setup.toolpath.passes]                       # group: Passes, rotary_rough's own fields
    machining_tolerance = 0.01                    # mm  
    radial_step_down    = 2.0
    stepover_x          = 2.4
    pass_layout         = "even"                  #
    stock_to_leave      = 0.3

    [setup.toolpath.linking]                      # group: Linking
    retract = "minimum"
    ramp    = { kind = "helix", angle_deg = 3 }

  [[setup.toolpath]]
  id   = "t2"
  name = "Rotary finish"
  type = "rotary_around_a"                        # separate toolpath, not a roughing option
  # …

# ── Setup 2: indexed at 0°, inherits everything else from s1 ─────
[[setup]]
id          = "s2"
name        = "Flat at 0°"
based_on    = "s1"                             # unset fields inherited   (03 §3.9.4)
kind        = "indexed"
index_angle = { mode = "absolute", value = 0 }
stock       = { kind = "from_previous" }

  [[setup.toolpath]]
  id   = "t3"
  name = "Pocket flat"
  type = "pocket_2d"
  # …

# ── Setup 3: indexed, 90° on from s2 ─────────────────────────────
[[setup]]
id          = "s3"
name        = "Flat at 90°"
based_on    = "s2"
index_angle = { mode = "relative", value = 90 }
stock       = { kind = "from_previous" }

  [[setup.toolpath]]
  id   = "t4"
  name = "Pocket flat"
  type = "pocket_2d"

[[view]]                                       # saved camera views
```

A three-axis Setup uses `orientation = { tool_axis = …, x_axis = … }`, a `block` stock, and any
zero point (03 §3.9.3):

```toml
orientation = { tool_axis = { face = "f-3" }, x_axis = "+X" }
zero        = { method = "stock_top_corner", corner = "front_left" }
stock       = { kind = "block", margin = [2, 2, 1] }
region      = { floor_z = -20 }
```

Inheritance in the file: a key present in a Setup is a local value; an absent key is inherited
from `based_on`. The writer MUST NOT copy inherited values into the child. Otherwise later parent
edits would stop flowing to it.

Toolpath keys:

- `type` names the toolpath type (04 §4.13).
- Parameters sit in the five groups of 03 §3.10.2: `tool_params`, `geometry`, `heights`,
  `passes`, `linking`. Only fields the type declares are allowed; any other key is an error.
- Keys use plain CAM terms (`stock_to_leave`, `step_down`, `stepover`).
- Geometry-group keys not shown above: `drive_faces`, `drive_curves`, `check_faces`,
  `excluded_faces`, `boundary`, `rest_from` (03 §3.10.5). A selection is an array of
  `{ body = id }`, `{ face = id }` or `{ edge = id }` tables; `edge` names a B-rep edge or a
  WireBody curve (03 §3.5).
- `material` on a Setup selects the `[[tool.preset]]` with the same `material` (2.4). A child
  Setup inherits it.
- Full per-type key lists: [D15](../design/D15-toolpath-parameters.md) for the common
  parameters and the M3 types; the rest **[OPEN]** (04 §4.13.7).

## 2.2 Post-processor definition

Engine semantics, inheritance, and resolution rules are in
[05](05-postprocessor-engine.md). Example of a leaf post that inherits from a dialect:

```toml
format  = "fluut.post/1"
id      = "user/my-router"
version = "3.0"
name    = "My router (LinuxCNC, 4th axis)"
extends = "fluut/linuxcnc@1"                # parent: latest 1.x
mixins  = ["fluut/mixin/rotary-a", "fluut/mixin/inverse-time-feed"]

[output]
extension = "ngc"

[words.X]                                     # override one word format from the parent
decimals = 4

[blocks.header]
mode  = "append"                              # add to parent's header, don't replace it
lines = ["(Machine: [MACHINE_NAME])", "G64 P0.005"]

[blocks.tool_change]                          # replace parent's block entirely (default mode)
lines = [
  "M5",
  "G53 G0 Z0",
  "T[T] M6",
  "G43 H[T]",
]
```

## 2.3 Machine definition

Machines are layered like posts (05 §5.2): a vendor model can
extend a generic machine type, and a user machine can extend the vendor model.

```toml
format  = "fluut.machine/1"
id      = "user/my-router"
version = "2.0"
name    = "My router"
extends = "fluut/generic-3axis-router@1"
post    = "user/my-router@3"

[travel]
x = [0, 1220]
y = [0, 2440]
z = [-150, 0]

[spindle]
rpm   = [6000, 24000]
ranges = []                                    # optional gear ranges

[feed]
max_cut   = 10000                              # mm/min
max_rapid = 15000

[[axis.rotary]]
name        = "A"
parallel_to = "X"
centre      = [0, 600, -75]
continuous  = true
direction   = "right_hand"                     # positive A sense (03 §3.9.3)
clamp       = "M10"                            # optional; emitted after indexing  
unclamp     = "M11"                            # optional; emitted before rotary motion or re-index
limits_deg  = []

[tool_changer]
kind   = "atc"                                 # none | manual | atc
slots  = 8

[post_overrides]                               # machine-specific post tweaks (05 §5.2.3)
"words.F.decimals" = 0
```

## 2.4 Tool library

One file holds many tools. Geometry uses the cutter profile of
04 §4.2.1.

```toml
format = "fluut.tools/1"
id     = "user/router-bits"
name   = "Router bits"

[[tool]]
id          = "flat-6"
name        = "6 mm 2-flute flat"
number      = 1                               # ATC / T-number
shape       = "flat"                          # flat | ball | bull | v | taper | drill
diameter    = 6.0
corner_radius = 0.0
flute_length  = 22
centre_cutting = true                         # can plunge straight down; default true
# point_angle_deg = 118                       # drill shape only; default 118
# pitch          = 1.0                        # taps only: thread pitch, mm
overall_length = 60
shank_diameter = 6.0
holder      = { diameter = 20, length = 40 }  # check geometry (04 §4.2.1)
names       = { de = "6 mm Schaftfräser" }    # localized names

  [[tool.preset]]                             # cutting data per material
  material   = "MDF"
  rpm        = 18000
  feed       = 2500
  plunge     = 800
  stepover   = 2.4
  step_down  = 3.0
  optimal_load = 0.6                          # adaptive radial engagement (optional)
```

## 2.5 Imported definition formats

Definitions from other CAM products are imported and converted to native definitions. Importer
details and fidelity rules are in [05 §5.10](05-postprocessor-engine.md).

| Product | Kind | File | Approach |
|---|---|---|---|
| Vectric (VCarve, Aspire, Cut2D, Cut3D) | post | `.pp` (declarative text) | full static conversion |
| DeskProto | post, machine, cutter | `.ppr`, `.mch`, `.ctr` (INI) | full static conversion |
| Autodesk Fusion, HSMWorks, Inventor CAM | post | `.cps` (JavaScript) | sandboxed execution, after v1 **[DECIDED — OQ-13]** |
| SheetCAM | post | `.scpost` (Lua) | not planned (OQ-13) |
| FreeCAD CAM | post | Python script | not planned (OQ-13) |
| Vectric | tools | `.vtdb` (SQLite) | static conversion |
| Autodesk Fusion | tools | tool library JSON | static conversion |

Other products' definition files are subject to their licences. FluutCAM imports
files the user supplies. It MUST NOT ship third-party post files unless their licence allows it.

## 2.6 Input geometry formats

| Format | Ext | Body kinds produced | Status |
|---|---|---|---|
| STEP | `.step`, `.stp`, `.stpz` (gzip) | SolidBody, SheetBody, WireBody | **primary** 3D input |
| STL | `.stl` (ASCII + binary) | MeshBody | supported |
| DXF | `.dxf` | WireBody | 2D/3D curves; arcs kept as circles |
| SVG | `.svg` | WireBody | 2D curves; replaces EPS (no EPS in v1, decided 2026-10-31) |
| Bitmap | `.png`, `.jpg` | relief height map | supported |

Bitmap relief is inherently a sampled height map (one Z per pixel). It is the only input whose
geometry is a grid. It is converted to a B-spline or bilinear sheet surface over the pixel
lattice, and toolpaths are then solved against that surface under the DC-02 rules. Pass
endpoints come from the region, not from pixel positions.

### 2.6.1 STEP reader requirements

- **Schemas:** AP203 (ed. 1 and 2), AP214, AP242 MUST be accepted. Part 21 clear-text encoding.
  Part 28 (XML) is not required.
- **Representations:** `ADVANCED_BREP_SHAPE_REPRESENTATION`, `MANIFOLD_SURFACE_SHAPE_REPRESENTATION`,
  `GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION`, `SHAPE_REPRESENTATION` with
  `MANIFOLD_SOLID_BREP`/`SHELL_BASED_SURFACE_MODEL`. Faceted B-rep
  (`FACETED_BREP`) imports as MeshBody.
- **Entities:** all geometry and topology entities in 03 §3.2 MUST be read without loss.
  Unsupported entities MUST be reported by name and count. They MUST NOT be silently skipped.
- **Assemblies:** `NEXT_ASSEMBLY_USAGE_OCCURRENCE` + `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION`
  placements MUST be applied. Each instance becomes a Body; the assembly tree is shown to the
  user for selection.
- **Units and tolerance:** read from the representation context
  (`GLOBAL_UNIT_ASSIGNED_CONTEXT`, `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT`).
- **Validation:** after import, check each body for open shells, non-manifold edges, and edges
  with gaps above modelling tolerance. Report findings.
- **Healing [DECIDED — OQ-04]:** v1 sews edges whose gaps are within modelling tolerance and
  merges duplicate vertices. It does no deeper repair (no gap filling, no trim rebuilding). Faces
  that are still invalid are reported and excluded from machining, and the user is told which.
- **Untrusted input:** bounded memory, bounded entity count, bounded reference depth, cycle
  detection on entity references. Malformed files fail with a located error (line, entity id).

### 2.6.2 STL reader

Vertices are welded within modelling tolerance and mesh adjacency is built. Normal flipping is
per body. Skipping back faces is a per-Toolpath option.

`unit` is REQUIRED for STL and DXF, because those formats carry no unit. The importer MUST ask
the user and MUST NOT assume.

### 2.6.3 Geometry file change detection

On project load, each geometry file is hashed and compared with its stored `hash`. If it
differs, the user is offered a re-import, which follows the identity rules in 03 §3.5.

## 2.7 Output format

G-code text, defined entirely by the resolved post definition (05). Each output file carries a
provenance header (05 §5.9).

## 2.8 Saved toolpaths

Toolpaths are not saved in the project. Results are cached in a sidecar (03 §3.4, OQ-03).
