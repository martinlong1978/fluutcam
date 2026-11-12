# Simulation paint speed (fu-5)

**Goal:** after the [mesh speed](2026-11-11-sim-mesh-speed.md) work, `plate2d` still took about 13.5 ms per forward move and 105-130 ms for a 300-move jump, and that plan put the blame on the per-vertex rest-class (paint) queries. Find where the time goes and cut it, with the colours (and the whole mesh) unchanged bit for bit.

## Finding: paint is not what `plate2d` waits for

Profile (release, 12 threads, `m7 projects 0.1`, 0.2 mm; temporary timers in `MeshTiles::update`, `build` and `SimPaint`, removed again). A forward step of one move on `plate2d` (13-14 ms):

| Part of the step | ms |
|---|---|
| Cutting the stock (`Simulation::cut`, one long chord, 48 blocks touched) | 5.5-6.0 |
| Dirty search | 0.5 |
| Block rebuild, 48 blocks in parallel | 7.0-7.5 |
| Stock clone, snapshot bookkeeping | under 0.1 |

Inside the rebuild, summed over threads (about 45-65 ms of CPU, noisy on a shared machine): ray ranges 6 %, edges 26 %, vertex step (counting sort, means) 36 %, normals 6 %, quad emit 23 %, **paint columns 1.3 %, paint vertices about 1 %**. For the 300-move jump (818 blocks): about 650-700 ms of CPU in the geometry up to the normals, 220-240 ms emit, 13-15 ms paint columns, 6-14 ms vertex refinement.

On `plate2d` at that place no vertex of a rebuilt block is in the rest class, so none reaches the distance query (`Clearance`) at all: the zero queries, zero cache hits and 0.4 ms of vertex refinement per step are the whole of the "per-vertex BVH walks". The hypothesis in the mesh-speed plan, that the distance queries cost more than the block rebuild, does not hold for this project; the remaining cost is the cut and the geometry of the blocks (vertex step and edges). The same holds for `plate`, `block`, `vcarve`, `holes` (queries are 1 773 for a full jump, 26 ms of CPU, cached afterwards).

Where paint does cost is a part with a large rest area: on `dome`, `ridge` and `rough3d` the two shortcuts in front of the query (`under`, `far_above`) are about a quarter of the CPU of a jump (`dome`, 300 moves: 12.5 ms and 21 ms of CPU against 74 ms geometry plus 25 ms emit), with queries themselves rare (`dome` 5 154 of 148 000 rest-class vertices, the rest answered by the shortcuts). `far_above` looked at the part heights of the 5 x 5 columns around every rest-class vertex, but only a column within the tolerance sideways of the vertex can bring the bound down to the tolerance.

## Change

`SimPaint::far_above` visits only the columns within `tol / res` of the vertex (one to four, and one more on each side for rounding) instead of 25, with the same arithmetic on each, and returns `false` at once when the tolerance is not under the reach (`REACH` ray spacings). The skipped columns have a bound above the tolerance and cannot change the answer.

No other change: the mesh, the tags, the cache, the cut and the snapshots are untouched.

## Tests

- `far_above_equals_the_full_window`: the new test against a copy of the old 5 x 5 loop, 24 000 random points (inside and just outside the grid), random heights with missing ones, tolerances from 0.02 to 0.6 mm at 0.25 mm spacing.
- `incremental_paint_equals_a_full_repaint`: a 40-row raster and a pass across a block against a wall part with distance queries (rest and within classes both present); 30 random steps and jumps, forward and back, with snapshots; after each, every block of the player's mesh (positions, normals, tags, indices) equals a mesh painted from scratch on the same stock.
- Unchanged and passing: `mesh_bits_are_pinned`, `random_updates_equal_a_fresh_mesh`, the Player tests, `rest_colours_do_not_depend_on_the_thread_count`.

## Measured

Release, Windows, 12 threads, `m7 projects 0.1`; the old and the new binary alternated, 5 runs each, medians, ms. "1 move": a step forward from the middle of the last toolpath. "Mesh, 300": the mesh update alone for a jump of 300 moves. "Back avg / worst": 20 single steps back by move.

| Case | | 1 move | Mesh, 300 | Back avg / worst |
|---|---|---|---|---|
| `plate` | before | 4.7 | 54.2 | 4.8 / 14.4 |
| `plate` | after | 3.9 | 54.2 | 4.9 / 15.7 |
| `plate2d` | before | 14.5 | 100.3 | 5.5 / 18.6 |
| `plate2d` | after | 13.6 | 95.7 | 5.9 / 20.5 |
| `dome` | before | 0.8 | 13.9 | 0.3 / 0.6 |
| `dome` | after | 0.8 | 14.1 | 0.3 / 0.8 |
| `rough3d` | before | 3.2 | 15.9 | 0.4 / 2.9 |
| `rough3d` | after | 3.2 | 15.2 | 0.4 / 2.9 |

All the others (`block`, `boss`, `flipside`, `holes`, `ridge`, `vcarve`) are within the noise (10-20 % between runs) both ways. The change is correct and removes work, but at this size it is below what the timings can resolve; it is not a speed-up of `plate2d`.

## Limits and next steps

- `plate2d` is limited by the cut of a long chord (5.5-6 ms for one move: the sweep of a long diagonal chord visits the rays of its bounding box) and by the geometry of the blocks (vertex step and edge generation, 60 % of the rebuild CPU). Next steps, outside paint: a tighter reach for long chords in `sim::cut_chords`, and reusing the ray ranges and edges of the margin (each block recomputes the 2-point margin of its neighbours: 529 columns for 256 owned).
- Caching the class per lattice vertex was not done: a block's vertices are rebuilt from scratch with new indices, and the cost of a lookup would be above that of the shortcuts, which are now a few column reads.
- The timings of the machine move by 10-20 % between runs.
