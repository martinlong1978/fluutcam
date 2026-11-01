# D04 — STEP Reader (`fluut-io-step`)

Implements: [S 02 §2.6.1, §2.6.3], [S 03 §3.2, §3.5], OQ-01 (own Part 21 reader), OQ-04
(report + basic sewing). Depends on: `fluut-geom`, `fluut-topo`, `memchr`, `flate2` (`.stpz`).

## 1. Pipeline

```
bytes ──► 1 lexer ──► 2 parser ──► RawModel (entity table)
                                      │
                                      ▼
                    3 schema detect (AP203 / AP214 / AP242)
                                      │
                                      ▼
                    4 product/assembly walk ──► list of (representation, placement, name)
                                      │
                                      ▼  (per representation, parallel)
                    5 geometry/topology mapping ──► BrepBody / MeshBody / WireBody
                                      │
                                      ▼
                    6 validation + sewing ──► diagnostics
                                      │
                                      ▼
                    7 persistent ids, BVH, trim caches (fluut-topo)
```

Each stage reports progress and checks cancellation (D01 §3).

## 2. Lexer

Part 21 (ISO 10303-21) clear text. Hand-written, byte-oriented, zero-copy over a memory-mapped
or fully read buffer.

Tokens: `#123` entity refs, keywords (`ADVANCED_FACE`), strings `'...'` with `''` escapes and
`\X2\…\X0\` Unicode escapes, integers, reals (`1.E-3`), enums `.T.`, `$` (unset), `*`
(derived), binaries `"…"`, punctuation ` , = ;`, comments `/* … */`.

- Uses `memchr` to skip to delimiters; target ≥ 300 MB/s.
- Positions (line, column) kept per entity start for error messages [S 02 §2.6.1].
- Limits (untrusted input, [S 06 §6.3]): file size ≤ 2 GB (configurable), string length
  ≤ 1 MB, nesting depth of parameter lists ≤ 64.

## 3. Parser and raw model

```rust
pub struct RawModel {
    pub header: Header,                        // FILE_SCHEMA, FILE_NAME, …
    pub entities: Vec<RawEntity>,              // dense
    pub index: HashMap<u64, u32>,              // #id → slot
}

pub enum RawEntity {
    Simple { kind: KwId, params: Params, pos: Pos },
    Complex { parts: SmallVec<[(KwId, Params); 4]>, pos: Pos },   // (A(...)B(...)) instances
}

pub enum Param { Ref(u64), Int(i64), Real(f64), Str(Box<str>), Enum(KwId), List(Params),
                 Typed(KwId, Box<Param>), Unset, Derived }
```

- Keywords are interned (`KwId`), so mapping code compares integers.
- Parsing is two-pass: first pass splits the DATA section into entity spans (fast scan for
  `#n=` and `;` outside strings) and is parallelised over chunks; second pass parses spans in
  parallel with `rayon`. Entity order does not matter in Part 21.
- Dangling references are recorded and reported when dereferenced, not at parse time.
- Cycle detection: mapping keeps a "visiting" set per resolution path; a cycle is an error
  located at the entity (untrusted input).

## 4. Schema and products

Schema from `FILE_SCHEMA`: `CONFIG_CONTROL_DESIGN` (AP203 ed1), `AP203_CONFIGURATION_CONTROLLED_3D_DESIGN…`
(ed2), `AUTOMOTIVE_DESIGN` (AP214), `AP242_MANAGED_MODEL_BASED_3D_ENGINEERING…` (AP242). The
geometry and topology entities are shared (Part 42), so one mapper serves all; schema only
changes where names, colours and persistent ids are found.

Assembly walk:

```
roots = PRODUCT_DEFINITIONs not used as a child in any NEXT_ASSEMBLY_USAGE_OCCURRENCE
for each root: walk NAUO children recursively, composing placements:
    placement(child) = placement(parent) ∘ ITEM_DEFINED_TRANSFORMATION
                       (from CONTEXT_DEPENDENT_SHAPE_REPRESENTATION → REPRESENTATION_RELATIONSHIP)
    shape reps of a product: via PRODUCT_DEFINITION_SHAPE ← SHAPE_DEFINITION_REPRESENTATION
emit (shape_rep, composed placement, instance name path)
```

- Each instance becomes its own `Body` [S 02 §2.6.1]. Shared representations are mapped once and
  instanced (`Arc` + transform), then transformed copies made lazily.
- Instance names: product name + NAUO id path (for the assembly tree in the UI).
- Files with geometry but no product structure (some exporters): every top-level shape
  representation becomes a body.

## 5. Geometry and topology mapping

### 5.1 Supported entities

| Group | Entities |
|---|---|
| Points/dirs/placement | `CARTESIAN_POINT`, `DIRECTION`, `VECTOR`, `AXIS1_PLACEMENT`, `AXIS2_PLACEMENT_2D/3D` |
| Curves | `LINE`, `CIRCLE`, `ELLIPSE`, `PARABOLA`, `HYPERBOLA`, `POLYLINE`, `TRIMMED_CURVE`, `B_SPLINE_CURVE_WITH_KNOTS`, `BEZIER_CURVE`, `QUASI_UNIFORM_CURVE`, `UNIFORM_CURVE`, rational complex (`B_SPLINE_CURVE … RATIONAL_B_SPLINE_CURVE`), `OFFSET_CURVE_3D`, `SURFACE_CURVE`, `SEAM_CURVE`, `PCURVE`, `COMPOSITE_CURVE` |
| Surfaces | `PLANE`, `CYLINDRICAL_SURFACE`, `CONICAL_SURFACE`, `SPHERICAL_SURFACE`, `TOROIDAL_SURFACE`, `DEGENERATE_TOROIDAL_SURFACE`, `SURFACE_OF_LINEAR_EXTRUSION`, `SURFACE_OF_REVOLUTION`, `B_SPLINE_SURFACE_WITH_KNOTS`, `BEZIER_SURFACE`, uniform variants, rational complex, `RECTANGULAR_TRIMMED_SURFACE`, `OFFSET_SURFACE` |
| Topology | `VERTEX_POINT`, `EDGE_CURVE`, `ORIENTED_EDGE`, `EDGE_LOOP`, `VERTEX_LOOP`, `POLY_LOOP`, `FACE_BOUND`, `FACE_OUTER_BOUND`, `ADVANCED_FACE`, `FACE_SURFACE`, `CLOSED_SHELL`, `OPEN_SHELL`, `ORIENTED_CLOSED_SHELL` |
| Bodies | `MANIFOLD_SOLID_BREP`, `BREP_WITH_VOIDS`, `SHELL_BASED_SURFACE_MODEL`, `FACETED_BREP` (→ mesh), `GEOMETRIC_CURVE_SET`, `GEOMETRIC_SET` |
| Context | units (`SI_UNIT`, `CONVERSION_BASED_UNIT`, `LENGTH_UNIT`, `PLANE_ANGLE_UNIT`), `UNCERTAINTY_MEASURE_WITH_UNIT` |
| Presentation | `STYLED_ITEM` → colours (AP214/242) |

Anything else inside a shape representation is counted and reported by name [S 02 §2.6.1].

### 5.2 Mapping rules

- Units: lengths converted to mm at mapping time; plane angles to radians. Missing unit → mm
  with a warning.
- `tol_model` = uncertainty measure if present, else `1e-6 × body diagonal`, clamped to
  [1e-7, 1e-2] mm.
- B-spline knots expanded from (multiplicities, knots); validity checked (non-decreasing, count =
  poles + degree + 1, weights > 0). Invalid → face reported and skipped.
- `SEAM_CURVE` → one edge with two coedges on the same face, p-curves differing by one period.
- **Missing p-curves.** Many files omit p-curves. They are computed by projecting the 3-D edge
  curve onto the surface: closed form for analytic surfaces (exact `Curve2`); for B-spline
  surfaces, project sample points (closest point, D02) and fit a 2-D B-spline to
  `tol_model / |S_uv|`. Seam handling picks the unwrap consistent with neighbouring coedges.
- Edge sense: `EDGE_CURVE.same_sense` and `ORIENTED_EDGE.orientation` combine into
  `Coedge.forward`.
- Vertex positions vs curve ends: if a curve end differs from its vertex by more than tol_model,
  the edge range is adjusted to the closest parameter and the gap is reported.

## 6. Validation and sewing (OQ-04)

Checks per body:

| Check | Action |
|---|---|
| Loop closure: consecutive coedge ends match within tol_model | report; attempt sewing |
| Edge used by two coedges of opposite direction (manifold) | report non-manifold or inconsistent orientation |
| Free edges in a body declared closed | report open shell; try sewing |
| P-curve vs 3-D curve deviation | report if > tol_model; recompute p-curve |
| Face with no valid outer loop | exclude face, report |

**Sewing** (basic, OQ-04):

```
free = all coedges whose edge has a single coedge
index free edges by midpoint in a k-d tree
for each free edge e: find candidates f with |ends match| ≤ tol_model and
    max deviation along the curves ≤ tol_model (sampled at 8 points + Newton)
merge e and f into one edge (keep the curve with the smaller deviation), relink coedges
merge coincident vertices
```

No geometry is modified beyond merging. Faces still invalid after sewing are excluded from
machining (marked in `Face`) and listed in the import report.

## 7. Import report

```rust
pub struct ImportReport {
    pub schema: Schema,
    pub bodies: Vec<BodySummary>,              // name, kind, faces, edges, tol_model
    pub unsupported: BTreeMap<String, usize>,  // entity name → count
    pub sewn_edges: usize,
    pub excluded_faces: Vec<(PersistId, Diagnostic)>,
    pub diagnostics: Vec<Diagnostic>,
}
```

Shown after import in the UI (D13) and returned to scripts (D12).

## 8. Performance targets

| Case | Target (8 cores) |
|---|---|
| 10 MB AP214 part (≈ 5 000 faces) | < 1.5 s to bodies with BVH |
| 100 MB assembly | < 15 s, memory < 3× file size |

## 9. Spike (OQ-01)

Before full implementation, a two-week spike:

1. Implement §2–§3 and the analytic subset of §5.
2. Evaluate `ruststep` and `truck-stepio` on the same corpus for coverage and speed.
3. Evaluate `truck-geometry` NURBS evaluation against D02 §8 targets and accuracy tests.
4. Record decisions in [S 01 §1.4] and close OQ-01's spike item.

Corpus: NIST CAx-IF public test files (AP203/214/242), STEP exports from FreeCAD, Onshape,
SolidWorks, Fusion, Inventor, Creo and CATIA where licensing allows, and fuzzed variants.

**Result (S1, 2026-10-31).** Own reader: 33/33 corpus files and 7 902/7 902 faces mapped,
61/63 shells clean at the declared uncertainty. `truck-stepio`: 17/33 files, 4 closed shells.
`truck-geometry` NURBS evaluation is 2.3× slower than `fluut-geom`. Decision: own reader and
kernel, no `truck` crates ([S1 report](../plan/spikes/s1-kernel-step.md)). Open: the §5.2 tolerance clamp hides real
gaps in files that declare more than 0.01 mm (report §6).

## 10. Tests

- Round-trip: for each corpus file, face count, edge count and total area match a reference
  produced by an independent kernel (recorded once, stored as test fixtures).
- Sewing: synthetic bodies with gaps of 0.5 × and 2 × tol_model.
- Fuzzing (`cargo-fuzz`) on the lexer, parser and mapper; no panics, bounded memory.

## 11. Challenges

- CH-07 Reference results for STEP corpus without a C++ kernel (deferred).
- CH-08 P-curve computation on B-spline faces near seams and singularities (resolved 2026-11-01: [report](../plan/spikes/step-pcurves-fuzz.md)).
