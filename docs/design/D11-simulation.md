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
    pub frame: Frame,                 // model frame, axis-aligned to the stock box
    pub res: f64,                     // ray spacing (mm), from SimulationResolution
    pub rays: [DexelGrid; 3],         // X, Y, Z families
}
pub struct DexelGrid { nx: u32, ny: u32, cells: Vec<SmallVec<[Interval; 2]>> }
```

Memory: 200 × 200 × 50 mm at 0.1 mm ≈ 6 M rays ≈ 150 MB worst case. Resolution is chosen
automatically from the stock size and an memory budget (default 512 MB), and the user can
refine a region of interest.

The use of a grid here is allowed: simulation is not toolpath computation (DC-02). Verification
compares against **exact** faces (§5), so verdicts do not depend on the grid alone.

## 3. Material removal

All supported tools are convex solids of revolution. For a straight move, the swept volume is
the Minkowski sum of the tool and the move segment, which is convex.

```
for each segment (in program order, model frame):
    if Arc / Helix / Rotary: split into chords with sagitta ≤ res/4
    for each chord: V = tool ⊕ chord (convex)
        candidate rays = rays whose line meets V's AABB (per family)
        per ray: [t_in, t_out] = ray ∩ V by convex ray cast (GJK ray cast on support functions)
                 subtract [t_in, t_out] from the ray's intervals
```

- Support function of the tool: exact for flat, ball, bull, V and tapered profiles.
- Parallel over rays within a chord batch (`rayon`), chords applied in order per ray: chords are
  batched by spatial tiles so that each tile's rays are processed by one thread in program
  order (deterministic).
- Rotary Setups: moves are transformed to the model frame using the A angle at each chord end
  (the part rotates, so the tool moves around it).

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

## 8. Tests

- Single-move cases vs analytic swept volumes (ball along a line: capsule).
- Simulated stock after machining a pocket equals the pocket within resolution.
- Deliberate gouge (toolpath offset 0.1 mm into the part) is detected at the right location.
- Time estimate vs measured run times on a test machine (calibration data set; future).

## 9. Challenges

- CH-22 Simulation memory and speed for large stock at fine resolution (deferred).
