# D05 — STL, DXF, SVG and Bitmap Import (`fluut-io-mesh`, `fluut-io-2d`)

Implements: [S 02 §2.6, §2.6.2], [S 03 §3.2.4–3.2.5]. Depends on: `fluut-geom`, `fluut-topo`,
`dxf` (MIT) or own reader (§3), `svgtypes` (path data parsing), `roxmltree`, `image`.

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
computation, so the use of a hash grid here does not conflict with DC-02.

```
cell = 2·tol_model; hash each vertex to its cell; compare with the 27 neighbouring cells;
union-find merge; representative = first vertex
```

Then: build half-edges; mark boundary and non-manifold edges; compute triangle normals from
vertex order and compare with the file's normals (majority vote decides whether to flip all, as
some exporters write inconsistent orders). Report holes (boundary loops) and non-manifold edges.

### 2.4 Writing

Binary STL writer for exporting simulation stock (D11) and debug output.

## 3. DXF (`fluut-io-2d`)

ASCII and binary DXF, R12 to current. Start with the `dxf` crate; if coverage or robustness is
inadequate in the spike, write a reader (group-code pairs are simple to parse).

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

- Layers and colours kept as attributes, so the user can select by layer.
- Curves are chained into wires where ends meet within tol_model; extrusion direction (OCS) is
  applied.
- `$INSUNITS` is shown as the suggested unit; the user confirms.

## 4. SVG (`fluut-io-2d`)

SVG path data is parsed directly with `svgtypes`, not converted by a renderer library, because
renderers convert elliptical arcs to cubic Béziers and the arcs would be lost [S 04 §4.8].

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
(`mm`, `in`), in which case the `viewBox` scale is applied. The result is offered as the
default; the user confirms.

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

- CH-09 DXF reader choice (`dxf` crate coverage) (deferred).
