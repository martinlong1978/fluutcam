# D05 — STL, DXF, SVG and Bitmap Import (`fluut-io-mesh`, `fluut-io-2d`)

Implements: [S 02 §2.6, §2.6.2], [S 03 §3.2.4–3.2.5]. Depends on: `fluut-geom`, `fluut-topo`,
`roxmltree` (SVG), `image` (bitmap). The DXF reader and the SVG path-data parser are our own
(§3, §4; decided in M7).

All readers treat input as untrusted (bounded sizes, no panics, fuzz targets) [S 06 §6.3].
All ask for or confirm the unit, because none of these formats carries a reliable one
[S 02 §2.6.2].

## 1. Common output

```rust
pub struct Import2D { pub wires: Vec<WireBody>, pub report: ImportReport }   // DXF, SVG
pub struct ImportMesh { pub body: MeshBody, pub report: ImportReport }        // STL
pub struct ImportRelief { pub body: BrepBody, pub report: ImportReport }      // bitmap
```

## 2. STL (`fluut-io-mesh`)

### 2.1 Format detection

A file is binary if its size equals `84 + 50·n` where `n` is the `u32` at byte 80. Otherwise it
is parsed as ASCII. The leading `solid` keyword is not used for detection, because many binary
exporters write it into the header.

### 2.2 Reading

- Binary: 80-byte header, `u32` count, 50-byte records (normal, 3 vertices as `f32`, attribute).
  Values widened to `f64`.
- ASCII: streaming tokenizer, `facet normal … outer loop vertex … endloop endfacet`.
- Limits: ≤ 50 M triangles (configurable).
- Degenerate triangles (area < tol_model²) are dropped and counted.

### 2.3 Welding and adjacency

Vertices within `tol_model` are merged. This is an import-time cleanup step, not toolpath
computation, so the use of a grid here does not conflict with DC-02.

```
cell = 2·tol_model; vertices in file order; each merges into the first representative within
tol_model in the 27 neighbouring cells, or becomes a representative; cells in a BTreeMap
```

Representatives are more than `tol_model` apart, so each cell holds only a few of them and a fan
of many triangles around one vertex stays linear. Union-find (as first designed) compares every
pair of points in a cell, which is quadratic for such fans. Unlike union-find, the merge is not
transitive: a chain of points each within `tol_model` of the next is not collapsed into one.
`fluut_topo::mesh::weld_points` implements it; the DXF and SVG chain report uses it too.

Then: build half-edges; mark boundary and non-manifold edges; compute triangle normals from
vertex order and compare with the file's normals (majority vote decides whether to flip all, as
some exporters write inconsistent orders). Report holes (boundary loops) and non-manifold edges.

### 2.5 Mesh bodies in the model

An imported STL becomes B-rep bodies, one per connected shell (triangles joined by shared welded
vertices), so every viewport, picking, Setup and toolpath path that works on STEP bodies works on
meshes without a second code path (`fluut_topo::mesh::to_breps`):

- Each triangle is a planar face (`Surface::Plane` with the triangle as its outer loop, normal from
  the vertex order). Edges are straight lines shared by the triangles that meet at them; the shell
  is `closed` when every edge has two triangles. `BrepBody::from_mesh` marks such a body.
- The drop, section, silhouette and rim queries therefore take triangles through the existing
  plane, line and vertex primitives: flat, ball and bull contacts with the triangle interior, its
  edges and its corners are the exact ones of D07.
- The body's `tol_model` is 1e-6 mm, not the weld tolerance: faces are exactly planar and edges
  exactly straight, and section chains join within 1000 times it.
- Ids: bodies `b-1`, `b-2`, … in order of their first triangle; faces `f-<n>.<i>` with `i` the
  triangle's index within its shell in file order. A re-import of a file with the same triangle
  order keeps every id; the matching of re-import (spec 03 §3.5) falls back on signatures.
- The viewport draws a mesh body's creases (normals more than 30° apart) and borders, not every
  triangle edge.
- Open or non-manifold meshes import with a warning (`model.geometry.mesh.open`); queries still
  follow the triangles that exist.
- Not done yet: facet groups (D03 §1.2) as selectable faces, and a compact triangle primitive for
  meshes over about a million triangles (a face costs about 1 KB here).

### 2.4 Writing

Binary STL writer for exporting simulation stock (D11) and debug output.

## 3. DXF (`fluut-io-2d`)

ASCII DXF, R12 to current, with an own reader of group-code pairs (M7, CH-09). It reads the
`HEADER` (for `$INSUNITS`), `BLOCKS` and `ENTITIES` sections, and bounds every size and count.
Binary DXF is detected and reported as unsupported for now.

| Entity | Maps to |
|---|---|
| `LINE` | `Line` trim |
| `ARC`, `CIRCLE` | `Circle` trim (kept as arcs, [S 03 §3.2.4]) |
| `LWPOLYLINE`, `POLYLINE` (2-D) | lines and arcs from bulge values: `θ = 4·atan(bulge)` |
| `POLYLINE` (3-D) | line chain |
| `SPLINE` | `BSplineCurve3` (control points + knots; fit points re-interpolated if no control points) |
| `ELLIPSE` | `Ellipse` trim |
| `INSERT` | block expanded with its transform (scale, rotation, mirror; nested ≤ 16 deep) |
| `3DFACE`, `MESH`, solids (`3DSOLID` ACIS) | not supported; reported |
| `TEXT`, `MTEXT`, dimensions, hatches | ignored; reported |

- Each layer becomes one wire body (ids `b-1`, `b-2`, … in order of first use), so the user
  selects by layer. Colours are not kept: `WireBody` has no attributes yet.
- Entities on layer `0` inside a block take the layer of the `INSERT`.
- The extrusion direction (OCS, arbitrary-axis algorithm) is applied. Curves stay in file order;
  the report counts chains, closed chains and open ends (`tol_model`), and `fluut-cam` chains
  them for toolpaths.
- Circles and ellipses go through block transforms as conjugate semi-diameters, so a non-uniform
  scale turns a circle into an ellipse and never into a spline.
- `$INSUNITS` is shown as the suggested unit; the user confirms. The project's `unit` is
  required.

## 4. SVG (`fluut-io-2d`)

SVG path data is parsed directly by an own parser, not converted by a renderer library, because
renderers convert elliptical arcs to cubic Béziers and the arcs would be lost [S 04 §4.8]. The
grammar is small; `svgtypes` would have brought `kurbo` along (M7). The XML tree comes from
`roxmltree`. DTDs are allowed, because Illustrator writes a `DOCTYPE`; `roxmltree` bounds
entity expansion and loads no external entities.

| SVG | Maps to |
|---|---|
| `M L H V Z` | lines |
| `A` with rx = ry, no rotation (after transform) | `Circle` trim |
| `A` otherwise | `Ellipse` trim |
| `C S Q T` | Bézier → `BSplineCurve3` (degree 3 or 2) |
| `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon` | equivalent curves |
| `transform` attributes, nested groups | composed affine transforms (non-uniform scale turns circles into ellipses) |
| `use`, `symbol` | expanded |
| text, images, filters, CSS styling | ignored; reported |

Units: SVG user units at 96 per inch unless the root `width`/`height` carry real units
(`mm`, `in`), in which case the `viewBox` scale is applied (with `preserveAspectRatio`). Y is
flipped about the viewport's bottom edge, so Y points up. The result is offered as the
default; the user confirms. A project `unit` overrides it as mm per user unit. The whole file is
one wire body.
CSS and `style` are ignored, so hidden elements are imported.

EPS: not in v1 (CH-10, confirmed); SVG and DXF cover 2-D input.

## 5. Bitmap relief (`fluut-io-2d`)

Images (`png`, `jpg`, 8/16-bit, colour converted to luminance) become a height-map surface
[S 02 §2.6].

User parameters: width (height derived from aspect, or set separately), Z range (black → `z0`,
white → `z1`, invertible), optional smoothing (Gaussian, σ in pixels), base thickness.

Surface:

- A bicubic B-spline surface **interpolating** the (smoothed) pixel heights at pixel centres,
  pixel pitch `p = width / pixel_columns`. Knots uniform. For images > 2 048² pixels the surface
  is split into tiles of 512² pixels (separate faces, C2 across tile edges by construction).
- The result is a `SheetBody` with one face per tile, plus optional side and bottom faces
  (planes) to close it into a block for simulation.

Toolpaths then run on this surface like any other (D07–D09). Pass endpoints come from the region
limits, not pixel positions [S 02 §2.6].

## 6. Tests

- STL: binary file whose header starts with "solid"; ASCII with mixed case and extra spaces;
  welding at tol_model; flipped-normal files.
- DXF: corpus of exports (LibreCAD, Inkscape, AutoCAD, Fusion sketches) including bulge
  polylines, nested blocks, OCS extrusion (0,0,−1) mirrored arcs.
- SVG: arcs under rotation and non-uniform scale; unit handling for mm/in/px documents.
- Bitmap: surface reproduces pixel heights exactly at pixel centres.

## 7. Challenges

- CH-09 DXF reader coverage: own reader chosen in M7; checking it on a corpus of real exports is
  still open (deferred).
