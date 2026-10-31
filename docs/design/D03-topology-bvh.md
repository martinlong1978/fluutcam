# D03 — B-rep Topology and Spatial Index (`fluut-topo`)

Implements: [S 03 §3.2.1–3.2.2, §3.2.5, §3.5], [S 01 §1.4 items 1–6], [S 04 §4.12.2].
Depends on: `fluut-geom`, `slotmap`, `rayon`, `spade` (constrained Delaunay, display only).

## 1. Body storage

Bodies are immutable after import and shared as `Arc<Body>` (D01 §3.1).

```rust
pub enum Body {
    Brep(BrepBody),       // SolidBody, SheetBody, WireBody of the spec
    Mesh(MeshBody),
}

pub struct BrepBody {
    pub id: PersistId,
    pub name: String,
    pub kind: BrepKind,                    // Solid | Sheet | Wire
    pub tol_model: f64,
    pub surfaces: Vec<Surface>,            // geometry pools, indexed by SurfId / CurveId
    pub curves: Vec<Curve3>,
    pub pcurves: Vec<Curve2>,
    pub vertices: SlotMap<VertKey, Vertex>,
    pub edges:    SlotMap<EdgeKey, Edge>,
    pub coedges:  SlotMap<CoedgeKey, Coedge>,
    pub loops:    SlotMap<LoopKey, Loop>,
    pub faces:    SlotMap<FaceKey, Face>,
    pub shells:   Vec<Shell>,
    pub face_ids: BiMap<PersistId, FaceKey>,   // §6
    pub bvh: Bvh,                              // §4
}

pub struct Vertex { pub p: Point3, pub tol: f64 }
pub struct Edge   { pub curve: Option<CurveId>, pub range: Interval, pub v0: VertKey, pub v1: VertKey,
                    pub same_sense: bool, pub tol: f64, pub coedges: SmallVec<[CoedgeKey; 2]> }
pub struct Coedge { pub edge: EdgeKey, pub forward: bool, pub pcurve: Option<PcurveId>,
                    pub loop_: LoopKey, pub next: CoedgeKey, pub prev: CoedgeKey }
pub struct Loop   { pub face: FaceKey, pub outer: bool, pub first: CoedgeKey }
pub struct Face   { pub surface: SurfId, pub same_sense: bool, pub loops: SmallVec<[LoopKey; 2]>,
                    pub uv_box: UvBox, pub bounds: Aabb3, pub trim: TrimCache }
```

- `Edge.curve = None` for degenerate edges (e.g. cone apex, sphere pole) where the coedge has a
  p-curve but no 3-D extent.
- An edge with two coedges is manifold; one coedge = sheet boundary; more = non-manifold (allowed
  in sheets, reported by validation, D04 §6).
- Face–face adjacency comes from `Edge.coedges` → `Coedge.loop_` → `Loop.face`.

### 1.1 Mesh bodies

```rust
pub struct MeshBody {
    pub id: PersistId,
    pub name: String,
    pub tol_model: f64,
    pub positions: Vec<Point3>,              // welded (D05 §2)
    pub tris: Vec<[u32; 3]>,
    pub normals: Vec<UnitVec3>,              // per triangle, from vertex order
    pub half_edges: HalfEdgeTable,           // twin, next, tri; boundary marked
    pub regions: Vec<MeshRegion>,            // selectable face groups (§1.2)
    pub bvh: Bvh,
}
```

### 1.2 Mesh selection regions

A user selects "faces" on a mesh as they would on a B-rep. Selecting single triangles is useless,
so triangles are grouped into **regions** at import: flood fill across edges whose dihedral angle
is below 20° (configurable). Regions have `PersistId`s derived from a hash of their triangle set,
so selections survive re-import of an identical STL.

## 2. Face orientation and sense

The outward normal of a face is `surface.normal(u,v) × (if same_sense { 1 } else { −1 })`.
Applying a mirroring `Affine` (det < 0) reverses every loop. It toggles `same_sense` only for
faces whose surface reports `Surface::transform_flips_normal(a)` (B-spline, extrusion, and
offsets of those). Analytic and revolution surfaces reverse one parameter instead and keep an
outward normal, so their sense is unchanged. P-curves are mapped with
`Surface::transformed_param` (CH-24).

## 3. Trim classification (point in face)

Every face-interior contact must be inside the trim loops [S 04 §4.2.2]. This is the hottest
topological query.

### 3.1 Trim cache (built at import)

For each loop, the p-curves are tessellated in UV space to `tol_model / |S_uv|_max` (so 3-D error
≤ tol_model), giving a closed UV polyline. Polylines are stored in a per-face 2-D grid of edge
buckets for O(1) average ray-crossing tests.

Periodic surfaces (cylinder, cone, sphere, torus, periodic B-splines) need care: loops can cross
the seam. UV polylines are unwrapped into one period window chosen per face (from the loop's
UV bounds), and query points are shifted into that window before classification.

### 3.2 Classification

```rust
pub enum Inside { In, Out, OnBoundary { coedge: CoedgeKey, t: f64 } }

pub fn classify(&self, face: FaceKey, uv: (f64, f64), tol: &Tol) -> Inside;
```

```
1. Shift uv into the face's period window.
2. Distance to the nearest UV polyline segment (bucket search).
   If the 3-D distance (via |S_uv|) > 2·tol_model: winding number on UV polylines → In / Out.
3. Otherwise (near the boundary): find the closest point on the exact 3-D edge curve
   (D02 §3.2), compute the 3-D distance d.
   d ≤ tol.solver → OnBoundary; else decide by the side of the exact edge in the
   surface tangent plane (sign of (p − c) · (n × t)).
```

The UV polylines are therefore only a fast filter. Points near trims are decided against the
exact edges, so DC-02 is kept: no endpoint comes from a polyline.

## 4. Spatial index (BVH)

### 4.1 Primitives

| Body | Leaf primitive |
|---|---|
| B-rep, analytic face | whole face (bounds of the trimmed face) |
| B-rep, B-spline face | Bezier patch ∩ face UV box (from `Surface::bezier_patches`), so large free-form faces cull well |
| B-rep, extrusion / revolution | face split at profile-curve spans |
| B-rep edges | each edge (for edge-contact queries, D07) |
| Mesh | each triangle |

### 4.2 Structure

Binary BVH, `f64` AABBs, built top-down with SAH (binned, 16 bins), leaves ≤ 4 primitives.
Built in parallel with `rayon` for > 10 000 primitives. Stored as a flat node array
(depth-first) for cache-friendly traversal.

```rust
pub struct Bvh { nodes: Vec<BvhNode>, prims: Vec<Prim> }

impl Bvh {
    pub fn query_aabb(&self, b: &Aabb3, out: &mut Vec<Prim>);
    pub fn query_vertical_cylinder(&self, x: f64, y: f64, r: f64, z_min: f64, out: &mut Vec<Prim>);
    pub fn query_swept(&self, seg: (Point3, Point3), r: f64, out: &mut Vec<Prim>);  // capsule
    pub fn nearest(&self, p: Point3) -> (Prim, f64);
    pub fn ray(&self, o: Point3, d: UnitVec3) -> Option<(Prim, f64)>;              // picking
}
```

`query_vertical_cylinder` is the drop-cutter cull: all primitives whose AABB overlaps the tool's
XY disc and lies above `z_min`.

### 4.3 Frames

Algorithms work in the Setup's work frame W (tool axis +Z; rotary axis X) [S 03 §3.9.3]. Rather
than transforming every query, `fluut-core` creates a **transformed copy** of each body per
Setup frame: surfaces and curves are transformed analytically (D02 `transformed`) and the BVH is
rebuilt. Cost: milliseconds for typical parts; done once per Setup and cached by frame hash
(D01 §4). This keeps every algorithm in a fixed, simple frame.

## 5. Sections and outlines

### 5.1 Body section by plane

Used by roughing levels, waterline levels and pocket floors [S 04 §4.4–4.5].

```rust
pub fn section_plane(body: &BrepBody, plane: &Plane, tol: &Tol, cx: &JobContext)
    -> Result<SectionResult>;

pub struct SectionResult {
    pub loops: Vec<Loop2>,         // closed 2-D loops in plane coordinates (exact curves)
    pub open: Vec<Chain2>,         // open chains (sheet bodies)
    pub flats: Vec<FaceKey>,       // planar faces lying in the plane (tangential, CH-03)
}
```

```
for each face whose bounds straddle the plane (BVH query):
    curves = surface ∩ plane            (D02 §5)
    clip each curve to the face trims   (classify along the curve; exact edge intersections)
chain clipped pieces end-to-end: pieces meet at edge points; match within tol_model,
    preferring pieces that share the same B-rep edge
orient loops: outer CCW, holes CW (material on the left)
```

Planar faces coincident with the plane are returned in `flats`, not sectioned.

### 5.2 Body section by cylinder

Same as §5.1 with a cylinder coaxial with X (rotary roughing [S 04 §4.7.4]). Loops are returned
in unrolled (x, r·θ) coordinates with the θ seam handled as a periodic boundary.

### 5.3 Drive-face outline

The XY extent of selected drive faces [S 04 §4.12.2]:

```
outline(faces) = 2-D union of the projections of faces onto the XY plane
```

Computed as: boundary edges of the face set and silhouette curves (where n·z = 0 on the face)
projected to XY, then a 2-D boolean union of the resulting regions (arc-aware, via the same
offset/boolean library as D08 §6). See CH-05.

## 6. Persistent face ids

Face ids must survive re-import [S 03 §3.5].

```
PersistId(face) = hash(source file id,
                       STEP entity name if non-empty,
                       AP242 persistent id if present,
                       else geometric signature)
geometric signature = (surface kind, rounded parameters at 1e-6 relative,
                       rounded face area, rounded bounds)
```

On re-import, old and new faces are matched in order: exact persistent id; equal signature;
then best overlap (shared area by sampling) above 90 %. Unmatched selected faces are reported
[S 03 §3.5].

## 7. Display tessellation

Display only; never used by CAM (DC-02).

- Edges first: each edge tessellated once to the view chord tolerance, so neighbouring faces
  share vertices (watertight display).
- Faces: UV-space constrained Delaunay (`spade`) of the boundary polylines plus interior
  points, refined until the 3-D chord error < view tolerance. Planar faces need no interior
  points.
- Output: `Arc<DisplayMesh>` per face (positions, normals, indices) and line strips per edge,
  uploaded by the renderer (D13). Re-tessellated lazily when the zoom changes the required
  tolerance by more than ×4.

## 8. Tests

- Classification: random points on random trimmed faces of the STEP corpus vs high-resolution
  reference classification; points within 1e-7 of edges must give `OnBoundary`.
- Seam tests: loops crossing the seam on cylinders, spheres and tori.
- Section of standard solids (box, cylinder, cone frustum, sphere, torus) vs analytic results.
- BVH: query results equal brute force on random sets.
- Re-import: modify a STEP model (move a hole, add a fillet) and check matched faces.

## 9. Challenges

- CH-03 Tangential and degenerate plane sections (also D02).
- CH-05 Silhouette outline robustness for free-form faces (deferred).
- CH-06 Mesh region grouping threshold and selection quality (deferred).
