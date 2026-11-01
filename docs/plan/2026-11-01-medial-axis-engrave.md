# Medial Axis and `engrave` Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The exact medial axis of 2-D regions bounded by lines and arcs (CH-17), and the `engrave` toolpath type (V-carving, D09 §2.6, D15b §2.4) built on it. This finishes the last type of M6 part 1.

**Architecture:**
- `fluut-cam::region::medial`: a new module. `medial_axis(&Region2)` returns sites, vertices and edges. Each edge is a bisector of two sites, stored exactly and evaluated in closed form, with its clearance radius.
- `fluut-cam::strategy::engrave`: depth from the clearance radius, flat areas, step-down levels, corner sharpening and the D15b codes.
- `fluut-cli`: the reference project `vcarve` (a DXF of lettering-like shapes), its golden G-code and translation invariance.

**Tech Stack:** as M6. No new crates.

**Design:** [D09](../design/D09-strategies.md) §2.6; [D15b](../design/D15b-toolpath-parameters-2.md) §2.4, §9 (decision 9); [challenges](../design/challenges.md) CH-17; [S3 report](spikes/s3-region-2d.md) §6; M6 part 1 [plan](2026-11-01-m6-2d-holes.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask check-layers` and `cargo deny check`.
- Determinism (D01 §6): `libm` for transcendental functions; no `HashMap` order in outputs. The axis of fixed shapes is pinned bit for bit.
- Keep changes in the new region module, in `engrave`, and in their tests and reference project. Other sessions work on other M6 strategies and on `fluut-script`.
- No DeskProto-derived content.

## Algorithm

**Choice: edge tracing with exact bisectors.** The axis is traced edge by edge from a seed point, as in the classic tracing methods for polygons, generalised to arcs.

- **Sites.** Every line and arc of the boundary is a site, and so is every reflex vertex (a point site: a circle of radius 0). Convex vertices are not sites; the axis ends there. A site has a parameter range, the *cone* of points whose foot lies on it. Lines are lines; arcs and points are circles with a side sign. So there are only two kinds of site.
- **Bisectors in closed form.** On the bisector of sites L and R, the point whose foot on R is at parameter u is `foot(u) + ρ·normal(u)`. The equation `d_L(p) = ρ` is linear in ρ for both kinds of L, because for a circle the ρ² terms cancel. So each edge is evaluated exactly at any u. The curves are a line (line–line, point–point), a parabola (line–circle) and a hyperbola or ellipse (circle–circle); concentric circles give a circle.
- **Vertices in closed form.** A vertex is a point equidistant from three sites (Apollonius' problem with signed distances). The site equations are linear in (x, y, ρ) for lines and quadratic with the same quadratic part x² + y² − ρ² for circles. Differences give two linear equations, and one quadratic is left, so there are at most two candidates. A Newton step on the signed distances polishes them. Arc centres reached by two straight sites (rounded ends) are tested directly, since they are double roots.
- **Tracing.** Walking along the edge (L, R) with R on the right, the foot on R moves forward along the boundary and the foot on L moves backward. The edge ends at the first event in R's parameter:
  - a third site becomes equidistant (a branch vertex);
  - the foot leaves R's cone at its end, or L's cone at its start. The edge then goes on with the next site (a reflex point, or the next segment at a tangent joint). At a convex corner the clearance is 0 and the branch ends in a leaf.
- **Vertices from the touching set.** At each event, the sites within tolerance of the clearance are collected with the angles of their feet (an interval for an arc whose centre is the vertex). Every angular gap between consecutive feet is an outgoing edge. This one rule covers degree-3 vertices, degenerate degree-4 or higher vertices (squares), arc-centre leaves (rounded ends), discs (an isolated point) and the degree-2 vertices where an edge changes sites.
- **Seeds and cycles.** One seed per connected area, from the midpoint of its longest site: the largest empty disc along the inward normal. Holes close cycles: a trace that arrives at an existing vertex marks the matching gap as done. A seed in the middle of an edge is found as an event by the trace that comes back to it (annuli).

**Why not the alternatives.**
- A full Voronoi diagram, incremental or by divide and conquer, builds the outside of the region too, and needs a robust topology update for arcs. That is the 3–5 weeks of the S3 estimate. Tracing only builds the interior, which is all `engrave` needs, and it never updates a topology: each step is a local, closed-form computation.
- `boostvoronoi` works on integer points and segments only (CH-17).
- Sampling the boundary and taking a point Voronoi diagram is not exact and converges slowly at arcs.

Tracing costs O(n) per edge with a uniform grid of sites, so O(n · E) at worst and close to linear on lettering.

**Exactness.** Edges are stored as (L, R, u₀, u₁) and evaluated exactly. Only `engrave` samples them, adaptively, so that the chord error in X, Y and Z stays under `machining_tolerance / 4`.

## Scope decisions

- **Flat areas.** Where `(ρ − Rt) / tan α` exceeds `max_depth`, the axis is not followed. The tool instead runs along the boundary of `region.offset(−ρmax)`, ρmax = Rt + `max_depth`·tan α, at `top − max_depth`, and `cam.engrave.flat_area` is raised. `Engrave::flat_areas` returns that region, so a command can make a WireBody from it (D15b decision 9). No live link.
- **Corner sharpening.** The exact axis already reaches every convex corner, so V-carved corners are sharp. The option therefore applies to flat areas, as D15b says: a branch from the edge of a flat area to a convex corner is cut only when `corner_sharpening` is on. Without it, such corners are rounded to the V width at `max_depth`.
- **Short branches.** A leaf branch end is dropped while the material it alone removes is thinner than the machining tolerance. The test is the sagitta ρ·(1 − cos(θ/2)), where θ is the angle between the two feet. This removes the long, useless branches of nearly flat corners in fitted curves.
- **Tip radius.** Where ρ < Rt the depth is 0; such parts at the ends of a branch are dropped.
- **Order.** Branches are joined into the longest paths possible: each path starts at a leaf where it can, and the next path starts at the free end nearest the last end. Every entry plunges, since `ramp.kind = "plunge"` is the only kind allowed.
- **In-process stock** is still not built, as in M6.

## Review Focus

1. **Property tests:** every axis point is at its clearance from the boundary, equidistant from two feet and no nearer any other point. The branch structure has E − V + 1 = holes in each area.
2. **Completeness:** for random interior points, the end of the largest empty disc along the ray from the foot is on the axis.
3. **Degenerate input:** squares, discs, annuli, rounded ends, touching loops, nearly square rectangles, nearly tangent joints.
4. **Bit-identical** vertices and edges on fixed shapes.
5. **Every `engrave` code** of D15b §2.4 with a passing and a failing case.
6. **Translation invariance** of the `vcarve` project.

---

### Task 1: Plan

- [x] **Step 1:** This plan; README row. Commit.

### Task 2: Sites, bisectors and vertices

**Files:** Create `crates/fluut-cam/src/region/medial.rs`, `crates/fluut-cam/src/region/medial/site.rs`, `crates/fluut-cam/src/region/medial/tests.rs`; one `mod` line in `region.rs`.

- [x] **Step 1:** Tests: sites of a polygon with reflex vertices and of loops with arcs; the bisector point at a parameter for each pair of kinds, checked against both distances; the conic kind of each pair; three-site vertices for line, arc and point triples, with known answers (incircles, arc centres).
- [x] **Step 2:** Implement; pass.

### Task 3: Tracing

- [x] **Step 1:** Tests: rectangle, square, triangle, L-shape, disc, annulus (concentric and not), rectangle with a round hole, stadium, rounded rectangle, a hole touching the outer loop, two squares touching at a corner, a nearly square rectangle, nearly tangent joints, lettering-like shapes with many arcs. Property tests (proptest) on random polygons and on random arc shapes: clearance, equidistance, completeness, E − V + 1 = holes. Bit-pinned vertices and edges.
- [x] **Step 2:** Implement; pass. Commit.

### Task 4: `engrave`

**Files:** `crates/fluut-cam/src/strategy/engrave.rs`, `crates/fluut-cam/tests/engrave.rs`.

- [ ] **Step 1:** Tests: depth from ρ, α and Rt along a known axis; a narrow stroke at constant depth; the ring and `cam.engrave.flat_area` on a wide shape; corner sharpening on and off; step-down levels; every D15b code passes and fails; no point of the path deeper than `max_depth` or with a cone that cuts outside the region.
- [ ] **Step 2:** Implement; pass. Commit.

### Task 5: Reference project

**Files:** Create `crates/fluut-cli/tests/projects/vcarve/`; `crates/fluut-cli/tests/golden/vcarve-linuxcnc.ngc`; `crates/fluut-cli/tests/cli.rs`.

- [ ] **Step 1:** A DXF of lettering-like shapes (strokes with round ends, a bowl with a hole, serifs) written by a test function, so that translation invariance can move it. Golden G-code; translation invariance. Commit.

### Task 6: Measure and close

- [ ] **Step 1:** Example `medial_axis`: time of the axis on the S3 shapes A, B, C and on a text-like outline with many arcs.
- [ ] **Step 2:** Update CH-17, D09 §2.6, D15b §8; mark this plan done with deviations; README row. Commit.

### Task 7: Merge

- [ ] **Step 1:** Merge `origin/main`; resolve conflicts; rerun all five checks; push.
