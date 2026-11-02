# Design Challenges

Problems found during detailed design. Each has a status:

- **deferred** — the design works around it for now; revisit at the end of the design pass.
- **blocking** — design cannot continue on that topic without a decision; raised with the
  product owner.
- **resolved** — decided; the resolution is recorded here and in the affected documents.

| Id | Title | Raised in | Status |
|---|---|---|---|
| CH-01 | Cross-platform floating-point determinism | D01 §6 | resolved |
| CH-02 | Document data structure and undo memory | D01 §3.1 | deferred |
| CH-03 | Tangential and degenerate plane sections | D02 §5, D03 §5.1 | handled for waterlines (M6); open for plane sections |
| CH-04 | Self-intersecting offsets of free-form surfaces | D02 §4.3 | deferred |
| CH-05 | Silhouette outline robustness for free-form faces | D03 §5.3 | deferred |
| CH-06 | Mesh region grouping for selection | D03 §1.2 | deferred |
| CH-07 | Reference results for the STEP test corpus | D04 §10 | resolved |
| CH-08 | P-curves on B-spline faces near seams and singularities | D04 §5.2 | resolved |
| CH-09 | DXF reader coverage (own reader chosen; corpus check open) | D05 §3 | deferred |
| CH-10 | EPS import dropped from v1 (design assumption) | D05 §4 | resolved |
| CH-11 | Definition version scheme | D06 §4.2 | resolved |
| CH-12 | Free-form face contact convergence and seeding | D07 §4.3 | deferred |
| CH-13 | B-spline boundaries through a lines-and-arcs offset library | D08 §3.1 | deferred |
| CH-14 | 3-D finishing toolpaths do not update in-process stock | D08 §7 | deferred |
| CH-15 | Reframing layered stock between indexed Setups | D08 §7 | resolved (conservative reframe) |
| CH-16 | Adaptive clearing: robustness and speed | D09 §2.3 | deferred |
| CH-17 | Exact medial axis for lines and arcs | D09 §2.6 | resolved (M6) |
| CH-18 | Scallop variable-distance offsetting | D09 §4.7 | deferred |
| CH-19 | Offsets in unrolled rotary coordinates | D09 §5.1 | deferred |
| CH-20 | Cutter-compensation events missing from the spec | D10 §4.6 | resolved |
| CH-21 | Vectric value-format field semantics | D10 §6.1 | resolved |
| CH-22 | Simulation memory and speed at fine resolution | D11 §2 | deferred |
| CH-23 | Live pass preview cost while dragging | D13 §5.3 | deferred |
| CH-24 | Mirror transforms: face sense and p-curves | D02 §1, D03 §2 | resolved |

## End-of-pass review (2026-10-31)

No challenge blocked the design. Six needed product-owner decisions and are resolved; the rest are deferred. Grouped by what resolves them:

**Product-owner decisions** — all resolved 2026-10-31 (CH-01, CH-07, CH-10, CH-11, CH-20,
CH-21); see each challenge's resolution below.

**Resolved by early prototypes or spikes** (implementation phase, time-boxed)

| Spike | Challenges |
|---|---|
| Kernel / STEP spike (OQ-01) | CH-03, CH-04, CH-08, CH-12 |
| 2-D geometry spike (offsets, medial axis, adaptive) | CH-13, CH-16, CH-17, CH-18, CH-19 |
| Importer spike | CH-09 |
| UI prototype (OQ-09) | CH-02, CH-23 |
| Simulation prototype | CH-22 |

**Deferred to later features** (acceptable in v1 as designed)

| Id | Note |
|---|---|
| CH-05 | Fallback outline with warning |
| CH-06 | Adjustable grouping, brush/lasso selection |
| CH-14 | Conservative rest stock after finishing |
| CH-15 | Conservative stock between indexed Setups |

## Details

### CH-01 Cross-platform floating-point determinism

**Problem.** D01 §6 requires bit-identical results on all platforms. IEEE-754 basic operations
(+ − × ÷ sqrt) are deterministic, but `sin`, `cos`, `acos`, `atan2`, `exp` call the platform libm,
which can differ in the last bit between Windows (MSVC CRT), macOS and Linux (glibc/musl). FMA
contraction by the compiler can also change results.

**Impact.** Golden-file post tests and cache keys could differ by platform. Toolpath geometry
differences would be ~1e-16 relative, far below tolerance, but bitwise comparison fails.

**Workaround for now.** Use a pure-Rust libm (`libm` crate) for all transcendental functions in
`fluut-geom` and `fluut-cam`, and disable FMA contraction (Rust does not contract by default).
Golden tests compare G-code text, where rounding to output decimals hides last-bit differences
in most cases.

**Revisit.** Measure the speed cost of `libm` vs platform functions; decide whether
cross-platform bit-identity is required or only same-platform determinism.

**Resolution (2026-10-31).** Bit-identical results on all platforms are required. All transcendental maths in `fluut-geom`, `fluut-cam`, `fluut-sim` and `fluut-post` uses the pure-Rust `libm` crate; platform `f64` methods for sin/cos/etc. are banned there by a clippy `disallowed_methods` lint. The CI determinism job compares hashes across platforms (D14 §3).

**S3 finding (2026-10-31).** `cavalier_contours` calls platform trigonometry (36 sites, offsets and intersections included). It is used through a copy patched to call `libm` ([S3 report](../plan/spikes/s3-region-2d.md) §3).

**S3 follow-up (2026-10-31).** Done: the workspace uses `third_party/cavalier_contours`, patched to call `libm` ([S3 report](../plan/spikes/s3-region-2d.md#follow-ups-2026-10-31)). The risk is real: on one Linux machine, glibc and `libm` already give different bits for the crate's arc helpers, and the S3 adaptive runs change pass counts with the patch alone. `fluut-cam/tests/determinism.rs` pins offset, boolean and arc-helper results bit by bit, so the CI determinism job (D14 §3) will catch platform differences once it runs on all three platforms.

### CH-02 Document data structure and undo memory

**Problem.** D01 §3.1 needs cheap snapshots of the project for background jobs, and D06 needs
undo. Options: persistent collections (`im`, `rpds`), `Arc` copy-on-write per Setup/Toolpath, or
command-based undo with mutable state plus explicit snapshots.

**Impact.** Affects every edit path, script API and UI binding.

**Workaround for now.** D06 designs the document as a tree of `Arc` nodes with copy-on-write at
Setup and Toolpath granularity (project documents are small: geometry is shared separately).
Undo stores previous root `Arc`s.

**Revisit.** Validate memory and speed with a 200-toolpath project during the UI prototype.

**S4 finding (2026-11-01).** Not exercised: the S4 prototypes have no document model, edits or undo ([S4 report](../plan/spikes/s4-ui-toolkit.md#9-challenges)). They only showed that a 200-toolpath browser costs the UI about 5 ms per frame (egui; slint with Skia). Memory and speed of the `Arc` tree and undo move to M4 (`fluut-model`), with the 200-toolpath project as the test.

### CH-03 Tangential and degenerate plane sections

**Problem.** Roughing and waterline levels are placed exactly at the heights of flat faces
[S 04 §4.4–4.5]. A section plane at exactly such a height is tangent to that face, and often
tangent to fillets and cylinder tops that meet it. Marching and chaining are ill-conditioned
there: curves degenerate to areas or points.

**Workaround for now.** `section_plane` returns coincident planar faces separately (`flats`) and
sections at `z ± δ` (δ = 10·ε_s) on the side the strategy needs (just above for "floor" levels).
Loops from the offset plane are then snapped to the exact flat-face boundary edges where they
coincide within tol_model.

**Revisit.** Verify on a corpus of parts with flats, fillets and bosses; consider a symbolic
treatment of tangency for analytic faces (they are the common case).

**Implementation note (M1, 2026-10-31).** Analytic sections flag tangent curves and touch
points. The free-form marcher seeds on an 8×8 cell grid per patch, so a small closed loop
lying entirely inside one cell (a bump top just cut by the plane) is missed. Fix candidates for
S1: seed also from critical points of `n·S` (interior extrema), found by Newton from each cell.

**S1 (2026-10-31).** Not exercised: no plane sections were run on corpus bodies. S2 and M3 use the S1 bodies for this.

**M6 waterlines (2026-11-01).** `contour`, `horizontal` and the steep part of
`steep_and_shallow` take their constant-Z contours from the drop function τ(x, y), not from
plane sections ([plan](../plan/2026-11-01-m6-3d-finishing.md)). Findings:
- *Tangent levels.* A level at the Z of a horizontal face makes τ = z on an area under a flat
  or bull tool, so classifying τ against z is noise. Every level is classified against
  z + 10⁻⁶ mm, the "just above" side of the workaround above; the CL point is output at z.
  A floor at the level then gives the foot of its walls, and a plateau top at the level gives
  nothing. Tested on the dome's block top (Z 10) with flat, bull and ball tools: the contour
  is the boss wall at R, with no loops on the flat.
- *The offset of δ.* On shallow ground the contour moves sideways by δ / tan β: 1.6 · 10⁻⁶ mm at
  32° in the tests. That is far below any tolerance.
- *Tangent peaks.* A level just below a smooth top gives a loop that can be smaller than the
  seed-line spacing (R/2) and be missed. The material left is within about R²/(32 ρ) of the
  top, on ground that is shallow there.
- *Model noise above δ (2026-11-02).* On `nist_ctc_02` the drafted walls' top edges stand up
  to 0.9 µm above the flat they meet at Z 0, so the level at Z 0 finds slivers a micron high
  that no trace can follow. A lost contour around material less than tol/4 high for a step
  inwards is now dropped, not reported as `cam.contour.loop_open`
  ([plan](../plan/2026-11-01-waterline-robust.md)).
- *Cliffs.* At a vertical wall τ jumps, and the drop's contact is on top of the wall, not at
  the level. Slope and drive face are then taken from the face the side of the tool touches
  at the level. Without this, the drive faces of a boss were mixed up at the level.
- *Inside corners.* Correcting along the contact direction finds no boundary just before a
  sharp inside corner. A circle search around the last point (from the open side to the
  material side) fixes it.

Plane sections (D03 §5.1) are still owed for `pocket_3d` and `adaptive_3d`; these findings
apply to them too.

### CH-04 Self-intersecting offsets of free-form surfaces

**Problem.** The offset of a concave free-form region by a tool radius larger than its radius of
curvature folds over itself. Treating the offset surface as the CL surface then gives gouges or
spurious loops.

**Workaround for now.** Cutter queries never build explicit offset surfaces for free-form faces.
They solve the contact condition directly and take the maximum over all contacts (D07), which is
fold-free by construction. Explicit offsets are used only for analytic faces, where validity is
checked (D02 §4.3).

**Revisit.** Sections of CL surfaces for waterline on free-form faces (D09) may need explicit
offsets; check whether contact-based sampling + fitting is fast enough there.

**S1 (2026-10-31).** Belongs to S2 (cutter queries); S1 provides the bodies and face BVH it needs.

**S2 (2026-11-01).** Confirmed for drop queries: no offset surface is built for any face. The contact condition (D07 §4.3) handles concave faces without folds: a radius-5 bowl under a radius-3 ball, and the concave torus fillets of the corpus, match the brute-force reference. Waterline (D07 §7) was not built, so the "Revisit" item stays open ([S2 report](../plan/spikes/s2-cutter-queries.md) §4).

### CH-05 Silhouette outline robustness for free-form faces

**Problem.** The drive-face outline needs silhouette curves (n·z = 0) of free-form faces and a
2-D union of many projected regions. Silhouettes can have cusps and tiny loops; unions of
near-coincident boundaries are fragile.

**Workaround for now.** Outline = union of projected face boundaries plus silhouettes computed by
marching on n·z = 0, simplified by biarc fitting at `machining/2`. Where the union fails, fall
back to the outline of the projected face bounds offset by the tool radius and report a warning.

**Revisit.** Test on organic models (relief, mould cavities).

### CH-06 Mesh region grouping for selection

**Problem.** STL meshes have no faces, so selectable regions are made by a dihedral-angle flood
fill. A fixed 20° threshold over-merges smooth organic meshes (one region) and over-splits coarse
cylinders (many regions).

**Workaround for now.** 20° default, user-adjustable per body; selection also supports brush and
lasso on triangles.

**Revisit.** Combine with mesh-to-analytic recognition when OQ-02 is implemented.

### CH-07 Reference results for the STEP test corpus

**Problem.** STEP import tests need reference face counts, areas and topology checks from an
independent kernel. Rust-only (DC-03) means no OCCT in the product, but the tests could still use
one.

**Workaround for now.** Generate reference fixtures once with an external tool (e.g. a FreeCAD
or OCCT script run by a developer, outside the product build), and commit the fixtures. CI only
compares against the stored fixtures, so no C++ enters the build.

**Revisit.** Confirm this is acceptable under the FFI policy (OQ-08): it is a test-data tool,
not a dependency.

**Resolution (2026-10-31).** Allowed. Developers may generate STEP reference fixtures with an external tool (e.g. a FreeCAD/OCCT script) outside the product build. Fixtures are committed; no C++ enters the build or CI.

### CH-08 P-curves on B-spline faces near seams and singularities

**Problem.** Files that omit p-curves force projection of 3-D edges onto surfaces. Near seams of
periodic B-splines and near singular points (poles of revolved surfaces, collapsed edges),
projection is ambiguous and Newton can jump across the seam.

**Workaround for now.** Project with continuity: seed each sample from the previous sample's
(u,v), unwrap across the period, and check against neighbouring coedges' p-curves. Faces where
this fails are excluded and reported.

**Revisit.** Measure the failure rate on the corpus during the OQ-01 spike.

**S1 (2026-10-31).** All 33 NIST corpus files omit p-curves, so projection runs on every face, not on a minority. The failure rate is still unmeasured; it is the first task of the M4 importer ([S1 report](../plan/spikes/s1-kernel-step.md) §5).

**Resolution (2026-11-01).** The workaround is implemented in `fluut_topo::pcurve` and measured ([report](../plan/spikes/step-pcurves-fuzz.md) §1). Exact p-curves where the image is known (34 288 coedges), otherwise sampled closest points with continuity, coordinates at poles and apexes taken from neighbours, and a C¹ cubic fit to `tol_model / 2` (3 915 coedges). Each face's p-curves are shifted by whole periods to join across seams. On the corpus, 99.74 % of 38 203 coedges are within `tol_model` and 99.92 % within the file's declared uncertainty, in 0.64 s. All 101 failures are edges that lie off their surface, not projection failures. Left open, reported as failures when they occur (none in the corpus): an edge crossing the seam of a closed, non-periodic B-spline surface, which must be split. Faces bounded only by edges through poles get p-curves in some period window; the trim cache (D03 §3.1) chooses the face's window.
**S2 (2026-11-01).** S2 needs trims for contact tests, so it projects every face loop into (u, v): closest point, then Gauss–Newton along the loop, unwrapping across seams, filling in at poles and closing through a pole. All 7 902 corpus faces get a trim, after a mapper fix for closed B-spline edges. Accuracy is the polyline chord: contacts up to about `chord × slope` outside a face are accepted (1e-3 mm chord gave differences up to 1e-3 mm against the reference). This does not test p-curve projection near B-spline seams, which the corpus barely has ([S2 report](../plan/spikes/s2-cutter-queries.md) §1, §4).

### CH-09 DXF reader coverage

**Problem.** The `dxf` crate may not handle all real-world files (binary DXF, newer versions,
malformed exports).

**Workaround for now.** Use it in the spike; fall back to an own group-code reader if coverage is
poor.

**Revisit.** After running the DXF corpus.

**M7 (2026-11-01).** An own reader was written instead of trying the `dxf` crate. Group-code
pairs are simple, and an own reader bounds every count (entities, block expansion, points) and
needs no new dependency. It covers the D05 §3 entities, blocks nested 16 deep and OCS. Binary
DXF is reported as unsupported. It is tested on hand-written files and fuzzed, but not yet on
exports from LibreCAD, Inkscape, AutoCAD or Fusion. That corpus check is what remains open
([plan](../plan/2026-11-01-m7-io-2d-mesh.md)).

### CH-10 EPS import dropped from v1 (design assumption)

**Problem.** The spec left EPS open. Parsing PostScript properly needs an interpreter; only simple
"flattened" EPS is practical.

**Workaround for now.** Design assumes no EPS in v1; SVG and DXF cover 2-D input.

**Revisit.** Product owner to confirm at the end of the design pass.

**Resolution (2026-10-31).** Confirmed: no EPS import in v1. Spec updated [S 02 §2.6].

### CH-11 Definition version scheme

**Problem.** The spec gives definitions an integer `version` that increases on every saved
change [S 03 §3.7], but its examples use ranges like `@^1` [S 02 §2.2], which only make sense
with a major/minor split. With integers, any edit to a base post is indistinguishable from a
breaking change, so `extends` must either pin exactly (no fixes flow through) or accept anything
(silent output changes).

**Workaround for now.** Design uses `major.minor` versions: minor = additive or fix, major = may
change output. `@3` = latest 3.x. Saved user edits bump the minor automatically; the user chooses
"breaking change" to bump the major.

**Revisit.** Product owner to confirm; then update [S 02 §2.2–2.3], [S 03 §3.7], [S 05 §5.2].

**Resolution (2026-10-31).** Confirmed: definition versions are `major.minor`. Spec updated [S 02 §2.2–2.3, S 03 §3.7].

### CH-12 Free-form face contact convergence and seeding

**Problem.** The general face-interior contact is a 2-D Newton solve on a non-convex function.
Poor seeds converge to a non-maximal stationary point (missing the true contact → gouge) or fail.
Bull-nose tools on saddle-shaped faces are the hardest case.

**Workaround for now.** Seeds from closest point and from every Bézier patch in the footprint;
branch-and-bound ensures every candidate whose upper bound exceeds the current best is solved;
non-convergence is reported, never ignored. The brute-force reference test (D07 §9) guards
correctness.

**Revisit.** Measure miss and failure rates on free-form corpus parts; consider interval
arithmetic on patches for guaranteed bounds if misses occur.

**S1 (2026-10-31).** Belongs to S2; S1 provides the bodies (176 B-spline faces in the corpus).

**Waterlines (2026-11-02).** The lost waterline traces on `nist_ctc_02` (188 of 391 contours at a 20 mm step-down) were drop misses, on analytic faces, not free-form ones. A Ø6 ball beside a cylinder wall drafted 2° touches it at the rim of its footprint. The face's tangent point lay just above its trimmed top, and the top edge, a B-spline 3 µm off the surface, lay just out of reach, so neither solver found the top of the wall and the drop fell 38–128 mm over a band about 3 µm wide. A face whose best stationary point is trimmed away now searches its own boundary (the trim loops on the surface); the result counts where it beats every other contact by more than the trim chord, so models without gaps give the same drops. A synthetic drafted band with a 3 µm edge gap is the regression test ([plan](../plan/2026-11-01-waterline-robust.md)). Model gaps meeting near-vertical walls are the case to watch: there a micron of gap moves τ by the wall's height.

**S2 (2026-11-01).** Measured. The final seeding: targets in the footprint projected vertically onto the surface, starts from closed-form closest points (analytic) or a 4 × 4 patch sample (B-spline), and 3 Newton starts ranked by τ. It has no misses on 600 synthetic reference points. On 7 560 corpus points the remaining differences are at most 3.2e-5 mm: trim polylines and a verified model gap, plus five 1e-6 mm cases beside model gaps that were not traced. Four seeding bugs found by the corpus gave misses of 0.3–30 mm before they were fixed. One Newton start misses by up to 1.4 mm. About half of all Newton runs do not converge because the primitive has no interior contact, so non-convergence cannot signal a failure; D07's `NoConvergence` rule is suspended. Interval bounds were not needed. Stays deferred: the guard is the reference test plus the hard points kept as tests ([S2 report](../plan/spikes/s2-cutter-queries.md) §3).

### CH-13 B-spline boundaries through a lines-and-arcs offset library

**Problem.** `cavalier_contours` (OQ-06) handles lines and arcs only. B-spline boundaries are
biarc-fitted, so offsets and clips are computed on an approximation. Endpoint refinement onto
the exact curve (D08 §3.1) fixes endpoints, but region topology (e.g. whether two boundaries
touch) is decided on the approximation.

**Workaround for now.** Fit at `machining/4`; refine every clip point on the exact source curve;
report when refinement moves a point by more than the fit tolerance (indicates a topology
mismatch).

**Revisit.** If problems appear, extend the offsetter to native B-spline offsets locally, or
fit at a tighter tolerance near near-touching features.

**S3 (2026-10-31).** Nothing found changes the workaround. `Region2` gets per-segment source references in M3.

### CH-14 3-D finishing toolpaths do not update in-process stock

**Problem.** Layered 2-D stock (OQ-11) represents material per Z level. A 3-D finishing pass
removes material in a shape that is not level-based; converting its swept volume into level
regions is expensive.

**Workaround for now.** In v1 only roughing and 2-D toolpaths update stock. Rest machining after
finishing toolpaths uses the stock after the last roughing toolpath, which is conservative (more
air cutting, never missed material).

**Revisit.** Needed for `rest_finishing` (later): a per-face reach comparison method, not stock
layers.

**2026-11-01.** The in-process stock now also sets the rapid floor of the 2-D and roughing types
(D08 §8.2) and is carried into child Setups. Finishing toolpaths still do not update it, so the
floor after them is the roughed stock: higher, never lower, than the real material.

### CH-15 Reframing layered stock between indexed Setups

**Problem.** A child indexed Setup at a different angle needs the parent's remaining stock in its
own frame (OQ-15). Layered stock is defined per level in the parent frame; rotating it is not a
layered structure any more.

**Workaround for now.** Convert to prisms between levels, rotate, re-slice at the child's levels.
Conservative where the prism approximation keeps material between levels (stepped walls).

**Revisit.** Check air-cutting overhead on typical rotary-then-indexed jobs; consider finer
re-slicing near the parent's walls.

**2026-11-01.** Implemented as `LayeredStock::reframed` (D08 §7): exact for half turns, boxes
round each outer loop of the parent's slabs at other angles (holes ignored), slabs at most 1 mm
high; rotary parents become planar slices of their revolved outline. The simulation of a child
starts from the parent's simulated stock for multiples of 90°. Other angles and rotary parents
start the simulation from the full stock (`sim.stock.full`).

### CH-16 Adaptive clearing: robustness and speed

**Problem.** Constant-engagement clearing is the hardest v1 strategy. Published and open-source
implementations (e.g. FreeCAD's Adaptive) track the cleared area on an integer polygon grid,
which DC-02 rules out. Exact arc-based engagement computation must stay fast with thousands of
steps per level, and the marching must not stall in narrow necks or leave uncut slivers.

**Workaround for now.** Design as in D09 §2.3 with exact arc regions; a sliver-cleanup pass
(contour of `C \ K` at the end of each level); time-boxed prototype early in implementation.

**Revisit.** After a prototype: measure speed and sliver rate; if needed, simplify (e.g.
trochoidal slotting in necks, offset-pattern fallback with feed reduction).

**S3 (2026-10-31).** Prototype on exact arc regions works on 3 test shapes: path length near ideal, ≈0.2 % sliver area before the final contour pass, no grid. Engagement must also exclude the current pass's own earlier tool positions. Speed is 12–30 s per level against a target under 1 s; fixes are listed in the [S3 report](../plan/spikes/s3-region-2d.md) §5.

**S3 follow-up (2026-10-31).** The region booleans now use a spatial index and a junction rule and stay consistent on every union of the adaptive runs. Sweeps and unions take under 1 % of a level, so speed now depends only on the engagement fixes of §5. The prototype's statistics move by a few degrees of RMS error depending on where loops start, because start points are sampled along K's loops; M6 should choose start points in a way that does not depend on that ([S3 report](../plan/spikes/s3-region-2d.md#follow-ups-2026-10-31)).

**M6 (2026-11-01).** `adaptive_2d` is built on the spike ([plan](../plan/2026-11-01-m6-2d-holes.md)). A level on the S3 shapes takes 0.65–1.04 s, against a target under 1 s. Three findings:
- The spike's bisection could step into walls. Steps now follow the tool-centre region's edge, which raises the engagement RMS error to 18–25°.
- Thin slivers (up to about 0.4 mm) stay between passes. A rest phase at lower engagement and one short move per remaining sliver take them.
- `cavalier_contours` can return only empty loops when it offsets a region with tiny holes, and one long swept loop failed at R − 1e-6 although it offset at R. Cleared-region offsets therefore go loop by loop, with a retry up to 0.01 mm less deep.

Still open: fewer passes and links, and arc-fitted output.

### CH-17 Exact medial axis for lines and arcs

**Problem.** V-carving needs the medial axis of regions bounded by lines and arcs. Available Rust
Voronoi crates (e.g. `boostvoronoi`) work on integer points and segments only, which loses arcs
and quantizes (DC-02).

**Workaround for now.** Own implementation: incremental Voronoi of line and arc sites in
floating point with robust predicates (adaptive precision where needed), restricted to the
interior. Bisector types: line–line (line), line–arc (parabola), arc–arc (conic).

**Revisit.** Estimate effort after CH-16's prototype; consider deferring `engrave` to after v1 if
the medial axis is too costly.

**S3 (2026-10-31).** `cavalier_contours` offers nothing here; own implementation stands, estimated 3–5 weeks.

**M6 (2026-11-01).** `engrave` has its parameters and validation; `generate` reports `cam.type.unavailable` until the medial axis exists ([plan](../plan/2026-11-01-m6-2d-holes.md)).

**Resolved (2026-11-01).** `fluut-cam::region::medial` traces the axis edge by edge instead of building a full Voronoi diagram ([plan](../plan/2026-11-01-medial-axis-engrave.md)):
- Sites are lines and circles (arcs, and reflex vertices as circles of radius 0). An edge is stored as two sites and a parameter range, and evaluated exactly: for each parameter the clearance radius solves a linear equation.
- Vertices solve three signed-distance equations in closed form (Apollonius' problem: two linear equations and one quadratic), polished by Newton steps.
- Every vertex is built from the angular gaps between its touching feet. So squares, arc centres, discs and annuli need no special case.
- Property tests on fixed and random shapes (polygons, arcs, holes): every axis point is at its clearance from the boundary, with two feet; the largest empty disc found from random interior points lies on the axis; E − V + areas = holes. The output of the S3 shapes is pinned bit for bit.

Time, best of 5 runs on one thread (`cargo run -p fluut-cam --example medial_axis --release`, Linux container):

| Shape | Segments (arcs) | Edges | Time |
|---|---|---|---|
| S3 A plate | 6 (2) | 11 | 0.03 ms |
| S3 B rooms | 12 (0) | 17 | 0.08 ms |
| S3 C slotted disc | 6 (3) | 6 | 0.02 ms |
| Text, 1 group of 3 letters | 24 (12) | 39 | 0.24 ms |
| Text, 40 groups (120 letters) | 960 (480) | 1 560 | 24 ms |
| Scalloped badge | 300 (300) | 551 | 14 ms |
| Scalloped badge | 1 500 (1 500) | 2 751 | 278 ms |

Text grows linearly. The badge grows faster, because its sites are dense and many lie near each empty disc. The edges are exact. Only `engrave` samples them, within `machining_tolerance / 4` in X, Y and depth.

### CH-18 Scallop variable-distance offsetting

**Problem.** Constant 3-D stepover needs a 2-D offset whose distance varies with slope. The
chosen offset library does uniform offsets only.

**Workaround for now.** Uniform offset by the minimum distance per pass plus gap-filling passes
(conservative, more passes than ideal).

**Revisit.** Compare pass counts with Fusion on test parts; consider own variable offsetter.

**S3 (2026-10-31).** Confirmed: uniform offsets only. Workaround stands.

**M6 `scallop` (2026-11-01).** Built as the workaround: pass k+1 is the region offset by
s·cos θ, θ the steepest cut contact on passes k and k+1, rechecked up to four times;
`cam.scallop.extra_passes` reports a step reduced because the next pass is steeper. On a
vertical wall s·cos θ is zero, so no step is below s/10, and the 3-D stepover is not kept on
walls steeper than about 84°. That is the job of `steep_and_shallow`. Pass counts against
Fusion are still to compare.

### CH-19 Offsets in unrolled rotary coordinates

**Problem.** Rotary roughing works on the unrolled cylinder (x, r·θ). A disc of radius R on the
real cylinder is not a disc in unrolled coordinates: its angular extent depends on the radius
and the tool is not tangent to the unrolled plane. Uniform 2-D offsets by R are only accurate
when R ≪ r.

**Workaround for now.** Offset by the exact angular half-width at the layer radius
(Δθ = asin(R/r)) in the θ direction and R in x, using an anisotropic offset (scale θ, offset,
unscale); conservative rounding toward leaving material.

**Revisit.** Validate on small-diameter stock where R/r is large.

**S3 (2026-10-31).** Scaling θ turns arcs into ellipses, which a line-and-arc region cannot hold: linearize at `tol.machining / 4` before scaling and refit biarcs (D02 §7) after unscaling.

### CH-20 Cutter-compensation events missing from the spec

**Problem.** `contour_2d` offers compensation "in control" and "wear" [S 04 §4.13.7], which need
G41/G42/G40 output, but the post event list [S 05 §5.4.1] has no cutter-compensation blocks.

**Workaround for now.** Design adds `cutter_comp_left`, `cutter_comp_right`, `cutter_comp_off`
events (matching Vectric's blocks) and a `cutter-comp` mixin.

**Revisit.** Update [S 05 §5.4.1] at the end of the design pass.

**Resolution (2026-10-31).** Confirmed: `cutter_comp_left`, `cutter_comp_right`, `cutter_comp_off` events added to [S 05 §5.4.1].

### CH-21 Vectric value-format field semantics

**Problem.** Vectric's `VF` field (`FORMAT_FLAGS FIELD_WIDTH DECIMAL_SEPARATOR DECIMAL_PLACES`)
is documented briefly; real posts use forms like `1.4`, `1.0`, `+.3`, `0.1`. The exact meaning of
field width and flag combinations must be confirmed to convert posts byte-faithfully.

**Workaround for now.** Implement the documented interpretation; validate against a corpus of
public `.pp` files and, if possible, output samples produced by Vectric itself.

**Revisit.** Needs sample outputs from Vectric software (product owner may be able to supply).

**Resolution (2026-10-31).** Product owner will supply Vectric `.pp` files with matching G-code output; the importer is verified against them (D10 §9).

### CH-22 Simulation memory and speed at fine resolution

**Problem.** Tri-dexel memory grows with the square of resolution per family. Large stock at
fine resolution (e.g. 1 m sheet at 0.05 mm) exceeds memory budgets; full simulation of millions
of moves may be slow.

**Workaround for now.** Automatic resolution from a memory budget; region-of-interest
refinement; tile-based copy-on-write checkpoints.

**Revisit.** Benchmark on large router jobs; consider GPU removal or adaptive (octree) dexels.

**S5 finding (2026-10-31).** Prototyped in `fluut-sim` on synthetic relief (200 × 150 mm) and rotary (Ø60 × 150 mm) stand-ins ([S5 report](../plan/spikes/s5-simulation.md)). Memory is 24 B per ray plus 6–20 % for spilled spans. At 0.1 mm the relief takes 106 MB and the rotary 52 MB; at 0.05 mm about 425 MB and 210 MB, within the 512 MB budget. Time grows as 1/res².
- **Relief at 0.1 mm:** rough and finish (208 k moves) take 29 s on 4 cores.
- **Rotary at 0.2 mm:** 221 µs per move, about 6× over the D11 §7 target. No ray family follows the tool axis there.

The error between rays is set by sharp machined detail. The relief needs 0.1 mm for 0.05 mm error and 0.025–0.05 mm for 0.01 mm. The rotary wrap needs 0.4 mm for 0.05 mm and 0.1 mm for 0.01 mm. The tri-dexel stays for three-axis work; rotary Setups should try cylindrical dexels (radial rays) in M7.

**S5 follow-ups (2026-11-01).** Measured on a different 4-core machine ([report](../plan/spikes/s5-simulation.md#follow-ups-2026-11-01)):
- **Closed forms:** flat and bull ramps and plunges seen from the side take 70–150 ns per ray instead of about 3 µs (GJK). A fast path for ball end mills decides most lines from two capsules.
- **Rotary at 0.2 mm:** from 254 to 117 µs per move on the tri-dexel, now about 3× over the D11 §7 target.
- **Cylindrical dexels:** tried. They are only 14–27 % faster than the tri-dexel with the same error, and need 1.4× the memory, so they are not adopted. The tri-dexel stays for rotary Setups too.
- **Relief finish at 0.2 mm:** 34 µs per move.

Rotary speed is still open: the remaining cost is ray tests across the tool's shank on rotary chords.

**M7 finding (2026-11-01).** The product simulation ([D11 §10](D11-simulation.md#10-m7-implementation-2026-11-01)) runs the relief stand-in at 18 µs per move at 0.2 mm (49 at 0.1) on 12 threads and the reference projects in 0.1–1.3 s. Checkpoints for scrubbing (copy-on-write tiles, every toolpath and every 32 768 chords) cost 3–5× the bare stock: 473 MB for the relief at 0.1 mm, inside the 512 MB budget, which now counts 115 B per ray. Re-cutting to a scrub time takes 0.01–0.2 s on three-axis work (1.3 s on the rotary wrap), but meshing the whole stock takes 0.2–11 s: per-tile meshing of dirty tiles, or GPU meshing, is needed for interactive scrubbing. Rotary speed is unchanged.

### CH-23 Live pass preview cost while dragging

**Problem.** [S 08 §8.4] asks for a live ghost of the pass pattern while editing stepover, angle
or direction. A full calculation (drops, fitting, linking) is too slow to run per drag frame on
complex parts.

**Workaround for now.** The preview shows the 2-D drive pattern clipped to the region (D08 §3,
§6; cheap), draped onto the display mesh on the GPU, not the true CL path. The real calculation
runs when the drag ends (if auto-calculate is on).

**Revisit.** UI prototype: check that the draped preview is convincing and fast enough.

**S4 finding (2026-11-01).** Prototyped for raster patterns in both UI prototypes ([S4 report](../plan/spikes/s4-ui-toolkit.md#7-other-s-08-811-requirements)). The fragment shader draws lines at `stepover` across `pass_angle_deg`, draped on the display mesh, from two uniforms. Editing rebuilds no buffers. On 1 M triangles the cost was within noise on a software rasteriser. Scrubbing moves the lines in the next frame, and the result reads as the pass layout on the part. Offset (ring) patterns cannot be drawn this way: they need 2-D offsets from `fluut-cam` draped as lines. That case stays open for M5.

### CH-24 Mirror transforms: face sense and p-curves

**Problem.** Found in implementation (M1). D02 §1 and D03 §2 said a mirroring transform flips
the sense of every face. That holds only if the surface keeps its parametrisation, so that its
normal maps to the opposite of the mirrored normal. Analytic surfaces cannot keep it: their
frames must stay right-handed, so one parameter reverses and the normal stays outward. Scaling
also rescales the length-like parameters of planes, cylinders, cones and extrusions, which moves
p-curves.

**Resolution (2026-10-31).** `fluut-geom` exposes, per surface kind,
`Surface::transformed_param(a, u, v)` (parameter map) and `Surface::transform_flips_normal(a)`
(whether face sense must toggle). D03 §2 uses both; a property test checks them against point
and normal mapping for every kind.

### CH-25 Geometry kernel misses some D02 §8 benchmark targets

**Problem.** Found in M1 Task 7. Analytic eval, bicubic derivatives and analytic closest point
run at about 1.7–1.8× their targets. Bicubic B-spline closest point runs at 99 µs against a 5 µs
target. Drop-cutter (D05) calls closest point or its equivalent in its inner loop, so the
B-spline figure sets the cost of 3-D finishing on freeform parts.

**Workaround for now.** Branch-and-bound over Bézier patches by pole bounds, then damped
Newton. Correct and 11× faster than the first grid seed. The targets were estimates, not
measured requirements.

**Revisit.** Spike S2 (drop-cutter cost model). Candidates: subdivide patches until their
bounds are tight before seeding; seed from the previous query point (drop-cutter queries are
spatially coherent); cache Bézier-form patches to skip knot-span work. Set the target from the
S2 cost model rather than D02 §8's estimate.

**S2 (2026-11-01).** The drop never calls B-spline closest point in its inner loop: seeds come from vertical projection (2-D Newton), so CH-25's 99 µs does not set the drop cost. A B-spline patch solve costs 40 µs, and a dense free-form model needs about 5 per drop, giving 210–320 µs median against D07 §8's 20 µs. Warm starts from the previous drop and tighter patch bounds come first ([S2 report](../plan/spikes/s2-cutter-queries.md) §2, §5). Closest point itself remains as measured in M1.
