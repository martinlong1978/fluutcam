# STEP P-curves and Fuzzing Report

Date: 2026-11-01. Plan: [`../2026-11-01-step-pcurves-fuzz.md`](../2026-11-01-step-pcurves-fuzz.md).
Follows the [S1 report](s1-kernel-step.md) §5 (CH-08) and D04 §10.

Code:
- `fluut-geom`: `Curve2` and `BSplineCurve2` (`curve2.rs`);
- `fluut-topo`: `BrepBody::pcurves` and the module `pcurve` (projection, seam alignment, check);
- `fluut-io-step`: `PCURVE` mapping in `map.rs`, statistics in `MappedBody::pcurves`;
- `fuzz/`: `cargo-fuzz` targets `parse` and `map`, outside the workspace.

Measurement: `FLUUT_STEP_CORPUS=<dir> cargo run -p fluut-io-step --example s1 --release`.
All numbers: release build, Linux, one thread, on the 33-file NIST corpus of the S1 report.

## 1. P-curves

Every coedge now has a p-curve that uses its edge's 3-D parameter, so `S(pc(t)) = C(t)`.
- **From the file** when its `SURFACE_CURVE` or `SEAM_CURVE` has a `PCURVE` on the face's surface. Coordinates are scaled by the surface's parameter units, the parameter is changed affinely where the file's differs from ours, and the curve is kept only if it lies within `tol_model` of the edge. The two p-curves of a seam go to the two coedges in the order that joins them to their neighbours.
- **Exact projection** for curves on planes, generator lines of cylinders and cones, circles about the axis of cylinders, cones, spheres and tori, and meridians of spheres and tori.
- **Sampled projection** otherwise: closest points (closed form on analytic surfaces), unwrapped across periods, a C¹ cubic B-spline through them, split until each piece maps within `tol_model / 2`. At a pole or apex the meaningless coordinate comes from the neighbouring samples.
- **Seams:** each face's p-curves are shifted by whole periods so that each starts where the previous coedge ends; joints at poles and apexes do not count. On closed but non-periodic B-spline surfaces, shifts stay inside the domain.
- **Check:** `|S(pc(t)) − C(t)|` at 17 points per coedge.

### Corpus results

| Measure | Result |
|---|---|
| Coedges | 38 203 |
| P-curves in the files | 0 (as S1 found) |
| Exact / sampled | 34 288 / 3 915 |
| Within `tol_model` at all 17 points | **38 102 (99.74 %)** |
| Within the file's declared uncertainty | 38 172 (99.92 %) |
| Projection time, whole corpus | 0.64 s (mapping without it: about 0.1 s) |

Coedges by surface kind: plane 17 108, cylinder 14 977, cone 2 708, torus 1 574, sphere 1 059, B-spline 777.

**Failures by surface kind:** cylinder 78, B-spline 18, sphere 5; none on planes, cones or tori.
- **Every failure is a 3-D edge that lies off its surface by more than `tol_model`.** In each case the closest point on the surface is further from the edge than the tolerance, so no p-curve could be within it. The projection itself never failed.
- 70 of the 101 are in the three files that declare 0.05–0.13 mm uncertainty, which the D04 §5.2 clamp lowers to 0.01 mm (S1 report §6). They are within the files' own tolerance.
- 31 are beyond the declared uncertainty, in five files: up to 0.05 mm in `ctc_04` (declared 0.005 mm), 0.021 mm in `ctc_05` (0.017 mm), and 0.005–0.014 mm in `ftc_11`, `stc_07` and `stc_10` (0.005 mm). The S1 check missed most of them because it tested only edge midpoints.

### Bugs the corpus found

Each has a regression test.
1. **Closed B-spline edges had a zero-length range.** An edge whose start and end vertex coincide got `t0 = t1` from the closest-point query unless the curve was a circle or ellipse. It now spans the curve's domain when the curve is closed. This was an S1 mapping bug; the corpus has 36 closed B-spline edges in three files, and those in two of them were hit.
2. **A range a few ulps long** gave repeated sample parameters, NaN tangents and then a panic in `Surface::closest`. Repeated parameters are now dropped.
3. **Seam alignment on closed, non-periodic B-spline surfaces** shifted p-curves by a domain length, out of the domain, where evaluation clamps. Shifts on such surfaces now keep the p-curve inside the domain.

### What stays open

- **Edges that cross the seam of a closed, non-periodic B-spline surface** cannot have one p-curve inside the domain. None occur in the corpus. They are reported as failures; the importer will have to split them.
- **Faces bounded only by edges through poles** (a sphere lune) get their p-curves in some period window, not necessarily the one the face occupies. The trim cache (D03 §3.1) picks the window per face.
- **File p-curves** are tested only on synthetic files: the corpus has none. A second corpus with p-curves (S1 report §6) is still needed.

## 2. Fuzzing

Targets (see `fuzz/README.md`), seeded with the four unit-test fixtures and a dictionary of Part 21 tokens and entity names:
- `parse`: any bytes into the parser;
- `map`: bytes that parse, into schema, census and mapping with p-curves.

| Run | Target | Time | Executions | Coverage (edges) | Result |
|---|---|---|---|---|---|
| 1 | `parse` | 3.5 min | 2.1 M | 1 011 | **panic** in string decoding |
| 1 | `map` | 10 min | 1.75 M | 3 319 | no crash |
| 2 (after the fix, corpora kept) | `parse` | 15 min | 5.96 M | 1 050 | no crash |
| 2 (`map` also seeded with the `parse` corpus) | `map` | 15 min | 2.93 M | 3 409 | no crash |

Settings: `-max_len=16384 -timeout=10 -rss_limit_mb=2048`, four cores shared by both targets.

**Found and fixed:**
1. **`\X2\X0\` in a string** (parser). The end marker `\X0\` shares its backslash with the opening `\X2\`, so the search found it inside the opening and the body slice ran backwards. The search now starts after the opening.

No hang (10 s limit per input) and no allocation above 2 GB was reported. Coverage was still rising slowly at the end of run 2, so longer runs (in CI, D14) are worth having; 25 minutes per target is a smoke test, not a proof.

Limits that bound the work on untrusted input were already in the parser (string length, nesting depth, file size) and the mapper (curve nesting, knot multiplicities ≤ 64). The projection adds at most 2 048 refinement samples per edge.

## 3. Design updates

- CH-08: failure rate measured; status changed (see `challenges.md`).
- D04 §5.2 tolerance clamp: unchanged. It is an open product decision; §1 gives the numbers it affects.
