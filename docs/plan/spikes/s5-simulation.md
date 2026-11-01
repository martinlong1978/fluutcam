# S5 — Simulation Spike Report

Date: 2026-10-31. Plan: [`../2026-10-31-s5-simulation.md`](../2026-10-31-s5-simulation.md).

Code that is kept:
- `crates/fluut-sim`: stock shapes, tri-dexel grids, tools, swept volumes, segments to chords, parallel removal.

Spike code, which may be thrown away:
- `crates/fluut-sim/src/spike`: the stand-ins and the exact single-ray probe.

Measurement tool: `cargo run -p fluut-sim --example s5 --release -- [rays|relief|rotary|cyl|error|error-rotary] [res…]`.

All numbers: release build, Linux cloud container, 4 cores of an Intel Xeon at 2.8 GHz, 15 GB RAM. "4 threads" means the whole machine. D11 §7 targets assume 8 cores.

## Stand-ins

The reference rotary and relief projects do not exist yet, so the spike uses two synthetic stand-ins (`src/spike`):

| | Relief | Rotary |
|---|---|---|
| Stock | box 200 × 150 × 25 mm | cylinder Ø60 × 150 mm on the A axis |
| Surface | `z(x, y)`, 8.5 to 23.5 mm: broad waves (70 and 55 mm), a diagonal 23 mm wave and a 1 mm, 4 × 5 mm detail pattern that a Ø3 ball cannot fully follow | wrapped `r(x, θ)`, 24 to 29 mm: a 40 mm wave times cos 3θ plus a 13 mm helical ripple |
| Roughing | flat Ø6, Z-levels every 3 mm from 23 down to 11, rows along X at 2.4 mm, one Line per run, 0.5 mm left; 3 695 segments | none |
| Finishing | ball Ø3, raster along X at 0.3 mm stepover, a Line every 0.5 mm, zigzag; 204 908 segments | ball Ø6, one spiral at 0.5 mm per turn, a Rotary segment every 2° of A; 56 880 segments, 316 turns |

The tool tip follows the surface directly (no gouge-free CL offset). This makes no difference to simulation cost. A second relief run finishes straight from the full stock. This is the worst case: the shank sweeps the full stock height.

## 1. Removal method

Every chord cuts each ray it crosses by the exact swept volume:
- `tool ⊕ chord` when the axis is fixed;
- the convex hull of the two end poses on rotary chords.

D11 §3 planned a GJK ray cast for every ray. At about 2.5 µs per ray it would make every case 10–40× slower, so the spike added exact closed forms for the common cases. GJK remains only for flat and bull tools on lines that are neither along nor across a level chord, such as ramps, plunges seen from the side and rotary chords.

| Method (one thread) | Case | ns per ray |
|---|---|---|
| Axial, ball | line along a fixed tool axis: the bottom capsule translated along the line | 23 |
| Axial, flat | same, 1-D minimum is at a chord end | 54 |
| Axial, bull | same, 1-D convex minimum by bisection on the derivative | 298 |
| Side, flat or bull | line across the axis on a level chord: 2-D capsule of the tool radius at the line's height | 56–58 |
| Ball | any other line, fixed axis (4 sphere-edge cylinders, 2 thick triangles) | 92 |
| Ball | rotary chord (hull of 4 sphere centres: 6 edges, 4 triangles) | 296 |
| GJK, flat or bull | ramp seen from the side | 2 430–3 330 |
| GJK, bull | rotary chord | 2 840 |

**How the methods are checked:**
- **Against brute force:** every method is compared with brute-force sampling of tool positions (`sweep/tests.rs`).
- **Closed forms against GJK:** they are compared on 300 random lines each and agree within 1e-3 mm, which is the GJK tolerance used in the tests.
- **Whole cuts:** whole-cut tests check a ball slot, an arc groove, a full rotary turn and pocket volumes against analytic values.

## 2. Memory

Each ray costs 24 bytes:
- a `SmallVec<[Span; 2]>` with `f32` span ends;
- spans past the second spill to the heap at 8 bytes each.

| Project | res (mm) | Rays | Stock (MB) | After machining (MB) | Spilled rays |
|---|---|---|---|---|---|
| Relief | 0.4 | 0.24 M | 6 | 7 | 19 k |
| Relief | 0.2 | 0.97 M | 22 | 27 | 77 k |
| Relief | 0.1 | 3.9 M | 89 | 106 | 308 k |
| Relief | 0.05 (extrapolated ×4) | 15.5 M | 355 | ≈ 425 | ≈ 1.2 M |
| Rotary | 0.4 | 0.14 M | 3 | 3 | 3 k |
| Rotary | 0.2 | 0.54 M | 12 | 13 | 13 k |
| Rotary | 0.1 | 2.2 M | 49 | 52 | 53 k |
| Rotary | 0.05 (extrapolated ×4) | 8.6 M | 198 | ≈ 210 | ≈ 210 k |

**Findings:**
- Machining adds 6–20 %: about 8 % of rays end with three or more spans on the relief and 2.5 % on the rotary.
- Memory is exactly `24 B × (XY + YZ + ZX) / res²` plus that spill.
- D11 §2's estimate for 200 × 200 × 50 mm at 0.1 mm (≈ 150 MB) holds: it is 144 MB before cutting.
- The 512 MB default budget allows 0.05 mm on both stand-ins.

## 3. Update time

Removal time, without building the stock or the chords. Chord generation takes 0.01–0.04 s for the whole program.

| Project, toolpath | res | 1 thread (s) | 4 threads (s) | µs per move (4 thr) | Ray tests per chord X / Y / Z |
|---|---|---|---|---|---|
| Relief rough (flat Ø6) | 0.4 | 0.90 | 0.23 | 62 | 82 / 250 / 214 |
| Relief rough | 0.2 | 3.36 | 0.90 | 245 | 316 / 963 / 825 |
| Relief rough | 0.1 | – | 3.61 | 978 | 1 262 / 3 851 / 3 266 |
| Relief finish after rough (ball Ø3) | 0.4 | 4.52 | 1.83 | 8.9 | 18 / 51 / 32 |
| Relief finish after rough | 0.2 | 17.99 | 6.74 | 33 | 71 / 208 / 129 |
| Relief finish after rough | 0.1 | – | 25.26 | 123 | 282 / 815 / 499 |
| Relief finish from full stock | 0.4 | 9.07 | 2.91 | 14 | 30 / 130 / 32 |
| Relief finish from full stock | 0.2 | 33.91 | 9.58 | 47 | 117 / 511 / 130 |
| Relief finish from full stock | 0.1 | – | 38.18 | 186 | 459 / 2 027 / 500 |
| Rotary wrap (ball Ø6) | 0.4 | 11.70 | 3.18 | 56 | 239 / 164 / 167 |
| Rotary wrap | 0.2 | 44.08 | 12.56 | 221 | 954 / 658 / 667 |
| Rotary wrap | 0.1 | – | 48.44 | 852 | 3 810 / 2 630 / 2 667 |

**Findings:**
1. **Time grows as 1/res².** Halving the resolution costs 3.75–3.9× more time. The cost is per ray tested, not per move. A chord tests about (tool width × tool height in stock) / res² rays in each side family, however short the chord is.
2. **Side families dominate 3-axis work.** On the relief finish, the Z family takes only 8–15 % of the time. Y rays run across the raster, and every row trims each of them by the stepover, about 7 times per row as consecutive chords pass. Roughing first cuts the finishing time by about a third.
3. **Rotary is the slow case.**
   - No ray family follows the tool axis, so no ray gets the 23 ns axial path.
   - Every ray takes the 300 ns hull method.
   - The Ø6 ball and its shank cut into all three families.
   - The result is 850 µs per move at 0.1 mm, 7× the relief finish.
4. **Parallel scaling on 4 threads is 2.7–3.7×.** Tiles are pairs of rows. A batch of 32 768 chords covers only part of the Z grid on a raster, which limits the Z family's parallelism.
5. **Against D11 §7** (1 M short moves, 0.2 mm, 150 × 100 × 40 mm, < 20 s on 8 cores):
   - **Relief finish:** at 33–47 µs per move on 4 threads, this is about 35–50 s per million moves on 4 cores. That would be about 18–25 s on 8 cores if scaling holds, on a larger stock. It is borderline but plausible.
   - **Rotary:** at 221 µs per move it misses by about 6×.

## 4. Error against resolution

**Error measure:** a bilinear surface through the tops of neighbouring Z rays, against an exact probe (one ray, chords at 0.0005 mm sagitta) at about 4 000 random points in a window. The window is 10 × 10 mm in the relief and 10 × 12 mm on top of the cylinder. Points whose four neighbour tops differ by more than `res` (slope over 45°) are skipped, because the X and Y families serve them. The error includes the res/4 chord sagitta.

| res (mm) | Relief max | Relief p99 | Relief RMS | Rotary max | Rotary p99 | Rotary RMS |
|---|---|---|---|---|---|---|
| 0.4 | 0.205 | 0.105 | 0.028 | 0.015 | 0.012 | 0.005 |
| 0.2 | 0.110 | 0.055 | 0.011 | 0.011 | 0.009 | 0.003 |
| 0.1 | 0.050 | 0.019 | 0.004 | 0.008 | 0.006 | 0.003 |
| 0.05 | 0.017 | 0.008 | 0.001 | 0.006 | 0.005 | 0.003 |
| 0.025 | 0.010 | 0.002 | 0.0005 | 0.002 | 0.001 | 0.0006 |

**Resolution needed:**
- **Relief:** 0.1 mm for a 0.05 mm maximum error; 0.05 mm for a 0.01 mm 99th percentile; 0.025 mm for a 0.01 mm maximum.
- **Rotary:** 0.4 mm for 0.05 mm; 0.1 mm for 0.01 mm.

**What limits the error:**
- **Relief:** the error comes from sharp ridges, where the Ø3 ball cannot follow the 4–5 mm detail, and scales about linearly with `res`.
- **Rotary:** the smooth wrap leaves only the 0.01 mm ball scallops at the 0.5 mm pitch. The error plateaus near the scallop height until the rays resolve the scallops.

The span ends themselves are exact to `f32` (about 1e-5 mm); all error is between rays.

**Implication:** the resolution needed depends on how sharp the machined detail is, not on the stock size. For detailed reliefs, 0.05–0.1 mm is needed for verification at 0.01–0.05 mm. At 0.05 mm the 200 × 150 relief needs about 425 MB and, extrapolating ×4, about 2 minutes on 4 threads. That is fine for a final check but not for interactive use.

## 5. Determinism

- **Thread count:** the stock hash is identical on 1 and 4 threads for every 0.4 and 0.2 mm run, and in the unit test (`same_hash_on_one_and_many_threads`).
- **Order of cuts:** within a batch each tile applies the chords in program order, so no ray sees a different order of cuts.
- **Maths:** all trigonometry goes through `libm`. Spans are rounded to `f32` by plain casts.

## 6. Decisions and design changes

1. **Tri-dexel is viable for v1 relief work.** At 0.1 mm the full relief job (rough and finish) takes 106 MB and 29 s on 4 cores. Keep it as D11's stock model.
2. **Closed forms before GJK** (D11 §3 updated). GJK stays as a general fallback only. Flat and bull ramps, plunges and rotary chords still use it (2.5–3.3 µs per ray). A closed form for ramps is a follow-up.
3. **Rotary chord rule** (D11 §3 updated):
   - Split moves with a changing A by the sagitta at the stock's outer radius, not the tip's.
   - Take each chord as the hull of its end poses, which bounds the error by the sagitta on every part of the tool.
4. **Rotary needs another layout to meet D11 §7.** Recommendation for M7, not tried here: in rotary Setups, use a cylindrical dexel layout:
   - radial rays on an (x, θ) grid, along the tool axis, which gives the axial fast path;
   - plus X rays.

   Rays near the axis are mostly empty and need not be stored.
5. **Interactive resolution.**
   - Default display and scrubbing at 0.2 mm (relief finish at 33 µs per move). Run verification at the budget resolution in the background.
   - The region-of-interest refinement in D11 §2 is the way to reach 0.025–0.05 mm locally.
6. **Shank and holder** are cut off just past the stock. Holder collision checks (D11 §5) need their own pass and were not measured.

## 7. Follow-ups

1. Closed form for flat and bull tools on ramps and plunges seen from the side (now GJK). Done; see [Follow-ups](#follow-ups-2026-11-01).
2. Cylindrical dexels for rotary Setups, measured against this tri-dexel on the same stand-in. Done; not adopted, see [Follow-ups](#follow-ups-2026-11-01).
3. Better Z-family parallelism on rasters: interleave the tiles, or batch by region instead of by program order. Done by scheduling only busy tiles; see [Follow-ups](#follow-ups-2026-11-01).
4. Re-run on the real reference projects once they exist, on an 8-core machine, to settle D11 §7.
5. Tile copy-on-write checkpoints and dual-contouring display (D11 §4) were not prototyped.

## Follow-ups (2026-11-01)

Plan: [`../2026-11-01-s5-followups.md`](../2026-11-01-s5-followups.md). Follow-ups 1–3 of §7 are done.

These numbers come from a different machine than the rest of this report: a Linux cloud container, release build, 4 cores. "Before" is `main` measured on that machine. Whole runs vary by about ±10 % between repeats, and single-ray timings by up to 30 %.

### Ramps and plunges seen from the side

**Change.** `Method::Side` now covers any chord with a fixed axis, for lines across the axis. Level chords were the only case before.
- The line stays at one height. At each point of the chord the tool's section at that height is a disc.
- **Straight flank:** the disc has radius `R`, so the sweep's section is a 2-D capsule. This is closed form, and covers flat tools completely.
- **Corner bands of a bull nose:** the disc shrinks. The part of the chord where it still reaches the line comes from a line against a 2-D capsule. The entry and exit are convex in the chord parameter, and a golden-section search finds them.
- GJK now remains only for flat and bull tools on rotary chords and on lines that are neither along nor across a fixed axis. The 3-axis tri-dexel no longer uses it.

**Checks:** the side method agrees with GJK within 1e-3 mm on 900 random ramps and plunges (flat, two bull radii, vertical and tilted axes), and with brute force on steep bull ramps.

| Case (one thread) | Before (GJK), ns per ray | After, ns per ray |
|---|---|---|
| Bull R3 r1, ramp | 3 040 | 145 |
| Flat R3, ramp | 3 020 | 68 |

**Effect:** relief roughing (flat Ø6, plunges at each run) goes from 1.00 to 0.14 s at 0.4 mm and from 4.04 to 0.54 s at 0.2 mm on one thread, about 7.5× faster.

### Ball fast path

This was found while building the cylindrical layout, and it serves both layouts.

**Rule.** The ball's sweep is the hull of four sphere centres (two per pose) grown by the radius. For each end of the line:
- **Bottom capsule:** a point on the capsule around `c0`–`c2` whose outward normal `n` has `n·w ≤ 0` for both axes lies on the hull's boundary. The hull's support in `n` is then at `c0` or `c2`.
- **Top capsule:** the same holds for the capsule around `c1`–`c3` when `n·w ≥ 0`.
- **Misses:** a line farther than the radius from the bottom segment, in a direction with `n·w ≤ 0`, misses the sweep.

Only lines this cannot decide go to the full hull. All of this is exact.

| Case (one thread) | Before, ns per ray | After, ns per ray |
|---|---|---|
| Ball R3, rotary chord, line across the axis | 390–490 | 290 |
| Ball R3, rotary chord, line near the axis | 390–490 | 150 |

The relief finish after roughing goes from 6.25 to 4.90 s at 0.4 mm on one thread.

### Cylindrical dexels for rotary Setups

**Layout** (`CylDexel`):
- **Radial rays** start on the rotary axis, on an (x, θ) grid. `nθ = ⌈2πR / res⌉`, so neighbouring rays are at most `res` apart at the stock surface.
- **X rays** lie on the tri-dexel's (y, z) grid.
- Both families are cut by the same `Sweep::ray`, by tiles that take their chords in program order.

**Near-axis rays.** Skipping them saved nothing, because no ray is empty near the axis:
- every radial ray starts there and costs 24 B whatever its length;
- the X rays there are full.

What could be skipped were the X rays outside the stock circle. They are empty for the whole run, and leaving them out saves 21 % of the X family.

**Rotary stand-in** (ball Ø6 spiral, 56 880 moves), 4 threads. The tri-dexel "after" includes the fast path.

| res | Tri-dexel before: 1 / 4 thr (s) | µs per move | Tri-dexel after: 1 / 4 thr (s) | µs per move | Cylindrical: 1 / 4 thr (s) | µs per move |
|---|---|---|---|---|---|---|
| 0.4 | 14.34 / 4.11 | 72 | 6.86 / 1.97 | 35 | 5.46 / 1.55 | 27 |
| 0.2 | 53.67 / 14.46 | 254 | 24.42 / 6.64 | 117 | 20.15 / 5.69 | 100 |
| 0.1 | – / 59.79 | 1 051 | – / 23.84 | 419 | – / 20.85 | 367 |

| res | Tri-dexel rays | Memory after (MB) | Cylindrical rays (radial + X) | Memory after (MB) | Ray tests per chord, tri-dexel X / Y / Z | Ray tests per chord, cylindrical X / radial |
|---|---|---|---|---|---|---|
| 0.4 | 135 k | 3 | 177 k + 18 k | 5 | 239 / 164 / 167 | 239 / 409 |
| 0.2 | 540 k | 13 | 707 k + 71 k | 18 | 954 / 658 / 667 | 954 / 1 623 |
| 0.1 | 2.16 M | 52 | 2.83 M + 0.28 M | 74 | 3 810 / 2 630 / 2 667 | 3 810 / 6 471 |

**Error against the exact probe**, in the same window on top of the cylinder (x 65–75 mm). The tri-dexel uses Z rays; the cylindrical layout uses radial rays at θ within ±0.2 rad.

| res | Tri-dexel: max / p99 / RMS (mm) | Cylindrical: max / p99 / RMS (mm) |
|---|---|---|
| 0.4 | 0.0146 / 0.0123 / 0.0046 | 0.0146 / 0.0122 / 0.0047 |
| 0.2 | 0.0110 / 0.0091 / 0.0031 | 0.0110 / 0.0089 / 0.0030 |
| 0.1 | 0.0080 / 0.0064 / 0.0029 | 0.0079 / 0.0062 / 0.0028 |

**Findings:**
1. **Most of the rotary gain is the fast path, not the layout.** The tri-dexel gets 2.1–2.5× faster with it. The cylindrical layout adds 14–27 % on top, less at finer resolution.
2. **Cost of the cylindrical layout:** 1.4× the memory, because radial rays outnumber the tri-dexel's Y and Z rays by π/2. It also tests 22 % more radial rays than the tri-dexel tests Y and Z rays, because the θ range per chord comes from the chord's box.
3. **X rays cost the same in both layouts** and take half the cylindrical time. Those that cross the tool's shank fall back to the full hull.
4. **The error is the same in both layouts.** The tri-dexel's Z rays are almost radial in this window.
5. **Against D11 §7** (< 20 s per million moves at 0.2 mm on 8 cores, about 40 µs per move on 4 cores): the rotary wrap was about 6× over. It is now about 3× over with the tri-dexel, and 2.5× with cylindrical dexels.

**Decision.** Keep the tri-dexel for rotary Setups. A second stock model with 40 % more memory is not worth a 14–27 % gain. `CylDexel` stays in the crate as spike code, for comparison, until M7.

**Further rotary speed has to come from fewer or cheaper ray tests:**
- a closed form for lines across a ball's shank on rotary chords (now the full hull);
- a tighter per-chord ray range.

### Z-family parallelism on rasters

**Measurement.** On the relief finish at 0.2 mm, before the change, the families sped up from 1 to 4 threads by:
- X: 2.3×;
- Y: 3.1×;
- Z: 1.9–2.0×.

A batch of 32 768 chords covers a band of about 60 of the 375 Z tiles, and rayon's first splits left that band to one or two threads.

**Change.** Each layout now runs only the tiles that some chord of the batch reaches, as one parallel list. The order within a tile is unchanged, so the hash is the same on any thread count. A new test cuts a raster whose batches cover narrow bands of a wide grid, against the probe and on 1 and 4 threads. Interleaving and region batching were not needed.

| Relief, 0.2 mm, Z family | 1 thread (s) | 4 threads before (s) | 4 threads after (s) | Speed-up |
|---|---|---|---|---|
| Finish after roughing | 2.3 | 1.2–1.4 | 0.6–0.7 | 1.9× → 3.3–3.8× |
| Finish from full stock | 2.3 | 1.1–1.3 | 0.6–0.7 | 2.0× → 3.3–3.8× |

The Z family is 10–20 % of the relief time, so totals change by less than the noise between runs.

### Relief totals, before and after all follow-ups

| Relief, toolpath | res | Before: 1 / 4 thr (s) | After: 1 / 4 thr (s) | µs per move (4 thr), before → after |
|---|---|---|---|---|
| Rough (flat Ø6) | 0.4 | 1.00 / 0.29 | 0.14 / 0.05 | 79 → 13 |
| Rough | 0.2 | 4.04 / 1.06 | 0.54 / 0.20 | 287 → 53 |
| Finish after rough (ball Ø3) | 0.4 | 6.25 / 2.15 | 4.90 / 2.02 | 10.5 → 9.8 |
| Finish after rough | 0.2 | 23.41 / 9.57 | 18.31 / 7.05 | 47 → 34 |
| Finish from full stock | 0.4 | 11.22 / 3.73 | 10.30 / 3.47 | 18 → 17 |
| Finish from full stock | 0.2 | 42.79 / 13.07 | 38.05 / 11.98 | 64 → 59 |

**Relief against D11 §7:** the relief finish at 0.2 mm now runs at 34 µs per move on 4 cores, inside the 40 µs that the target allows. The worst case (finishing from full stock, 59 µs) is still above it.

Follow-ups 4 and 5 of §7 remain open, and so do the two rotary items above.
