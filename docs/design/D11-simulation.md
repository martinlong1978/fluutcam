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

Rotary Setups use the same tri-dexel. S5 tried cylindrical dexels (radial rays on an (x, θ)
grid plus X rays). They were 14–27 % faster with the same error but needed 1.4× the memory, so
they were not adopted ([S5 follow-ups](../plan/spikes/s5-simulation.md#follow-ups-2026-11-01)).

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
| line across a fixed axis on a ramp or plunge, flat/bull | the tool's section at the line's height is a disc moving along the chord. On the straight flank it sweeps a 2-D capsule; in a bull's corner bands, golden-section search on a convex 1-D function | 70 / 150 |
| ball, any other line (including rotary chords) | first the bottom and top capsules: an end whose normal points away from both axes (down on the bottom capsule, up on the top) lies on the hull; else the hull of four sphere centres ⊕ ball | 150–300 |
| flat/bull, rotary chords and lines neither along nor across the axis | GJK ray cast on the support function, to 1e-4 mm | 2 500–3 300 |

- Parallel over rays (`rayon`), deterministic: chords are taken in batches of 32 768; for each
  family, rows of rays are split into tiles of two rows. Only tiles that some chord of the batch
  reaches are scheduled, and each applies the batch's chords in program order. The stock is
  bit-identical on any thread count.
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
| Index turn | before an indexed Setup: the start stock turned a full revolution about X (per Z ray its largest radius + the ray spacing) against the tool assembly where the post leaves it for the turn, as a vertical cylinder of its largest radius from the tip up | error `sim.index_turn` on the Setup's first toolpath |
| Fixture | indexed Setups: the cutter (and a wider shank) from the tip up, and the holder from the overall length up, as vertical cylinders, against the Setup's fixtures that turn with A; segment ends and lines every millimetre | error `sim.fixture` on the toolpath |

Gouge and rest checks use exact geometry for the distance, and the dexel resolution only for
where samples are taken [S 04 §4.10].

The index turn check (2026-11-02, [plan](../plan/2026-11-02-index-clearance.md)) takes the tool at
the end of the previous Setup's last toolpath, raised to the turn height the post uses (D08 §7).
It does not trust the turn radius: it measures the simulated stock. It is skipped when the
previous Setup is `three_axis` (its frame is not on the axis) or has no toolpath.

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

S5 measured (4 cores, synthetic stand-ins, [report](../plan/spikes/s5-simulation.md), after the
[follow-ups](../plan/spikes/s5-simulation.md#follow-ups-2026-11-01)):
- **Relief finish at 0.2 mm:** 34 µs per move after roughing, 59 µs from full stock. That is near the target on 8 cores.
- **Rotary wrap at 0.2 mm:** 117 µs per move, about 3× over the target. Cylindrical dexels gave only 100 µs and are not used. The remaining cost is ray tests across the tool's shank on rotary chords.

Error between rays depends on the sharpness of the machined detail. The relief needs 0.1 mm for 0.05 mm error; the smooth rotary wrap needs 0.4 mm.

## 8. Tests

- Single-move cases vs analytic swept volumes (ball along a line: capsule).
- Simulated stock after machining a pocket equals the pocket within resolution.
- Deliberate gouge (toolpath offset 0.1 mm into the part) is detected at the right location.
- Time estimate vs measured run times on a test machine (calibration data set; future).

M7 measured the product API (`cargo run -p fluut-core --example m7 --release`), 12 threads of a desktop machine; see [§10](#10-m7-implementation-2026-11-01).

## 9. Challenges

- CH-22 Simulation memory and speed for large stock at fine resolution (deferred; S5 measured it, rotary speed is still about 3× over target; M7: mesh extraction is now the slow part of scrubbing).

## 10. M7 implementation (2026-11-01)

Plan: [`../plan/2026-11-01-m7-simulation.md`](../plan/2026-11-01-m7-simulation.md). Code: `fluut-sim` (`run`, `verify`, `time`, `mesh`), `fluut-core` (`sim`, `Session::simulate`, `NodeId::Simulation`), `fluut-app` (Simulate, Simulation tab), `fluut sim`.

How it differs from §2–§6:

- **Resolution (§2):** twice the error target (S5: the largest error on sharp detail is about half the spacing), coarsened to fit the budget at 115 B per ray (24 B, +20 % spill, ×4 for checkpoint copies, as measured). The default error target is 0.1 mm, so 0.2 mm rays. Region-of-interest refinement is not built.
- **Stock:** block stock from the Setup frame. Cylinder stock works in `fluut-sim` but no Setup reaches it (rotary Setups are not calculated yet). Body stock is not built: it needs exact line–body intersection, which `fluut-topo` lacks.
- **Tools (§3):** V, taper and drill tools are cones and use the GJK fallback for every ray; a taper's corner radius is left out. Chords of arcs have a sagitta of at most `min(res/4, tol/4)`, so they stay within the gouge threshold.
- **Checkpoints (§4):** copy-on-write tiles of two rows. A checkpoint is taken before each toolpath and every 32 768 chords within one, so a scrub re-runs at most that many chords.
- **Mesh (§4):** the dual of the lattice through the three families' rays: inside and outside from the Z family, crossings from the ray on each edge, one vertex per cell at the mean of its crossings (no QEF, no normals stored). ~~The whole stock is meshed on each scrub, not only dirty tiles; no deviation colours.~~ Meshed per block and only where rays changed, with rest colours, since 2026-11-02 (below).
- **Gouge (§5):** the part is the exact top height from a drop query of a 1 µm flat cutter (D07), not a closest-point distance. A Z ray is a gouge when its stock top is below the part by more than `1.25 · tol` there and at 8 points on a ring of that radius, which keeps cuts within tolerance beside vertical walls from counting. `tol` is the largest segment tolerance of the Setup's toolpaths. The finding names the first chord that cut the ray below the part.
- **Rest (§5):** volumes on and off the part's footprint and the thickest rest; no heat map.
- **Holder, shank, flute length (§5):** the holder (from the tool's overall length up), a shank wider than the cutter and the cutter above its flute length are swept as flat cylinders and tested against the material before each chord cuts. Holder and shank are errors, flute length a warning.
- **Rapids (§5):** a rapid chord that cuts material.
- **Findings:** one per toolpath, segment (index in `ToolpathResult::segments`) and check, with a position in W; at most 1 000 per check.
- **Time (§6):** as written, with every linear axis at the machine's `max_cut` and `max_rapid`. Machine definitions have no acceleration, correction factor, tool-change time or rotary speeds yet, so the estimate is length over speed; a rotary move stops the machine at both ends.
- **Invalidation:** `NodeId::Simulation(setup)` hashes the Setup node, each enabled toolpath's post node (result and feeds) and its whole tool. The session keys a simulation by that hash, the settings and the resolved machine.

**Measurements** (Windows, 12 threads, release):

| Case | res (mm) | Segments | Memory (MB) | Simulate (s) | µs per segment | Scrub, re-cut (s) | Mesh (s) |
|---|---|---|---|---|---|---|---|
| `block` | 0.2 / 0.1 | 481 | 17 / 69 | 0.14 / 0.47 | 290 / 980 | 0.01 / 0.03 | 0.43 / 1.66 |
| `dome` | 0.2 / 0.1 | 7 832 | 27 / 109 | 1.08 / 4.52 | 138 / 577 | 0.04 / 0.19 | 0.36 / 1.54 |
| `plate2d` | 0.2 / 0.1 | 2 166 | 28 / 111 | 0.45 / 1.66 | 209 / 768 | 0.02 / 0.07 | 0.51 / 2.37 |
| `ridge` | 0.2 / 0.1 | 1 975 | 7 / 27 | 0.13 / 0.51 | 65 / 258 | 0.01 / 0.02 | 0.23 / 1.00 |
| S5 relief, rough + finish | 0.2 / 0.1 | 208 603 | 118 / 473 | 3.8 / 10.3 | 18 / 49 | 0.06 / 0.22 | 3.2 / 11.1 |
| S5 rotary wrap | 0.2 / 0.1 | 56 880 | 49 / 194 | 5.6 / 21.6 | 98 / 381 | 1.3 / 4.9 | 0.8 / 4.1 |

On the small reference projects the simulate time is mostly the part's drop queries (gouge and rest), not removal: `holes` takes 0.8 s for 25 segments. The relief finish meets §7 (18 µs per move at 0.2 mm). Scrubbing within a toolpath meets 150 ms for the re-cut but not with the mesh: meshing the whole stock takes 0.2–3 s, so per-tile meshing (or meshing on the GPU) is the next step (done 2026-11-02, below). Viewport frame rates with millions of stock triangles need a real GPU to measure; the tests render headless only.

**Playback (2026-11-02).** Plan: [`../plan/2026-11-02-sim-playback.md`](../plan/2026-11-02-sim-playback.md). Code: `fluut-sim` (`mesh::MeshTiles`, `player`, `Simulation::step`), `fluut-app` (worker, renderer, Simulation tab).

- **Blocks:** the mesh is built in blocks of 16 × 16 lattice columns, each an indexed mesh with shared vertices and area-weighted vertex normals. A block owns the crossing edges whose lower lattice point is in it and reads the rays two lattice points around it, so it depends on no other block. Vertices on a seam are repeated in both blocks at the same position. The full mesh is all blocks, built in parallel; the old whole-stock mesh is gone.
- **Dirty blocks:** after a scrub the new stock is compared with the one shown, tile by tile (`Arc::ptr_eq`, then ray by ray). A changed Z ray marks the edges at its column; a changed X or Y ray marks the edges along it within the range where its spans differ. Only blocks whose read range holds a marked edge are rebuilt. Tests check that the result equals a new mesh block by block, after random scrubs, cuts at every offset around a seam, and a wall moved between two Z rays (only X rays change).
- **Player:** keeps the stock it shows. Forward within a checkpoint interval it cuts on from that stock instead of the checkpoint; span subtraction is a set difference, so the stock is bit-identical to `stock_after`.
- **Paint:** each vertex carries a tag: the rest class and the toolpath that last cut the four Z rays around it. The class of a ray (a first guess, refined per vertex since 2026-11-04, see below) compares its material top with the part's top there (kept from the verification): above by more than `tol` is rest, below by more than `1.25 · tol` a gouge, else within; no part is plain stock. A vertex is a gouge only if every ray around it that has the part is one, so walls beside the part are not; else rest if any ray is. The last toolpath is the current one if the ray differs from the checkpoint before it, else the latest earlier toolpath whose checkpoints differ there. Both depend only on the ray and its history, so unchanged blocks keep their paint. Colours are Okabe–Ito (blue rest, bluish green within, vermillion gouge; seven colours by toolpath), chosen in the shader from a uniform, so switching costs no upload.
- **App:** the worker keeps a player per Setup and sends the list of block `Arc`s; the renderer keeps a vertex and index buffer per block and uploads those whose `Arc` changed. Scrubs stay coalesced. Steps go to the end of the next or previous move, pass or toolpath (`Simulation::step`); moves and passes end with their last chord, so moves that touch no stock are stepped over.

**Playback measurements** (Windows, 12 threads, release; from the middle of the last toolpath, `fluut-core/examples/m7.rs`):

| Case | res (mm) | Full mesh, M7 → now (s) | Blocks | 1 move (ms) | 300 moves (ms) | Mesh update (ms) | Blocks rebuilt | 300 moves back (ms) |
|---|---|---|---|---|---|---|---|---|
| S5 relief | 0.2 | 3.2 → 0.46 | 2 961 | 2.4 | 28–32 | 21–24 | 67 | 75–81 |
| S5 relief | 0.1 | 11.1 → 6.8 | 11 844 | 18 | 300 | 242 | 174 | 445 |
| S5 rotary wrap | 0.2 | 0.8 → 0.16 | 893 | 2.5 | 51–53 | 15–17 | 44 | 1 100 |
| `dome` | 0.2 | 0.36 → 0.06 | 638 | 0.8 | 32 | 17 | 238 | 68 |
| `plate2d` | 0.2 | 0.51 → 0.08 | 910 | 13 | 114 | 69 | 818 | 137 |

A step of a few hundred moves on the relief at 0.2 mm re-meshes in about 22 ms, within the 50 ms target. On the small projects 300 moves cover most of the stock, so most blocks are rebuilt; that is still about 0.1 s. Stepping back re-cut from the checkpoint (up to 32 768 chords), which dominated backward steps and the rotary wrap (1.1 s, its re-cut is slow, CH-22); fixed 2026-11-04 (below).

**Step back (2026-11-04).** Plan: [`../plan/2026-11-04-sim-step-back.md`](../plan/2026-11-04-sim-step-back.md). The `Player` keeps copy-on-write snapshots of the stock (`TriDexel` clones sharing tiles) keyed by chord count; a seek cuts on from the latest of the stock shown, a snapshot and a checkpoint, so a step back cuts less than one stride (chosen from the measured cutting speed to take about 3 ms; `Limits::stride` 0). The long span before the target is cut in one batch; snapshots are left every stride in the last 6 strides before it. `Limits::budget` (default 64 MiB) bounds them; over it the snapshots above the stock shown go first, then the farthest below. The stock stays bit-identical to a forward cut from the start (tested on random seeks). A back step of one move on the S5 relief at 0.2 mm takes 3 ms (was 59), on the rotary wrap 3.5 ms (was 1 100), on the reference projects 1 to 16 ms; what is left of a back step is the mesh update. A jump back of many moves costs 10-15 % more than before (snapshot copies).

**Rest colour by distance (2026-11-04).** Plan: [`../plan/2026-11-04-sim-rest-closest.md`](../plan/2026-11-04-sim-rest-closest.md). Code: `fluut-sim` (`Clearance`, `ClearCache`, `SimPaint::vertex`), `fluut-core` (`sim::DropPart`).

- **Why:** the vertical gap between stock top and part top at the Z rays misstates the rest on steep and vertical walls (a thin skin against a wall read as the wall's height). The true rest is the distance from the stock surface to the part.
- **Method:** the class of a vertex is refined from the class the four columns around it give (`combine`, unchanged). Gouge and within stay as they were: the vertical gap is an upper bound of the distance, so a vertex within tolerance vertically is within. A vertex the columns call *rest* is rest only if the part's surface is at least `tol` from it and it is not inside the part; otherwise it is within. Gouge findings, the rest volumes and the Z-ray class (`PartHeights::class`) are not touched.
- **The distance query:** `Part::clearance()` returns an owned `Clearance` kept in `PartHeights`. `clear_height(x, y, r)` is the lowest height at which a ball of radius `r` at `(x, y)` is clear of the part, that is the part's surface offset by `r` as seen from above; a vertex is at least `r` from the part (and outside it) when it is at or above that height. The core implements it by a ball-nosed drop query (D07) with the tolerance as radius on the exact faces: the ball resting at its highest contact has its centre `tau + r` high. This is the closest-point distance for a part a three-axis tool reaches (the limit the gouge check has too); stock under an overhang reads as near the part. The faces have no closest-point query with trimming, and a tessellated part would have to be as fine as the tolerance, so the drop is used. A part without a query (`None`) keeps the vertical class. `QueryGeometry` keeps its body as a `Cow`, so the part owns a copy that outlives the Setup's geometry.
- **Shortcuts, which keep the answer:** (1) a vertex below the lowest sampled part height of the 3 x 3 columns around it, all with a part, is inside the part (or under an overhang) and so within: this is the stock's bottom and its sides below the part's top, which were most of the vertices the columns call rest, and need no query. (2) A vertex is rest without a query when, for the part heights in the 5 x 5 columns around it, every column is farther than the tolerance (the larger of the vertical gap to its sampled height and the sideways gap to its edge), and beyond them the part is at least two ray spacings away. Heights are samples: a part feature narrower than a ray spacing between them is missed, as by the vertical class. (3) The answer depends only on the position `(x, y)` of the vertex (as `f32`, as stored) and `tol`, so it is cached by position in `PartHeights::cache` (64 locked shards, emptied when full): the vertices up a vertical wall share one query, and a block rebuilt after a step asks again only for vertices that moved. The cache saves time only; every answer is a function of the position, so colours are the same whatever the order or number of threads. (4) A block refines the classes of all its vertices at once (`Paint::vertices`), in parallel, since the queries near walls are the slow ones and a block is otherwise one task.
- **Making the drop query cheap** (found by measuring `holes`, where a query took 0.3 to 1 ms): the ball is tiny against the edges of a hole, and the drop (a) sampled the whole circle of each hole edge, a quarter of a ball radius apart: `Seeding::split_edges` halves an edge down to pieces of about eight ball radii and solves only those within reach of the ball; (b) searched the face boundaries ("rims", a net for gaps in the model) for every trimmed plane over a hole: `DropOptions::rims` is off for this query, the edges cover the boundary; (c) tested trim containment against every segment of a 1 µm-chord polyline: `Trim` now keeps runs of 16 segments with their range and skips the runs the test line cannot cross. The flat 1 µm drop of the verification uses none of (a) and (b), so the gouge check is unchanged; (c) gives the same answers (tested against the full scan).
- **Measured** (Windows, 12 threads, release, `fluut-core/examples/m7.rs`; the machine was shared, so before and after were run alternately, 7 times, medians; mesh update after 300 moves, ms): `holes` 0.1 mm 4.1 to 5.7 (+1.6 ms), 0.05 mm 15.9 to 18.8; `dome` 0.1 mm 20.2 to 21.8, 0.05 mm 166 to 189; `flipside` s1 8.1 to 8.8 and 59.6 to 56.6, s2 7.9 to 8.3 and 42.6 to 43.5; `boss`, `ridge`, `rough3d` and the others are within noise (projects with no part moved by up to 15 % between runs). The first mesh of a new `Player` is within noise except `dome` at 0.05 mm (0.38 to 0.46 s). A step back is unchanged (`holes` worst 52.8 to 55.1 ms). Remaining: the one block ring of queries around a hole rim on `holes` at 0.1 mm.

**Paint speed (2026-11-11).** Plan: [`../plan/2026-11-11-sim-paint-speed.md`](../plan/2026-11-11-sim-paint-speed.md). Profiling a step on `plate2d` showed paint is about 2 % of the block rebuild there (no vertex is in the rest class, so no query runs); the step is the cut of a long chord (5.5 ms) plus block geometry (vertex step, edges). Where the part has a large rest area (`dome`, `ridge`, `rough3d`) the shortcuts before the query are about a quarter of the CPU of a jump. `SimPaint::far_above` now reads only the columns within the tolerance of the vertex instead of 5 x 5, with the same answers (tested against the full scan); incremental paint equals a full repaint (tested). Timings are unchanged within noise.
