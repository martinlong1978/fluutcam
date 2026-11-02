# 03 — Data Model

## 3.1 Object hierarchy

FluutCAM uses the tree familiar from Fusion 360 and similar CAM products: **Setup →
Toolpath**.

```
Project
├─ Model                 imported geometry, not part of the machining tree
│  └─ GeometrySource[]   one per imported file (STEP, STL, DXF/SVG, bitmap)
│       └─ Body[]        model coordinates, see §3.2                        
└─ Setup[]               ordered = machining order                          
   ├─ basedOn            optional earlier Setup; unset fields inherited (§3.9.4)
   ├─ kind               three_axis | rotary | indexed                       (§3.9.2)
   ├─ machine            MachineDefinition ref                              
   ├─ geometry           bodies machined + scaling/mirror                    (§3.9.3)
   ├─ orientation        model axes → machine axes                           (§3.9.3)
   ├─ indexAngle         rotary position for kind = indexed                  (§3.9.5)
   ├─ stock              block | cylinder | body | from_previous             (§3.9.6)
   ├─ zero               work-coordinate origin on model or stock            (§3.9.3)
   ├─ workOffset         G54 … (controller work offset)
   ├─ region, clearance  machining box / clearance plane or cylinder
   └─ Toolpath[]         ordered                                            
      ├─ type            fixed at creation, e.g. optimised_load_3d, parallel     
      ├─ parameters      common groups + the type's own group (§3.10)      
      ├─ FaceSelection   drive faces, check faces, excluded faces      
      ├─ machiningTolerance                                           
      └─ ToolpathResult  computed passes and segments, see §3.4        
```

Naming: "Toolpath" is the tree item the user creates (tool + toolpath type + parameters). Its computed motion is the **ToolpathResult**.

Height-field and voxel structures MAY exist as caches; they are not part of the persistent
model.

## 3.2 Geometry model

The geometry model follows the STEP integrated resources ISO 10303-42 (geometry and topology),
as used by AP203, AP214 and AP242. Entity names in brackets are the STEP entities each type maps
from. The mapping is close to one-to-one, so that import loses nothing and export (if added)
round-trips.

### 3.2.1 Bodies

```
Body                                   abstract
├─ SolidBody      [MANIFOLD_SOLID_BREP, BREP_WITH_VOIDS]       closed shell(s)
├─ SheetBody      [SHELL_BASED_SURFACE_MODEL, OPEN_SHELL]      open surfaces
├─ WireBody       [GEOMETRIC_CURVE_SET, EDGE_BASED_WIREFRAME]  curves only (2D/3D)
└─ MeshBody       (STL)                                         triangles only
```

Each Body carries:

| Field | Type | Meaning |
|---|---|---|
| `id` | stable id | survives re-import where the source entity is unchanged (see §3.5) |
| `name` | string | from STEP `PRODUCT` / representation name, else file name |
| `sourceRef` | GeometrySource + entity id | traceability to the file |
| `unit` | length unit | from STEP `SI_UNIT`/`CONVERSION_BASED_UNIT`; STL: user-declared |
| `modellingTolerance` | length | from STEP uncertainty; default 1e-6 × body diagonal |
| `placement` | rigid transform | from STEP assembly placement (`AXIS2_PLACEMENT_3D` chain) |
| `colour` | optional RGB | from STEP presentation (AP214/242) |

### 3.2.2 Topology

```
SolidBody / SheetBody
└─ Shell[]            [CLOSED_SHELL, OPEN_SHELL]
   └─ Face[]          [ADVANCED_FACE, FACE_SURFACE]   surface + sameSense flag
      └─ Loop[]       [FACE_OUTER_BOUND, FACE_BOUND, EDGE_LOOP]  outer + inner (holes)
         └─ Coedge[]  [ORIENTED_EDGE]                  edge + orientation
            └─ Edge   [EDGE_CURVE]                     curve + start/end Vertex + sameSense
               └─ Vertex [VERTEX_POINT]                point
```

Adjacency (face ↔ edge ↔ face) MUST be queryable. Strategies need it for boundary following,
rest machining, and tangent-continuous chaining.

### 3.2.3 Surface types

| Type | STEP entity | Parameters | Evaluation |
|---|---|---|---|
| Plane | `PLANE` | origin, normal, ref dir | closed form |
| Cylinder | `CYLINDRICAL_SURFACE` | axis placement, radius | closed form |
| Cone | `CONICAL_SURFACE` | axis placement, radius at origin, semi-angle | closed form |
| Sphere | `SPHERICAL_SURFACE` | centre, radius | closed form |
| Torus | `TOROIDAL_SURFACE`, `DEGENERATE_TOROIDAL_SURFACE` | axis placement, major R, minor r | closed form |
| Extrusion | `SURFACE_OF_LINEAR_EXTRUSION` | profile curve, direction | via profile curve |
| Revolution | `SURFACE_OF_REVOLUTION` | profile curve, axis | via profile curve |
| B-spline | `B_SPLINE_SURFACE_WITH_KNOTS`, `RATIONAL_B_SPLINE_SURFACE`, `BEZIER_SURFACE` | degrees, knots, poles, weights | de Boor; tolerance-driven |
| Offset | `OFFSET_SURFACE` | basis surface, distance | basis + normal × d |
| Trimmed rect | `RECTANGULAR_TRIMMED_SURFACE` | basis, u/v bounds | basis |
| Triangle | (MeshBody only) | 3 vertices | planar |

Every surface type MUST implement:

```
evaluate(u, v) -> Point3
derivatives(u, v, order) -> (Su, Sv, Suu, Suv, Svv)
normal(u, v) -> Vec3                 // respects Face.sameSense
closestPoint(p) -> (u, v, distance)
bounds -> AABB                      // of the trimmed face, not the infinite surface
kind -> SurfaceKind                 // lets strategies take analytic fast paths
```

### 3.2.4 Curve types

| Type | STEP entity | Notes |
|---|---|---|
| Line | `LINE` | point + direction |
| Circle | `CIRCLE` | placement + radius; maps directly to G2/G3 (05) |
| Ellipse | `ELLIPSE` | placement + semi-axes |
| Parabola / Hyperbola | `PARABOLA`, `HYPERBOLA` | rare; MAY be converted to B-spline on import |
| B-spline | `B_SPLINE_CURVE_WITH_KNOTS`, `RATIONAL_B_SPLINE_CURVE`, `BEZIER_CURVE` | general |
| Polyline | `POLYLINE` | MAY be stored as line chain |
| Trimmed | `TRIMMED_CURVE` | basis + parameter or point trims |
| Offset | `OFFSET_CURVE_3D` | basis + distance |
| Surface curve | `SURFACE_CURVE`, `SEAM_CURVE`, `PCURVE` | 3D curve + p-curves in face (u,v) |

2D vector input (DXF/SVG) maps into the same curve types inside a WireBody.
Arcs in DXF become `Circle` trims, not polylines.

### 3.2.5 MeshBody

STL triangles are held as an indexed mesh (shared vertices, welded within modelling tolerance).
Each triangle is a planar face. Mesh
adjacency MUST be built at import, so mesh bodies support the same edge/face queries as B-rep
bodies.

Mesh-to-analytic recognition (fitting planes, cylinders, and similar to triangle groups)
**[DECIDED — OQ-02]**: not in v1. When added, it is an explicit, opt-in user action. It MUST
produce a new SheetBody/SolidBody alongside the mesh, MUST show the fit deviation per face, and
MUST NOT silently replace the mesh.

## 3.3 Toolpath fields

Toolpath parameters are arranged in typed parameter groups (DC-06, §3.10). The table below lists
fields introduced by DC-01 and DC-02; §3.10 places them in groups.

| Field | Change | Notes |
|---|---|---|
| `machiningTolerance` | new | Max deviation of CL path from ideal (default 0.01 mm). |
| `maxSegmentLength` | new | Optional maximum segment length cap. |
| `driveFaces` | new | Faces the strategy machines. Empty = all faces of the Setup's bodies. |
| `checkFaces` | new | Faces the tool MUST NOT gouge but does not machine. Default: all remaining faces. |
| `excludedFaces` | new | Faces ignored entirely (e.g. fixture bodies imported with the model). |
| `outputArcs` | new | Fit arcs/helices in output (default true; post capability permitting). |
| `passLayout` | new | `even` (default) \| `fixedFromEdge`; see 04 §4.12.4 |
| `boundary` | new | optional containment curve (WireBody ref) + side (inside/outside) + tool-position (centre/inside/outside) |

## 3.4 ToolpathResult model

```
ToolpathResult                    (computed; coordinates in the Setup's work frame, §3.9.3)
└─ Pass[]                         (one Z level, one raster line, …)
   └─ Segment[]                   (one typed move: line, arc, helix, rotary, …)
```

```
Segment                                   abstract
  start: AxisPosition                     // X Y Z (+ A, B when multi-axis)
  end:   AxisPosition
  motion: Rapid | Feed | Plunge | LeadIn | LeadOut | Link
  feed:   value (resolved)
  sourceFace: FaceId?                     // CC face, for traceability / display

LineSegment     : Segment
ArcSegment      : Segment   centre, axis (plane normal), sweep angle (signed), radius
HelixSegment    : Segment   ArcSegment + axial pitch
RotarySegment   : Segment   rotary axis sweep (A or B) with linear axes held or
                            linearly interpolated; see 04 §4.7
SplineSegment   : Segment   B-spline CL curve; emitted only if post supports it (05 §5.5)
CycleSegment    : Segment   one drilling cycle at one hole: cycle kind, hole top, depth,
                            retract plane, peck/dwell parameters; post emits a
                            canned cycle or expands it (05 §5.5)
```

Every Segment MUST record the machining tolerance it was fitted to. The fitter MUST guarantee
that the segment deviates from the ideal CL path by no more than that tolerance.

Segment start and end positions are full-precision `f64`, solved as defined in 04 §4.12.
Adjacent segments share their junction point exactly (bitwise equal), so no gaps or overlaps
come from storage.

Serialization of computed toolpaths **[DECIDED — OQ-03]**: a sidecar folder
`<project>.fluut-cache/` next to the project, one file per ToolpathResult, `serde` binary
encoding (e.g. `postcard`). Each file holds a schema version and a hash of every input (geometry
hashes, Setup, the Toolpath parameters that affect the result, tool, FluutCAM version).
Feeds, spindle speed and other parameters applied only at post time are not in this hash: changing
them invalidates the post output, not the ToolpathResult (D15 §1.8). On load, a missing file, version
mismatch or hash mismatch means "not calculated" (08 §8.6). The cache is disposable: deleting it
loses nothing but calculation time. The project file stays small and diffable.

## 3.5 Re-import and identity

STEP files change during design iteration. On re-import:

1. Match bodies and faces by STEP entity name and persistent id where present (AP242
   persistent ids), else by geometric signature (surface kind + parameters + bounds within
   modelling tolerance).
2. Toolpaths keep face selections for matched faces. Edges and WireBody curves selected as drive
   curves (`{ edge = id }`, 02 §2.1) get persistent ids by the same scheme and are matched the
   same way.
3. Unmatched selected faces MUST be reported to the user. They MUST NOT be silently dropped.

## 3.6 Scriptable object model

Defined in [09 §9.4](09-scripting.md): `Project`, `Model`, `Body`,
`Face`, `Setup`, `Toolpath`, `Library`, `Ui`, with the same keys as the project file.

## 3.7 Definitions: machines, posts, tools

Machines, posts and tools are layered definitions. Formats are in [02 §2.2–2.4](02-data-formats.md); the post engine is
in [05](05-postprocessor-engine.md).

```
DefinitionLibrary
├─ PostDefinition[]      layered: each has optional `extends` parent + `mixins`
├─ MachineDefinition[]   layered the same way; references one PostDefinition
└─ ToolLibrary[]
     └─ Tool[]           geometry (04 §4.2.1), holder, cutting data presets

Resolved views (computed, not stored by the user):
ResolvedPost      = merge(parent chain, mixins, own layer, machine overrides)
ResolvedMachine   = merge(parent chain, own layer)
```

Each definition has:

| Field | Meaning |
|---|---|
| `id` | stable, namespaced, e.g. `fluut/fanuc`, `user/my-router` |
| `version` | `major.minor`; a saved edit bumps the minor; a change that alters output bumps the major (decided 2026-10-31) |
| `origin` | `builtin` \| `user` \| `imported` (+ source format and file hash) |
| `extends` | parent id + version: `id` (latest), `id@3` (latest 3.x), `id@3.2` (exact) |

A Setup references a MachineDefinition. A Toolpath references a Tool. When a project is posted,
the **resolved** post and machine (with every layer's id and version) are recorded with the
output (05 §5.9). Later edits to a base layer therefore never change the meaning of an already
posted program without notice.

## 3.8 Implementation note

Types in this section are logical. They are implemented as Rust types: enums for closed sets
(`SurfaceKind`, `Segment` variants, `Body` variants), `serde` derive for persisted types, and
newtype ids (`FaceId(u64)`, `BodyId(u128)`). Field names follow Rust `snake_case` in code and
persisted formats.

## 3.9 Setup model

### 3.9.1 Purpose

A Setup is one way of holding the work on the machine. It defines everything the Toolpaths in it
share:

- which geometry is machined, and its scaling;
- how model axes align with machine axes;
- the stock;
- the work zero;
- the setup kind: plain 3-axis, rotary (rotary axis moves while cutting), or indexed (rotary
  axis locked at an angle).

Toolpaths hold only what differs per cut: tool, toolpath type, parameters, face selection.

### 3.9.2 Setup kinds

| Kind | Rotary axis during cutting | Toolpath strategies allowed | Output |
|---|---|---|---|
| `three_axis` | not used (or absent) | 3-axis strategies (04 §4.3–4.6, §4.8) | X Y Z |
| `rotary` | moves (simultaneous 4-axis) | rotary strategies (04 §4.7) | X Y Z A (Y usually constant) |
| `indexed` | locked at `indexAngle` | 3-axis strategies, in the rotated frame | A once at setup start, then X Y Z |

`rotary` and `indexed` need a machine with a rotary axis (02 §2.3). A 5-axis machine's second
rotary axis (B) follows the same pattern as `indexed` with two angles (3+2 machining). Full
5-axis setup kinds **[DECIDED — OQ-16]**: v1 has `three_axis`, `rotary` and `indexed` (one rotary
axis, A). Indexed with two angles (A + B, 3+2) follows in the next release. Simultaneous 5-axis
is future work.

### 3.9.3 Frames and transforms

Two frames:

- **Model frame M**: body coordinates after import (unit conversion and STEP assembly
  placements, 02 §2.6). The user never edits model coordinates to set up machining.
- **Work frame W**: machine-aligned axes, origin at the work zero. ToolpathResult coordinates are
  in W, and the post emits them directly.

```
p_W = R_index(θ) · ( R_align · S · p_M  −  z )

S        scale and mirror (diagonal), about the model origin
R_align  orientation: rotation taking model directions to machine directions
z        work zero, in the aligned frame
R_index  rotation by θ about the rotary axis line (through the W origin);
         identity for three_axis; θ = indexAngle for indexed;
         for rotary, θ is the per-segment A value inside the toolpath (04 §4.7)
```

**Orientation (`R_align`)** is picked from geometry, not typed as angles:

| Kind | User picks | Result |
|---|---|---|
| `three_axis` | tool axis (+Z of W): a planar face normal, cylinder axis, edge, or ±X/±Y/±Z of M; then +X of W the same way | `R_align` |
| `rotary`, `indexed` | rotary axis line: axis of a cylindrical/conical face (04 §4.7.3), an edge, two points, or an M axis; then the A = 0 reference: which model direction points to +Z of W | `R_align` and axis line |

**Work zero (`z`)**:

| Kind | Allowed zero |
|---|---|
| `three_axis` | stock box point (corner, centre, top centre, …), model point (vertex, arc centre), or explicit coordinates |
| `rotary`, `indexed` | Y and Z MUST lie on the rotary axis line. X is free (stock end, model point, explicit) |

Putting Y and Z on the rotary axis means every rotary and indexed Setup on the same fixture can
share one work offset. Indexing never moves the zero.

The positive rotation sense of A follows the machine definition (`axis.rotary.direction`,
default right-hand rule about the parallel linear axis).

### 3.9.4 Inheritance ("based on")

A Setup MAY name an earlier Setup as `basedOn`. Then:

1. Every Setup field is either **set locally** or **inherited** from the parent. Inherited fields
   follow later changes to the parent.
2. The UI shows which fields are inherited and offers "reset to inherited".
3. Toolpaths are never inherited. Each Setup has its own list.
4. The parent MUST come earlier in machining order. Cycles are impossible as a result.
5. Chains may be any length: Setup C based on B based on A.

Validation rule: when `stock = from_previous`, the Setup MUST keep the parent's machine,
geometry, scaling, rotary axis line and work zero. Only `kind`, `indexAngle`, region and
clearance may change. Otherwise the carried-over stock would not line up with the part. This is
a validation error, not a warning.

### 3.9.5 Index angle

```
indexAngle = { mode = "absolute" | "relative", value = degrees }

resolved(setup) =
    mode = absolute:  value
    mode = relative:  resolved(parent) + value       // parent of kind rotary resolves to 0
```

The resolved angle MUST lie within the machine's rotary limits (02 §2.3), unless the axis is
continuous.

At the start of an indexed Setup, the post emits the rotary positioning move to the resolved
angle, then the clamp code if the machine defines one (05 §5.4.1). At the end, or before a
`rotary` Setup, it emits the unclamp code.

In FluutCAM the geometry is never rotated by the user. The index angle is one Setup parameter, and both the toolpath frame and the A output
derive from it, so they cannot disagree.

### 3.9.6 Stock

| Stock kind | Meaning | Default for |
|---|---|---|
| `block` | box, from model bounds + margins, or explicit size | `three_axis` |
| `cylinder` | coaxial with rotary axis; radius and length | `rotary`, `indexed` |
| `body` | a body from the Model (e.g. a casting or prior-op STEP) | — |
| `from_previous` | material left after the parent Setup's toolpaths | Setups with `basedOn` |

For simulation and display, `from_previous` stock MAY be the parent's discretized simulation
stock (04 §4.10). For toolpath computation it MUST be geometric (DC-02, 01 §1.2a).
**[DECIDED — OQ-15]**: the in-process stock model of 04 §4.12.6 (layered exact 2D regions) is
carried from the parent Setup, transformed into the child's frame. For an `indexed` child of a
different angle, the parent's layers are re-sliced in the child's frame from the parent's
stock boundary (method detailed in the strategy specification). The child starts from the
parent's stock after the parent's last stock-updating toolpath, and its toolpaths that read the
stock go stale when the parent's toolpaths change. The re-sliced stock is never smaller than the
parent's material (D08 §7).

### 3.9.7 Example: rotary, then two indexed sides

```
Setup 1  "Turn profile"   kind = rotary
         geometry = [shaft], rotary axis = axis of face F12, A0 reference = +Z of model
         stock = cylinder r 26 × 120, zero = left end of stock on axis
         Toolpaths: rotary rough, rotary finish

Setup 2  "Flat at 0°"     basedOn = 1, kind = indexed, indexAngle = absolute 0
         stock = from_previous                     (everything else inherited)
         Toolpaths: pocket flat, drill holes

Setup 3  "Flat at 90°"    basedOn = 2, indexAngle = relative +90
         stock = from_previous
         Toolpaths: pocket flat, chamfer
```

Setups 2 and 3 set one or two fields each. The post output is: Setup 1 rotary moves; then
`A0` + clamp, Setup 2 cuts; then unclamp, `A90`, clamp, Setup 3 cuts. All three use the same work
offset. The project file form is in 02 §2.1.

## 3.10 Toolpath types and parameters

### 3.10.1 Principle

The user adds a **specific toolpath type** (e.g. "3D Optimised Load Clearing",
"Parallel", "2D Contour"), as in Fusion. Then:

1. The type is fixed at creation. Changing type means creating a new toolpath; "Duplicate as…"
   copies the parameters the two types share.
2. The editor shows only the parameters of that type.
3. Roughing (bulk stock removal) and finishing are separate toolpaths. No finishing type has a
   roughing option (§3.10.4).

The catalogue of types is in [04 §4.13](04-toolpath-algorithms.md).

### 3.10.2 Parameter groups

Every type's parameters are arranged in the same five groups (the editor shows them as tabs,
like Fusion). A type declares which fields of each group it uses.

| Group | Contents (fields used vary by type) |
|---|---|
| **Tool** | tool ref; spindle rpm; feeds: cutting, lead-in, lead-out, ramp, plunge, transition; coolant |
| **Geometry** | drive faces / curves / holes; check faces; excluded faces; boundary + tool containment; rest machining source (§3.10.5) |
| **Heights** | clearance, retract, feed, top, bottom. Each is "reference + offset"; references: stock top/bottom, model top/bottom, selection, Setup clearance, work zero. Rotary Setups use radii instead of Z (03 §3.9) |
| **Passes** | the type's own pattern parameters (stepover, step-down, angle, direction, pass layout, stock to leave radial/axial, machining tolerance, spring passes, …) |
| **Linking** | retract policy (full / minimum / stay down); lead-in/out (arc, linear, radius); ramp type (plunge, zigzag, helix, profile) and angle; transitions; ordering |

Defaults: feeds, speeds, stepover and step-down come from the tool's cutting-data preset for
the Setup's material (the Setup's `material` key, 02 §2.1, §2.4). Heights default from the Setup's stock and clearance.

### 3.10.3 Implementation

```rust
pub struct Toolpath {
    pub id: ToolpathId,
    pub name: String,
    pub tool: ToolRef,
    pub common: CommonParams,   // Tool, Geometry, Heights, Linking groups
    pub kind: ToolpathKind,     // type + its own Passes group
    pub result: Option<ToolpathResult>,
}

#[serde(tag = "type")]
pub enum ToolpathKind {
    Face(FaceParams),
    OptimisedLoad2d(OptimisedLoad2dParams),
    Pocket2d(Pocket2dParams),
    Contour2d(Contour2dParams),
    Parallel(ParallelParams),
    // … one variant per catalogue entry (04 §4.13)
}
```

Each `*Params` struct holds only that type's pass parameters. A parameter that does not apply
cannot be set, stored or shown. Each type also declares:

- which Setup kinds allow it (03 §3.9.2; e.g. rotary types only in `rotary` Setups);
- which common fields it uses (e.g. Drill has no stepover; Face has no check faces);
- validation rules (ranges, cross-field rules) with user-facing messages;
- the editor layout (field order, labels, help text), generated from the struct metadata.

Unknown keys for a type in the project file are errors (02 §2.0).

### 3.10.4 Roughing and finishing separated

| Role | Types | Rule |
|---|---|---|
| Roughing | `face`, `optimised_load_2d`, `pocket_2d`, `optimised_load_3d`, `pocket_3d`, `rotary_rough` | Remove bulk stock, in layers, down to `stock_to_leave`. |
| Finishing | all 3D finishing, rotary finishing, `contour_2d`, `trace`, `engrave`, `chamfer_2d`, `bore` | Cut to the final surface (`stock_to_leave` default 0). No stock-clearing layers. |

A finishing type MAY have repeated passes of its own pattern: multiple depths on a 2D contour,
or spring passes. These follow the finished geometry; they never clear stock.

### 3.10.5 Rest machining between toolpaths

A roughing or rest-finishing toolpath MAY take its starting stock from earlier toolpaths in the
same Setup (Fusion "rest machining"). The stock is the in-process stock model of 04 §4.12.6
(OQ-11), updated by each earlier toolpath.

### 3.10.6 Templates

The user SHOULD be able to save a toolpath (type + parameters, without geometry selection) as a
template, and to save an ordered set of toolpaths as a Setup template. Templates are TOML files
in the definition library (02 §2.0), layered like other definitions (05 §5.2): a template MAY
`extends` another template. **[DECIDED — OQ-18]** Toolpath and Setup templates are in v1.

