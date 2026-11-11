# Simulation mesh speed (sim-6)

**Goal:** playback is mesh-bound (9-24 ms per step on `plate` and `plate2d`). Make the mesh update cheaper per step, with the mesh unchanged bit for bit.

**Where we started.** `MeshTiles::update` was already incremental by block (16 x 16 lattice columns): only blocks that read a changed ray are rebuilt, and tiles that were not touched are skipped by pointer. What was left was the cost of finding the dirty blocks and of rebuilding one block. Per-phase timing of a rebuilt block (release, 12 threads): the vertex step (sorting the crossings of each cell) was about 45 % of the CPU time, edge generation 15 %, paint 10-15 % (more on `plate2d`, where the rest class needs distances to the part).

## Approach

1. **Dirty search in parallel** (`MeshTiles::dirty`): the changed tiles are compared in parallel, each giving the block ranges its changed rays are read by; the marks are merged afterwards. Equal range lists are skipped early in `differ`.
2. **Vertex step without a global sort** (`build`): crossings are bucketed by lattice column with a counting sort, ordered by height inside the column (stable, so crossings of a cell keep edge order and the means sum in the same order), and each edge keeps the four vertex ids of its quad. This replaces the sort of `(key, point)` pairs and the binary search per quad corner.
3. **Step back** (`Player`): snapshots are also kept at the starts of the moves before a seek target, and at the stock shown when stepping forward, so a step back by move lands on a snapshot (no cut longer than a stride). `Player::new` leaves those snapshots too. Snapshot eviction goes down to 3/4 of the budget instead of 1/2.

The mesh is identical: the pinned hashes in `mesh_bits_are_pinned` were taken from the build before this change and still pass.

## Tests

- `mesh_bits_are_pinned`: FNV hash of every block (positions, normals, tags, indices) over a scene of cuts in all directions, two resolutions, with and without a banded paint; values from the old build.
- `random_updates_equal_a_fresh_mesh`: 40 random updates between the stocks of the scene, in any order (forward and back); every block equals a fresh mesh, bit for bit.
- Player: `seeks_equal_a_fresh_forward_cut` (random seeks, five limit settings), `steps_by_move_show_the_blocks_of_a_new_player` (runs of steps forward and back by move; stock hash and all blocks equal a new player's), `steps_back_by_move_land_on_snapshots`, `a_jump_back_covers_the_steps_after_it`.

## Measured

Release, Windows, 12 threads, `cargo run -p fluut-core --example m7 --release -- projects 0.1` (res 0.2 mm), two runs each, ms. "1 move": a step forward from the middle of the last toolpath. "Mesh, 300 moves": the mesh update alone for a jump of 300 moves (about all blocks). "Back avg / worst": 20 single steps back by move.

| Case | | 1 move | Mesh, 300 moves | Back avg / worst |
|---|---|---|---|---|
| `plate` | before | 5.0 / 6.8 | 87 / 108 | 8.9 / 46.8 and 9.3 / 43.9 |
| `plate` | after | 3.9 / 3.5 | 53 / 49 | 4.4 / 13.3 and 5.0 / 16.4 |
| `plate2d` | before | 19.3 / 18.1 | 92 / 101 | 22.4 / 90.5 and 26.0 / 112.6 |
| `plate2d` | after | 13.4 / 13.6 | 108 / 110 | 5.5 / 19.8 and 5.4 / 18.0 |

## Limits

- `plate2d` still takes 13 ms for a forward move: one long move dirties about 50 blocks, and the rest class (paint) of each vertex is a distance query against the part; that cost is unchanged. A full-stock jump on `plate2d` is not faster. Next steps would be a cheaper paint query or smaller blocks (which changes the mesh layout).
- The machine is shared; timings move by 10-20 % between runs.
- Snapshots stay within the 64 MiB budget, so a very large stock keeps fewer of them and a step back after a long jump can still cut from a checkpoint.
