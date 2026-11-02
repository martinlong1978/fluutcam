# 04 — Toolpath Algorithms

## 4.0 Kernel approach — exact cutter-contact queries

A common CAM design builds a model height field from triangles, dilates it by the tool
profile, and reads toolpath Z values from the grid. Error sources in that design:

1. Triangulation chord error in the STL itself.
2. Grid quantization in XY (cell size).
3. Interpolation between grid cells.

FluutCAM removes all three for B-rep input. Every cutter-location (CL) point is solved
against the exact face geometry (03 §3.2). For MeshBody input, error source 1 remains; sources 2
and 3 are still removed.

```
Body faces (exact) ──► BVH over trimmed-face bounds
                         │
strategy drive geometry  │  (raster lines, Z planes, spiral, rotary sweep …)
        │                ▼
        └──► cutter query per sample:  candidates = BVH.cull(tool footprint)
                                       CL = max over candidates of contact(tool, face|edge|vertex)
                         │
                         ▼
             adaptive refinement to machining tolerance   (§4.3)
                         │
                         ▼
             segment fitting: line / arc / helix / rotary / spline   (§4.3)
                         │
                         ▼
             Toolpath: Pass[] → Segment[]   (03 §3.4)  ──►  post-processor (05)
```

No height field, Z-map or other grid is used, not even as a seed or cache. The BVH is
the only acceleration structure. It culls candidates and never supplies coordinates.

## 4.1 Pipeline

As 01 §1.6, with step 4 using the flow in §4.0. Iteration is per
Setup, then per Toolpath, in machining order. Each Toolpath is computed in its Setup's
work frame W (03 §3.9.3): bodies are transformed once per Setup by `R_index · (R_align · S · p − z)`,
then all queries run in W.

## 4.2 Cutter model and cutter-contact queries

### 4.2.1 Cutter profile

Cutter shapes are expressed as one parametric profile:

| Shape | Diameter D | Corner radius r | Taper half-angle α |
|---|---|---|---|
| Flat end mill | D | 0 | 0 |
| Ball nose | D | D/2 | 0 |
| Bull nose | D | 0 < r < D/2 | 0 |
| V / tapered | tip Ø | 0 or r | α > 0 |

Holder and shank (collet and shaft diameters) are treated as additional
check geometry, not cutting geometry.

### 4.2.2 Contact cases

For one tool position, the CL height is the maximum over three contact cases. With exact geometry, each case uses the true primitive:

| Case | Mesh | B-rep |
|---|---|---|
| Face interior | triangle plane | exact surface, contact point inside trim loops |
| Edge | straight triangle edge | exact edge curve (line, circle, ellipse, B-spline) |
| Vertex | triangle vertex | B-rep vertex |

A face-interior contact whose (u,v) falls outside the face's trim loops is rejected. The edge and
vertex cases on that face boundary then apply.

### 4.2.3 Analytic fast paths

For a ball-nose tool of radius R, the CL surface of a face is the face's offset surface at
distance R. Offsets of analytic surfaces stay analytic:

| Face kind | Ball-nose CL surface | Solve |
|---|---|---|
| Plane | plane offset by R along normal | closed form |
| Cylinder radius ρ | coaxial cylinder radius ρ ± R | quadratic |
| Cone | coaxial cone (offset) | quadratic |
| Sphere radius ρ | concentric sphere ρ ± R | quadratic |
| Torus (Rmaj, rmin) | torus (Rmaj, rmin ± R) | quartic, closed form or robust root-finder |

For flat, bull-nose and tapered tools on analytic faces, the implementation SHOULD use closed-form
or low-order root-finding solutions where they exist. Otherwise it falls back to §4.2.4.

### 4.2.4 General surfaces

B-spline and other non-analytic faces are solved numerically:

1. Decompose the face into Bézier patches; index patch bounds in the face's BVH node.
2. Cull patches by the tool footprint.
3. Per surviving patch, solve the contact condition with Newton iteration, seeded by subdivision.
4. Converge to ≤ machining tolerance / 10 in position.

The solver MUST report failure to converge. A sample with no converged contact MUST NOT be
emitted silently. The strategy refines the sample spacing or reports the location.

## 4.3 Tolerance-driven sampling and segment fitting

### 4.3.1 Direct emission (no sampling)

Where the drive geometry and the contact face give a known curve, the segment is emitted
directly, without sampling:

| Drive | Contact face | Resulting CL curve | Segment |
|---|---|---|---|
| Plane section (raster, waterline) | Plane | line | LineSegment |
| Plane ⟂ axis (waterline) | Cylinder/cone/torus with vertical axis | circle | ArcSegment |
| Plane ∥ axis (raster) | Cylinder | line | LineSegment |
| Rotary sweep (§4.7) | Cylinder coaxial with rotary axis | constant radius | RotarySegment |
| Edge following (contour, chamfer) | Circular edge | circle | ArcSegment |

The contact face can change along the path. The path then splits at each change.

### 4.3.2 Adaptive sampling

Where direct emission does not apply, the path is sampled adaptively:

```
refine(a, b):                     // a, b = solved CL points with parameters ta, tb
    m  = solve(CL at (ta+tb)/2)
    if distance(m, chord(a, b)) > tol / 2 or contact face changed between a and b:
        refine(a, m); refine(m, b)
    else:
        accept chord a→b
```

Half the tolerance goes to sampling and half to segment fitting. Total deviation is therefore
≤ `machiningTolerance`.

### 4.3.3 Segment fitting

Accepted CL points are fitted greedily into the longest segments that stay within tol/2:

1. LineSegment if all points lie within tol/2 of the chord.
2. ArcSegment if points lie within tol/2 of a circle. The arc plane is restricted to XY/XZ/YZ
   unless the post supports arbitrary-plane arcs (05 §5.5).
3. HelixSegment for an arc with constant axial motion.
4. SplineSegment only if enabled and the post supports it.

Fitting MUST preserve tangent continuity where the ideal path is tangent-continuous. This avoids
the start-stop motion of short collinear chords that point-per-sample output produces.

### 4.3.4 Stepover from scallop height

Exact surfaces provide curvature, so FluutCAM SHOULD offer stepover from a
target scallop height h:

```
flat region, ball radius R:   s = 2·sqrt(2Rh − h²)            (≈ sqrt(8Rh))

curved region, normal curvature κ across the passes
(κ > 0 convex, κ < 0 concave), small-step approximation:
                              s ≈ sqrt( 8·h·R / (1 + R·κ) )
valid while 1 + R·κ > 0 (concave radius larger than R); otherwise the tool
cannot reach the surface and the region is reported as rest material.
```

The approximation follows from two adjacent ball positions on the offset surface of radius
1/κ + R. Because κ varies across a face, stepover varies along each pass.

**[DECIDED — OQ-05]** v1: the user MAY give a scallop height instead of a stepover. FluutCAM then
uses one stepover for the whole toolpath, computed from the worst-case (most convex) curvature
across the drive faces, so the scallop is never exceeded. Variable per-pass stepover is later.

## 4.4 Roughing (layered)

Parameters: strategy specification **[OPEN]**. Each layer's cross-section is
computed by slicing the B-rep with the plane Z = z (exact section curves: lines, arcs, conics,
B-splines). The section is then offset outward by stock to leave + tool radius.

In-process stock: the material left after each roughing layer, used to decide where the
next layer cuts, MUST be held as exact 2D regions per layer (boundaries of lines, arcs and
B-splines). It MUST NOT be a voxel or height-field stock model.

Flat-shelf handling: planar faces with normal parallel to Z are found directly from the B-rep.
Their exact Z heights are added as extra layer heights, so a shelf is never left with partial
layer stock. Mesh bodies use a heuristic search for flat areas instead.

## 4.5 Waterline / Z-level

Level spacing is adaptive (closer on shallow walls). Rules:

- Contours are exact plane sections of the CL surface, not grid iso-lines.
- Wall slope used by the level spacing comes from exact face normals.
- Exact Z of horizontal planar faces is always included as a level (as in §4.4).
- Vertical cylinders, cones and tori give circular contours. These are emitted as ArcSegments
  (§4.3.1).

## 4.6 Contour / outline

The outline is taken from the silhouette or selected B-rep edges, not a grid boundary. Circular
edges give arcs.

## 4.7 Rotary (4th-axis) machining

Rotary strategies run in Setups of kind `rotary` (03 §3.9.2). Machining at a fixed
rotary angle is not a rotary strategy: it is a 3-axis strategy in an `indexed` Setup (§4.9).

### 4.7.1 The facet problem

A cylinder of radius ρ tessellated with N facets has a radius that varies between ρ·cos(π/N) and
ρ around its circumference. Facet count for chord tolerance t:

```
N = ceil( π / acos(1 − t/ρ) )
```

Example: ρ = 25 mm, t = 0.01 mm gives N = 112 facets.

When a mesh is wrapped around the rotary axis, a tool following
this cylinder in A must move radially (machine Z) up and down once per facet. That is 2N = 224
direction reversals of Z per revolution, each of amplitude ρ(1 − cos(π/N)) ≈ t. The machine
decelerates at each reversal. The surface shows N flats or witness marks. Feed drops. The G-code
holds one line per facet edge.

On the exact cylinder, the same pass is one `RotarySegment`: A from 0° to 360° with X and Z held
constant. That is one G-code line (or a few, if the post splits rotary moves).

### 4.7.2 Rotary coordinate frame

Rotary strategies work in a cylindrical frame aligned with the machine's rotary axis
(axis position from the machine definition, 02 §2.3):

```
x  = position along the rotary axis
θ  = rotary angle (machine A)
r  = radial distance from the axis  → machine Z = z_axis + r
```

The cutter approaches along a radial line toward the axis. The cutter query is a
**radial drop**: the largest r at which the tool touches any face. It is the rotary counterpart of the
vertical drop-cutter.

### 4.7.3 Fast paths for surfaces of revolution

When a face's axis is coincident with the rotary axis (within modelling tolerance), the CL
radius does not depend on θ:

| Face (coaxial) | CL radius r(x) | Segment along θ |
|---|---|---|
| Cylinder | constant | RotarySegment, X and Z held |
| Cone | linear in x | RotarySegment with X and Z linear in θ (helical) |
| Torus / sphere | function of x only | RotarySegment per x step |
| Surface of revolution | from profile curve | RotarySegment per x step |

The importer SHOULD detect coaxial faces. A rotary or indexed Setup takes its rotary axis from a
selected cylindrical or conical face (03 §3.9.3), so the user never aligns the model by hand
.

### 4.7.4 Rotary strategies

| Strategy | Drive | Notes |
|---|---|---|
| Rotary raster along X | for each θ step: pass along x | adaptive θ step from tolerance |
| Rotary raster around A | for each x step: full θ sweep | uses §4.7.3 fast path on coaxial faces |
| Rotary spiral | continuous θ with x advancing per revolution | no retracts; helical on coaxial faces |
| Rotary roughing | radial layers r = const, sectioned by cylinder | section of B-rep by cylinder r |

The θ step for non-coaxial faces comes from tolerance at the current radius: Δθ such that
r·(1 − cos(Δθ/2)) ≤ tol/2, then §4.3.2 refinement.

### 4.7.5 Rotary feed

Feed in rotary moves depends on radius. Feed handling (degrees/min, inverse-time `G93`, or
virtual surface speed) is chosen by the post's `feed.rotary_method`. The toolpath engine MUST supply the CL radius per RotarySegment so that the post
can compute it. See 05 §5.7.

## 4.8 2D curve operations (vector, pocket)

Parameters: strategy specification **[OPEN]**. Integer polygon libraries (e.g. Clipper) turn
arcs into chords and snap coordinates to an integer grid. FluutCAM MUST offset
curves in floating point so that arcs stay arcs in the output (an offset of an arc is a
concentric arc), and no coordinate is snapped.

DC-02 rules out the "polygon offset then re-fit" option, because integer polygon libraries
quantize. **[DECIDED — OQ-06]** FluutCAM uses `cavalier_contours` (Rust, polylines with arc
segments, floating point) behind an internal trait. B-spline boundaries are approximated by arcs
within tolerance before offsetting (biarc fitting).

## 4.9 Multi-axis (5th)

Indexed machining: an `indexed` Setup (03 §3.9) applies `R_index(θ)` to the geometry
once. Every 3-axis strategy (§4.3–4.6, §4.8) then runs unchanged in that frame. The ToolpathResult
holds only X Y Z; the A angle belongs to the Setup and is emitted by the post at Setup start
(05 §5.4.1). The same applies to 3+2 on 5-axis machines with a second angle (B).

3+2 with A and B is planned after v1; simultaneous
5-axis is future work (OQ-16, 03 §3.9.2).

## 4.10 Simulation

The simulation stock model MAY be discretized
(voxel/dexel), because simulation is for display and verification only. Simulation results MUST
NOT feed back into toolpath coordinates. Rules:

- Stock MAY be a cylinder (rotary) or a body (03 §3.1).
- Gouge and rest checks SHOULD measure the distance from the simulated stock to the exact faces,
  not to a tessellation. This keeps simulation verdicts consistent with toolpath tolerance.

Simulation also reports, as verification of §4.14: holder or shank contact with stock or model,
rapid moves through stock, and cutting moves exceeding the tool's flute length.

**Machining time estimate** (used by the timeline, 08 §8.7, and scripts): computed per segment
from the programmed feed, the machine's maximum feeds and axis accelerations (02 §2.3), with a
trapezoidal velocity profile and junction speed limits between segments. A per-machine
correction factor scales the estimate.

## 4.11 Decisions

| Id | Raised by | Decision |
|---|---|---|
| OQ-05 | DC-01 | Scallop-height input in v1 with one worst-case stepover per toolpath; variable stepover later (§4.3.4) |
| OQ-06 | DC-01, DC-02 | `cavalier_contours` behind an internal trait (§4.8) |
| OQ-11 | DC-02 | In-process stock as layered exact 2D regions (§4.12.6) |
| OQ-17 | DC-06 | v1 set as proposed plus `scallop` and `steep_and_shallow` (§4.13) |

## 4.12 Exact pass geometry

### 4.12.1 Definition of exact

A toolpath coordinate is **exact** when it is the solution of a geometric equation (contact,
intersection, tangency), solved to the solver tolerance:

```
ε_s = min(1e-6 mm, machiningTolerance / 1000)
```

It is never the nearest node, cell centre, or sample of a lattice. Machining tolerance (§4.3)
limits only the *shape* of the path between exact points (chordal and fitting error). It never
moves an endpoint.

### 4.12.2 Region boundaries are curves

All limits on where a strategy cuts are exact curves:

| Limit | Grid-based CAM | FluutCAM |
|---|---|---|
| Machining region (segment box) | grid extent | box edges as lines |
| Borders | grid cells | exact offsets of the region |
| Selected-face extent | — | silhouette of drive faces (projected B-rep edges and silhouette curves) |
| Containment / boundary curve | — | user curve (WireBody) |
| Stock boundary | grid | stock body or 2D stock region |
| Tool-radius compensation of any limit | cell dilation | exact offset (§4.8) |

### 4.12.3 Pass endpoints

Each pass's drive curve (raster line, rotary line, spiral, Z-plane contour) is intersected with
the active limits. The parameters of those intersections are solved to ε_s. The pass starts and
ends at those exact points.

Where a pass ends because the tool reaches a face boundary, the endpoint is the exact transition
point between contact cases (face → edge → vertex, §4.2.2).

### 4.12.4 Pass layout

Pass positions come from the region, not from a lattice anchored at the origin:

```
W  = extent of the region across the passes  (exact)
s  = requested stepover
mode "even" (default):    n = ceil(W / s);  s' = W / n;   passes at  a + i·s',  i = 0..n
mode "fixed from edge":   passes at  a + i·s,  i = 0..floor(W/s),  plus a final pass at b
```

`a` and `b` are the exact region limits along the stepover direction. In both modes, the first
and last passes lie exactly on the region limits. With "even", the stepover never exceeds the
requested value.

The same rule applies to Z levels (roughing, waterline): exact top, exact bottom, exact flat-face
heights (§4.4), and even spacing between them.

### 4.12.5 Sample points

Adaptive sampling (§4.3.2) places samples by bisection of the pass parameter, starting from the
exact endpoints. Samples are not aligned to any global spacing.

### 4.12.6 Rest machining

Rest regions (material left by a larger previous tool) MUST be computed geometrically, by
comparing the reach of the previous and current tools against the faces. They MUST NOT come from
a simulation grid.

**[DECIDED — OQ-11]** In-process stock is held as **layered exact 2D regions**:

- one region set per Z level used by roughing (exact level heights, §4.12.4), with boundaries
  made of lines, arcs and biarc-fitted B-splines;
- each toolpath subtracts its tool-swept area at each level (tool radius offset of its passes,
  via §4.8 offsetting);
- a roughing toolpath with rest machining clears only the remaining regions at its levels;
- in `rotary` Setups the layers are cylindrical (constant radius r), each region drawn on the
  unrolled cylinder (x, θ·r).

Rest *finishing* (`rest_finishing`, later) extends this with a per-face reach comparison of the
previous and current tools (offset-surface comparison, bitangent detection for concave edges).

### 4.12.7 Verification

The test suite MUST check, for every strategy, that:

1. every pass endpoint lies on an active limit or a contact transition, within ε_s;
2. the spacing between adjacent passes, and between the outer passes and the region limits,
   never exceeds the requested stepover;
3. translation invariance: moving the model and region together by an arbitrary non-round
   offset (e.g. (0.123457, −0.987651, 0.31) mm) moves every toolpath coordinate by the same
   offset within ε_s. A hidden grid anchored to the origin fails this test.

## 4.13 Toolpath type catalogue

### 4.13.1 Basis

FluutCAM offers a catalogue of separate toolpath types (03 §3.10),
modelled on Fusion's milling strategies. Fusion's API enum `OperationStrategyTypes` (now retired
in the API) lists 49 strategy types, including turning, inspection, probing and cutting (jet)
types. Newer Fusion releases add strategies not in that enum. Survey these before planning
releases beyond v1.

Release column: **v1** = first release; **later** = catalogued, not scheduled.
**[DECIDED — OQ-17]** v1 is the proposed set plus `scallop` and `steep_and_shallow`.

### 4.13.2 2D types

Drive geometry: WireBody curves, B-rep edges, or planar faces (pocket floors, walls). 2D types
run in `three_axis` and `indexed` Setups.

| Type id | Role | Description | Fusion | Release |
|---|---|---|---|---|
| `face` | rough/finish | Clear the stock top to a flat Z | Face | v1 |
| `optimised_load_2d` | rough | Constant tool engagement clearing; no full-width cuts; smooth direction changes | 2D Optimised Load Clearing | v1 |
| `pocket_2d` | rough | Offset or raster pocket clearing in Z steps | 2D Pocket | v1 |
| `contour_2d` | finish | Profile along curves or walls; multiple depths, finishing passes, tabs, lead-in/out | 2D Contour | v1 |
| `trace` | finish | Follow curves at their Z or a set depth; optional side offset | Trace | v1 |
| `engrave` | finish | V-carving: V tool, depth varies with curve width (medial axis) | Engrave | v1 |
| `chamfer_2d` | finish | Chamfer along edges with a V or chamfer tool | 2D Chamfer | v1 |
| `bore` | finish | Helical milling of a cylindrical hole or boss (HelixSegments, 03 §3.4) | Bore | v1 |
| `slot` | finish | Slot along its centre line | Slot | later |
| `circular` | finish | Circular passes for holes and bosses | Circular | later |
| `thread_mill` | finish | Helical thread milling of holes and bosses | Thread | later |

### 4.13.3 Hole types

| Type id | Role | Description | Fusion | Release |
|---|---|---|---|---|
| `drill` | hole | Drill, spot, peck, chip-break, tap, ream, bore cycles. Holes picked from cylindrical faces; axis and depth exact from the B-rep (DC-01) | Drilling | v1 |

Drilling needs canned-cycle output (05 §5.4.1).

### 4.13.4 3D types

Drive geometry: B-rep and mesh faces (DC-01). 3D types run in `three_axis` and `indexed` Setups.

| Type id | Role | Description | Fusion | Release |
|---|---|---|---|---|
| `optimised_load_3d` | rough | Constant engagement clearing per Z level against the 3D model | 3D Optimised Load Clearing | v1 |
| `pocket_3d` | rough | Offset or raster clearing per Z level (§4.4 algorithm) | Pocket Clearing | v1 |
| `parallel` | finish | Parallel passes in XY following the surface in Z (§4.3, §4.12) | Parallel | v1 |
| `contour` | finish | Constant-Z (waterline) passes for steep walls (§4.5) | Contour | v1 |
| `horizontal` | finish | Flat faces only; exact planar faces found from the B-rep (DC-01) | Horizontal | v1 |
| `spiral` | finish | Spiral from a centre point, trimmed to the boundary | Spiral | v1 |
| `scallop` | finish | Passes offset inward along the surface at constant 3D stepover (§4.3.4) | Scallop | v1 |
| `steep_and_shallow` | finish | Contour on steep areas, parallel or scallop on shallow; slope threshold | Steep and Shallow | v1 |
| `pencil` | finish | Along internal corners and small fillets (bitangent contact) | Pencil | later |
| `rest_finishing` | finish | Only where a previous, larger tool left material (OQ-11) | Rest Finishing | later |
| `ramp` | finish | Steep walls by ramping down instead of Z steps | Ramp | later |
| `radial` | finish | Radial passes from a centre point | Radial | later |
| `morphed_spiral` | finish | Spiral formed from the boundary shape, not trimmed | Morphed Spiral | later |
| `morph` | finish | Passes morphed between two selected contours | Morph | later |
| `project` | finish | 2D curves projected onto the surface | Projection | later |
| `flow` | finish | Passes along a face's isoparametric curves; needs exact B-rep faces (DC-01) | Flow | later |

### 4.13.5 Rotary types

Run only in `rotary` Setups (03 §3.9.2). Algorithms in §4.7.

| Type id | Role | Description | Fusion | Release |
|---|---|---|---|---|
| `rotary_rough` | rough | Radial layers (constant r), sectioned by cylinders (§4.7.4) | (clearing with rotary option) | v1 |
| `rotary_along_x` | finish | Passes along X, A indexed between passes | Rotary Finishing | v1 |
| `rotary_around_a` | finish | Full A sweeps, X indexed between passes; single-move sweeps on coaxial faces (§4.7.3) | Rotary Finishing | v1 |
| `rotary_spiral` | finish | Continuous A with X advancing per turn | Rotary Finishing | v1 |
| `rotary_wrap_2d` | finish | 2D curves wrapped onto the stock cylinder (engrave, contour, pocket on a cylinder) | — | later |

### 4.13.6 Multi-axis and other types

| Type id | Role | Fusion | Release |
|---|---|---|---|
| `swarf` | finish (side of tool) | Swarf | future (OQ-16) |
| `multi_axis_contour` | finish | Multi-Axis Contour | future (OQ-16) |
| `multi_axis_morph` | finish | Multi-Axis Morph | future (OQ-16) |
| `manual_nc` | utility: stop, optional stop, comment, dwell, pass-through line | Manual NC | v1 |
| `probe_wcs` | utility: probe stock to set work zero | Probe WCS | later |

Not catalogued for now: turning, waterjet/laser/plasma (Fusion `Jet2D`), and inspection types.

### 4.13.7 Per-type parameters (v1)

Passes-group parameters per v1 type. Common groups are in 03 §3.10.2. Every type also has
`machining_tolerance` and, where it lays out passes, `pass_layout`.

| Type id | Passes-group parameters |
|---|---|
| `face` | stepover, direction (climb/conventional), pass angle, overlap beyond stock, step-down + final step-down |
| `optimised_load_2d` | optimal load (radial engagement), step-down, stock to leave (radial, axial), min cutting radius, direction |
| `pocket_2d` | pattern (offset/raster), stepover, step-down, stock to leave, direction |
| `contour_2d` | side (inside/outside for closed chains, left/right for open, on), finish every level, compensation (computer / control / wear), multiple depths (count, step), finishing passes (count, stepover), tabs (positions, width, height) |
| `trace` | depth (curve Z or set depth), side offset, multiple depths |
| `engrave` | max depth; flat-bottom area left for a separate clearing toolpath; corner sharpening. V tool required |
| `chamfer_2d` | chamfer width, tip offset, side |
| `bore` | pitch per turn, direction, finishing pass, bottom clean-up circle |
| `drill` | cycle (drill, spot, dwell, peck, chip-break, tap, ream, bore), peck depth, dwell, retract mode, hole order |
| `optimised_load_3d` | optimal load, step-down, fine step-down (intermediate flat levels), stock to leave (radial, axial), flat-area detection |
| `pocket_3d` | pattern, stepover, step-down, stock to leave, flat-area detection |
| `parallel` | stepover or scallop height, pass angle, direction (one-way/zigzag, climb/conventional), slope limits (min/max angle) |
| `contour` | max step-down + scallop-adaptive levels (§4.5), slope limits, direction, order (by level / by area) |
| `horizontal` | stepover, pattern, stock to leave on flats |
| `spiral` | centre (auto / point), stepover, direction (in/out), machine corners  |
| `scallop` | stepover or scallop height, direction (inside-out / outside-in), continuous (spiral) option, slope limits |
| `steep_and_shallow` | threshold slope angle, steep: max step-down + scallop-adaptive levels; shallow: pattern (parallel/scallop), stepover or scallop height; overlap between regions |
| `rotary_rough` | radial step-down, stepover along X, stock to leave |
| `rotary_along_x` | angular stepover (or scallop height), direction (one-way/zigzag) |
| `rotary_around_a` | X stepover (or scallop height), direction |
| `rotary_spiral` | X advance per revolution (or scallop height), direction |
| `manual_nc` | kind (stop, optional stop, comment, dwell, pass-through text) |

Full field definitions (types, units, ranges, defaults, validation) are to be written per type
in a strategy specification **[OPEN]**.

## 4.14 Holder and shank collision avoidance

Decision recorded 2026-10-31: in v1, collision avoidance is part of toolpath calculation, not
only a simulation check.

**Tool assembly.** A tool is a stack of solids of revolution: cutting part (profile of §4.2.1,
up to flute length), non-cutting flute and neck, shank, and holder (02 §2.4 `holder`). Only the
cutting part may touch the model; every other part is check geometry.

**During calculation**, for every pass:

1. Solve the cutting contact as usual (§4.2).
2. Check the non-cutting parts at that position against the model faces (exact, via the BVH)
   and against the in-process stock (§4.12.6).
3. Where a collision occurs, the pass is **trimmed**: cut at the exact entry and exit parameters
   of the collision zone (solved to ε_s, §4.12.1) and linked around it at a safe height.
4. Trimmed zones are reported: a warning on the toolpath, viewport markers, and the minimum tool
   stick-out that would avoid the collision.

**Linking moves** (rapids and links, §4.1 step 6) are checked against model and stock with the
full tool assembly. A link that would collide is raised to the retract or clearance height.

Tool axis tilting to avoid collisions is out of scope until simultaneous 5-axis (OQ-16).
