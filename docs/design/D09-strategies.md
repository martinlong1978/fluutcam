# D09 — v1 Strategy Designs (`fluut-cam::strategies`)

Implements: [S 04 §4.4–4.8, §4.13] for the 22 v1 types (OQ-17). Builds on D07 (queries) and
D08 (framework). For each type: drive pattern, special rules, parameters beyond
[S 04 §4.13.7], and risks.

Notation: R = tool radius, s = stepover, Δz = step-down, `region` = tool-centre region from
D08 §3, `layout` = D08 §6, `sample` = D08 §4.2, `fit` = D08 §5, `link` = D08 §8.

## 1. Type registry

```rust
pub fn registry -> &'static [&'static dyn ToolpathType] { &[
    &Face, &Adaptive2d, &Pocket2d, &Contour2d, &Trace, &Engrave, &Chamfer2d, &Bore,   // 2D
    &Drill,                                                                            // holes
    &Adaptive3d, &Pocket3d, &Parallel, &Contour, &Horizontal, &Spiral,                 // 3D
    &Scallop, &SteepAndShallow,
    &RotaryRough, &RotaryAlongX, &RotaryAroundA, &RotarySpiral,                        // rotary
    &ManualNc,                                                                         // utility
]}
```

## 2. 2-D types

2-D types work on `Region2` built from selected curves (WireBody), B-rep edges, or planar faces
(pocket floor faces give region + floor Z; walls give side boundaries). Depths: top and bottom
heights (D06 §3.1). Levels: `layout` over [bottom, top] with Δz (even mode), always including
the exact bottom.

### 2.1 `face`

```
area = stock top outline (Setup stock) offset by overlap (param) ∪ selected faces' outline
levels from stock top down to the face height (bottom), Δz, final pass at bottom
per level: zig-zag passes at `pass_angle`, layout(area, s), clip exact, extend by R beyond area
link: stay down between zig-zag passes (always clear: above the part)
```

### 2.2 `pocket_2d`

```
per level z (top → bottom):
    clear = region(z)                            (selected pocket boundary, islands as holes)
    if rest: clear ∩= stock_in.remaining(z) offset by R
    pattern offset: rings = successive inward offsets of clear by −R, −R−s, … until empty;
                    the first ring is the finishing boundary at exactly −R − stock_to_leave
    pattern raster: layout lines at pass_angle across clear; final contour pass at the boundary
    order rings inside-out (entry in the centre: ramp helix there) or outside-in (param)
stock_out = stock_in minus swept
```

Ring offsets come from `Region2::offset`; ring endpoints are not needed (closed loops), and
the boundary ring is exact (D08 §3.1). Where part of the region runs out less than a stepover
inside ring k (a thin band between a wall and an island, or the middle), the loops of the ring
at −(k + ½)·s through that part are added after ring k, so no point is more than s/2 from a
pass along the ring normals. Where rings from two boundaries merge at an angle θ, the merged
corner is up to (s/2)/sin(θ/2) from a pass; with s ≤ R that stays within R for θ ≥ 60°.

### 2.3 `adaptive_2d`

Constant-engagement clearing. The highest-risk v1 algorithm (CH-16).

```
inputs: clear region C at level z, target engagement angle φ* (from optimal load: φ* = acos(1 − load/R)),
        min cutting radius ρmin
state:  cleared region K (starts as: entry helix disc, or outside region if open pocket)
loop:
    find the boundary of K inside C; pick the start point nearest the current tool position
    march the tool centre p along a path that keeps engagement φ(p) = φ* :
        φ(p) = angular extent of the tool circle at p that lies in C \ K   (exact: circle ∩ region arcs)
        direction: follow the K boundary offset; step length ≤ min(R/4, curvature limit);
        corrector: adjust the offset distance by 1-D root finding so φ = φ*
        smooth: limit path curvature to 1/ρmin
    when the path closes or reaches the C boundary: K ∪= swept(path); link (stay down if inside K)
until C \ K is empty within tol
final pass: contour of C at stock_to_leave
```

Path points are designed (not contact points), so the DC-02 requirement applies to region
limits: start/end points on the C boundary are exact, and K is updated with exact arc geometry
(no pixel grid, unlike common implementations).

### 2.4 `contour_2d`

```
path = boundary curve offset by R (side = left/right; on = 0), tool compensation:
    computer: offset applied here; control: path on the curve, post emits G41/G42 (D10);
    wear: offset applied and G41/G42 emitted for wear adjustment
depths: layout(top, bottom, Δz) when multiple depths; finishing passes: offsets R + i·s_f, last at R
tabs: intervals along the path (positions as curve parameters, from param or evenly spaced);
      within a tab interval, below tab height, the path rises to tab top:
      z(t) = max(level z, tab_top) inside, with ramps at tab_angle at both ends; tab ends exact
leads: from linking group (D08 §8.3)
```

Open curves are machined end to end; closed curves start at the point nearest the previous end
(or a user-picked start point).

### 2.5 `trace`

Follows selected curves at their own Z (3-D curves) or at a set depth, with optional side offset
(2-D offset of the projected curve). Multiple depths as `contour_2d`. No region clipping beyond
the Setup box.

### 2.6 `engrave` (V-carve)

```
region = closed text/shape curves
M = medial axis of region (exact for lines and arcs: CH-17)
for each medial-axis branch with clearance radius ρ(t):
    depth(t) = min(ρ(t) / tan α, max_depth)          (α = tool half-angle)
    path point = medial point at Z = top − depth(t)
where ρ(t)/tan α > max_depth: flat area → left for a separate flat-bottom clearing toolpath;
    its boundary is emitted as a curve set the user can pick (S 04 §4.13.7)
corner sharpening (option): extend branches into convex corners until depth reaches top
fit: segments along branches (depth varies linearly on line bisectors, parabolic on line/arc
     bisectors → fitted within tol)
```

### 2.7 `chamfer_2d`

Tool: V or chamfer cutter with half-angle α. For an edge chamfer of horizontal width w at a 45°
edge, or generally a chamfer of width w for a tool angle matching the chamfer:

```
tip depth below the top edge   d = w / tan α + tip_offset
lateral offset of tool centre  o = d · tan α − w   (positive = away from the material)
path = edge curve offset by o (side away from material), at z = top − d
```

Fails with a diagnostic if `d` exceeds the tool's flute length or `tip_offset` would put the tip
into the part below.

### 2.8 `bore`

```
hole = selected cylindrical face (axis ∥ tool axis, else error) → centre c, radius rh, top, bottom (exact)
helix radius = rh − R − stock_to_leave;  error if ≤ 0
helix from top to bottom at `pitch` per turn (HelixSegment per turn, direction = climb by default)
bottom: full circle at bottom depth; finishing pass (optional): circle at rh − R
retract along the axis from the centre
```

## 3. Holes

### 3.1 `drill`

```
holes = selected cylindrical faces (or auto: all cylinders with axis ∥ Z whose diameter matches a
        filter), grouped by coaxial stacks (counterbores): one hole = axis, top Z, bottom Z, diameter
bottom: depth to cylinder bottom; if the bottom face is a cone, depth to the cone tip optionally;
        drill point compensation: + (D/2)/tan(point_angle/2) if "to full diameter" is chosen
order: shortest path (nearest neighbour + 2-opt) starting at the point nearest the last position
emit: one Cycle segment per hole with cycle kind, retract plane, peck, dwell (D10 §5 for output)
```

Diagnostics: holes whose diameter differs from the tool diameter by more than 5 % (wrong tool),
holes not parallel to the tool axis.

## 4. 3-D types

3-D types run in `three_axis` and `indexed` Setups on drive and check faces.

### 4.1 `pocket_3d` (roughing)

```
levels = layout over [stock top, region floor]: even Δz, plus exact flat-face heights (S 04 §4.4)
per level z (top → bottom):
    M = machinable(z)  = region \ (section(bodies, z) offset by R + stock_to_leave)    (D08 §7.1)
    clear = M ∩ stock_in.remaining(z) offset by R      (rest, or full stock)
    clear with the pocket_2d pattern (offset or raster) at level z
    entries: helix ramp where the clear region is not reachable from outside the stock
stock_out = stock_in minus swept
```

Flat levels use CH-03 handling: the section is taken just above the flat, so the level cuts to
the flat face's exact height without the section degenerating.

### 4.2 `adaptive_3d` (roughing)

As `pocket_3d`, but each level is cleared with the `adaptive_2d` algorithm (§2.3). "Fine
step-down" adds intermediate levels between main levels that only clear the stepped material left
on shallow areas (where `stock_in.remaining(z)` differs from the part section by more than the
fine step).

### 4.3 `parallel`

```
dir = (cos θ, sin θ) from pass_angle
positions = layout(region, s, normal(dir), pass_layout)
    where s = stepover, or from scallop height: s = worst-case over drive faces of
          sqrt(8 h R / (1 + R κ_max))   (OQ-05; κ_max = max convex normal curvature across passes,
          computed per face analytically or by sampling B-spline curvature)
per position (parallel, rayon): line = drive line at offset; intervals = region.clip_line(line)
    per interval: sample (D08 §4.2) with drop (D07) → CL points; direct emission where possible
slope limits: drop contacts whose surface normal angle to Z is outside [min, max] are removed;
    the cut-off points are found by bisection on the contact normal angle (exact to tol.solver)
direction: zig-zag (alternate) or one-way; climb/conventional only meaningful for one-way
```

### 4.4 `contour` (waterline)

```
levels: adaptive (S 04 §4.5): θ = acos((R − tol)/R) chord rule per wall slope, bounded by max Δz,
        plus exact flat-face heights
per level z (parallel):
    seeds = section(bodies, z) offset in XY by R      (exact for vertical walls; seeds otherwise)
    for each seed loop: trace the CL contour τ(x,y) = z (D07 §7):
        predictor: step along the tangent ⟂ ∇τ, length from contour curvature so chord ≤ tol/2
        corrector: Newton on τ(x,y) − z along ∇τ
        stop when back at start (closed) or at a region limit (exact clip) or slope limit
    loops nested by containment → order (outside first, or by area)
slope limits: keep only portions whose contact normal is steeper than min angle
```

On vertical cylinders, cones and tori the trace detects a single analytic contact face and
emits an arc directly (D08 §4.3).

### 4.5 `horizontal`

```
flats = planar faces among drive faces with normal within tol.angle of +Z (exact, from B-rep)
per flat (grouped by equal height): area = face region offset by −R (tool stays on the flat),
    minus areas where the tool collides with walls (section just above the flat, offset by R)
    clear with offset pattern at z = flat height + axial stock_to_leave
```

### 4.6 `spiral`

```
centre = param point, or centroid of region
r_max  = max distance from centre to region boundary (exact support)
drive  = Archimedean spiral r = s·φ/2π from 0 to r_max, as a G1 biarc chain within tol/4
         (designed pattern: approximation error is pattern, not part geometry)
clip spiral by region exactly (arc ∩ region boundary); sample CL along each kept piece
machine_corners: after the spiral, a contour pass along the region boundary to clear corners
direction: inside-out or outside-in
```

### 4.7 `scallop`

Passes at constant 3-D stepover, offset inward from the region boundary over the surface.

```
pass 0 = CL boundary loop (region boundary projected onto CL surface by drop sampling)
pass k+1 = offset of pass k by a variable 2-D distance s_xy(p) = s · cos(slope at p)
           (s = 3-D stepover; slope from the contact normal) so the 3-D distance between passes ≈ s
until empty; continuous option: connect consecutive loops into a spiral by blending over 1 step
```

Variable-distance offsetting is not supported by `cavalier_contours` (uniform offsets only).
Design: approximate each pass as an offset of the previous one by the minimum `s_xy` over that
pass (conservative: scallop never exceeds the target), then insert extra passes where the 3-D
gap exceeds `s` (CH-18).

### 4.8 `steep_and_shallow`

```
classify drive-face area by slope: steep if contact normal angle to Z ≥ threshold, else shallow
    (per face for analytic faces with constant slope; by sampling + bisection for others);
    shallow region in XY = projection of shallow parts, unioned, offset by `overlap`
steep part: contour (§4.4) restricted to steep areas (slope limit = threshold)
shallow part: parallel (§4.3) or scallop (§4.7) restricted to the shallow region
order: steep first by default (param)
```

## 5. Rotary types

Run in `rotary` Setups. Work frame: rotary axis = X, tool axis = radial (D07 §2 with
**a** = (0, sin θ, cos θ)). The drive domain is the unrolled cylinder (x, θ).

### 5.1 `rotary_rough`

```
radii = layout over [stock radius, min part radius in region] with radial Δr, plus exact radii of
        coaxial cylindrical faces (S 04 §4.7.3, like flat-face heights)
per radius r:
    section = section_cylinder(bodies, r) (D03 §5.2), unrolled to (x, r·θ)
    M = region (x range × full θ) \ (section offset by R + stock_to_leave, in unrolled coordinates)
        — offset in unrolled space is exact only for small R/r; see CH-19
    clear M with passes along X (θ steps) or around A (x steps), stepover s converted to Δθ = s/r
stock: cylindrical LayeredStock updated per radius
```

### 5.2 `rotary_along_x`

```
θ positions = layout over [0, 2π) (or θ range) with Δθ = s / r_max (or from scallop height)
per θ: drive line along X between region x limits; radial drop sampling → CL points (x, θ, r)
emit Rotary-frame Line segments (X and Z vary, A fixed per pass)
```

### 5.3 `rotary_around_a`

```
x positions = layout over region x range with s (or from scallop height)
per x: drive = full circle θ ∈ [0, 2π); radial drop sampling;
       if the contact stays on one coaxial face: single RotarySegment (S 04 §4.7.3)
       else: CL points (θ, r) fitted to Rotary segments with linear r interpolation within tol
link between rings: step in X at the current θ (stay down) or helical transition (param)
```

### 5.4 `rotary_spiral`

```
drive = helix in (x, θ): x = x0 + p·θ/(2π), p = advance per revolution (s or from scallop)
radial drop sampling along the helix;
coaxial cylinder: whole spiral = one RotarySegment with X linear in A (S 04 §4.7.3, cone similar)
ends exact at region x limits (solve x(θ) = x_end)
```

## 6. Utility

### 6.1 `manual_nc`

No geometry. Result: one pass with a `Marker` segment (D08 §2). The post emits the matching
block (stop `M0`, optional stop `M1`, comment, dwell, pass-through text; [S 05 §5.4.1]).

## 7. Validation rules (examples)

| Type | Rule |
|---|---|
| all | tool diameter > 0; stepover ≤ tool diameter; machining tolerance ≥ 1e-4 mm |
| `engrave`, `chamfer_2d` | V or chamfer tool required |
| `bore` | hole radius > tool radius + stock_to_leave |
| `drill` | tool type drill (or warning for end mill) |
| `adaptive_*` | optimal load ≤ 50 % of diameter (warning above 30 %) |
| rotary types | Setup kind = rotary; machine has a rotary axis |

## 8. Tests per strategy

- Golden toolpaths on a reference part set (prismatic block, pocketed plate, mould cavity,
  relief, turned shaft for rotary), compared by geometry (Hausdorff distance ≤ tol) not bytes.
- Gouge and endpoint checks (D08 §11) on every golden toolpath.
- Performance budget per strategy on the reference parts (tracked in CI).

## 9. Challenges

- CH-16 Adaptive clearing algorithm: robustness and speed (deferred).
- CH-17 Exact medial axis for lines and arcs (deferred).
- CH-18 Scallop variable-distance offsetting (deferred).
- CH-19 Offsets in unrolled rotary coordinates (deferred).
