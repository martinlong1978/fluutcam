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
