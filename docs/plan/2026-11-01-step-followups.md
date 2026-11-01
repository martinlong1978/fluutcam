# STEP Follow-ups Plan: Outer Loops and Parallel Parsing

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Items 2 and 4 of the [S1 report](spikes/s1-kernel-step.md) §6:
- choose the outer loop of faces bounded only by `FACE_BOUND` in the face's parameter plane (D03 §3.1), replacing the spike's "widest loop" rule;
- parse the DATA section in parallel toward the 300 MB/s of D04 §2.

Item 1 (model tolerance from the file) waits for a product-owner decision. Item 3 (second corpus) needs files from the product owner.

**Architecture:**
- `fluut-topo` gets a module `trim`: UV polylines of a face's loops from their p-curves, winding numbers, and `outer_loop`, which picks the loop that contains all the others. It is the first piece of the trim cache of D03 §3.1; the bucket grid and `classify` come later.
- `fluut-io-step`'s mapper keeps the widest-loop rule only as a fallback. After the p-curves are computed, `map_bodies` asks `trim::outer_loop` for each face that had no `FACE_OUTER_BOUND`, moves the `outer` flag if needed and re-aligns that face's p-curves. `MappedBody::outer` counts what was decided and how.
- `fluut-io-step`'s parser splits each DATA section into chunks at entity boundaries and parses them with `rayon`. The chunks' keyword tables and entity lists are merged in file order, so the `RawModel` is the serial one bit for bit. When any chunk fails, the file is parsed again serially, so errors and their positions are always the serial ones.

**Tech Stack:** as S1; `fluut-io-step` adds `rayon` (already a workspace dependency).

**Design:** [`../design/D03-topology-bvh.md`](../design/D03-topology-bvh.md) §3.1, [`../design/D04-step-reader.md`](../design/D04-step-reader.md) §2–§3, [`../design/D01-workspace-architecture.md`](../design/D01-workspace-architecture.md) §6.

## Global Constraints

All S1 and p-curve follow-up Global Constraints apply. In addition:

- Work stays inside `fluut-io-step` and `fluut-topo` (plus `fuzz/` and docs). `fluut-topo` changes are additive.
- The D04 §5.2 tolerance clamp stays as it is.
- Determinism (D01 §6): the parallel parser gives a `RawModel` equal to the serial one, for any number of threads and any chunk size. Keyword ids keep their first-seen order.
- The D04 §2 limits (file size, string length, nesting depth) hold on the parallel path, and errors carry the same line and column as on the serial path.

## Review Focus

1. **Outer loop**: holes across a seam, loops that wrap around a period (cylinder bands) and loops through a pole give the right answer or fall back to the old rule; they never leave a face with no outer loop or two.
2. **Chunk boundaries**: a `;` inside a string, a comment or a binary never becomes an accepted split, whatever the input.
3. **Equality**: parallel and serial parsing agree on every input, success or error; a differential fuzz target checks it.

---

### Task 1: Plan

**Files:** Create `docs/plan/2026-11-01-step-followups.md`; modify `docs/plan/README.md`.

- [x] **Step 1:** Write this plan and add it to the plans table.

### Task 2: Outer loop by trim classification

**Files:** Create `crates/fluut-topo/src/trim.rs`, `crates/fluut-topo/src/trim/tests.rs`; modify `crates/fluut-topo/src/lib.rs`, `crates/fluut-io-step/src/map.rs`, `crates/fluut-io-step/src/map/tests.rs`, `crates/fluut-io-step/examples/s1.rs`.

- `trim::loop_polyline(b, loop)`: each coedge's p-curve sampled in its direction of travel and joined into one UV polyline. A gap between two p-curves (at a pole or apex) becomes a straight segment, which runs along the degenerate side.
- A loop **wraps** when its polyline ends a whole period (or more) from where it started: a circle around a cylinder. Wrapping loops contain nothing in the plane.
- `trim::winding(poly, p)`: winding number of a closed polyline about a point.
- `trim::outer_loop(b, face) -> Outer`:
  - **Contains:** the one closed loop that contains points of every other loop. A test point is shifted by whole periods into the candidate's window, so a hole across the seam still counts. Each other loop is judged by a majority of its polyline points, so points on or near the boundary do not decide.
  - **Wraps:** a face with wrapping loops (a band) has no loop that contains the others; one of the wrapping loops is the outer one, and the widest of those in 3-D is taken.
  - **Undecided:** missing p-curves, or no or several candidates. The mapper keeps the widest-loop choice.
- [x] **Step 1:** Tests: a square with two holes, listed hole first; a cylinder face with a seam and a round hole across the seam; a cylinder band with two wrapping circles; a sphere face whose outer loop is a small circle around the pole and whose hole is wider in 3-D (so the widest 3-D loop is not the outer one); a face with a missing p-curve is undecided.
- [x] **Step 2:** Fail, implement, pass.
- [x] **Step 3:** Mapper: faces without `FACE_OUTER_BOUND` are decided by `outer_loop` after `pcurve::compute`; on a change, `pcurve::align_face` runs again with the new outer loop first. `MappedBody::outer` gives faces decided by containment, by wrapping and by the fallback, and the faces where the choice differs from the widest loop (with their STEP ids). Test on a hand-written STEP face bounded by two `FACE_BOUND`s, the hole listed first and its 3-D box wider.
- [x] **Step 4:** Corpus: the `s1` example prints the counts and every disagreement with the old rule. Investigate each disagreement.

### Task 3: Parallel parsing

**Files:** Modify `crates/fluut-io-step/Cargo.toml`, `crates/fluut-io-step/src/parse.rs`, `crates/fluut-io-step/src/parse/tests.rs`, `crates/fluut-io-step/src/lib.rs`; create `crates/fluut-io-step/examples/parse_speed.rs`, `fuzz/fuzz_targets/parse_parallel.rs`; modify `fuzz/Cargo.toml`, `fuzz/README.md`.

- `parse(src)` parses in parallel when the DATA section is larger than a threshold; `parse_serial(src)` is the reference; `parse_chunked(src, chunk_bytes)` exposes the chunk size for tests and fuzzing.
- **Split:** the DATA section, up to the first `ENDSEC`, is cut about every `chunk_bytes` at a `;` followed by optional white space and `#`. A cut is a guess: it may fall inside a string or comment.
- **Why a wrong guess cannot be accepted:** each chunk is parsed by the serial parser's instance loop, starting from the chunk's absolute position (so lines and columns are absolute) and stopping at the chunk end. The first chunk starts where the serial parser is between instances. If the serial parser is between instances at a chunk's end, the chunk parses as it would serially; if not, the chunk ends inside an instance, string or comment and its parse fails. By induction, all chunks succeed only when every cut is a real entity boundary. Any failure falls back to `parse_serial`.
- **Merge** in file order: each chunk's keywords are interned into the model's table in the chunk's first-seen order (equal to the serial order) and its keyword ids remapped; entities are appended; the index is built from sorted `(id, slot)` pairs. A duplicate id falls back to `parse_serial` for its error.
- Line numbers at chunk starts: newlines counted per chunk in parallel, then summed.
- [ ] **Step 1:** Tests: `parse_chunked` with chunk sizes from 1 byte up equals `parse_serial` on the fixtures and on snippets with `;#` inside strings, comments and binaries, several DATA sections, complex instances, typed values; errors (unterminated string at the end, duplicate id across chunks, bad token in a late chunk, nesting too deep) equal the serial ones, positions included.
- [ ] **Step 2:** Fail, implement, pass.
- [ ] **Step 3:** Fuzz target `parse_parallel`: `parse_chunked` with a chunk size from the first input byte must equal `parse_serial`, `Ok` or `Err`. Run it, and the `parse` target, for a while; record executions, coverage and findings.
- [ ] **Step 4:** Example `parse_speed`: MB/s on the corpus with 1, 2, 4 and all threads (a `rayon` pool per run), serial for reference, and a check that every file gives the serial `RawModel`.

### Task 4: Report and design updates

**Files:** Modify `docs/plan/spikes/s1-kernel-step.md` (dated "Follow-ups" section), `docs/design/D03-topology-bvh.md` §3.1, `docs/design/D04-step-reader.md` §2–§3, `docs/plan/README.md`.

- [ ] **Step 1:** Report: outer-loop counts and disagreements; parse speed per thread count; fuzzing.
- [ ] **Step 2:** Update D03 §3.1 and D04 §2–§3 where the findings change them; mark this plan done.
