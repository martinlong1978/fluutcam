# D11 — Simulation, Verification and Time Estimate (`fluut-sim`)

Implements: [S 04 §4.10, §4.14 (verification)], [S 08 §8.7], DC-02 (simulation never feeds
toolpaths). Depends on: `fluut-geom`, `fluut-topo`, `fluut-path`, `rayon`.

## 1. Purpose

Simulation shows the material removed by the program and verifies it: gouges, rest material,
holder and shank contact with stock, rapid moves through stock, cuts beyond flute length. It is
display and verification only; nothing it computes changes a toolpath [S 01 §1.2a].

## 2. Stock representation: tri-dexel

The stock is a **tri-dexel** model: three orthogonal families of rays (along X, Y and Z) on
regular grids, each ray storing a sorted list of material intervals. It works for any tool
orientation, so one stock carries through rotary and indexed Setups in the model frame.

```rust
pub struct TriDexel {
    pub res: f64,                     // ray spacing (mm), from SimulationResolution
    pub origin: DVec3,                // stock box minimum, in the stock frame
    pub grids: [DexelGrid; 3],        // X, Y, Z families
}
pub struct DexelGrid { axis: usize, nu: usize, nv: usize, rays: Vec<SmallVec<[Span; 2]>> }
pub struct Span { lo: f32, hi: f32 }  // offsets from origin along the ray
```

The stock frame is the work frame for three-axis Setups. For rotary Setups it is the frame
before the A rotation: the rotary axis is its X axis, and `p_W = R_x(A)·p_S` (spec 03 §3.9.3).
Spans are `f32` offsets (resolution 1.5e-5 mm at 200 mm), so a ray header is 24 bytes and a
ray with up to two spans needs no heap.

Memory: 24 bytes per ray plus 8 bytes per span beyond the second. 200 × 200 × 50 mm at
0.1 mm is 6 M rays ≈ 144 MB before cutting; machining spills 5–10 % of the rays to the heap
(S5: +20 % memory). Memory grows as 1/res². Resolution is chosen
automatically from the stock size and an memory budget (default 512 MB), and the user can
refine a region of interest.

The use of a grid here is allowed: simulation is not toolpath computation (DC-02). Verification
compares against **exact** faces (§5), so verdicts do not depend on the grid alone.

## 3. Material removal

All supported tools are convex solids of revolution: `disk(R − r) ⊕ axis segment ⊕ ball(r)`
(flat r = 0, ball r = R, bull between). The shank is straight and is cut off just past the
stock (`Stock::extent_along`), which changes no result and keeps candidate rays few. For a
straight move with a fixed axis, the swept volume is `tool ⊕ chord`, which is convex.

```
for each segment (in program order, stock frame):
    Arc / Helix / Spline: split into chords with sagitta ≤ res/4 at the tool tip
    moves with a changing A (Rotary, 4-axis lines): split so the sagitta at the stock's
        largest radius is ≤ res/4; each chord is the hull of its two end poses
    for each chord V:
        candidate rays = rays whose line meets V's AABB and that still have material there
        per ray: [t_in, t_out] = ray ∩ V; subtract from the ray's spans
```

Ray ∩ V uses the cheapest exact method (S5 measured the cost per ray):

| Case | Method | ns per ray |
|---|---|---|
| line along a fixed tool axis, ball | one capsule (V is the bottom capsule translated along the line) | 25 |
| line along a fixed tool axis, flat/bull | 1-D convex minimum of the bottom profile over the chord | 55 / 300 |
| line across a fixed axis on a level chord, flat/bull | 2-D capsule of the tool's radius at the line's height | 60 |
| ball, any other line (including rotary chords) | hull of four sphere centres ⊕ ball: spheres, edge cylinders, thickened triangles | 90–300 |
| flat/bull, any other line | GJK ray cast on the support function, to 1e-4 mm | 2 500–3 300 |

- Parallel over rays (`rayon`), deterministic: chords are taken in batches of 32 768; for each
  family, rows of rays are split into tiles of two rows, and each tile applies the batch's
  chords in program order. The stock is bit-identical on any thread count.
- Rotary Setups: the tool tip at machine `xyz, A` is at `R_x(−A)·xyz` in the stock frame and
  its axis is `R_x(−A)·Z`.

## 4. Playback and scrubbing

- Checkpoints: a copy-on-write snapshot of the stock after each toolpath (tiles shared when
  unchanged).
- Scrubbing to time *t* [S 08 §8.7]: restore the latest checkpoint before *t*, re-run the
  segments up to *t*. Typical re-run < 100 ms because a toolpath is short relative to the whole
  program.
- Display: mesh extracted per tile by dual contouring on the tri-dexel intervals, updated only
  for dirty tiles. Optional colouring by deviation from the part (§5).

## 5. Verification

| Check | Method | Report |
|---|---|---|
| Gouge | for stock surface sample points (interval ends), signed distance to exact faces (BVH closest point, D02/D03); inside the part by more than `tol.machining` → gouge | error with location, toolpath and segment |
| Rest material | stock surface points farther than stock_to_leave + tolerance outside the part | heat-map display; summary volume |
| Holder/shank contact | at each chord, ray cast the non-cutting parts' convex hulls against stock intervals | error with location |
| Rapid through stock | rapid moves: swept volume ∩ stock non-empty | error |
| Flute length | cutting contact height above tip > flute length | warning |

Gouge and rest checks use exact geometry for the distance, and the dexel resolution only for
where samples are taken [S 04 §4.10].

## 6. Time estimate

Used by the timeline [S 08 §8.7], toolpath stats and scripts.

```
for each segment: path length L, programmed feed F (resolved, mm/min) or machine rapid feed
                  axis-limited max speed v_max = min over axes of (axis max feed / |axis component|)
junction speed between segments: v_j = min(v_max(a), v_max(b), junction-deviation limit)
    junction deviation (GRBL-style): v_j² = a_max · δ · sin(θ/2) / (1 − sin(θ/2)),  δ = 0.01 mm default
per segment: trapezoidal (or triangular) profile from v_j_in to v_j_out with axis-limited accel a_max
time += profile duration;  dwell and tool-change times from machine definition
result × machine correction factor
```

Rotary axes: angular speeds and accelerations from the machine definition; rotary moves use the
slowest axis. Arcs: centripetal limit `v ≤ sqrt(a_max · r)`.

## 7. Performance targets

| Case | Target (8 cores) |
|---|---|
| 1 M short linear moves, 0.2 mm resolution, 150 × 100 × 40 mm stock | < 20 s full simulation |
| Scrub within one toolpath | < 150 ms |
| Time estimate, 1 M segments | < 1 s |

S5 measured (4 cores, synthetic stand-ins, [report](../plan/spikes/s5-simulation.md)):
- **Relief finish at 0.2 mm:** 33–47 µs per move, about 35–50 s per million moves. That is near the target on 8 cores.
- **Rotary wrap at 0.2 mm:** 221 µs per move, about 6× over the target. Rotary Setups should use a cylindrical dexel layout (radial rays along the tool axis); this is to be measured in M7.

Error between rays depends on the sharpness of the machined detail. The relief needs 0.1 mm for 0.05 mm error; the smooth rotary wrap needs 0.4 mm.

## 8. Tests

- Single-move cases vs analytic swept volumes (ball along a line: capsule).
- Simulated stock after machining a pocket equals the pocket within resolution.
- Deliberate gouge (toolpath offset 0.1 mm into the part) is detected at the right location.
- Time estimate vs measured run times on a test machine (calibration data set; future).

## 9. Challenges

- CH-22 Simulation memory and speed for large stock at fine resolution (deferred; S5 measured it, rotary speed is open).
