# S1 — Kernel and STEP Spike Report

Date: 2026-10-31. Plan: [`../2026-10-31-s1-kernel-step.md`](../2026-10-31-s1-kernel-step.md).
Code that is kept:
- `crates/fluut-io-step`: parser, schema and units, mapping;
- `crates/fluut-topo`: arena, checks and BVH.

Measurement tools:
- `cargo run -p fluut-io-step --example s1 --release` (set `FLUUT_STEP_CORPUS` first);
- `--example s1_skips` (per-file diagnosis);
- `spikes/s1-truck` (the `truck` comparison, outside the workspace).

All numbers: release build, Windows 11, one thread.

## Corpus

The NIST MBE PMI test files used by the CAx-IF: 33 files, 54.3 MB, 428 094 entities.
- **Schemas:** AP203 ed1 (3 files), AP203 ed2 (13) and AP242 (17).
- **Source and terms:** NIST says they may be used without restriction. They are kept outside the repository and described in the corpus `README.md`.
- **Exporters:** NIST blanked the originating-system fields. File dates, time zones and header styles point to at least five different exporters, but the CAD systems cannot be named from the files.

**Gaps in the corpus:**
- No p-curves at all. Every file omits them.
- Few free-form faces: 176 B-spline surfaces among 7 902 faces.
- No assemblies with placements.
- No FreeCAD, Onshape or Fusion exports.

The spec's "at least four CAD systems" is therefore probably met but not proven. A second corpus is needed before M4 import work (§6).

## 1. Own reader: results

| Measure | Result |
|---|---|
| Files parsed | **33 / 33** |
| Parse speed | 100 MB/s on one thread (D04 §2 target: 300 MB/s with parallel parsing, not yet tried) |
| Entity kinds inside bodies | all supported (D04 §5.1); census in the `s1` output |
| Bodies mapped | 63 / 63 (32 closed solids, 31 open shell-based surface models) |
| Faces mapped | **7 902 / 7 902** |
| Mapping time | 0.16 s for the whole corpus |
| Shells clean at `tol_model` (the declared uncertainty clamped to [1e-7, 1e-2] mm) | **59 / 63** (closed: 28 / 32) |
| Shells clean at each file's declared uncertainty | **61 / 63** |

**The four shells that are not clean:**
- **Edges off their B-spline or cylinder faces by 0.015–0.02 mm** (two files). The declared uncertainty is 0.12 mm and 0.093 mm, but the D04 §5.2 clamp lowers it to 0.01 mm. So the exporter worked within its own tolerance; our tolerance is tighter. These need either sewing or a model tolerance taken from the file, reported to the user as larger than CAM precision. This is a design question for M4 (§6).
- **An ellipse edge 2.4e-5 mm from its vertex** in a file that declares 2e-5 mm. The file is slightly outside its own tolerance.

**Bugs the corpus found in the spike reader:**
1. A non-unit `DIRECTION` (−0.03937, 0, 0). That is 1/25.4, so an exporter scaled directions by the inch conversion. It broke the fallback for a missing reference direction. Directions are now normalised before use.
2. **AP203 faces bounded only by `FACE_BOUND`**, with no `FACE_OUTER_BOUND`: 680 faces. The spike takes the widest loop as the outer one. The design's trim cache (D03 §3.1) must decide this in the parameter plane.

Both are fixed and covered by the corpus run.

## 2. `truck` on the same corpus

`truck-stepio` 0.3, which uses `ruststep`, with `truck-topology` 0.6:

| Measure | Result |
|---|---|
| Files parsed | **17 / 33**: 15 of the 17 AP242 files fail in `ruststep` |
| Parse speed | 18 MB/s, about 5.5× slower |
| Shells converted / extracted / closed | 23 / **9** / **4** |
| Panics | 0, guarded by `catch_unwind` |

Many circles fail to convert ("Failed to convert Circle"). Most shells that do convert cannot be extracted into a `truck_topology::Shell`.

## 3. NURBS evaluation

On an 8 × 8 bicubic surface, 200 000 random evaluations of the point and both first derivatives:

| | ns per evaluation |
|---|---|
| `truck-geometry` 0.5 | 882 |
| `fluut-geom` | **389** |

The two agree to 8.7e-14 in position and 5.4e-13 in the derivative.

## 4. Decision (OQ-01)

**Own Part 21 reader and own kernel. No `truck` crates.**

- `truck-stepio` reads half the corpus and gives a usable closed shell for 4 of 63 bodies.
- `truck-geometry`'s NURBS evaluation is correct but 2.3× slower than `fluut-geom`, which already passes its accuracy tests (D02 §9).

Under the OQ-01 rule ("borrowed if they pass the spike"), nothing is borrowed. This narrows option C to option A in spec 01 §1.4. No `truck` dependency enters the workspace; `spikes/s1-truck` stays outside it.

The own reader met the spike's exit criterion on this corpus:
- every file loads;
- 97 % of shells are clean at the file's declared tolerance;
- the rest are real data gaps, not reader errors.

## 5. Challenges

- **CH-03 (tangential sections).** Not exercised: the spike did not run plane sections on corpus bodies. S2 and M3 use these bodies for that.
- **CH-04, CH-12 (offsets, contact seeding).** These are about cutter queries and belong to S2. S1 provides the bodies and the face BVH that S2 needs.
- **CH-08 (missing p-curves).** Every corpus file omits p-curves, so the projection workaround runs on 100 % of faces, not on "many files". Its failure rate could not be measured yet, because the spike maps no p-curves. It is the first task of the M4 importer, measured on this corpus.

## 6. Follow-ups

1. **Model tolerance from the file.** Files declaring uncertainty above 0.01 mm have real gaps above 0.01 mm. D04 §5.2's clamp hides that. The design should take the file's value, warn when it exceeds a CAM limit, and try sewing.
2. **Outer loop by trim classification** (D03 §3.1), replacing the "widest loop" rule.
3. **Second corpus.** STEP exports from FreeCAD, Onshape, Fusion, SolidWorks and Inventor, with free-form parts and assemblies. This needs files from the product owner or downloads under clear terms.
4. **Parallel parsing** toward 300 MB/s, once larger files exist in the corpus.

## Follow-ups (2026-11-01)

Plan: [`../2026-11-01-step-followups.md`](../2026-11-01-step-followups.md). Items 2 and 4 of §6; item 1 waits for a product decision and item 3 for files.
All numbers: release build, Linux, 4 cores, same 33-file corpus.

### Outer loop by trim classification (§6 item 2)

Code: `fluut_topo::trim` (UV polylines of loops, winding numbers, `outer_loop`); in `fluut-io-step`, `MappedBody::outer`. D03 §3.1 now describes the rule.

For each face without a `FACE_OUTER_BOUND`, the mapper first takes the widest loop in 3-D. Once the p-curves are known, it decides in the parameter plane:
- **Containment:** the one loop that contains most points of every other loop. Points are moved by whole periods into the loop's window, so a hole across a seam counts.
- **Band:** a loop that ends a whole period from its start (not across a pole) wraps round the surface and contains nothing. The widest wrapping loop is outer.
- **Undecided:** the widest loop is kept.

The face's p-curves are re-aligned when the outer loop changes.

| Faces bounded only by `FACE_BOUND` | Count |
|---|---|
| All | 1 202 |
| One loop (nothing to choose) | 522 |
| Several loops | **680** |
| … decided by containment | 171 |
| … bands | 509 |
| … undecided | **0** |
| … different from the widest-loop rule | **0** |

- **No disagreements.** On this corpus the spike's rule was right every time. The new rule matters for cases the corpus lacks, such as a sphere face whose hole is wider in 3-D than its outer loop. That case has a unit test.
- **One bug found on the way.** The first run left 5 B-spline faces in `ctc_02` undecided. Their surfaces are closed but not periodic. To stay inside the domain, the p-curve of a loop round such a surface jumps back by the domain length where it crosses the seam. The polyline now undoes such jumps, and a regression test (a square tube) covers it. These p-curves cross the seam, which the [p-curve report](step-pcurves-fuzz.md) said never happens in the corpus. Sampled at 1 000 points, one of them is exact to 1e-12 mm, because the jump falls on the seam. Another is 0.0105 mm from its edge between the 17 check points, although the check found it within `tol_model` (0.01 mm) at those points. So the p-curve check can miss small excesses between its points.
- **`FACE_OUTER_BOUND` is not always right.** The same classification was also run on the 530 faces that declare an outer loop and have more than one loop:
  - in `nist_stc_06_asme1_ap242-e3.stp`, 23 planar faces declare the hole as the outer bound. For example, an r = 5.08 mm circle is declared outer to a concentric r = 22.2 mm circle;
  - in 4 torus bands, the declared outer loop is the narrower one. That is a convention, not an error.

  The mapper still trusts `FACE_OUTER_BOUND` (out of scope here). D03 §3.1 now says that the trim cache decides the outer loop for every face, and reports disagreements with the file.

### Parallel parsing (§6 item 4, D04 §2)

Code: `fluut_io_step::parse` (parallel), `parse_serial`, `parse_chunked`; `--example parse_speed`; fuzz target `parse_parallel`. D04 §3 now describes the method.

- **Cuts.** Each DATA section is cut about every max(128 KiB, size / (4 × threads)) bytes, after a `;` that is followed by white space and `#`. The chunks are parsed with `rayon`.
- **Why a wrong cut is never accepted.** No pass looks for strings and comments first, so a cut may land inside one. Each chunk runs the serial parser from its absolute position, with the chunk end as end of input. A chunk that does not end between two instances therefore fails. Any failure, or a repeated id, re-parses the file serially. So errors, positions and limits are always the serial ones.
- **Merge.** Chunks are merged in file order, with keywords interned in first-seen order, so the `RawModel` equals the serial one.

`--example parse_speed`, whole corpus (54.3 MB) held in memory. Each figure is the best of 5 parses, and the table gives the median of three such runs. Models are dropped outside the timing. Every parallel result was checked equal to the serial one.

| Threads | Corpus | Largest file (6.1 MB) |
|---|---|---|
| serial (`parse_serial`) | 154 MB/s | — |
| 1 | 150 MB/s | 202 MB/s |
| 2 | 240 MB/s | 353 MB/s |
| 4 (= N here) | **338 MB/s** | **529 MB/s** |

So the D04 §2 target of 300 MB/s is reached on four cores. Smaller files gain less: each file pays a fixed cost to merge chunks and build the index on one thread. The S1 figure of 100 MB/s came from Windows.

**Changes on the serial path:**
- Line numbers after a binary that spans lines were one short per line break (bug, now tested).
- Keywords are interned without allocating, ids are parsed directly, and parameter lists are allocated once, at their size. Together these make the serial path about 4 % faster.

**Fuzzing** (`-max_len=16384 -timeout=10 -rss_limit_mb=2048`, seeds and corpora as in the p-curve report, both targets running at once on four cores):

| Target | Time | Executions | Coverage (edges) | Result |
|---|---|---|---|---|
| `parse_parallel` (3 workers; chunk sizes 1–256 bytes) | 25 min | 1.83 M | 2 417 | no difference from serial, no crash |
| `parse` | 25 min | 14.4 M | 1 615 | no crash |

Tiny chunks make cuts inside strings, comments and binaries common, so the retry path got heavy use.

### What stays open

- `FACE_OUTER_BOUND` is still trusted on import; the trim cache should decide the outer loop for every face (D03 §3.1).
- Faces on a sphere or torus with several holes and no loop round the surface (a sphere with two holes) have no containing loop. None are in the corpus without a `FACE_OUTER_BOUND`. They would fall back to the widest loop.
- The p-curve check uses 17 points per edge and can miss small excesses between them (see above).
- Item 1 (model tolerance from the file) and item 3 (second corpus) as in §6.
