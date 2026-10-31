# D02 — Geometry Core (`fluut-geom`)

Implements: [S 01 §1.4–1.5], [S 03 §3.2.3–3.2.4], [S 04 §4.2.3, §4.3.4]. Depends on: `glam`,
`libm`, `smallvec`. No I/O, no threads.

## 1. Basic types

The minor open item "linear algebra crate" [S README] is settled here: **`glam`** (`DVec3`,
`DQuat`, `DMat4`). It is small, fast for 3D, MIT/Apache, and has no generic-dimension overhead.
`fluut-geom` wraps it so that the rest of the code base does not depend on `glam` directly.

```rust
#[derive(Copy, Clone, PartialEq, Debug)] pub struct Point3(pub DVec3);
#[derive(Copy, Clone, PartialEq, Debug)] pub struct Vec3(pub DVec3);
#[derive(Copy, Clone, PartialEq, Debug)] pub struct Point2(pub DVec2);
#[derive(Copy, Clone, PartialEq, Debug)] pub struct Vec2(pub DVec2);
pub struct UnitVec3(Vec3);                 // invariant: |v| = 1 ± 1e-12

pub struct Frame {                         // STEP AXIS2_PLACEMENT_3D
    pub origin: Point3,
    pub z: UnitVec3,                       // axis
    pub x: UnitVec3,                       // reference direction, ⟂ z
}                                          // y = z × x

pub struct Rigid { rot: DQuat, trans: DVec3 }          // rotation + translation
pub struct Affine { m: DMat3, trans: DVec3 }           // + scale / mirror (Setup scale, S 03 §3.9.3)

pub struct Aabb3 { pub min: Point3, pub max: Point3 }
pub struct Interval { pub lo: f64, pub hi: f64 }
```

`Point3` and `Vec3` are distinct so that transforms apply translation to points only.
`Affine` tracks whether it reverses orientation (`det < 0`, mirror); applying it to a face flips
the face's sense (D03 §2).

## 2. Tolerances

```rust
pub struct Tol {
    pub model: f64,      // modelling tolerance: gap treated as coincident (per body, S 01 §1.5)
    pub machining: f64,  // max deviation of CL path (per toolpath)
    pub solver: f64,     // ε_s = min(1e-6, machining / 1000)  (S 04 §4.12.1)
    pub angle: f64,      // radians; parallel/perpendicular tests, default 1e-9
}
```

Every function that compares geometry takes the tolerance it needs as an argument. There are no
global epsilons. Display tessellation uses its own chord tolerance and never a `Tol`.

## 3. Curves

### 3.1 Representation

Closed set of kinds, so an enum (static dispatch, `match`), not trait objects:

```rust
pub enum Curve3 {
    Line    { origin: Point3, dir: UnitVec3 },
    Circle  { frame: Frame, radius: f64 },
    Ellipse { frame: Frame, a: f64, b: f64 },
    BSpline(Arc<BSplineCurve3>),            // includes rational; Bezier as special case
    Offset  { basis: Box<Curve3>, dist: f64, dir: UnitVec3 },
}

pub struct BSplineCurve3 {
    pub degree: u8,
    pub knots: Vec<f64>,                    // full (expanded) knot vector, non-decreasing
    pub poles: Vec<Point3>,
    pub weights: Option<Vec<f64>>,          // None = non-rational
    pub periodic: bool,
}

pub struct TrimmedCurve { pub curve: Curve3, pub range: Interval, pub same_sense: bool }

pub enum Curve2 {                           // p-curves in (u,v) and 2D wire geometry
    Line { origin: Point2, dir: Vec2 },
    Circle { centre: Point2, radius: f64, x_dir: Vec2 },
    Ellipse { centre: Point2, a: f64, b: f64, x_dir: Vec2 },
    BSpline(Arc<BSplineCurve2>),
}
```

STEP `PARABOLA`, `HYPERBOLA`, `POLYLINE` and `TRIMMED_CURVE` are converted on import
(parabola/hyperbola → rational B-spline exactly; polyline → chain of `Line` trims;
[S 03 §3.2.4]).

### 3.2 Operations

```rust
impl Curve3 {
    pub fn eval(&self, t: f64) -> Point3;
    pub fn derivs(&self, t: f64, n: usize) -> SmallVec<[Vec3; 4]>;    // C, C', C'', C'''
    pub fn natural_range(&self) -> Option<Interval>;                   // None = unbounded line
    pub fn closest(&self, p: Point3, range: Interval, tol: &Tol) -> (f64, f64); // (t, dist)
    pub fn bounds(&self, range: Interval) -> Aabb3;
    pub fn length(&self, range: Interval, tol: &Tol) -> f64;
    pub fn param_at_length(&self, range: Interval, s: f64, tol: &Tol) -> f64;
    pub fn tessellate(&self, range: Interval, chord: f64) -> Vec<(f64, Point3)>;
    pub fn transformed(&self, a: &Affine) -> Curve3;
    pub fn as_circle(&self) -> Option<(&Frame, f64)>;                  // fast paths (S 04 §4.3.1)
}
```

Algorithms:

| Operation | Line / circle / ellipse | B-spline |
|---|---|---|
| eval, derivs | closed form | de Boor (A2.2/A2.3 in *The NURBS Book*); rational via homogeneous coordinates and quotient rule |
| closest | closed form (ellipse: 1-D Newton on a robust bracket) | seed: closest of Bezier-span control-polygon projections; refine: Newton on `(C(t)-p)·C'(t)=0` with bisection fallback; check span ends |
| bounds | closed form | convex hull of poles of the range (after knot insertion at range ends) |
| length | closed form (ellipse: adaptive Gauss–Legendre) | adaptive 5-point Gauss–Legendre per span, to `tol.solver` |
| tessellate | angular step from chord: `Δθ = 2·acos(1 − chord/r)` | adaptive midpoint subdivision on chord error |

## 4. Surfaces

### 4.1 Representation

```rust
pub enum Surface {
    Plane     { frame: Frame },                                  // u along x, v along y
    Cylinder  { frame: Frame, r: f64 },                          // u = angle, v = along z
    Cone      { frame: Frame, r: f64, semi_angle: f64 },         // r at v=0; v along generatrix
    Sphere    { frame: Frame, r: f64 },                          // u = longitude, v = latitude
    Torus     { frame: Frame, major: f64, minor: f64 },          // u major angle, v minor angle
    Extrusion { profile: Box<Curve3>, dir: UnitVec3 },           // S(u,v) = C(u) + v·d
    Revolution{ profile: Box<Curve3>, axis_origin: Point3, axis: UnitVec3 },
    BSpline(Arc<BSplineSurface>),
    Offset    { basis: Box<Surface>, dist: f64 },
}

pub struct BSplineSurface {
    pub deg_u: u8, pub deg_v: u8,
    pub knots_u: Vec<f64>, pub knots_v: Vec<f64>,
    pub poles: Vec<Point3>,              // row-major nu × nv
    pub weights: Option<Vec<f64>>,
    pub periodic_u: bool, pub periodic_v: bool,
}
```

Parametrisations follow STEP (ISO 10303-42) exactly, so that p-curves from STEP files are valid
without conversion. Degenerate torus (STEP `DEGENERATE_TOROIDAL_SURFACE`, major < minor) is a
`Torus` with a `select_outer` flag on the face (D03).

### 4.2 Operations

```rust
impl Surface {
    pub fn kind(&self) -> SurfaceKind;
    pub fn eval(&self, u: f64, v: f64) -> Point3;
    pub fn derivs(&self, u: f64, v: f64) -> SurfDerivs;       // S, Su, Sv, Suu, Suv, Svv
    pub fn normal(&self, u: f64, v: f64) -> Option<UnitVec3>; // None at singular points (cone apex, sphere pole)
    pub fn curvatures(&self, u: f64, v: f64) -> Curvatures;   // k1, k2, principal dirs
    pub fn closest(&self, p: Point3, uv_box: UvBox, tol: &Tol) -> SurfClosest;
    pub fn offset(&self, d: f64) -> Surface;                  // analytic when possible (§4.3)
    pub fn periodicity(&self) -> (Option<f64>, Option<f64>);  // u and v periods
    pub fn transformed(&self, a: &Affine) -> Surface;
    pub fn bezier_patches(&self) -> Option<Vec<BezierPatch>>; // B-spline only; used by BVH (D03)
}
```

| Operation | Analytic surfaces | B-spline / extrusion / revolution |
|---|---|---|
| eval, derivs | closed form | B-spline: tensor de Boor (A3.6), rational quotient rule; extrusion/revolution: from profile curve derivs |
| curvatures | closed form (plane 0; cylinder 1/r; sphere 1/r; torus per point) | first and second fundamental forms |
| closest | closed form: project to axis/centre frame | seeds from patch control nets; 2-D Newton on `(S-p)·Su = (S-p)·Sv = 0` with step limit and UV clamping; boundary fallback to edge curves |
| offset | §4.3 | `Offset` wrapper, evaluated as `S + d·n` |

### 4.3 Analytic offsets

Used by ball-nose fast paths [S 04 §4.2.3] and by sectioning CL surfaces. `d` is signed along
the face normal (D03 handles face sense).

| Surface | Offset by d | Valid while |
|---|---|---|
| Plane | plane, origin + d·z | always |
| Cylinder r | cylinder r + d | r + d > 0 |
| Cone (r, α) | cone (r + d·cos α, α), origin shifted −d·sin α along z | not passing apex |
| Sphere r | sphere r + d | r + d > 0 |
| Torus (R, ρ) | torus (R, ρ + d) | ρ + d > 0 |
| Extrusion / Revolution / B-spline | `Offset` wrapper | numerically evaluated; may self-intersect (CH-04) |

When the "valid while" condition fails, the offset degenerates (e.g. concave cylinder radius
smaller than the tool). The caller treats the face as unreachable for that tool (S 04 §4.3.4,
reported as rest material).

## 5. Intersections needed by CAM

Only the intersections CAM needs are implemented; there is no general surface/surface
intersector.

| Intersection | Used by | Method |
|---|---|---|
| Plane ∩ analytic surface | roughing, waterline sections [S 04 §4.4–4.5] | closed form: line(s), circle, ellipse, conic → exact `Curve3` |
| Plane ∩ B-spline / extrusion / revolution | same | marching (§5.1) to `machining/2`, then fitted to `BSplineCurve3` or arcs |
| Cylinder (coaxial with X) ∩ surface | rotary roughing layers [S 04 §4.7.4] | analytic for coaxial surfaces of revolution; marching otherwise |
| Line ∩ curve (2D) | pass endpoints on region limits [S 04 §4.12.3] | closed form for lines/arcs; Bezier clipping + Newton for B-splines |
| Curve ∩ curve (2D) | boundary building, pocket regions | same |

### 5.1 Plane–surface marching

```
input: surface S on face UV domain D, plane P (n·x = h), step tolerance tol
1. Seeds: subdivide D (B-spline: Bezier patches) until each cell's control net is either
   entirely on one side of P (discard) or flat within tol; collect sign-change cells.
2. From each seed solve S(u,v)·n = h with Newton on the cell → start point.
3. March: tangent t = n × N(u,v) projected to UV; step length from curvature so chord ≤ tol/2;
   corrector: Newton back onto the plane.
4. Stop at D's boundary (trim handled by D03), at the start point (closed loop), or at a
   singularity (|∇(S·n)| < ε: tangential contact) → report point.
5. Fit result: if all points lie on a circle/line within tol/2 → exact type; else B-spline fit.
```

Tangential contacts (plane touching a flat face top, or tangent to a cylinder) are special:
see CH-03.

## 6. Root finding and robustness

- Quadratic: numerically stable form (`q = −½(b + sign(b)√disc)`).
- Cubic, quartic (torus): closed form (Cardano / Ferrari) for seeds, then Newton polishing on the
  original polynomial; double roots detected by derivative test and merged within `tol.solver`.
- 1-D Newton: always inside a bracket; falls back to bisection if a step leaves the bracket or
  does not decrease |f|. Max 50 iterations; failure is returned, never hidden
  [S 04 §4.2.4].
- 2-D Newton (closest point, contact): damped step, UV clamped to the domain, converged when the
  3-D step < `tol.solver` and residual < `tol.solver`.

## 7. Biarc and arc fitting

Needed by: 2D offsetting of B-spline boundaries [S 04 §4.8], segment fitting [S 04 §4.3.3].

- **Biarc fit of a curve** (tolerance τ): split the curve at inflections and at points where
  tangent turns > 90°; fit a biarc per piece with the equal-chord rule; measure max deviation by
  sampling plus local Newton; split at the worst point if > τ. Output: G1-continuous chain of
  lines and arcs.
- **Point-sequence fitting** (D08 §5): greedy longest-run line/arc fit within τ, with arc test by
  least-squares circle (Kåsa) followed by max-deviation check against the actual samples.

## 8. Performance targets (single thread, release build)

| Operation | Target |
|---|---|
| eval: analytic surface | < 20 ns |
| eval: bicubic B-spline surface (with derivs to 1st order) | < 250 ns |
| closest point: analytic | < 50 ns |
| closest point: bicubic B-spline | < 5 µs median |
| plane ∩ cylinder/cone/sphere | < 200 ns |

## 9. Tests

- Property tests (`proptest`): `closest(eval(u,v)) ≈ (u,v)`; offset-then-closest distance = |d|;
  transformed-then-evaluated = evaluated-then-transformed; rational B-spline circle equals exact
  circle within 1e-12.
- Analytic offset vs numeric `Offset` wrapper agree within 1e-10.
- Curvature of cylinder/sphere/torus vs fundamental-form computation on the equivalent
  B-spline.
- Root finders: random polynomials with known roots, including double roots.
- Benchmarks (`criterion`) for §8 targets, tracked in CI.

## 10. Challenges

- CH-03 Tangential and degenerate plane sections (deferred).
- CH-04 Self-intersecting offsets of free-form surfaces (deferred).
