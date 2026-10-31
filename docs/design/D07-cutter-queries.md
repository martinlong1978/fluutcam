# D07 — Tool Model and Cutter-Contact Queries (`fluut-cam::query`)

Implements: [S 04 §4.2, §4.7.2, §4.14], DC-01, DC-02. Depends on: `fluut-geom`, `fluut-topo`.

This is the kernel every strategy calls. It answers: "with the tool axis along **a** and the tool
centre line through point **q**, how far along **a** can the tool tip go before touching the
geometry, and where does it touch?"

## 1. Tool model

```rust
pub struct ToolAssembly {
    pub cutter: CutterProfile,
    pub parts: Vec<RevPart>,        // non-cutting parts, bottom to top
}

pub struct CutterProfile {          // generalised APT cutter [S 04 §4.2.1]
    pub diameter: f64,              // D
    pub corner_radius: f64,         // r  (0 = flat, D/2 = ball)
    pub taper: f64,                 // α half-angle (0 = straight)
    pub tip_diameter: f64,          // for V / tapered tools
    pub flute_length: f64,
}

pub struct RevPart {                // frustum between heights z0 < z1 above the tip
    pub z0: f64, pub z1: f64,
    pub r0: f64, pub r1: f64,
    pub kind: PartKind,             // Flute (non-cutting) | Neck | Shank | Holder
}
```

### 1.1 Lower-profile height function

For every tool, define `h(d)`: the height above the tip of the tool's lower surface at radial
distance `d` from the axis (0 ≤ d ≤ R, R = D/2). With `Rf = R − r` (flat part radius):

```
flat / bull / ball (α = 0):
    h(d) = 0                                 for d ≤ Rf
    h(d) = r − sqrt(r² − (d − Rf)²)          for Rf < d ≤ R
V / tapered (tip radius Rt, half-angle α):
    h(d) = 0                                 for d ≤ Rt
    h(d) = (d − Rt) / tan α                  for Rt < d ≤ R      (corner radius blends if r > 0)
```

`h` is continuous, monotone non-decreasing, and smooth except at `d = Rf` and `d = Rt`.

## 2. The unified drop formulation

For a tool with axis direction **a** (unit, pointing from tip to shank) and centre line through
**q**, the tool can move along −**a** until it touches. The tip position along the axis is:

```
τ(q, a) = max over geometry points p with d(p) ≤ R of   ( p·a − h(d(p)) )
where   d(p) = | (p − q) − ((p − q)·a) a |        (lateral distance to the axis)
```

The maximising point is the contact point. This single formula covers:

- 3-axis drop-cutter: **a** = +Z, q = (x, y, ·) [S 04 §4.2];
- rotary radial drop: **a** = radial unit vector at angle θ, q on the line through (x, θ)
  [S 04 §4.7.2], result r = τ;
- holder and shank checks with another profile function (§6).

All contact cases (face interior, edge, vertex [S 04 §4.2.2]) are the same maximisation
restricted to 2-D, 1-D and 0-D sets.

```rust
pub struct DropQuery<'a> {
    pub axis: UnitVec3,                 // a
    pub through: Point3,                // q
    pub profile: &'a dyn HeightProfile, // h(d), R
    pub floor: f64,                     // ignore contacts below this τ (region floor)
}

pub struct Contact {
    pub tau: f64,
    pub point: Point3,
    pub on: ContactOn,                  // Face{key, uv} | Edge{key, t} | Vertex{key} | Tri{idx, bary} | Floor
}

pub fn drop(geom: &QueryGeometry, q: &DropQuery, tol: &Tol) -> Result<Contact, QueryError>;
```

`QueryGeometry` = the Setup frame's bodies restricted to drive + check faces, minus excluded
faces [S 03 §3.3], with BVH.

## 3. Search: branch and bound over primitives

```
best = Contact::floor(q.floor)
cands = bvh.query_cylinder(axis a, line through q, radius R)            (D03 §4.2)
for c in cands: c.bound = upper bound of (p·a − h(d)) over c's AABB
                         = max over box corners of p·a  −  h(min lateral distance of box)
sort cands by bound descending
for c in cands:
    if c.bound ≤ best.tau + tol.solver: break           // nothing left can beat best
    r = solve(c)                                        // §4
    if r.tau > best.tau: best = r
return best
```

Most queries touch one or two primitives after sorting, so cost is dominated by one solve.

## 4. Per-primitive solvers

### 4.1 Vertex (0-D)

Closed form: `τ = p·a − h(d(p))` if d(p) ≤ R.

### 4.2 Edge (1-D)

Maximise `f(t) = C(t)·a − h(d(C(t)))` over the edge's parameter range clipped to the footprint
(`d ≤ R`).

| Edge curve × tool | Method |
|---|---|
| Line × ball | closed form (cylinder of radius R around the line ∩ axis line → quadratic) |
| Line × flat | closed form (ellipse in the plane through the line) |
| Line × bull, V | 1-D: stationary point of f on each smooth piece of h (Brent's method on f′, brackets from the pieces) |
| Circle, ellipse, B-spline × any | 1-D: split the range at footprint entry/exit and at h-kinks (where d = Rf, Rt); on each piece, find stationary points of f (Brent on f′, with sign-change seeding from 16 samples); compare with piece ends |

Endpoints of the range are vertices (covered by §4.1), so the solver only reports interior
maxima.

### 4.3 Face interior (2-D)

Maximise `F(u,v) = S(u,v)·a − h(d(S(u,v)))` over the face, interior points only (boundary maxima
are edge contacts).

**Analytic fast paths** [S 04 §4.2.3]. For a ball tool, the interior contact is where the
offset surface S + R·n meets the axis line; for analytic faces this is the analytic offset
(D02 §4.3) ∩ line → closed form (plane: linear; cylinder, cone, sphere: quadratic; torus:
quartic). For flat and bull tools on planes and on cylinders/cones parallel or perpendicular to
**a**: closed form. Every closed-form solution is checked with the trim classifier (D03 §3);
`OnBoundary` or `Out` results are discarded (the edge case covers them).

**General case** (free-form faces, other combinations). Stationary point of F:

```
∇F = 0  ⇔  the surface normal n at the contact, the axis a, and the tool's lower-surface
          normal at radius d are consistent:
          contact point P = S(u,v); with m = unit lateral direction of (P − q):
          tool surface normal at d:  t(d) = (−h′(d)·m + a) / |…|
          condition: n(u,v) ∥ t(d)   (two equations)
```

Solve with damped 2-D Newton in (u,v), seeded from:

1. the closest point on the face to the tool's lowest point at the current best τ (D02);
2. for B-spline faces, the centre of each Bézier patch in the candidate (D03 §4.1).

Converged when the 3-D step < `tol.solver` and the residual angle < `tol.angle`. Check the
trim. If no seed converges for a candidate whose bound exceeds the current best, return
`QueryError::NoConvergence { face, q }`: the caller reports it at the location rather than
emitting a gouging point [S 04 §4.2.4].

### 4.4 Triangles (mesh bodies)

Classic drop-cutter cases: triangle plane (closed form per tool), three edges (§4.2 line cases),
three vertices (§4.1). Barycentric test for the plane contact.

## 5. Transitions between contacts

Along a drive curve, the contact moves from primitive to primitive. Where it changes (e.g. from
face A interior to edge e), the CL path has a kink or a change of curve type. Strategies need the
exact transition parameter [S 04 §4.12.3].

```rust
pub fn transition(geom, curve: &DriveCurve, t0: f64, c0: &Contact, t1: f64, c1: &Contact,
                  tol: &Tol) -> (f64, Contact);
```

```
// c0 on primitive P0 at t0, c1 on P1 at t1, P0 ≠ P1
solve g(t) = τ_P0(t) − τ_P1(t) = 0 on [t0, t1]   (τ_Pi = drop restricted to primitive i)
Brent's method to |Δt|·|curve'| ≤ tol.solver
```

Restricting to a primitive gives smooth functions, so the root is well-defined. If more than
two primitives are involved, bisect the interval first.

## 6. Holder and shank collision [S 04 §4.14]

A non-cutting part is a frustum `RevPart {z0, z1, r0, r1}`. Treat it as a "tool" whose lower
profile is `h_part(d) = z0 + (z1 − z0)·max(0, d − r0)/(r1 − r0)` (flat bottom at z0 out to r0,
conical to r1), radius `max(r0, r1)`.

```
collides(part, q, a, τ_tip) =
    drop(geom, profile = h_part, through = q, axis = a).tau  >  τ_tip + z0 − ε
```

That is: if the part, placed at the height it has when the tip is at τ_tip, would have to sit
higher, it intersects the model. The same branch-and-bound machinery applies, so this costs about
one extra drop per part.

Against the in-process stock (layered 2-D regions, D08 §7): for each stock layer between the
part's z0 and z1 (in work frame), test whether the disc of the part's radius at that height
intersects the remaining region (2-D distance from the axis point to the region, D08 §6).

**Collision zones along a pass**: `collides(t)` is evaluated with the adaptive sampling of D08;
where it changes value between samples, the transition parameter is found by bisection to
`tol.solver` (the predicate is not smooth, so Brent does not apply). These are the exact trim
points of [S 04 §4.14].

## 7. Push queries for waterline

Waterline (D09 §3.4) needs the CL contour at fixed tip height z: the set of (x, y) where
τ(x, y) = z. The design marches along that contour rather than building fibre grids:

```rust
pub fn level_residual(geom, xy: Point2, z: f64) -> (f64, Vec2);   // τ(xy) − z and ∇τ (from contact normal)
```

∇τ follows from the contact: for an interior face contact with surface normal n,
`∇τ = −(n_x, n_y)/n_z` (generalised for edges and vertices via the tool surface normal at the
contact). The waterline tracer in D09 uses predictor–corrector on `level_residual = 0`.

## 8. Performance targets (single thread)

| Query | Target |
|---|---|
| Drop, ball, analytic part (1 000 faces) | ≤ 2 µs median |
| Drop, bull, B-spline faces | ≤ 20 µs median |
| Drop, mesh 1 M triangles | ≤ 5 µs median |
| Holder check (3 parts) | ≤ 3× drop |

## 9. Tests

- Brute-force reference: dense sampling of every primitive (10⁶ points) for τ; solver result
  must be ≥ reference − 1e-9 and within 1e-7 of it (the solver may only find a higher, more
  exact maximum).
- Analytic: ball on sphere, cylinder, torus; flat on tilted plane; V-tool on edges — compared
  with hand-derived closed forms.
- Mesh vs B-rep: a fine tessellation of a B-rep part gives τ within chord error of the B-rep τ.
- Transition: synthetic two-face cases with known kink positions.
- Gouge property: for random q, no geometry point lies strictly inside the tool at τ (sampled).

## 10. Challenges

- CH-12 Free-form face contact convergence and seeding (deferred).
